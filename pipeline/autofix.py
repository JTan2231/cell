"""Prepare a private Rust repair patch without changing the checked source."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys
import tempfile


class AutofixError(Exception):
    """The helper cannot establish a safe repair patch."""


@dataclass(frozen=True)
class Edit:
    path: str
    start: int
    end: int
    replacement: bytes


def suggestion_groups(message: dict) -> list[list[dict]]:
    """Keep each complete multipart suggestion together."""
    groups = []
    spans = message.get("spans", [])
    if isinstance(spans, list):
        replacements = [span for span in spans if isinstance(span, dict)
                        and span.get("suggested_replacement") is not None]
        if replacements and all(span.get("suggestion_applicability") == "MachineApplicable"
                                for span in replacements):
            groups.append(replacements)
    children = message.get("children", [])
    if isinstance(children, list):
        for child in children:
            if isinstance(child, dict):
                groups.extend(suggestion_groups(child))
    return groups


def run_clippy(root: Path, command: list[str]) -> tuple[int, list[list[dict]]]:
    child = subprocess.Popen(command, cwd=root, stdout=subprocess.PIPE)
    groups = []
    if child.stdout is None:
        raise AutofixError("Clippy has no diagnostic stream")
    for line in child.stdout:
        try:
            record = json.loads(line)
        except (ValueError, UnicodeError):
            sys.stderr.write(line.decode("utf-8", "replace"))
            continue
        if not isinstance(record, dict) or record.get("reason") != "compiler-message":
            continue
        message = record.get("message")
        if not isinstance(message, dict):
            continue
        rendered = message.get("rendered")
        if isinstance(rendered, str):
            sys.stderr.write(rendered)
        groups.extend(suggestion_groups(message))
    child.stdout.close()
    return child.wait(), groups


def relative_source(root: Path, name: str) -> str | None:
    path = Path(name)
    if path.is_absolute():
        try:
            path = path.relative_to(root)
        except ValueError:
            return None
    if not path.parts or ".." in path.parts:
        return None
    return path.as_posix()


def overlap(left: Edit, right: Edit) -> bool:
    if left.path != right.path or left == right:
        return False
    if left.start == left.end:
        return right.start <= left.start <= right.end
    if right.start == right.end:
        return left.start <= right.start <= left.end
    return left.start < right.end and right.start < left.end


def select_edits(groups: list[list[dict]], root: Path,
                 originals: dict[str, bytes]) -> list[Edit]:
    """Accept complete, compatible groups against the original byte offsets."""
    accepted: list[Edit] = []
    seen = set()
    for spans in groups:
        edits = []
        valid = bool(spans)
        for span in spans:
            name, replacement = span.get("file_name"), span.get("suggested_replacement")
            start, end = span.get("byte_start"), span.get("byte_end")
            path = relative_source(root, name) if isinstance(name, str) else None
            if (path not in originals or not path.endswith(".rs")
                    or span.get("suggestion_applicability") != "MachineApplicable"
                    or not isinstance(replacement, str) or type(start) is not int or type(end) is not int):
                valid = False
                break
            source = originals[path]
            if not 0 <= start <= end <= len(source):
                valid = False
                break
            if any(offset < len(source) and source[offset] & 0xC0 == 0x80 for offset in (start, end)):
                valid = False
                break
            try:
                source.decode("utf-8")
                edit = Edit(path, start, end, replacement.encode("utf-8"))
            except UnicodeError:
                valid = False
                break
            if edit not in edits:
                edits.append(edit)
        if not valid:
            continue
        identity = tuple(sorted(edits, key=lambda edit: (edit.path, edit.start, edit.end, edit.replacement)))
        if identity in seen:
            continue
        seen.add(identity)
        if any(overlap(edit, other) for index, edit in enumerate(edits) for other in edits[:index]):
            continue
        if any(overlap(edit, other) for edit in edits for other in accepted):
            continue
        accepted.extend(edit for edit in edits if edit not in accepted)
    return accepted


def apply_edits(originals: dict[str, bytes], edits: list[Edit]) -> dict[str, bytes]:
    changed = {}
    for path in sorted({edit.path for edit in edits}):
        source = originals[path]
        for edit in sorted((edit for edit in edits if edit.path == path),
                           key=lambda edit: (edit.start, edit.end), reverse=True):
            source = source[:edit.start] + edit.replacement + source[edit.end:]
        if source != originals[path]:
            changed[path] = source
    return changed


def snapshot(root: Path, destination: Path) -> tuple[dict[str, bytes], dict[str, int]]:
    listed = subprocess.run(["git", "-C", str(root), "ls-files", "-z"], check=True, capture_output=True)
    originals, modes = {}, {}
    symlinks = []
    for raw in listed.stdout.split(b"\0"):
        if not raw:
            continue
        relative = relative_source(root, os.fsdecode(raw))
        if relative is None:
            raise AutofixError("Git returned a path outside the candidate")
        source, copied = root / relative, destination / relative
        if any(parent.is_symlink() for parent in source.parents if parent != root and root in parent.parents):
            raise AutofixError("tracked source has a symlink parent")
        mode = source.lstat().st_mode
        if not stat.S_ISREG(mode) and not stat.S_ISLNK(mode):
            raise AutofixError(f"tracked source is not a regular file or symlink: {relative}")
        copied.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, copied, follow_symlinks=False)
        if stat.S_ISLNK(mode):
            symlinks.append(copied)
        elif relative.endswith(".rs"):
            originals[relative] = source.read_bytes()
            modes[relative] = stat.S_IMODE(mode)
    for link in symlinks:
        try:
            link.resolve().relative_to(destination)
        except (ValueError, RuntimeError):
            raise AutofixError(f"snapshot symlink escapes the source: {link.relative_to(destination)}") from None
    return originals, modes


def format_command(root: Path, copied: Path, command: list[str]) -> list[str]:
    packages = []
    manifest = "Cargo.toml"
    options = iter(command[2:])
    for item in options:
        if item == "--":
            break
        if item in ("--package", "-p", "--manifest-path"):
            value = next(options, None)
            if value is None:
                raise AutofixError(f"Clippy option has no value: {item}")
            if item == "--manifest-path":
                manifest = value
            else:
                packages.append(value)
        elif item.startswith("--package="):
            packages.append(item.split("=", 1)[1])
        elif item.startswith("--manifest-path="):
            manifest = item.split("=", 1)[1]
    relative = relative_source(root, manifest)
    if relative is None:
        raise AutofixError("Clippy manifest is outside the candidate")
    result = [command[0], "fmt", "--manifest-path", str(copied / relative)]
    for package in dict.fromkeys(packages):
        result.extend(("--package", package))
    return result


def make_patch(scratch: Path, originals: dict[str, bytes],
               modes: dict[str, int]) -> bytes:
    patches = []
    for relative, original in sorted(originals.items()):
        copied = scratch / "b" / relative
        if copied.is_symlink() or not copied.is_file():
            raise AutofixError(f"formatter replaced a tracked regular file: {relative}")
        if copied.read_bytes() == original:
            continue
        before = scratch / "a" / relative
        before.parent.mkdir(parents=True, exist_ok=True)
        before.write_bytes(original)
        before.chmod(modes[relative])
        difference = subprocess.run(
            ["git", "diff", "--no-index", "--no-prefix", "--binary", "--no-ext-diff", "--no-textconv",
             "--", "a/" + relative, "b/" + relative],
            cwd=scratch, capture_output=True,
        )
        if difference.returncode not in (0, 1):
            raise AutofixError(difference.stderr.decode("utf-8", "replace") or "Git could not retain the repair patch")
        patches.append(difference.stdout)
    return b"".join(patches)


def atomic_patch(path: Path, value: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary = tempfile.mkstemp(prefix="." + path.name + ".", dir=path.parent)
    try:
        os.fchmod(descriptor, 0o600)
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(value)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
        directory = os.open(path.parent, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        Path(temporary).unlink(missing_ok=True)


def run(root: Path, patch_path: Path, command: list[str]) -> int:
    atomic_patch(patch_path, b"")
    code, groups = run_clippy(root, command)
    if code < 0:
        return 128 - code
    with tempfile.TemporaryDirectory(prefix="cell-autofix-") as directory:
        scratch = Path(directory)
        copied = scratch / "b"
        originals, modes = snapshot(root, copied)
        edits = select_edits(groups, root, originals)
        for relative, source in apply_edits(originals, edits).items():
            (copied / relative).write_bytes(source)
        formatted = subprocess.run(format_command(root, copied, command), cwd=copied, check=False)
        if formatted.returncode:
            raise AutofixError(f"rustfmt failed with exit code {formatted.returncode}")
        patch = make_patch(scratch, originals, modes)
    atomic_patch(patch_path, patch)
    return 0 if patch else code


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--patch", type=Path, required=True)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if len(command) < 2 or command[1] != "clippy":
        parser.error("the command must invoke cargo clippy")
    if not args.root.is_absolute() or not args.patch.is_absolute():
        parser.error("root and patch paths must be absolute")
    root = args.root.resolve()
    patch = args.patch.resolve()
    if patch == root or root in patch.parents:
        parser.error("the repair patch must be outside the checked source")
    try:
        return run(root, patch, command)
    except (AutofixError, OSError, subprocess.SubprocessError) as error:
        print(f"autofix: {error}", file=sys.stderr)
        return 78


if __name__ == "__main__":
    raise SystemExit(main())
