#!/usr/bin/env python3
"""Build opaque artifacts and execute explicitly ordered deployment instructions."""

from __future__ import annotations

import argparse
import contextlib
import datetime as dt
import fcntl
import hashlib
import json
import os
from pathlib import Path
import pwd
import re
import shutil
import stat
import subprocess
import sys
import time
from typing import Any, Iterator, Sequence
import uuid

sys.dont_write_bytecode = True
if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from ci_broker import client as ci_client
from ci_broker.broker import MINIMAL_ENVIRONMENT, process_token
from ci_manager import workspace
from deployment import candidate, manifest
from deployment.inventory import descriptor

SCHEMA = 2
MANIFEST_EXECUTOR = 1
NAME = re.compile(r"[a-z][a-z0-9-]*")
REQUEST_ID = re.compile(r"[A-Za-z0-9][A-Za-z0-9_.:/-]{0,255}")
COMMIT_ID = re.compile(r"(?:[0-9a-f]{40}|[0-9a-f]{64})")
MAX_DETAIL = 4096
HEARTBEAT_SECONDS = 60


class DeploymentError(RuntimeError):
    """The requested execution could not complete."""


class DeploymentBusy(DeploymentError):
    """Another execution or its child owns host admission."""


class ExecutionInterrupted(DeploymentError):
    """The command exceeded its declared limit; application effects are unknown."""


def now() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat()


def json_bytes(value: Any) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode()


def bounded_text(value: Any, limit: int = MAX_DETAIL) -> str:
    return str(value).encode("utf-8", "replace")[-limit:].decode("utf-8", "replace")


def sync_directory(path: Path) -> None:
    descriptor_fd = os.open(path, os.O_RDONLY)
    try:
        os.fsync(descriptor_fd)
    finally:
        os.close(descriptor_fd)


def private_directory(path: Path) -> None:
    if path.is_symlink():
        raise DeploymentError("executor state directory must not be symbolic")
    missing = []
    parent = path
    while not parent.exists():
        missing.append(parent)
        parent = parent.parent
    path.mkdir(mode=0o700, parents=True, exist_ok=True)
    info = path.stat()
    if info.st_uid != os.getuid() or stat.S_IMODE(info.st_mode) != 0o700:
        raise DeploymentError("executor state directory must be private and owned")
    for directory in reversed(missing):
        sync_directory(directory.parent)


def durable_json(path: Path, value: Any) -> None:
    temporary = path.with_name(f".{path.name}.{uuid.uuid4().hex}")
    try:
        descriptor_fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor_fd, "wb") as stream:
            stream.write(json_bytes(value))
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
        sync_directory(path.parent)
    finally:
        temporary.unlink(missing_ok=True)


def read_json(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text())
    if not isinstance(value, dict):
        raise DeploymentError("JSON input must be an object")
    return value


def state_root() -> Path:
    return workspace.directory("deployments")


def runtime_environment() -> dict[str, str]:
    environment = {name: os.environ[name] for name in MINIMAL_ENVIRONMENT if name in os.environ}
    for name in ("CELL_RELEASE_CACHE_DIR", "CELL_RELEASE_BUILD_JOBS"):
        if name in os.environ:
            environment[name] = os.environ[name]
    environment["HOME"] = pwd.getpwuid(os.getuid()).pw_dir
    environment["PYTHONDONTWRITEBYTECODE"] = "1"
    environment["PYTHONUNBUFFERED"] = "1"
    environment.update(workspace.environment())
    return environment


@contextlib.contextmanager
def deployment_lock(storage: Path) -> Iterator[int]:
    private_directory(storage)
    descriptor_fd = os.open(storage / "deployment.lock", os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    try:
        info = os.fstat(descriptor_fd)
        if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or info.st_nlink != 1:
            raise DeploymentError("executor lock must be an owned regular file")
        try:
            fcntl.flock(descriptor_fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            raise DeploymentBusy("another execution or its child owns the deployment lock") from error
        yield descriptor_fd
    finally:
        os.close(descriptor_fd)


def deployment_busy(storage: Path) -> bool:
    try:
        with deployment_lock(storage):
            return False
    except DeploymentBusy:
        return True


def git(root: Path, *arguments: str) -> str:
    return ci_client.git(root, *arguments).decode().strip()


def source_commit(root: Path, selected: str | None = None) -> str:
    if selected is not None and not COMMIT_ID.fullmatch(selected):
        raise DeploymentError("source commit must be a complete lowercase commit ID")
    revision = git(root, "rev-parse", "--verify", f"{selected or 'refs/heads/main'}^{{commit}}")
    if selected is not None and revision != selected:
        raise DeploymentError("source commit does not identify that exact commit")
    return revision


def catalog(root: Path, revision: str) -> dict[str, dict[str, Any]]:
    result = {}
    for path in git(root, "ls-tree", "--name-only", revision, "pipeline/products/").splitlines():
        if not path.endswith(".sh"):
            continue
        values = descriptor(git(root, "show", f"{revision}:{path}"))
        identity = values["PRODUCT_ID"]
        product = "krisis" if identity == "decisions" else identity
        directory = values["PRODUCT_DIR"]
        if not NAME.fullmatch(product) or not NAME.fullmatch(directory):
            raise DeploymentError("invalid product identity in build inventory")
        declaration = json.loads(git(root, "show", f"{revision}:{directory}/deployment/manifest.json"))
        manifest.declaration(declaration, product)
        result[product] = {"directory": directory, "aliases": values.get("PRODUCT_ALIASES", "").split(),
                           "manifest": declaration}
    if not result:
        raise DeploymentError("selected source contains no deployment declarations")
    return result


def requested_products(inventory: dict[str, dict[str, Any]], requested: Sequence[str]) -> list[str]:
    aliases = {}
    for product, item in inventory.items():
        for alias in [product, *item["aliases"]]:
            if alias in aliases and aliases[alias] != product:
                raise DeploymentError("ambiguous product alias")
            aliases[alias] = product
    aliases["annals-usage"] = "annals"
    selected = set()
    for requested_name in requested:
        name = aliases.get(requested_name.lower())
        if name is None:
            raise DeploymentError(f"unknown product: {requested_name}")
        selected.add(name)
    if not selected:
        raise DeploymentError("select at least one product")
    # These are literal caller-owned ranks, not inferred dependency edges.
    return sorted(selected, key=lambda name: (inventory[name]["manifest"]["order"], name))


def plan(root: Path, requested: Sequence[str], selected_commit: str | None = None) -> dict[str, Any]:
    revision = source_commit(root, selected_commit)
    inventory = catalog(root, revision)
    selected = requested_products(inventory, requested)
    return {"schema": SCHEMA, "manifest_executor": MANIFEST_EXECUTOR, "source_commit": revision,
            "products": selected, "instructions": [{"product": name, **step}
                for name in selected for step in inventory[name]["manifest"]["steps"]],
            "publication": "none"}


def setup_settings(path: Path | None) -> dict[str, Any]:
    return read_json(path) if path is not None else {}


def operation_path(storage: Path, request_id: str) -> Path:
    if not REQUEST_ID.fullmatch(request_id):
        raise DeploymentError("request ID must be 1-256 supported ASCII characters")
    return storage / "operations" / (hashlib.sha256(request_id.encode()).hexdigest() + ".json")


def read_operation(storage: Path, request_id: str) -> dict[str, Any] | None:
    path = operation_path(storage, request_id)
    if not path.exists():
        return None
    record = read_json(path)
    if record.get("request_id") != request_id or record.get("schema") not in (1, SCHEMA):
        raise DeploymentError("retained execution identity is invalid")
    return record


def save_operation(storage: Path, record: dict[str, Any]) -> None:
    private_directory(storage)
    private_directory(storage / "operations")
    record["updated_at"] = now()
    durable_json(operation_path(storage, record["request_id"]), record)


def result_from_record(record: dict[str, Any]) -> dict[str, Any]:
    fields = ("schema", "manifest_executor", "request_id", "run_id", "source_commit", "products",
              "state", "detail", "steps", "build", "prepared_build", "signing_policy", "started_at",
              "finished_at", "elapsed_seconds", "run_dir", "acknowledged_at", "workspace_cleanup")
    result = {key: record[key] for key in fields if key in record}
    for key in ("prepared_build", "signing_policy"):
        if key in record.get("request", {}):
            result[key] = record["request"][key]
    result["exit_code"] = 0 if record.get("state") == "succeeded" else 1
    result["operation_state"] = record["phase"]
    return result


def operation_status(storage: Path, request_id: str, *, running: bool | None = None) -> dict[str, Any]:
    record = read_operation(storage, request_id)
    if record is None:
        return {"schema": SCHEMA, "manifest_executor": MANIFEST_EXECUTOR, "request_id": request_id,
                "state": "not_found", "operation_state": "not_found", "exit_code": 0}
    if record["schema"] == 1:
        if record.get("phase") == "terminal":
            return {**record["result"], "operation_state": "terminal"}
        return {"schema": 1, "request_id": request_id, "state": "interrupted",
                "operation_state": "needs_reconciliation", "exit_code": 1,
                "detail": "Legacy execution is unsupported; inspect its retained effects and use explicit product procedures."}
    if record["phase"] == "active":
        active = marker(storage)
        owns_marker = active is not None and active.get("run_id") == record["run_id"]
        live = owns_marker and (deployment_busy(storage) if running is None else running)
        if not live:
            result = result_from_record(record)
            result.update(state="interrupted", operation_state="interrupted", exit_code=1)
            return result
    return result_from_record(record)


def marker(storage: Path) -> dict[str, Any] | None:
    path = storage / "active" / "run.json"
    if path.exists():
        return read_json(path)
    if (storage / "active").exists():
        raise DeploymentError("active executor marker has no retained identity")
    return None


def clear_marker(storage: Path, record: dict[str, Any]) -> None:
    active = marker(storage)
    if active is not None:
        if active.get("run_id") != record["run_id"]:
            raise DeploymentError("another run owns the active executor marker")
        shutil.rmtree(storage / "active")
        sync_directory(storage)


def canonical_request(root: Path, products: Sequence[str], selected_commit: str | None,
                      settings: dict[str, Any] | None, signing_policy: dict[str, Any] | None = None,
                      prepared_build: Path | None = None,
                      prepared_build_snapshot: dict[str, Any] | None = None) -> dict[str, Any]:
    chosen = plan(root, products, selected_commit)
    request = {"source_commit": chosen["source_commit"], "products": chosen["products"],
               "repository": str(ci_client.common_git_directory(root)), "settings": settings or {},
               "signing_policy": signing_policy}
    if prepared_build is not None:
        snapshot = read_json(prepared_build)
        if prepared_build_snapshot is not None and snapshot != prepared_build_snapshot:
            raise DeploymentError("prepared build differs from the caller's recorded input")
        request["prepared_build"] = {"path": str(prepared_build), "snapshot": snapshot}
    elif prepared_build_snapshot is not None:
        raise DeploymentError("prepared build snapshot requires its result path")
    return request


def matching_request(old: dict[str, Any], new: dict[str, Any], *, explicit_policy: bool) -> bool:
    if explicit_policy:
        return old == new
    return {key: value for key, value in old.items() if key != "signing_policy"} == {
        key: value for key, value in new.items() if key != "signing_policy"}


class Run:
    def __init__(self, storage: Path, record: dict[str, Any], lock_fd: int):
        self.storage, self.record, self.lock_fd = storage, record, lock_fd
        self.path = Path(record["run_dir"])
        self.source = self.path / "worktree"
        self.environment = runtime_environment()
        self.started = time.monotonic()

    def save(self) -> None:
        save_operation(self.storage, self.record)

    def command(self, step: dict[str, Any], argv: Sequence[str], *, stdin: Any = None,
                cwd: Path | None = None, environment: dict[str, str] | None = None,
                timeout_seconds: float | None = None) -> None:
        index = len(self.record["steps"])
        output_path, error_path = self.path / f"{index:04d}.stdout", self.path / f"{index:04d}.stderr"
        step.update(state="running", started_at=now(), stdout=str(output_path), stderr=str(error_path))
        self.record["steps"].append(step)
        input_path = self.path / f"{index:04d}.stdin"
        if stdin is not None:
            input_path.write_bytes(stdin.encode() if isinstance(stdin, str) else json_bytes(stdin))
        read_fd, write_fd = os.pipe()
        process = None
        started = time.monotonic()
        try:
            with contextlib.ExitStack() as stack:
                incoming = stack.enter_context(input_path.open("rb")) if stdin is not None else subprocess.DEVNULL
                output = stack.enter_context(output_path.open("xb"))
                errors = stack.enter_context(error_path.open("xb"))
                gate = [sys.executable, str(Path(__file__).resolve()), "_exec", str(read_fd),
                        str(self.lock_fd), *argv]
                process = subprocess.Popen(gate, stdin=incoming, stdout=output, stderr=errors,
                    cwd=cwd or self.source, env=environment or self.environment, pass_fds=(read_fd, self.lock_fd))
                os.close(read_fd)
                read_fd = -1
                birth = process_token(process.pid)
                if birth is None:
                    raise DeploymentError("cannot establish child identity before execution")
                step.update(pid=process.pid, birth=birth)
                # One durable attempt record precedes effects; the blocked child
                # exits without executing if this process dies before permission.
                self.save()
                os.write(write_fd, b"1")
                os.close(write_fd)
                write_fd = -1
                deadline = time.monotonic() + timeout_seconds if timeout_seconds is not None else None
                timed_out = False
                while True:
                    try:
                        wait_seconds = HEARTBEAT_SECONDS if deadline is None else max(0, min(
                            HEARTBEAT_SECONDS, deadline - time.monotonic()))
                        process.wait(timeout=wait_seconds)
                        break
                    except subprocess.TimeoutExpired:
                        if deadline is not None and time.monotonic() >= deadline:
                            timed_out = True
                            process.terminate()
                            try:
                                process.wait(timeout=1)
                            except subprocess.TimeoutExpired:
                                process.kill()
                                process.wait()
                            break
                        print(f"cell-deploy: still running {step['id']}", file=sys.stderr, flush=True)
                step.update(state="unknown" if timed_out else "succeeded" if process.returncode == 0 else "failed",
                            returncode=process.returncode, finished_at=now(),
                            elapsed_seconds=time.monotonic() - started)
                self.save()
                if timed_out:
                    raise ExecutionInterrupted(f"{step['id']} exceeded its declared timeout; application effects are unknown")
                if process.returncode:
                    with error_path.open("rb") as errors:
                        errors.seek(max(0, error_path.stat().st_size - MAX_DETAIL))
                        detail = errors.read().decode("utf-8", "replace")
                    raise DeploymentError(f"{step['id']} exited {process.returncode}: {bounded_text(detail)}")
        finally:
            if read_fd >= 0:
                os.close(read_fd)
            if write_fd >= 0:
                os.close(write_fd)

    def file_step(self, step: dict[str, Any], context: dict[str, str]) -> None:
        result = {"id": context["product"] + ":" + step["id"], "kind": step["kind"],
                  "product": context["product"], "state": "running", "started_at": now()}
        self.record["steps"].append(result)
        self.save()
        started = time.monotonic()
        temporary = None
        try:
            destination = manifest.absolute(step["destination"], context)
            destination.parent.mkdir(parents=True, exist_ok=True)
            temporary = destination.with_name(f".{destination.name}.{uuid.uuid4().hex}")
            if step["kind"] == "copy":
                source = manifest.absolute(step["source"], context)
                if source.is_dir():
                    shutil.copytree(source, temporary, symlinks=True)
                else:
                    shutil.copy2(source, temporary)
                if "mode" in step:
                    temporary.chmod(step["mode"])
            else:
                temporary.symlink_to(manifest.expand(step["target"], context))
            os.replace(temporary, destination)
            result.update(state="succeeded", finished_at=now(), elapsed_seconds=time.monotonic() - started)
            self.save()
        except (OSError, ValueError, KeyError, TypeError):
            result.update(state="failed", finished_at=now(), elapsed_seconds=time.monotonic() - started)
            self.save()
            raise
        finally:
            if temporary is not None:
                if temporary.is_dir() and not temporary.is_symlink():
                    shutil.rmtree(temporary)
                else:
                    temporary.unlink(missing_ok=True)

    def execute(self) -> None:
        source_root = Path(self.record["repository_root"])
        previous_umask = os.umask(0o022)
        try:
            git(source_root, "worktree", "add", "--detach", str(self.source), self.record["source_commit"])
        finally:
            os.umask(previous_umask)
        preparation = self.path / "preparation"
        argv = [sys.executable, str(self.source / "deployment/build.py"), "--source-root", str(self.source),
                "--output", str(preparation)]
        if self.record["request"].get("signing_policy") is not None:
            policy = self.path / "signing-policy.json"
            policy.write_bytes(json_bytes(self.record["request"]["signing_policy"]))
            argv += ["--signing-policy-file", str(policy)]
        supplied = self.record["request"].get("prepared_build")
        if supplied is not None:
            snapshot = self.path / "prepared-build-snapshot.json"
            snapshot.write_bytes(json_bytes(supplied["snapshot"]))
            argv += ["--prepared-build", supplied["path"], "--prepared-build-snapshot-file", str(snapshot)]
        for product in self.record["products"]:
            argv += ["--product", product]
        self.command({"id": "build", "kind": "run", "product": "cell"}, argv)
        # This is builder-produced path metadata, not an integrity or domain check.
        build = read_json(preparation / "result.json")
        self.record["build"] = {key: build[key] for key in ("elapsed_seconds", "queue_seconds", "build_seconds",
            "reused_products", "built_products") if key in build}
        if not isinstance(build.get("candidates"), dict) or any(
                not isinstance(build["candidates"].get(name), dict)
                or not isinstance(build["candidates"][name].get("candidate_dir"), str)
                for name in self.record["products"]):
            raise DeploymentError("builder did not return the requested artifact paths")
        candidates = {name: {"candidate_dir": value["candidate_dir"],
            "candidate": candidate.read_manifest(Path(value["candidate_dir"]))}
            for name, value in build["candidates"].items() if name in self.record["products"]}
        for product in self.record["products"]:
            directory = candidates[product]["candidate_dir"]
            context = {"product": product, "candidate_dir": directory, "source_root": str(self.source),
                       "run_dir": str(self.path), "home": self.environment["HOME"]}
            request = {"schema": SCHEMA, "product": product, "run_id": self.record["run_id"],
                "run_dir": str(self.path), "source_root": str(self.source), "candidate_dir": directory,
                "candidate": candidates[product]["candidate"], "selected_products": self.record["products"],
                "settings": self.record["request"]["settings"].get(product),
                "dependency_settings": self.record["request"]["settings"],
                "dependency_candidates": {name: value for name, value in candidates.items() if name != product}}
            for step in self.record["manifest"][product]["steps"]:
                if step["kind"] == "run":
                    argv = [manifest.expand(value, context) for value in step["argv"]]
                    env = {**self.environment, **{key: manifest.expand(value, context)
                        for key, value in step.get("env", {}).items()}}
                    cwd = manifest.absolute(step["cwd"], context) if "cwd" in step else self.source
                    self.command({"id": product + ":" + step["id"], "kind": "run", "product": product}, argv,
                        stdin=request if step.get("stdin") == "deployment_request" else step.get("stdin"),
                        cwd=cwd, environment=env, timeout_seconds=step.get("timeout_seconds"))
                else:
                    self.file_step(step, context)

    def finish(self) -> None:
        self.record.update(finished_at=now(), elapsed_seconds=time.monotonic() - self.started)
        self.save()
        if self.record["state"] in ("succeeded", "failed"):
            clear_marker(self.storage, self.record)
            try:
                if self.source.exists():
                    git(Path(self.record["repository_root"]), "worktree", "remove", "--force", str(self.source))
                if (self.path / "preparation").exists():
                    candidate.remove_tree(self.path / "preparation")
                self.record["workspace_cleanup"] = "removed"
            except (OSError, RuntimeError) as error:
                self.record["workspace_cleanup"] = bounded_text(error)
            self.save()


def blocked_result(request_id: str, detail: str) -> dict[str, Any]:
    return {"schema": SCHEMA, "manifest_executor": MANIFEST_EXECUTOR, "request_id": request_id,
            "state": "blocked", "operation_state": "blocked", "exit_code": 75, "detail": detail}


def start(root: Path, products: Sequence[str], storage: Path | None = None, *, verbose: bool = False,
          settings: dict[str, Any] | None = None, selected_commit: str | None = None,
          request_id: str | None = None, signing_policy: dict[str, Any] | None = None,
          prepared_build: Path | None = None, prepared_build_snapshot: dict[str, Any] | None = None) -> dict[str, Any]:
    storage = storage or state_root()
    if request_id is not None and selected_commit is None:
        raise DeploymentError("caller-correlated execution requires an exact source commit")
    request_id = request_id or "manual:" + uuid.uuid4().hex
    request = canonical_request(root, products, selected_commit, settings, signing_policy,
                                prepared_build, prepared_build_snapshot)
    try:
        with deployment_lock(storage) as lock_fd:
            old = read_operation(storage, request_id)
            if old is not None:
                if not matching_request(old["request"], request, explicit_policy=signing_policy is not None):
                    raise DeploymentError("request ID already belongs to a different instruction request")
                return {**operation_status(storage, request_id, running=False), "replayed": True}
            if marker(storage) is not None:
                return blocked_result(request_id, "acknowledge the interrupted execution before another admission")
            chosen = plan(root, products, request["source_commit"])
            inventory = catalog(root, request["source_commit"])
            run_id = uuid.uuid4().hex
            path = storage / "runs" / run_id
            private_directory(path)
            record = {"schema": SCHEMA, "manifest_executor": MANIFEST_EXECUTOR, "request_id": request_id,
                "run_id": run_id, "source_commit": request["source_commit"], "products": chosen["products"],
                "request": request, "manifest": {name: inventory[name]["manifest"] for name in chosen["products"]},
                "repository_root": str(root), "run_dir": str(path), "steps": [], "state": "active",
                "phase": "active", "started_at": now(), "worker_pid": os.getpid(),
                "worker_birth": process_token(os.getpid())}
            save_operation(storage, record)
            private_directory(storage / "active")
            durable_json(storage / "active/run.json", {"schema": SCHEMA, "request_id": request_id, "run_id": run_id})
            run = Run(storage, record, lock_fd)
            try:
                run.execute()
                record.update(state="succeeded", phase="terminal", detail="Declared placements and commands completed.")
            except (OSError, ValueError, RuntimeError, KeyError, TypeError, subprocess.SubprocessError) as error:
                uncertain = isinstance(error, ExecutionInterrupted) or any(
                    step["state"] == "running" for step in record["steps"])
                record.update(state="interrupted" if uncertain else "failed",
                              phase="interrupted" if uncertain else "terminal", detail=bounded_text(error))
                for step in record["steps"]:
                    if step["state"] == "running":
                        step["state"] = "unknown"
            run.finish()
            return result_from_record(record)
    except DeploymentBusy:
        old = read_operation(storage, request_id)
        if old is not None:
            if not matching_request(old["request"], request, explicit_policy=signing_policy is not None):
                raise DeploymentError("request ID belongs to a different instruction request")
            return {**operation_status(storage, request_id, running=True), "replayed": True, "exit_code": 75}
        return blocked_result(request_id, "another executor or its child is active")


def reconcile_request(storage: Path, request_id: str, *, acknowledge: bool = False) -> dict[str, Any]:
    try:
        with deployment_lock(storage):
            record = read_operation(storage, request_id)
            if record is None:
                return operation_status(storage, request_id, running=False)
            if record["schema"] != SCHEMA:
                if record.get("phase") == "terminal":
                    return operation_status(storage, request_id, running=False)
                raise DeploymentError("legacy execution is unsupported; inspect retained effects and use explicit product procedures")
            if record.get("phase") == "terminal":
                active = marker(storage)
                if active is not None and active.get("run_id") == record["run_id"]:
                    clear_marker(storage, record)
                return result_from_record(record)
            if record.get("phase") == "active":
                record.update(state="interrupted", phase="interrupted", finished_at=now(),
                              detail="Execution ended without a completed result; application effects are unknown.")
                for step in record["steps"]:
                    if step["state"] == "running":
                        step.update(state="unknown", finished_at=now())
                save_operation(storage, record)
            if acknowledge:
                record.update(acknowledged_at=now(), phase="terminal")
                save_operation(storage, record)
                clear_marker(storage, record)
            return result_from_record(record)
    except DeploymentBusy:
        result = operation_status(storage, request_id, running=True)
        result["exit_code"] = 75
        return result


def main(argv: Sequence[str] | None = None) -> int:
    os.umask(0o077)
    arguments = list(sys.argv[1:] if argv is None else argv)
    if arguments[:1] == ["_exec"]:
        read_fd, lock_fd = int(arguments[1]), int(arguments[2])
        permit = os.read(read_fd, 1)
        os.close(read_fd)
        if permit != b"1":
            return 125
        os.set_inheritable(lock_fd, True)
        os.environ["CELL_DEPLOYMENT_LOCK_FD"] = str(lock_fd)
        os.execvpe(arguments[3], arguments[3:], os.environ)
        return 127
    if arguments and arguments[0] not in ("start", "plan", "status", "reconcile", "acknowledge", "-h", "--help"):
        arguments.insert(0, "start")
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    for name in ("start", "plan"):
        command = commands.add_parser(name)
        command.add_argument("products", nargs="+")
        command.add_argument("--verbose", action="store_true")
        command.add_argument("--settings", type=Path)
        command.add_argument("--source-commit")
        if name == "start":
            command.add_argument("--request-id")
            command.add_argument("--signing-policy-file", type=Path)
            command.add_argument("--prepared-build", type=Path)
            command.add_argument("--prepared-build-snapshot-file", type=Path)
    for name in ("status", "reconcile", "acknowledge"):
        command = commands.add_parser(name)
        command.add_argument("--request-id", required=True)
    parsed = parser.parse_args(arguments)
    try:
        if parsed.command == "status":
            result = operation_status(state_root(), parsed.request_id)
        elif parsed.command in ("reconcile", "acknowledge"):
            result = reconcile_request(state_root(), parsed.request_id, acknowledge=parsed.command == "acknowledge")
        else:
            root = ci_client.repository_root(Path(__file__).resolve().parent.parent)
            if parsed.command == "plan":
                result = plan(root, parsed.products, parsed.source_commit)
            else:
                if sys.platform != "darwin":
                    raise DeploymentError("live Cell deployment supports the current macOS operator")
                result = start(root, parsed.products, verbose=parsed.verbose, settings=setup_settings(parsed.settings),
                    selected_commit=parsed.source_commit, request_id=parsed.request_id,
                    signing_policy=read_json(parsed.signing_policy_file) if parsed.signing_policy_file else None,
                    prepared_build=parsed.prepared_build,
                    prepared_build_snapshot=read_json(parsed.prepared_build_snapshot_file)
                        if parsed.prepared_build_snapshot_file else None)
        print(json.dumps(result, sort_keys=True, separators=(",", ":")))
        return result.get("exit_code", 0)
    except (DeploymentError, OSError, ValueError, RuntimeError, KeyError, TypeError) as error:
        print(json.dumps({"schema": SCHEMA, "manifest_executor": MANIFEST_EXECUTOR, "state": "stopped",
                          "detail": bounded_text(error), "exit_code": 1}, separators=(",", ":")))
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
