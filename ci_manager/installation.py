"""Explicit installation of pinned CI-manager code and its paused user service."""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import shlex
import shutil
import stat
import subprocess
import sys
import tempfile
import uuid

from ci_manager.storage import ManagerError, Store, home, lock, private_directory, state_root


LABEL = "dev.cell.ci-manager"
PROVIDER = "ci-manager"
FORMAT = 1


def program_root() -> Path:
    return home() / ".local/share/cell-ci"


def _paths() -> dict[str, Path]:
    programs = program_root()
    return {
        "programs": programs,
        "current": programs / "current",
        "wrapper": home() / ".local/bin/cell-ci",
        "provider": home() / "Library/Application Support/Chancery/providers" / PROVIDER,
        "plist": home() / "Library/LaunchAgents" / f"{LABEL}.plist",
    }


def _json(value: object) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode("utf-8")


def _digest(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def _owned_directory(path: Path) -> None:
    if path.is_symlink():
        raise ManagerError(f"symbolic installation directory: {path}")
    path.mkdir(parents=True, exist_ok=True, mode=0o700)
    if not path.is_dir() or path.stat().st_uid != os.getuid():
        raise ManagerError(f"installation directory is not user-owned: {path}")


def _sync_directory(path: Path) -> None:
    descriptor = os.open(path, os.O_RDONLY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def _write_file(path: Path, contents: bytes, mode: int = 0o600) -> None:
    _owned_directory(path.parent)
    if path.is_symlink():
        raise ManagerError(f"symbolic installation file: {path}")
    temporary = path.with_name(f".{path.name}.{uuid.uuid4().hex}")
    try:
        with temporary.open("xb") as stream:
            os.chmod(temporary, mode)
            stream.write(contents)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
        _sync_directory(path.parent)
    finally:
        temporary.unlink(missing_ok=True)


def _link(path: Path, target: str) -> None:
    _owned_directory(path.parent)
    temporary = path.with_name(f".{path.name}.{uuid.uuid4().hex}")
    try:
        temporary.symlink_to(target)
        os.replace(temporary, path)
        _sync_directory(path.parent)
    finally:
        temporary.unlink(missing_ok=True)


def _launcher(python: str, release: Path) -> bytes:
    bootstrap = (
        "import runpy,sys; "
        f"sys.path.insert(0,{str(release)!r}); "
        f"runpy.run_path({str(release / 'ci_manager/client.py')!r},run_name='__main__')"
    )
    return (
        "#!/bin/sh\n# Cell CI manager: pinned immutable release.\n"
        f"exec {shlex.quote(python)} -I -B -c {shlex.quote(bootstrap)} \"$@\"\n"
    ).encode("utf-8")


def _verify_release(release: Path) -> dict:
    if release.is_symlink() or release.parent != program_root() / "releases":
        raise ManagerError("CI manager release is outside its owned release directory")
    if not re.fullmatch(r"[0-9a-f]{64}", release.name):
        raise ManagerError("invalid CI manager release identity")
    try:
        manifest = json.loads((release / "manifest.json").read_bytes())
        recipe = manifest["recipe"]
        files = recipe["files"]
        if (recipe["schema_version"] != FORMAT or recipe["product"] != "cell-ci"
                or recipe["program_root"] != str(program_root())
                or recipe["entry"] != "ci_manager/client.py"
                or _digest(_json(recipe)) != release.name or not isinstance(files, dict)
                or not Path(recipe["python"]).is_absolute()):
            raise ValueError("invalid release recipe")
        expected = set(files) | {"manifest.json", "bin/cell-ci"}
        observed = set()
        for path in release.rglob("*"):
            if path.is_symlink():
                raise ValueError("symbolic release content")
            if path.is_file():
                relative = path.relative_to(release).as_posix()
                observed.add(relative)
                if path.stat().st_uid != os.getuid():
                    raise ValueError("foreign release content")
                if relative in files:
                    record = files[relative]
                    if (_digest(path.read_bytes()) != record["sha256"]
                            or stat.S_IMODE(path.stat().st_mode) != record["mode"]):
                        raise ValueError("changed release content")
        wrapper = _launcher(recipe["python"], release)
        if (observed != expected or (release / "bin/cell-ci").read_bytes() != wrapper
                or manifest["wrapper_sha256"] != _digest(wrapper)
                or stat.S_IMODE((release / "bin/cell-ci").stat().st_mode) != 0o555):
            raise ValueError("changed release inventory")
        return manifest
    except (OSError, ValueError, KeyError, TypeError) as error:
        raise ManagerError(f"CI manager release integrity failed: {release.name}") from error


def _selected_release(paths: dict[str, Path]) -> Path | None:
    current = paths["current"]
    if not current.exists() and not current.is_symlink():
        return None
    if not current.is_symlink():
        raise ManagerError("foreign CI manager current selector")
    target = os.readlink(current)
    if not re.fullmatch(r"releases/[0-9a-f]{64}", target):
        raise ManagerError("foreign CI manager current selector")
    release = paths["programs"] / target
    _verify_release(release)
    return release


def _check_selector(path: Path, expected: Path) -> None:
    if path.is_symlink():
        if os.readlink(path) != str(expected):
            raise ManagerError(f"foreign CI manager selector: {path}")
    elif path.exists():
        raise ManagerError(f"foreign CI manager selector: {path}")


def _launch_arguments(release: Path) -> list[str]:
    return [str(release / "bin/cell-ci"), "worker"]


def _check_plist(path: Path) -> bytes | None:
    if not path.exists() and not path.is_symlink():
        return None
    if path.is_symlink() or not path.is_file() or path.stat().st_uid != os.getuid():
        raise ManagerError("foreign CI manager LaunchAgent")
    raw = path.read_bytes()
    try:
        value = plistlib.loads(raw)
        arguments = value["ProgramArguments"]
        if value["Label"] != LABEL or not isinstance(arguments, list) or len(arguments) != 2:
            raise ValueError("unexpected LaunchAgent")
        wrapper = Path(arguments[0])
        release = wrapper.parent.parent
        if arguments != _launch_arguments(release):
            raise ValueError("unexpected LaunchAgent command")
        _verify_release(release)
    except (ValueError, TypeError, KeyError, plistlib.InvalidFileException) as error:
        raise ManagerError("foreign CI manager LaunchAgent") from error
    return raw


def _launchctl(*arguments: str, check: bool = True) -> subprocess.CompletedProcess:
    try:
        result = subprocess.run(["/bin/launchctl", *arguments], stdin=subprocess.DEVNULL,
                                capture_output=True, timeout=30, check=False)
    except (OSError, subprocess.TimeoutExpired) as error:
        raise ManagerError("CI manager launchctl operation did not complete") from error
    if check and result.returncode:
        diagnostic = (result.stderr or result.stdout).decode("utf-8", "replace")[:4096].strip()
        raise ManagerError(f"CI manager launchctl operation failed: {diagnostic}")
    return result


def _loaded() -> bool:
    return _launchctl("print", f"gui/{os.getuid()}/{LABEL}", check=False).returncode == 0


def _stop_if_loaded(owned_plist: bytes | None) -> bool:
    loaded = _loaded()
    if loaded:
        if owned_plist is None:
            raise ManagerError("loaded CI manager service has no owned LaunchAgent")
        _launchctl("bootout", f"gui/{os.getuid()}/{LABEL}")
    return loaded


def _require_idle(store: Store) -> None:
    if store.get("paused") is not True or store.active() is not None:
        raise ManagerError("pause CI admission and let the active job finish before service changes")


def _prepare_release(source: Path, python: Path) -> Path:
    if not (source / "client.py").is_file() or not (source / "chancery/provider.json").is_file():
        raise ManagerError("CI manager source package or provider bundle is incomplete")
    contents = {}
    records = {}
    for path in sorted(source.rglob("*")):
        relative = path.relative_to(source)
        if "__pycache__" in relative.parts or path.suffix in {".pyc", ".pyo"}:
            continue
        if path.is_symlink():
            raise ManagerError(f"symbolic CI manager package input: {relative}")
        if not path.is_file():
            continue
        name = (Path("ci_manager") / relative).as_posix()
        content = path.read_bytes()
        mode = 0o555 if path.stat().st_mode & 0o111 else 0o444
        contents[name] = content
        records[name] = {"sha256": _digest(content), "mode": mode}
    recipe = {
        "schema_version": FORMAT, "product": "cell-ci", "entry": "ci_manager/client.py",
        "program_root": str(program_root()), "python": str(python), "files": records,
    }
    identity = _digest(_json(recipe))
    releases = program_root() / "releases"
    private_directory(program_root())
    private_directory(releases)
    release = releases / identity
    if release.exists() or release.is_symlink():
        _verify_release(release)
        return release
    stage = Path(tempfile.mkdtemp(prefix=".stage-", dir=releases))
    try:
        for name, content in contents.items():
            _write_file(stage / name, content, records[name]["mode"])
        wrapper = _launcher(str(python), release)
        _write_file(stage / "bin/cell-ci", wrapper, 0o555)
        _write_file(stage / "manifest.json",
                    _json({"recipe": recipe, "wrapper_sha256": _digest(wrapper)}), 0o444)
        for directory in sorted((path for path in stage.rglob("*") if path.is_dir()), reverse=True):
            os.chmod(directory, 0o555)
        os.chmod(stage, 0o555)
        os.rename(stage, release)
        _sync_directory(releases)
    finally:
        if stage.exists():
            for directory in [stage, *(path for path in stage.rglob("*") if path.is_dir())]:
                os.chmod(directory, 0o700)
            shutil.rmtree(stage)
    _verify_release(release)
    return release


def _plist(release: Path, root: Path) -> bytes:
    logs = root / "logs"
    private_directory(logs)
    return plistlib.dumps({
        "Label": LABEL,
        "ProgramArguments": _launch_arguments(release),
        "WorkingDirectory": str(root),
        "RunAtLoad": True,
        "KeepAlive": True,
        "ThrottleInterval": 10,
        "Umask": 0o077,
        "EnvironmentVariables": {
            "HOME": str(home()),
            "PATH": f"{home() / '.local/bin'}:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin",
        },
        "StandardOutPath": str(logs / "worker.stdout.log"),
        "StandardErrorPath": str(logs / "worker.stderr.log"),
    }, sort_keys=True)


def install() -> dict:
    """Install or replace code only while the initialized queue is paused and idle."""
    if sys.platform != "darwin":
        raise ManagerError("CI manager service installation requires macOS launchd")
    paths = _paths()
    root = state_root()
    source = Path(__file__).resolve().parent
    python = Path(sys.executable).resolve()
    if not python.is_file() or not os.access(python, os.X_OK):
        raise ManagerError("CI manager installation needs an absolute executable Python interpreter")
    with lock(root / "admission.lock"):
        store = Store(root)
        try:
            _require_idle(store)
            previous = _selected_release(paths)
            wrapper_target = paths["current"] / "bin/cell-ci"
            provider_target = paths["current"] / "ci_manager/chancery"
            _check_selector(paths["wrapper"], wrapper_target)
            _check_selector(paths["provider"], provider_target)
            old_plist = _check_plist(paths["plist"])
            had_wrapper = paths["wrapper"].is_symlink()
            had_provider = paths["provider"].is_symlink()
            was_loaded = _stop_if_loaded(old_plist)
            switched = False
            try:
                with lock(root / "worker.lock", blocking=False):
                    _require_idle(store)
                    release = _prepare_release(source, python)
                    _link(paths["current"], f"releases/{release.name}")
                    switched = True
                    _link(paths["wrapper"], str(wrapper_target))
                    _link(paths["provider"], str(provider_target))
                    _write_file(paths["plist"], _plist(release, root))
                _launchctl("bootstrap", f"gui/{os.getuid()}", str(paths["plist"]))
            except Exception as error:
                try:
                    if switched:
                        _stop_if_loaded(_check_plist(paths["plist"]))
                        with lock(root / "worker.lock", blocking=False):
                            if previous is None:
                                paths["current"].unlink(missing_ok=True)
                            else:
                                _link(paths["current"], f"releases/{previous.name}")
                            if not had_wrapper:
                                paths["wrapper"].unlink(missing_ok=True)
                            if not had_provider:
                                paths["provider"].unlink(missing_ok=True)
                            if old_plist is None:
                                paths["plist"].unlink(missing_ok=True)
                            else:
                                _write_file(paths["plist"], old_plist)
                    if was_loaded:
                        _launchctl("bootstrap", f"gui/{os.getuid()}", str(paths["plist"]))
                except Exception as recovery:
                    raise ManagerError(
                        f"CI manager installation failed; paused installation needs recovery: {recovery}"
                    ) from error
                raise ManagerError(f"CI manager installation failed: {error}") from error
            return {"installed": True, "release": release.name, "paused": True,
                    "wrapper": str(paths["wrapper"]), "service": LABEL}
        finally:
            store.db.close()


def service(action: str) -> dict:
    """Operate only the owned service; stopping requires paused, idle admission."""
    if action not in {"start", "stop", "status"}:
        raise ManagerError("service action must be start, stop, or status")
    if sys.platform != "darwin":
        raise ManagerError("CI manager service operations require macOS launchd")
    paths = _paths()
    release = _selected_release(paths)
    plist = _check_plist(paths["plist"])
    if action == "status":
        return {"installed": release is not None, "release": release.name if release else None,
                "loaded": _loaded(), "service": LABEL}
    if release is None or plist is None:
        raise ManagerError("CI manager service is not installed")
    with lock(state_root() / "admission.lock"):
        release = _selected_release(paths)
        plist = _check_plist(paths["plist"])
        if release is None or plist is None:
            raise ManagerError("CI manager service is not installed")
        store = Store(state_root())
        try:
            if action == "stop":
                _require_idle(store)
                _stop_if_loaded(plist)
                with lock(state_root() / "worker.lock", blocking=False):
                    _require_idle(store)
            elif not _loaded():
                if plistlib.loads(plist)["ProgramArguments"] != _launch_arguments(release):
                    raise ManagerError("CI manager installation is interrupted; run install to recover")
                _launchctl("bootstrap", f"gui/{os.getuid()}", str(paths["plist"]))
            return {"installed": True, "release": release.name, "loaded": _loaded(), "service": LABEL}
        finally:
            store.db.close()
