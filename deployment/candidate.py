#!/usr/bin/env python3
"""Seal tested executables before the CI broker releases its Cargo lane."""

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

sys.dont_write_bytecode = True
if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from ci_broker.client import git, source_snapshot


class CandidateError(RuntimeError):
    """An artifact cannot be tied to its tested source."""


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


def verify(root: Path, *, product: str | None = None,
           commit: str | None = None) -> dict[str, Any]:
    regular(root / "candidate.json")
    try:
        manifest = json.loads((root / "candidate.json").read_text())
        content = dict(manifest)
        identity = content.pop("candidate_id")
        if manifest["schema"] != 1:
            raise CandidateError("unsupported candidate schema")
        if identity != "sha256:" + hashlib.sha256(json_bytes(content)).hexdigest():
            raise CandidateError("candidate manifest identity changed")
        if product is not None and manifest["product"] != product:
            raise CandidateError("candidate belongs to another product")
        if commit is not None and manifest["source_commit"] != commit:
            raise CandidateError("candidate belongs to another source commit")
        expected = {record["path"]: {"sha256": record["sha256"], "executable": True}
                    for record in manifest["binaries"].values()}
        files = tree_files(root)
        files.pop("candidate.json")
        if files != expected:
            raise CandidateError("candidate executable tree changed")
        return manifest
    except (KeyError, TypeError, ValueError) as error:
        raise CandidateError("invalid candidate manifest") from error


def stage(source: Path, product: str, output: Path, binary_spec: str) -> dict[str, Any]:
    if not re.fullmatch(r"[a-z][a-z0-9-]*", product):
        raise CandidateError("invalid product identity")
    source = source.resolve()
    if not output.is_absolute() or output.is_symlink():
        raise CandidateError("candidate output must be an absolute non-symbolic path")
    output = output.absolute()
    if output == source or source in output.parents:
        raise CandidateError("candidate staging must be outside the source worktree")
    commit = git(source, "rev-parse", "HEAD").decode().strip()
    source_key, clean = source_snapshot(source)
    if not clean:
        raise CandidateError("candidate preparation requires an unchanged committed worktree")
    product_dir = "decisions" if product == "krisis" else product
    canonical = "krisis" if product == "decisions" else product
    source_inputs: dict[str, str] = {}
    tracked = git(source, "ls-files", "-z").split(b"\x00")
    prefixes = (f"{product_dir}/packaging/", f"{product_dir}/chancery",
                f"{product_dir}/provider/", f"{product_dir}/deployment/", "deployment/")
    for raw in tracked:
        path = os.fsdecode(raw)
        if path and path.startswith(prefixes):
            regular(source / path)
            source_inputs[path] = digest(source / path)
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
            target = Path(os.environ.get("CARGO_TARGET_DIR", str(source / "target")))
            binary = target / relative.removeprefix("target/")
            regular(binary)
            sealed = temporary / "bin" / name
            with binary.open("rb") as incoming, sealed.open("xb") as destination:
                shutil.copyfileobj(incoming, destination)
                destination.flush()
                os.fsync(destination.fileno())
            sealed.chmod(0o555)
            version = subprocess.run([str(sealed), "--version"], check=True,
                                     capture_output=True, text=True, timeout=30).stdout.strip()
            binaries[name] = {"path": f"bin/{name}", "sha256": digest(sealed), "version": version}
        if not binaries:
            raise CandidateError("product declares no deployment executables")
        if source_snapshot(source) != (source_key, True):
            raise CandidateError("source changed while staging candidate")
        manifest: dict[str, Any] = {
            "schema": 1, "product": canonical, "source_commit": commit,
            "source_key": source_key, "source_inputs": source_inputs, "binaries": binaries,
        }
        manifest["candidate_id"] = "sha256:" + hashlib.sha256(json_bytes(manifest)).hexdigest()
        with (temporary / "candidate.json").open("xb") as stream:
            stream.write(json_bytes(manifest))
            stream.flush()
            os.fsync(stream.fileno())
        seal_tree(temporary)
        if output.exists():
            if verify(output, product=canonical, commit=commit) != manifest:
                raise CandidateError("existing staged candidate differs from this successful gate")
        else:
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
    except (CandidateError, OSError, subprocess.SubprocessError) as error:
        print(f"cell-deploy: cannot stage candidate: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
