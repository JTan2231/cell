"""One configured external volume for Cell's generated working material."""

from __future__ import annotations

import json
import os
from pathlib import Path
import plistlib
import pwd
import shutil
import shlex
import stat
import subprocess
import tempfile
import sys


class WorkspaceError(ValueError):
    pass


def config_path() -> Path:
    return Path(pwd.getpwuid(os.getuid()).pw_dir) / "Library/Application Support/Cell/workspace.json"


def volume_info(volume: Path) -> dict:
    if not volume.is_absolute() or volume.is_symlink() or not volume.is_mount():
        raise WorkspaceError(f"external work volume is not mounted: {volume}")
    result = subprocess.run(["/usr/sbin/diskutil", "info", "-plist", str(volume)],
                            check=True, capture_output=True, timeout=15)
    info = plistlib.loads(result.stdout)
    if (info.get("MountPoint") != str(volume) or info.get("Internal") is not False
            or info.get("FilesystemType") != "apfs" or not info.get("WritableVolume")
            or not info.get("GlobalPermissionsEnabled") or not info.get("VolumeUUID")):
        raise WorkspaceError("work storage requires a writable external APFS volume with ownership enabled")
    return info


def private_directory(path: Path) -> Path:
    if path.is_symlink():
        raise WorkspaceError(f"symbolic work directory: {path}")
    path.mkdir(mode=0o700, parents=True, exist_ok=True)
    info = path.stat()
    if info.st_uid != os.getuid() or stat.S_IMODE(info.st_mode) != 0o700:
        raise WorkspaceError(f"work directory must be user-owned and mode 0700: {path}")
    return path


def configure(volume: Path) -> dict:
    """Select once after the old infrastructure has been stopped and drained."""
    volume = volume.absolute()
    info = volume_info(volume)
    value = {"schema_version": 1, "volume": str(volume),
             "volume_uuid": info["VolumeUUID"], "directory": "cell"}
    path = config_path()
    if path.exists() or path.is_symlink():
        if path.is_symlink() or json.loads(path.read_text()) != value:
            raise WorkspaceError("work storage is already configured; do not create a second queue scope")
        private_directory(volume / "cell")
        return value
    # A destination must never bypass an existing delivery or deployment.
    from contextlib import ExitStack
    from ci_manager.storage import Store, lock
    from ci_manager.installation import _loaded
    if _loaded():
        raise WorkspaceError("stop the existing CI manager service before selecting storage")
    legacy = path.parent
    with ExitStack() as locks:
        manager = legacy / "ci-manager"
        if (manager / "queue.sqlite3").exists():
            locks.enter_context(lock(manager / "admission.lock", blocking=False))
            locks.enter_context(lock(manager / "worker.lock", blocking=False))
            store = Store(manager)
            try:
                if (not store.get("paused") or store.active() or store.owners()
                        or store.db.execute("SELECT 1 FROM jobs WHERE phase='queued'").fetchone()):
                    raise WorkspaceError("pause, drain and stop the existing CI manager before selecting storage")
            finally:
                store.db.close()
        deployments = legacy / "deployments"
        if deployments.exists():
            locks.enter_context(lock(deployments / "deployment.lock", blocking=False))
            if (deployments / "active").exists():
                raise WorkspaceError("resolve the existing deployment before selecting storage")
        return _save_configuration(path, value, volume)


def _save_configuration(path: Path, value: dict, volume: Path) -> dict:
    private_directory(volume / "cell")
    # This small installation setting is the only local work-storage file.
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    with path.open("x", encoding="utf-8") as stream:
        os.chmod(path, 0o600)
        json.dump(value, stream, sort_keys=True, indent=2)
        stream.write("\n")
        stream.flush()
        os.fsync(stream.fileno())
    return value


def root() -> Path:
    path = config_path()
    try:
        if path.is_symlink():
            raise WorkspaceError("symbolic work-storage configuration")
        value = json.loads(path.read_text())
        if value["schema_version"] != 1 or value["directory"] != "cell":
            raise WorkspaceError("unsupported work-storage configuration")
        volume = Path(value["volume"])
        info = volume_info(volume)
        if info["VolumeUUID"] != value["volume_uuid"]:
            raise WorkspaceError("the mounted work volume has the wrong identity")
        result = volume / "cell"
        if result.is_symlink() or not result.is_dir() or result.stat().st_dev != volume.stat().st_dev:
            raise WorkspaceError("external workspace is missing or replaced; restore it before continuing")
        return result
    except FileNotFoundError as error:
        raise WorkspaceError("external work storage is unavailable; configure it with cell-ci storage configure --volume PATH") from error


def directory(name: str) -> Path:
    base = root()
    path = base / name
    if not path.resolve().is_relative_to(base) or path == base:
        raise WorkspaceError("work directory escapes the external workspace")
    return private_directory(path)


def require_path(path: Path) -> Path:
    base = root()
    if not path.is_absolute() or not path.resolve().is_relative_to(base):
        raise WorkspaceError(f"generated output must be inside {base}: {path}")
    return path


def environment() -> dict[str, str]:
    temporary = directory("tmp")
    cargo = directory("cargo")
    cache = directory("cache")
    return {
        "TMPDIR": str(temporary), "TMP": str(temporary), "TEMP": str(temporary),
        "CARGO_HOME": str(cargo), "XDG_CACHE_HOME": str(cache),
        "CLANG_MODULE_CACHE_PATH": str(directory("cache/clang")),
        "SWIFT_MODULECACHE_PATH": str(directory("cache/swift")),
        "PYTHONDONTWRITEBYTECODE": "1", "PYTHONPYCACHEPREFIX": str(directory("cache/python")),
        "GIT_OPTIONAL_LOCKS": "0",
    }


def activate() -> None:
    os.environ.update(environment())
    tempfile.tempdir = None


def require_capacity() -> None:
    if shutil.disk_usage(root()).free < 2 * 1024**3:
        raise WorkspaceError("external work volume has less than 2 GiB free; no new work can start")


def confined_command(command: list[str]) -> list[str]:
    """Deny host writes even when a child ignores TMPDIR or a cache setting."""
    base = root()
    sandbox = Path("/usr/bin/sandbox-exec")
    if not sandbox.is_file():
        raise WorkspaceError("Cell external builds require the macOS filesystem sandbox")
    profile = ('(version 1) (allow default) (deny file-write*) '
               f'(allow file-write* (subpath {json.dumps(str(base))}) '
               '(literal "/dev/null") (literal "/dev/tty"))')
    return [str(sandbox), "-p", profile, *command]


if __name__ == "__main__":
    try:
        if sys.argv[1:] != ["environment"]:
            raise WorkspaceError("usage: workspace.py environment")
        require_capacity()
        for key, value in environment().items():
            print(f"export {key}={shlex.quote(value)}")
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        print(f"cell-storage: {error}", file=sys.stderr)
        raise SystemExit(78) from error
