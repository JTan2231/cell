#!/usr/bin/env python3
"""Seal exact executables and bind them to their source and preparation record."""

from __future__ import annotations

import argparse
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

from deployment.runtime import git, repository_is_clean, source_commit
from deployment import signing
from deployment.inventory import descriptor, product_root


class CandidateError(RuntimeError):
    """An artifact cannot be tied to its declared source."""


def json_bytes(value: Any) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode()


def regular(path: Path) -> None:
    try:
        mode = path.lstat().st_mode
    except OSError as error:
        raise CandidateError(f"missing artifact: {path}") from error
    if not stat.S_ISREG(mode):
        raise CandidateError(f"artifact must be a regular non-symbolic file: {path}")


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
                versions: dict[str, str] | None = None,
                signing_policy: dict[str, Any] | None = None) -> dict[str, Any]:
    """Seal a successful release build without claiming a CI pass."""
    return _stage(source, product, output, binary_spec, target=target,
                  build_source_key=source_key, signing_policy=signing_policy,
                  versions=versions, preflight_done=True)


def _stage(source: Path, product: str, output: Path, binary_spec: str, *,
           target: Path | None = None, build_source_key: str | None = None,
           versions: dict[str, str] | None = None,
           signing_policy: dict[str, Any] | None = None,
           preflight_done: bool = False) -> dict[str, Any]:
    if not re.fullmatch(r"[a-z][a-z0-9-]*", product):
        raise CandidateError("invalid product identity")
    policy = signing.load_policy() if signing_policy is None else signing_policy
    if policy is not None and not preflight_done:
        signing.assert_current(policy)
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
    if values.get("PRODUCT_ID") != descriptor_id:
        raise CandidateError("invalid product directory declaration")
    try:
        product_root(source, values.get("PRODUCT_DIR", ""))
    except ValueError as error:
        raise CandidateError(str(error)) from error
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
            sealed = temporary / "bin" / name
            with binary.open("rb") as incoming, sealed.open("xb") as destination:
                shutil.copyfileobj(incoming, destination)
                destination.flush()
                os.fsync(destination.fileno())
            sealed.chmod(0o755)
            if policy is not None:
                signing.sign(sealed, policy, canonical, name)
            sealed.chmod(0o555)
            binaries[name] = {"path": f"bin/{name}",
                              "code_identifier": signing.identifier(policy, canonical, name)
                              if policy is not None else None}
            if versions is not None and name in versions:
                binaries[name]["version"] = versions[name]
        if not binaries:
            raise CandidateError("product declares no deployment executables")
        manifest: dict[str, Any] = {
            "schema": 1, "product": canonical, "source_commit": commit,
            "source_key": source_key, "binaries": binaries,
            "signing_policy": policy,
            "candidate_id": "uuid:" + uuid.uuid4().hex,
        }
        with (temporary / "candidate.json").open("xb") as stream:
            stream.write(json_bytes(manifest))
            stream.flush()
            os.fsync(stream.fileno())
        (temporary / "candidate.json").chmod(0o444)
        (temporary / "bin").chmod(0o555)
        temporary.chmod(0o555)
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
