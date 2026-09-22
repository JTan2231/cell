"""Private candidate commits and Git-owned patch application."""

from __future__ import annotations

import os
from pathlib import Path
import re
import subprocess

from ci_manager.storage import ManagerError

ACCEPTED = "refs/ci/accepted"


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


def patch_tree(root: Path, parent: str, raw: bytes, index_path: Path) -> str:
    if not raw.endswith(b"\n"):
        raw += b"\n"
    index_path.unlink(missing_ok=True)
    environment = dict(os.environ, GIT_INDEX_FILE=str(index_path))
    try:
        git(root, "read-tree", parent, env=environment)
        git(root, "apply", "--cached", "--recount", "--whitespace=nowarn", "-", data=raw, env=environment)
        return git(root, "write-tree", env=environment).stdout.decode().strip()
    finally:
        index_path.unlink(missing_ok=True)
