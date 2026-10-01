#!/usr/bin/env python3
"""Seal exact executables and bind them to their source and preparation record."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys
import tempfile
from typing import Any
import uuid

sys.dont_write_bytecode = True
if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from ci_broker.client import git, repository_is_clean, source_commit
from ci_broker.broker import MINIMAL_ENVIRONMENT
from ci_manager import workspace
from deployment import signing
from deployment.inventory import descriptor


class CandidateError(RuntimeError):
    """An artifact cannot be tied to its declared source."""


def json_bytes(value: Any) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode()


def digest(path: Path) -> str:
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()


def regular(path: Path) -> None:
    try:
        mode = path.lstat().st_mode
    except OSError as error:
        raise CandidateError(f"missing artifact: {path}") from error
    if not stat.S_ISREG(mode):
        raise CandidateError(f"artifact must be a regular non-symbolic file: {path}")


def tree_files(root: Path) -> dict[str, dict[str, Any]]:
    """Hash a tree without following symbolic paths or accepting special files."""
    if root.is_symlink() or not root.is_dir():
        raise CandidateError(f"not a regular artifact directory: {root}")
    entries: dict[str, dict[str, Any]] = {}
    for path in sorted(root.rglob("*")):
        mode = path.lstat().st_mode
        if stat.S_ISDIR(mode):
            continue
        regular(path)
        entries[path.relative_to(root).as_posix()] = {
            "sha256": digest(path),
            "executable": bool(mode & stat.S_IXUSR),
        }
    return entries


def seal_tree(root: Path) -> None:
    for path in sorted(root.rglob("*"), reverse=True):
        if path.is_symlink():
            raise CandidateError(f"symbolic artifact path: {path}")
        if path.is_file():
            path.chmod(0o555 if path.stat().st_mode & stat.S_IXUSR else 0o444)
        elif path.is_dir():
            path.chmod(0o555)
        else:
            raise CandidateError(f"special artifact path: {path}")
    root.chmod(0o555)


def remove_tree(root: Path) -> None:
    if root.is_dir() and not root.is_symlink():
        for path in root.rglob("*"):
            if path.is_dir() and not path.is_symlink():
                path.chmod(0o700)
        root.chmod(0o700)
    shutil.rmtree(root)


def source_identity(source: Path) -> str:
    """Use the commit for clean source and a fresh identity for dirty builds."""
    if repository_is_clean(source):
        return source_commit(source)
    return "dirty:" + uuid.uuid4().hex

def read_manifest(root: Path) -> dict[str, Any]:
    """Read the prepared package metadata used to locate its files."""
    manifest = json.loads((root / "candidate.json").read_text())
    if not isinstance(manifest, dict):
        raise CandidateError("candidate metadata must be an object")
    return manifest


def stage(source: Path, product: str, output: Path, binary_spec: str, *,
          signing_policy: dict[str, Any] | None = None) -> dict[str, Any]:
    """Prepare a product package from the selected source and binaries."""
    return _stage(source, product, output, binary_spec, signing_policy=signing_policy)


def stage_build(source: Path, product: str, output: Path, binary_spec: str, *,
                target: Path, source_key: str,
                signing_policy: dict[str, Any] | None = None) -> dict[str, Any]:
    """Seal a successful release build without claiming a CI pass."""
    return _stage(source, product, output, binary_spec, target=target,
                  build_source_key=source_key, signing_policy=signing_policy,
                  signed_build=True)


def verify(root: Path, *, signing_policy: dict[str, Any] | None = None) -> dict[str, Any]:
    """Verify candidate bytes against an externally selected signing policy."""
    policy = signing.load_policy() if signing_policy is None else signing_policy
    if policy is not None:
        signing.assert_current(policy)
    files = tree_files(root)
    regular(root / "candidate.json")
    manifest = read_manifest(root)
    if manifest.get("schema") != 1:
        raise CandidateError("unsupported candidate metadata")
    retained_digest = signing.policy_digest(policy) if policy is not None else None
    if (manifest.get("signing_policy") != policy
            or manifest.get("signing_policy_digest") != retained_digest):
        raise CandidateError("candidate signing policy does not match the selected policy")
    unsigned_manifest = {key: value for key, value in manifest.items() if key != "candidate_id"}
    expected_id = "sha256:" + hashlib.sha256(json_bytes(unsigned_manifest)).hexdigest()
    if manifest.get("candidate_id") != expected_id:
        raise CandidateError("candidate metadata identity mismatch")
    product = manifest.get("product")
    if not isinstance(product, str) or not re.fullmatch(r"[a-z][a-z0-9-]*", product):
        raise CandidateError("invalid candidate product identity")
    binaries = manifest.get("binaries")
    if not isinstance(binaries, dict) or not binaries:
        raise CandidateError("candidate declares no executables")
    if set(files) != {"candidate.json", *(f"bin/{name}" for name in binaries)}:
        raise CandidateError("candidate file inventory changed")
    for name, record in binaries.items():
        if (not isinstance(name, str) or not re.fullmatch(r"[a-z][a-z0-9-]*", name)
                or not isinstance(record, dict) or record.get("path") != f"bin/{name}"):
            raise CandidateError("invalid candidate executable declaration")
        path = root / "bin" / name
        regular(path)
        if digest(path) != record.get("sha256"):
            raise CandidateError(f"candidate executable changed: {name}")
        expected_identifier = signing.identifier(policy, product, name) if policy is not None else None
        if record.get("code_identifier") != expected_identifier:
            raise CandidateError(f"candidate executable identifier mismatch: {name}")
        if policy is not None:
            signing.verify(path, policy, product, name)
    return manifest


def verify_signatures(root: Path, policy: dict[str, Any] | None = None) -> dict[str, Any]:
    return verify(root, signing_policy=policy)


def _stage(source: Path, product: str, output: Path, binary_spec: str, *,
           target: Path | None = None, build_source_key: str | None = None,
           signing_policy: dict[str, Any] | None = None,
           signed_build: bool = False) -> dict[str, Any]:
    if not re.fullmatch(r"[a-z][a-z0-9-]*", product):
        raise CandidateError("invalid product identity")
    policy = signing.load_policy() if signing_policy is None else signing_policy
    if policy is not None:
        signing.assert_current(policy)
        if not signed_build:
            signing.preflight(policy)
    source = source.resolve()
    if not output.is_absolute() or output.is_symlink():
        raise CandidateError("candidate output must be an absolute non-symbolic path")
    output = output.absolute()
    if output == source or source in output.parents:
        raise CandidateError("candidate staging must be outside the source worktree")
    commit = git(source, "rev-parse", "HEAD").decode().strip()
    source_key = build_source_key or source_identity(source)
    descriptor_id = "decisions" if product == "krisis" else product
    descriptor_path = source / "pipeline/products" / f"{descriptor_id}.sh"
    regular(descriptor_path)
    values = descriptor(descriptor_path.read_text())
    product_dir = values.get("PRODUCT_DIR", "")
    if values.get("PRODUCT_ID") != descriptor_id or not re.fullmatch(r"[a-z][a-z0-9-]*", product_dir):
        raise CandidateError("invalid product directory declaration")
    canonical = "krisis" if product == "decisions" else product
    output.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    temporary = Path(tempfile.mkdtemp(prefix=".candidate-", dir=output.parent))
    try:
        (temporary / "bin").mkdir(mode=0o700)
        binaries: dict[str, dict[str, str]] = {}
        for row in binary_spec.splitlines():
            if not row:
                continue
            fields = row.split("|")
            if len(fields) != 3:
                raise CandidateError("invalid product binary declaration")
            _, relative, name = fields
            if not re.fullmatch(r"[a-z][a-z0-9-]*", name) or name in binaries:
                raise CandidateError("invalid or repeated product executable")
            if not relative.startswith("target/") or ".." in Path(relative).parts:
                raise CandidateError("product executable is outside the Cargo target")
            binary_target = target or Path(os.environ.get("CARGO_TARGET_DIR", str(source / "target")))
            binary = binary_target / relative.removeprefix("target/")
            regular(binary)
            if policy is not None and signed_build:
                signing.verify(binary, policy, canonical, name)
            sealed = temporary / "bin" / name
            with binary.open("rb") as incoming, sealed.open("xb") as destination:
                shutil.copyfileobj(incoming, destination)
                destination.flush()
                os.fsync(destination.fileno())
            sealed.chmod(0o755)
            if policy is not None:
                if signed_build:
                    signing.verify(sealed, policy, canonical, name)
                else:
                    signing.sign(sealed, policy, canonical, name)
            environment = {key: value for key, value in os.environ.items() if key in MINIMAL_ENVIRONMENT}
            environment.update(workspace.environment())
            version = subprocess.run(workspace.confined_command([str(sealed), "--version"]),
                                     env=environment, check=True, capture_output=True,
                                     text=True, timeout=30).stdout.strip()
            if policy is not None:
                signing.verify(sealed, policy, canonical, name)
            sealed.chmod(0o555)
            binaries[name] = {"path": f"bin/{name}", "sha256": digest(sealed), "version": version,
                              "code_identifier": signing.identifier(policy, canonical, name)
                              if policy is not None else None}
        if not binaries:
            raise CandidateError("product declares no deployment executables")
        manifest: dict[str, Any] = {
            "schema": 1, "product": canonical, "source_commit": commit,
            "source_key": source_key, "binaries": binaries,
            "signing_policy": policy,
            "signing_policy_digest": signing.policy_digest(policy) if policy is not None else None,
        }
        manifest["candidate_id"] = "sha256:" + hashlib.sha256(json_bytes(manifest)).hexdigest()
        with (temporary / "candidate.json").open("xb") as stream:
            stream.write(json_bytes(manifest))
            stream.flush()
            os.fsync(stream.fileno())
        seal_tree(temporary)
        if output.exists():
            remove_tree(output)
        os.rename(temporary, output)
        return manifest
    finally:
        if temporary.exists():
            remove_tree(temporary)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-root", type=Path, required=True)
    parser.add_argument("--product", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--binary-spec", required=True)
    arguments = parser.parse_args()
    try:
        result = stage(arguments.source_root, arguments.product, arguments.output, arguments.binary_spec)
        print(json.dumps({"candidate_id": result["candidate_id"], "product": result["product"]}))
        return 0
    except (CandidateError, OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        print(f"cell-deploy: cannot stage candidate: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
