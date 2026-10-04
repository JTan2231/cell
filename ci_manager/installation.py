"""Explicit installation of fixed-path CI-manager code and its paused user service."""

from __future__ import annotations

from contextlib import ExitStack, contextmanager
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
import time
import uuid

from ci_manager.storage import ManagerError, Store, home, lock, private_directory, state_root
from ci_manager.process import validation_exited
from ci_manager.integrations import IntegrationError, NucleusClient


LABEL = "dev.cell.ci-manager"
PROVIDER = "ci-manager"
FORMAT = 3
STOP_LOCK_TIMEOUT = 10.0


def program_root() -> Path:
    return home() / ".local/share/cell-ci"


def _paths() -> dict[str, Path]:
    programs = program_root()
    return {
        "programs": programs,
        "current": programs / "current",
        "runtime": programs / "runtime",
        "wrapper": home() / ".local/bin/cell-ci",
        "provider": home() / "Library/Application Support/Chancery/providers" / PROVIDER,
        "plist": home() / "Library/LaunchAgents" / f"{LABEL}.plist",
    }


def _json(value: object) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode("utf-8")


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


def _launcher(python: str, release: Path, *, stable: bool = False) -> bytes:
    bootstrap = (
        "import runpy,sys; "
        f"sys.path.insert(0,{str(release)!r}); "
        f"runpy.run_path({str(release / 'ci_manager/client.py')!r},run_name='__main__')"
    )
    description = "fixed runtime path" if stable else "pinned immutable release"
    return (
        f"#!/bin/sh\n# Cell CI manager: {description}.\n"
        f"exec {shlex.quote(python)} -I -B -c {shlex.quote(bootstrap)} \"$@\"\n"
    ).encode("utf-8")


def _verify_release(release: Path) -> dict:
    if release.is_symlink() or release.parent != program_root() / "releases":
        raise ManagerError("CI manager release is outside its owned release directory")
    if not re.fullmatch(r"(?:[0-9a-f]{32}|[0-9a-f]{64})", release.name):
        raise ManagerError("invalid CI manager release identity")
    try:
        manifest = json.loads((release / "manifest.json").read_bytes())
        recipe = manifest["recipe"]
        files = recipe["files"]
        version = recipe["schema_version"]
        if (version not in {1, 2, FORMAT} or recipe["product"] != "cell-ci"
                or recipe["program_root"] != str(program_root())
                or recipe["entry"] != "ci_manager/client.py"
                or len(release.name) != (64 if version == 1 else 32)
                or not isinstance(files, dict)
                or not Path(recipe["python"]).is_absolute()
                or (version == FORMAT and recipe.get("runtime_root") != str(program_root() / "runtime"))):
            raise ValueError("invalid release recipe")
        for relative, record in files.items():
            path = Path(relative)
            if (path.is_absolute() or ".." in path.parts or path.as_posix() != relative
                    or not isinstance(record, dict) or record.get("mode") not in {0o444, 0o555}):
                raise ValueError("invalid release inventory")
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
                    if stat.S_IMODE(path.stat().st_mode) != record["mode"]:
                        raise ValueError("changed release content")
        runtime = program_root() / "runtime"
        wrapper = _launcher(recipe["python"], runtime if version == FORMAT else release,
                            stable=version == FORMAT)
        if (observed != expected or (release / "bin/cell-ci").read_bytes() != wrapper
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
    if not re.fullmatch(r"releases/(?:[0-9a-f]{32}|[0-9a-f]{64})", target):
        raise ManagerError("foreign CI manager current selector")
    release = paths["programs"] / target
    return release


def _check_selector(path: Path, expected: Path, legacy: Path | None = None) -> None:
    if path.is_symlink():
        if os.readlink(path) not in {str(expected), str(legacy) if legacy is not None else str(expected)}:
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
        if (arguments != _launch_arguments(release)
                or (release != program_root() / "runtime"
                    and (release.parent != program_root() / "releases"
                         or not re.fullmatch(r"(?:[0-9a-f]{32}|[0-9a-f]{64})", release.name)))):
            raise ValueError("unexpected LaunchAgent command")
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


@contextmanager
def _worker_lock_after_stop(root: Path):
    deadline = time.monotonic() + STOP_LOCK_TIMEOUT
    with ExitStack() as stack:
        while True:
            try:
                stack.enter_context(lock(root / "worker.lock", blocking=False))
                break
            except BlockingIOError as error:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise ManagerError("CI manager worker lock did not drain within 10 seconds") from error
                time.sleep(min(0.05, remaining))
        yield


def _cancelled_validation_exited(store: Store, job: dict) -> bool:
    if (job["phase"] != "blocked" or job.get("stopped_phase") != "checking"
            or not job["cancel_requested"] or job.get("attempts") != []
            or job.get("model_unresolved") or job.get("accepted")
            or job.get("acceptance_intent") or job.get("deployment_request")
            or job.get("deployment_result") or store.get("recovery_request")):
        return False
    return validation_exited(store.root / "jobs" / job["id"], job)


def _prepared_repair_not_admitted(store: Store, job: dict) -> bool:
    if (job["phase"] != "blocked" or job.get("stopped_phase") != "repair_wait"
            or job.get("model_unresolved") is not False or job.get("accepted")
            or job.get("acceptance_intent") or job.get("deployment_request")
            or job.get("deployment_result") or store.get("recovery_request")):
        return False
    directory = store.root / "jobs" / job["id"]
    attempts = job.get("attempts")
    if not isinstance(attempts, list) or not attempts or not validation_exited(directory, job):
        return False
    attempt = attempts[-1]
    number = len(attempts)
    identity = f"ci-{job['id']}-repair-{number}"
    request_path = directory / f"repair-{number}.request.json"
    try:
        if (not isinstance(attempt, dict) or type(attempt.get("number")) is not int
                or attempt["number"] != number or attempt.get("nucleus_job_id") != identity
                or attempt.get("parent") != job["candidate_commit"]
                or attempt.get("request") != str(request_path)
                or attempt.get("state") not in {None, "not_admitted"}
                or attempt.get("terminal") or attempt.get("patch") or request_path.is_symlink()):
            return False
        request = json.loads(request_path.read_bytes())
        if (not isinstance(request, dict) or request.get("version") != 1
                or request.get("id") != identity
                or request.get("requester") != {"program": "ci-manager", "id": job["id"]}
                or request["invocation"]["cwd"] != str(directory / "worktree")
                or request["invocation"]["workspaceAccess"] != "read-only"):
            return False
        return NucleusClient().get(identity) is None
    except (OSError, ValueError, KeyError, TypeError, IntegrationError):
        return False


def _require_idle(store: Store, *, allow_install: bool = False) -> None:
    active = store.active()
    if (store.get("paused") is not True
            or (active is not None and not (allow_install
                                           and (_cancelled_validation_exited(store, active)
                                                or _prepared_repair_not_admitted(store, active))))):
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
        records[name] = {"mode": mode}
    # Host preparation uses pinned code from this manager release. Candidate
    # worktrees supply data and compiler inputs, never the signing implementation.
    shared_modules = (
        "deployment/__init__.py", "deployment/signing.py", "deployment/build.py",
        "deployment/candidate.py", "deployment/inventory.py", "ci_broker/__init__.py",
        "ci_broker/client.py", "ci_broker/broker.py",
    )
    for name in shared_modules:
        path = source.parent / name
        if path.is_symlink() or not path.is_file():
            raise ManagerError(f"CI manager host preparation module is missing or symbolic: {name}")
        content = path.read_bytes()
        mode = 0o555 if path.stat().st_mode & 0o111 else 0o444
        contents[name] = content
        records[name] = {"mode": mode}
    recipe = {
        "schema_version": FORMAT, "product": "cell-ci", "entry": "ci_manager/client.py",
        "program_root": str(program_root()), "python": str(python), "files": records,
        "runtime_root": str(program_root() / "runtime"),
    }
    identity = uuid.uuid4().hex
    releases = program_root() / "releases"
    private_directory(program_root())
    private_directory(releases)
    release = releases / identity
    if release.exists() or release.is_symlink():
        raise ManagerError("CI manager release identity already exists")
    stage = Path(tempfile.mkdtemp(prefix=".stage-", dir=releases))
    try:
        for name, content in contents.items():
            _write_file(stage / name, content, records[name]["mode"])
        wrapper = _launcher(str(python), program_root() / "runtime", stable=True)
        _write_file(stage / "bin/cell-ci", wrapper, 0o555)
        _write_file(stage / "manifest.json", _json({"recipe": recipe}), 0o444)
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
    return release


def _runtime_files(release: Path) -> dict[str, int]:
    recipe = _verify_release(release)["recipe"]
    return {**{name: record["mode"] for name, record in recipe["files"].items()},
            "manifest.json": 0o444, "bin/cell-ci": 0o555}


def _verify_runtime(release: Path, runtime: Path) -> None:
    expected = _runtime_files(release)
    observed = set()
    if runtime.is_symlink() or not runtime.is_dir() or runtime.stat().st_uid != os.getuid():
        raise ManagerError("foreign CI manager runtime directory")
    for path in runtime.rglob("*"):
        if path.is_symlink() or path.stat().st_uid != os.getuid():
            raise ManagerError("foreign CI manager runtime content")
        if path.is_file():
            name = path.relative_to(runtime).as_posix()
            observed.add(name)
            if name not in expected or stat.S_IMODE(path.stat().st_mode) != expected[name]:
                raise ManagerError("changed CI manager runtime inventory")
        elif not path.is_dir():
            raise ManagerError("foreign CI manager runtime content")
    recipe = _verify_release(release)["recipe"]
    try:
        manifest = json.loads((runtime / "manifest.json").read_bytes())
    except (OSError, ValueError) as error:
        raise ManagerError("changed CI manager runtime selection") from error
    if (observed != set(expected) or manifest != {"recipe": recipe}
            or (runtime / "bin/cell-ci").read_bytes() != _launcher(recipe["python"], runtime, stable=True)):
        raise ManagerError("changed CI manager runtime selection")


def _runtime_snapshot(runtime: Path, release: Path | None) -> dict[str, tuple[bytes, int]] | None:
    if not runtime.exists() and not runtime.is_symlink():
        return None
    if release is None:
        raise ManagerError("CI manager runtime has no owned release selection")
    _verify_runtime(release, runtime)
    return {name: ((runtime / name).read_bytes(), mode)
            for name, mode in _runtime_files(release).items()}


def _publish_runtime(release: Path, runtime: Path) -> None:
    files = _runtime_files(release)
    previous = {path.relative_to(runtime).as_posix() for path in runtime.rglob("*") if path.is_file()}
    _owned_directory(runtime)
    for name, mode in files.items():
        _write_file(runtime / name, (release / name).read_bytes(), mode)
    recipe = _verify_release(release)["recipe"]
    # Compatible prior archives carry launchers that name their archive path.
    _write_file(runtime / "bin/cell-ci", _launcher(recipe["python"], runtime, stable=True), 0o555)
    for name in previous - set(files):
        (runtime / name).unlink()
    _sync_directory(runtime)


def _restore_runtime(runtime: Path, snapshot: dict[str, tuple[bytes, int]] | None) -> None:
    if snapshot is None:
        if runtime.exists():
            shutil.rmtree(runtime)
        return
    for name, (contents, mode) in snapshot.items():
        _write_file(runtime / name, contents, mode)
    for path in runtime.rglob("*"):
        if path.is_file() and path.relative_to(runtime).as_posix() not in snapshot:
            path.unlink()
    _sync_directory(runtime)


def _verify_service_target(plist: bytes, release: Path | None, runtime: Path) -> None:
    target = Path(plistlib.loads(plist)["ProgramArguments"][0]).parent.parent
    if target == runtime:
        if release is None:
            raise ManagerError("CI manager runtime has no selected archive")
        _verify_runtime(release, runtime)
    else:
        _verify_release(target)


def _plist(release: Path) -> bytes:
    return plistlib.dumps({
        "Label": LABEL,
        "ProgramArguments": _launch_arguments(release),
        # launchd cannot open removable-volume output paths before spawning.
        # The worker validates storage and opens its own external logs.
        "WorkingDirectory": str(release),
        "RunAtLoad": True,
        "KeepAlive": True,
        "ThrottleInterval": 10,
        "Umask": 0o077,
        "EnvironmentVariables": {
            "HOME": str(home()),
            "PATH": f"{home() / '.local/bin'}:{home() / '.cargo/bin'}:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin",
        },
        "StandardOutPath": "/dev/null",
        "StandardErrorPath": "/dev/null",
    }, sort_keys=True)


def install() -> dict:
    """Replace paused code when active validation or model admission is proved absent."""
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
            _require_idle(store, allow_install=True)
            previous = _selected_release(paths)
            runtime = paths["runtime"]
            wrapper_target = runtime / "bin/cell-ci"
            provider_target = paths["current"] / "ci_manager/chancery"
            _check_selector(paths["wrapper"], wrapper_target, paths["current"] / "bin/cell-ci")
            _check_selector(paths["provider"], provider_target)
            old_plist = _check_plist(paths["plist"])
            old_wrapper = os.readlink(paths["wrapper"]) if paths["wrapper"].is_symlink() else None
            old_provider = os.readlink(paths["provider"]) if paths["provider"].is_symlink() else None
            was_loaded = _stop_if_loaded(old_plist)
            switched = False
            runtime_changed = False
            runtime_before = None
            try:
                with _worker_lock_after_stop(root):
                    _require_idle(store, allow_install=True)
                    runtime_before = _runtime_snapshot(runtime, previous)
                    release = _prepare_release(source, python)
                    runtime_changed = True
                    _publish_runtime(release, runtime)
                    _link(paths["current"], f"releases/{release.name}")
                    switched = True
                    _link(paths["wrapper"], str(wrapper_target))
                    _link(paths["provider"], str(provider_target))
                    _write_file(paths["plist"], _plist(runtime))
                _launchctl("bootstrap", f"gui/{os.getuid()}", str(paths["plist"]))
            except Exception as error:
                try:
                    if switched or runtime_changed:
                        _stop_if_loaded(_check_plist(paths["plist"]))
                        with _worker_lock_after_stop(root):
                            _restore_runtime(runtime, runtime_before)
                            if previous is None:
                                paths["current"].unlink(missing_ok=True)
                            else:
                                _link(paths["current"], f"releases/{previous.name}")
                            if old_wrapper is None:
                                paths["wrapper"].unlink(missing_ok=True)
                            else:
                                _link(paths["wrapper"], old_wrapper)
                            if old_provider is None:
                                paths["provider"].unlink(missing_ok=True)
                            else:
                                _link(paths["provider"], old_provider)
                            if old_plist is None:
                                paths["plist"].unlink(missing_ok=True)
                            else:
                                _write_file(paths["plist"], old_plist)
                    if was_loaded:
                        _verify_service_target(old_plist, previous, runtime)
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
    if release is not None:
        _verify_release(release)
    plist = _check_plist(paths["plist"])
    if plist is not None:
        _verify_service_target(plist, release, paths["runtime"])
    if action == "status":
        return {"installed": release is not None, "release": release.name if release else None,
                "loaded": _loaded(), "service": LABEL}
    if release is None or plist is None:
        raise ManagerError("CI manager service is not installed")
    with lock(state_root() / "admission.lock"):
        release = _selected_release(paths)
        if release is not None:
            _verify_release(release)
        plist = _check_plist(paths["plist"])
        if plist is not None:
            _verify_service_target(plist, release, paths["runtime"])
        if release is None or plist is None:
            raise ManagerError("CI manager service is not installed")
        store = Store(state_root())
        try:
            if action == "stop":
                _require_idle(store)
                _stop_if_loaded(plist)
                with _worker_lock_after_stop(state_root()):
                    _require_idle(store)
            elif not _loaded():
                arguments = plistlib.loads(plist)["ProgramArguments"]
                if arguments not in [_launch_arguments(release), _launch_arguments(paths["runtime"])]:
                    raise ManagerError("CI manager installation is interrupted; run install to recover")
                _launchctl("bootstrap", f"gui/{os.getuid()}", str(paths["plist"]))
            return {"installed": True, "release": release.name, "loaded": _loaded(), "service": LABEL}
        finally:
            store.db.close()
