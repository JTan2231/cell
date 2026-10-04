"""Local Git, tool environment, and process identity helpers for deployment."""
from __future__ import annotations

import ctypes
import os
from pathlib import Path
import pwd
import shutil
import struct
import subprocess
import sys

MINIMAL_ENVIRONMENT = (
    "AR",
    "CARGO_HOME",
    "CARGO_INCREMENTAL",
    "CARGO_NET_OFFLINE",
    "CARGO_TERM_COLOR",
    "CC",
    "COLORTERM",
    "CXX",
    "DEVELOPER_DIR",
    "FORCE_COLOR",
    "HOME",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "LOGNAME",
    "MACOSX_DEPLOYMENT_TARGET",
    "NO_COLOR",
    "PATH",
    "RUSTDOCFLAGS",
    "RUSTFLAGS",
    "RUSTUP_HOME",
    "RUSTUP_TOOLCHAIN",
    "SDKROOT",
    "SHELL",
    "SSL_CERT_DIR",
    "SSL_CERT_FILE",
    "TERM",
    "TMPDIR",
    "USER",
)


def bootstrap_cargo_path() -> None:
    """Match the product gates' existing rustup PATH bootstrap."""

    if shutil.which("cargo") is not None and shutil.which("rustc") is not None:
        return
    cargo_home = os.environ.get("CARGO_HOME")
    if cargo_home is None:
        home = os.environ.get("HOME")
        if home:
            cargo_home = str(Path(home) / ".cargo")
    candidates = [Path(cargo_home).expanduser() / "bin"] if cargo_home else []
    if pwd is not None:
        candidates.append(Path(pwd.getpwuid(os.getuid()).pw_dir) / ".cargo/bin")
    for cargo_bin in candidates:
        if not (cargo_bin / "cargo").is_file():
            continue
        existing = os.environ.get("PATH")
        os.environ["PATH"] = os.pathsep.join(
            part for part in (str(cargo_bin), existing) if part
        )
        return

def git(repository: Path, *arguments: str) -> bytes:
    try:
        result = subprocess.run(
            ("git", "-C", str(repository), *arguments),
            check=False,
            capture_output=True,
            timeout=30,
        )
    except (OSError, subprocess.SubprocessError) as error:
        raise RuntimeError(f"cannot inspect Git repository: {error}") from error
    if result.returncode != 0:
        message = result.stderr.decode("utf-8", "replace").strip()
        raise RuntimeError(message or "Git repository inspection failed")
    return result.stdout

def repository_root(path: Path) -> Path:
    raw = git(path, "rev-parse", "--show-toplevel").decode("utf-8", "strict").strip()
    root = Path(raw).resolve()
    if not root.is_dir():
        raise RuntimeError(f"Git worktree root does not exist: {root}")
    return root

def common_git_directory(root: Path) -> Path:
    try:
        raw = git(
            root, "rev-parse", "--path-format=absolute", "--git-common-dir"
        ).decode("utf-8", "strict").strip()
        common = Path(raw)
    except RuntimeError:
        raw = git(root, "rev-parse", "--git-common-dir").decode(
            "utf-8", "strict"
        ).strip()
        common = Path(raw)
        if not common.is_absolute():
            common = root / common
    common = common.resolve()
    if not common.is_dir():
        raise RuntimeError(f"Git common directory does not exist: {common}")
    return common

def source_commit(root: Path) -> str:
    return git(root, "rev-parse", "--verify", "HEAD^{commit}").decode("ascii").strip()

def repository_is_clean(root: Path) -> bool:
    return not git(
        root,
        "status",
        "--porcelain=v1",
        "-z",
        "--untracked-files=all",
        "--ignore-submodules=none",
    )


def process_token(pid: int) -> str | None:
    """Return a PID-reuse-resistant process start token when inspectable."""

    if pid <= 0:
        return None
    if sys.platform == "darwin":
        # proc_bsdinfo from the public macOS sys/proc_info.h ABI. Reading the
        # owning user's processes needs no setuid ps executable in a sandbox.
        library = ctypes.CDLL("/usr/lib/libproc.dylib")
        read = library.proc_pidinfo
        read.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_uint64,
                         ctypes.c_void_p, ctypes.c_int]
        read.restype = ctypes.c_int
        info = ctypes.create_string_buffer(136)
        if read(pid, 3, 0, info, len(info)) != len(info):
            return None
        seconds, microseconds = struct.unpack_from("=QQ", info.raw, 120)
        return f"proc:{seconds}:{microseconds}"
    stat_path = Path(f"/proc/{pid}/stat")
    if stat_path.exists():
        try:
            fields = stat_path.read_text(encoding="utf-8").split()
            return f"proc:{fields[21]}"
        except (OSError, IndexError):
            return None
    try:
        result = subprocess.run(
            ("ps", "-o", "lstart=", "-p", str(pid)),
            check=False,
            capture_output=True,
            text=True,
            timeout=2,
        )
    except (OSError, subprocess.SubprocessError):
        return None
    started = result.stdout.strip()
    if result.returncode != 0 or not started:
        return None
    return f"ps:{started}"
