"""Private candidate commits and Git-owned patch application."""

from __future__ import annotations

import os
from pathlib import Path
import re
import shutil
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


def _worktree_registration(root: Path, path: Path, directory: Path) -> Path:
    parent = common(root) / "worktrees"
    if (not directory.is_absolute() or directory.parent != parent
            or directory.resolve().parent != parent.resolve()
            or parent.is_symlink() or directory.is_symlink()):
        raise ManagerError("candidate registration is outside the repository's linked-worktree directory")
    marker = directory / "gitdir"
    if marker.is_symlink():
        raise ManagerError("symbolic candidate registration")
    if marker.is_file() and Path(marker.read_text().strip()).resolve() != path.resolve() / ".git":
        raise ManagerError("candidate registration belongs to another worktree")
    return directory


def worktree_registration(root: Path, path: Path) -> Path | None:
    """Find this tree's exact admin directory before deleting its files."""
    if path.is_symlink():
        raise ManagerError(f"symbolic candidate worktree: {path}")
    marker = path / ".git"
    if marker.is_symlink():
        raise ManagerError("symbolic candidate Git file")
    if marker.is_file():
        value = marker.read_text().strip()
        if not value.startswith("gitdir: "):
            raise ManagerError("invalid candidate Git file")
        directory = Path(value[len("gitdir: "):])
        if not directory.is_absolute():
            directory = (path / directory).absolute()
        return _worktree_registration(root, path, directory)
    parent = common(root) / "worktrees"
    if parent.is_symlink():
        raise ManagerError("symbolic linked-worktree directory")
    if parent.is_dir():
        for directory in parent.iterdir():
            marker = directory / "gitdir"
            if (not directory.is_symlink() and marker.is_file() and not marker.is_symlink()
                    and Path(marker.read_text().strip()).resolve() == path.resolve() / ".git"):
                return _worktree_registration(root, path, directory)
    return None


def remove_worktree(root: Path, path: Path, registration: Path | None = None) -> None:
    """Remove one finished job's files and linked-worktree registration."""
    if path.is_symlink():
        raise ManagerError(f"symbolic candidate worktree: {path}")
    target = path.resolve()
    records = git(root, "worktree", "list", "--porcelain", "-z").stdout.split(b"\0")
    paths = [Path(os.fsdecode(record[len(b"worktree "):])).resolve()
             for record in records if record.startswith(b"worktree ")]
    if paths and target == paths[0]:
        raise ManagerError("cannot remove the repository's main worktree")
    registration = (_worktree_registration(root, path, registration)
                    if registration is not None else worktree_registration(root, path))
    if path.exists():
        # Remove all private files, including dirty, ignored and nested Git
        # content. File-first removal also recovers a partially deleted .git.
        shutil.rmtree(path)
    if target in paths:
        # Git removes the absent tree's exact registration. Force twice also
        # clears a lock on this settled, manager-owned worktree.
        git(root, "worktree", "remove", "--force", "--force", str(path))
    if registration is not None and registration.exists():
        # Replay also clears partial admin deletion that Git no longer lists.
        shutil.rmtree(registration)


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
