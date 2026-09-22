"""Private candidate commits and a deliberately small raw-text patch grammar."""

from __future__ import annotations

import os
from pathlib import Path, PurePosixPath
import re
import subprocess

from ci_manager.storage import ManagerError

ACCEPTED = "refs/ci/accepted"
MAX_PATCH_BYTES = 1024 * 1024


def git(root: Path, *args: str, data: bytes | None = None, env: dict | None = None,
        check: bool = True) -> subprocess.CompletedProcess:
    command = ["git", "-c", "core.hooksPath=/dev/null", "-c", "commit.gpgSign=false",
               "-C", str(root), *args]
    result = subprocess.run(command, input=data, capture_output=True, env=env, timeout=120)
    if check and result.returncode:
        raise ManagerError(result.stderr.decode("utf-8", "replace").strip() or f"Git failed: {args[0]}")
    return result


def value(root: Path, *args: str) -> str:
    return git(root, *args).stdout.decode().strip()


def commit(root: Path, revision: str) -> str:
    result = value(root, "rev-parse", "--verify", "--end-of-options", revision + "^{commit}")
    if not re.fullmatch(r"[0-9a-f]{40}|[0-9a-f]{64}", result):
        raise ManagerError("invalid resolved Git commit")
    return result


def common(root: Path) -> Path:
    return Path(value(root, "rev-parse", "--path-format=absolute", "--git-common-dir")).resolve()


def private_ref(identity: str, name: str) -> str:
    return f"refs/ci/jobs/{identity}/{name}"


def advance(root: Path, ref: str, old: str, new: str) -> None:
    observed = value(root, "rev-parse", "--verify", ref)
    if observed == new:
        return
    if observed != old:
        raise ManagerError(f"unexpected {ref}: expected {old}, found {observed}")
    git(root, "update-ref", ref, new, old)


def ensure_worktree(root: Path, path: Path, revision: str) -> None:
    if not path.exists():
        git(root, "worktree", "add", "--detach", str(path), revision)
    if common(path) != common(root):
        raise ManagerError("private candidate belongs to another repository")
    git(path, "reset", "--hard", revision)
    git(path, "clean", "-ffd")


def clean_candidate(path: Path, revision: str) -> None:
    if commit(path, "HEAD") != revision or value(path, "status", "--porcelain", "--untracked-files=all"):
        raise ManagerError("candidate worktree changed outside the recorded operation")


def commit_tree(root: Path, tree: str, parents: list[str], identity: str, stamp: int, message: str) -> str:
    environment = dict(os.environ, GIT_AUTHOR_NAME="Cell CI", GIT_AUTHOR_EMAIL="ci@cell.local",
                       GIT_COMMITTER_NAME="Cell CI", GIT_COMMITTER_EMAIL="ci@cell.local",
                       GIT_AUTHOR_DATE=f"{stamp} +0000", GIT_COMMITTER_DATE=f"{stamp} +0000")
    args = ["commit-tree", tree]
    for parent in parents:
        args += ["-p", parent]
    return git(root, *args, data=f"{message}\n\nCell-CI-Job: {identity}\n".encode(),
               env=environment).stdout.decode().strip()


def check_patch(text: str) -> bytes:
    raw = text.encode("utf-8")
    if not raw or len(raw) > MAX_PATCH_BYTES or "\x00" in text or "\r" in text:
        raise ManagerError("patch must be nonempty UTF-8 text, at most 1 MiB, with LF lines")
    lines = text.splitlines(keepends=True)
    index = 0
    seen = set()
    while index < len(lines):
        header = re.fullmatch(r"diff --git a/([^\s]+) b/([^\s]+)\n", lines[index])
        if not header or header[1] != header[2]:
            raise ManagerError("expected a raw same-path Git text diff; use delete/add for renames")
        path = header[1]
        parts = PurePosixPath(path).parts
        if (not parts or path.startswith("/") or str(PurePosixPath(path)) != path
                or any(part in {".", ".."} or part.lower() == ".git" for part in parts)
                or "\\" in path or '"' in path or path in seen):
            raise ManagerError(f"unsupported patch path: {path}")
        seen.add(path)
        index += 1
        while index < len(lines) and re.fullmatch(
            r"(?:(?:new file|deleted file|old|new) mode 100(?:644|755)|index [0-9a-f]+\.\.[0-9a-f]+(?: 100(?:644|755))?)\n", lines[index]
        ):
            index += 1
        if index + 1 >= len(lines):
            raise ManagerError("text patch has no file headers")
        before, after = lines[index:index + 2]
        if before not in {f"--- a/{path}\n", "--- /dev/null\n"} or after not in {f"+++ b/{path}\n", "+++ /dev/null\n"}:
            raise ManagerError("patch file headers do not match the path")
        if before == "--- /dev/null\n" and after == "+++ /dev/null\n":
            raise ManagerError("patch has no file")
        index += 2
        hunks = 0
        while index < len(lines) and lines[index].startswith("@@ "):
            match = re.fullmatch(r"@@ -\d+(?:,(\d+))? \+\d+(?:,(\d+))? @@[^\n]*\n", lines[index])
            if not match:
                raise ManagerError("invalid unified hunk")
            old, new = int(match[1] or "1"), int(match[2] or "1")
            index += 1
            hunks += 1
            while old or new:
                if index >= len(lines):
                    raise ManagerError("truncated patch hunk")
                line = lines[index]
                if line == "\\ No newline at end of file\n":
                    index += 1
                    continue
                if not line.endswith("\n") or line[:1] not in {" ", "+", "-"}:
                    raise ManagerError("invalid patch hunk body")
                old -= line[0] in " -"
                new -= line[0] in " +"
                if old < 0 or new < 0:
                    raise ManagerError("patch hunk count mismatch")
                index += 1
            if index < len(lines) and lines[index] == "\\ No newline at end of file\n":
                index += 1
        if not hunks:
            raise ManagerError("each file must contain a text hunk")
    return raw


def patch_tree(root: Path, parent: str, raw: bytes, index_path: Path) -> str:
    index_path.unlink(missing_ok=True)
    environment = dict(os.environ, GIT_INDEX_FILE=str(index_path))
    try:
        git(root, "read-tree", parent, env=environment)
        git(root, "apply", "--cached", "--check", "--whitespace=nowarn", "-", data=raw, env=environment)
        git(root, "apply", "--cached", "--whitespace=nowarn", "-", data=raw, env=environment)
        tree = git(root, "write-tree", env=environment).stdout.decode().strip()
        changes = git(root, "diff-tree", "--raw", "-z", "--no-renames", "-r", parent, tree).stdout.split(b"\0")
        for record in changes[0::2]:
            if not record:
                continue
            fields = record.split()
            if len(fields) != 5 or fields[0][1:] not in {b"000000", b"100644", b"100755"} or fields[1] not in {b"000000", b"100644", b"100755"}:
                raise ManagerError("patch changes a symlink, submodule, or unsupported file type")
        return tree
    finally:
        index_path.unlink(missing_ok=True)
