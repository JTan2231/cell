#!/usr/bin/env python3
"""Deploy selected Cell systems from one committed main snapshot."""

from __future__ import annotations

import argparse
import contextlib
import datetime as dt
import fcntl
import io
import json
import os
from pathlib import Path
import pwd
import re
import shlex
import shutil
import stat
import subprocess
import sys
import tarfile
import time
from typing import Any, Iterator, Sequence
import uuid

sys.dont_write_bytecode = True
if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from ci_broker import client as ci_client
from ci_broker.broker import MINIMAL_ENVIRONMENT
from deployment import candidate

SCHEMA = 1
MUTATIONS = frozenset(("hold", "drain", "apply", "release", "recover"))
EXPECTED = {"inspect": "ready", "hold": "held", "drain": "drained",
            "apply": "applied", "verify": "verified", "release": "released",
            "recover": "recovered"}
NAME = re.compile(r"[a-z][a-z0-9-]*")
RUN_ID = re.compile(r"[0-9a-f]{32}")
MAX_REPLY = 1024 * 1024
MAX_DETAIL = 1024
MAX_DIAGNOSTICS = 4096
HEARTBEAT_SECONDS = 60


class DeploymentError(RuntimeError):
    """A deployment cannot safely advance."""


def now() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat()


def bounded_text(value: Any, limit: int, *, tail: bool = False) -> str:
    text = re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", "", str(value))
    text = re.sub(r"[\x00-\x08\x0b-\x1f\x7f]", "", text)
    encoded = text.encode("utf-8", "replace")
    if len(encoded) <= limit:
        return text
    suffix = "\n[truncated]"
    size = max(0, limit - len(suffix))
    if tail:
        return "[truncated]\n" + (encoded[-size:] if size else b"").decode("utf-8", "ignore")
    return encoded[:size].decode("utf-8", "ignore") + suffix


def state_root() -> Path:
    home = Path(pwd.getpwuid(os.getuid()).pw_dir)
    if sys.platform == "darwin":
        return home / "Library" / "Application Support" / "Cell" / "deployments"
    return home / ".local" / "state" / "cell" / "deployments"


def private_directory(path: Path) -> None:
    if path.is_symlink():
        raise DeploymentError(f"refusing symbolic state directory: {path}")
    path.mkdir(mode=0o700, parents=True, exist_ok=True)
    info = path.stat()
    if info.st_uid != os.getuid() or stat.S_IMODE(info.st_mode) != 0o700:
        raise DeploymentError(f"state directory must be owned by this user and mode 0700: {path}")


def durable_json(path: Path, value: Any) -> None:
    if path.is_symlink():
        raise DeploymentError(f"refusing symbolic state file: {path}")
    temporary = path.with_name(f".{path.name}.{uuid.uuid4().hex}")
    try:
        descriptor = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(candidate.json_bytes(value))
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
        directory = os.open(path.parent, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        temporary.unlink(missing_ok=True)


def read_json(path: Path) -> dict[str, Any]:
    candidate.regular(path)
    value = json.loads(path.read_text())
    if not isinstance(value, dict):
        raise DeploymentError(f"expected a JSON object: {path}")
    return value


def git(root: Path, *arguments: str) -> str:
    return ci_client.git(root, *arguments).decode("utf-8", "strict").strip()


def source_file(root: Path, revision: str, relative: str) -> str:
    return git(root, "show", f"{revision}:{relative}")


def descriptor(text: str) -> dict[str, str]:
    """The existing product inventory contains literal shell assignments only."""
    values: dict[str, str] = {}
    for token in shlex.split(text, comments=True):
        name, separator, value = token.partition("=")
        if not separator or not re.fullmatch(r"[A-Z][A-Z0-9_]*", name) or name in values:
            raise DeploymentError("invalid literal product descriptor")
        values[name] = value
    return values


def catalog(root: Path, revision: str) -> dict[str, dict[str, Any]]:
    paths = git(root, "ls-tree", "--name-only", revision, "pipeline/products/").splitlines()
    products: dict[str, dict[str, Any]] = {}
    for path in paths:
        if not path.endswith(".sh"):
            continue
        values = descriptor(source_file(root, revision, path))
        identity = values["PRODUCT_ID"]
        name = "krisis" if identity == "decisions" else identity
        directory = values["PRODUCT_DIR"]
        if not NAME.fullmatch(name) or not NAME.fullmatch(directory):
            raise DeploymentError("invalid product identity in committed inventory")
        adapter_path = f"{directory}/deployment/adapter.py"
        metadata_path = f"{directory}/deployment/adapter.json"
        try:
            metadata = json.loads(source_file(root, revision, metadata_path))
        except (RuntimeError, ValueError):
            metadata = None
        if metadata is not None:
            if not isinstance(metadata, dict) or metadata.get("schema") != 1 or metadata.get("product") != name:
                raise DeploymentError(f"invalid committed adapter declaration: {name}")
            names(metadata.get("dependencies", []))
            adapter_binary = metadata.get("adapter_binary")
            if adapter_binary is not None:
                if adapter_binary != f"{name}-install":
                    raise DeploymentError(f"unsupported committed binary adapter: {name}")
                adapter_path = None
            else:
                try:
                    git(root, "cat-file", "-e", f"{revision}:{adapter_path}")
                except RuntimeError:
                    metadata = None
        products[name] = {
            "product": name, "directory": directory, "adapter": adapter_path,
            "metadata": metadata, "profile": values["DEPLOY_PROFILE"],
            "aliases": values.get("PRODUCT_ALIASES", "").split(),
        }
    if not products:
        raise DeploymentError("committed source contains no Cell product inventory")
    for name, item in products.items():
        if item["metadata"] is not None:
            unknown = set(item["metadata"].get("dependencies", [])) - products.keys()
            if unknown:
                raise DeploymentError(f"{name} names unknown deployment prerequisites: {', '.join(sorted(unknown))}")
    return products


def names(value: Any) -> list[str]:
    if not isinstance(value, list) or any(not isinstance(item, str) or not NAME.fullmatch(item)
                                          for item in value):
        raise DeploymentError("adapter product references must be a list of system names")
    return value


def ordered(selected: Sequence[str], after: dict[str, Sequence[str]]) -> list[str]:
    pending = set(selected)
    result: list[str] = []
    while pending:
        ready = sorted(name for name in pending if not (set(after.get(name, [])) & pending))
        if not ready:
            raise DeploymentError("selected product ordering contains a cycle")
        result.extend(ready)
        pending.difference_update(ready)
    return result


def plan(root: Path, requested: Sequence[str]) -> dict[str, Any]:
    revision = git(root, "rev-parse", "--verify", "refs/heads/main^{commit}")
    products = catalog(root, revision)
    aliases: dict[str, str] = {}
    for name, item in products.items():
        for alias in [name, *item["aliases"]]:
            if alias in aliases and aliases[alias] != name:
                raise DeploymentError(f"ambiguous system alias: {alias}")
            aliases[alias] = name
    aliases["annals-usage"] = "annals"
    selected: list[str] = []
    for value in requested:
        name = aliases.get(value.lower())
        if name is None:
            raise DeploymentError(f"unknown system: {value}; available: {', '.join(sorted(products))}")
        if products[name]["metadata"] is None:
            raise DeploymentError(f"{name} has no deployment adapter in committed main")
        if name not in selected:
            selected.append(name)
    if not selected:
        raise DeploymentError("select at least one system")
    dependencies = {name: item["metadata"].get("dependencies", [])
                    for name, item in products.items() if item["metadata"] is not None}
    return {"schema": SCHEMA, "source_commit": revision,
            "products": ordered(selected, dependencies), "catalog": products,
            "publication": "none", "source_policy": "committed local main; working edits excluded"}


def archive_source(root: Path, revision: str, destination: Path) -> dict[str, Any]:
    archive = ci_client.git(root, "archive", "--format=tar", revision)
    destination.mkdir(mode=0o700)
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:") as stream:
        for member in stream.getmembers():
            path = Path(member.name)
            if path.is_absolute() or ".." in path.parts or not (member.isdir() or member.isfile()):
                raise DeploymentError("committed runtime source contains an unsupported archive path")
            target = destination / path
            if member.isdir():
                target.mkdir(mode=0o700, parents=True, exist_ok=True)
            else:
                target.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
                incoming = stream.extractfile(member)
                if incoming is None:
                    raise DeploymentError("cannot read committed runtime source")
                with incoming, target.open("xb") as output:
                    shutil.copyfileobj(incoming, output)
                target.chmod(0o555 if member.mode & 0o111 else 0o444)
    manifest = candidate.tree_files(destination)
    candidate.seal_tree(destination)
    return manifest


def create_run(root: Path, requested: Sequence[str], storage: Path | None = None,
               *, verbose: bool = False) -> Path:
    if sys.version_info < (3, 11):
        raise DeploymentError("deployment requires Python 3.11 or newer")
    chosen = plan(root, requested)
    storage = storage or state_root()
    private_directory(storage)
    run_id = uuid.uuid4().hex
    run = storage / "active"
    if run.exists() or run.is_symlink():
        raise DeploymentError("an active deployment workspace already exists")
    private_directory(run)
    for directory in ("candidates", "steps"):
        private_directory(run / directory)
    data = {**chosen, "run_id": run_id, "state": "created", "created_at": now(),
            "updated_at": now(), "repository": str(root), "run_dir": str(run),
            "source_root": str(run / "source"), "worktree": str(run / "worktree"),
            "python": str(Path(sys.executable).resolve()), "records": {}, "events": [],
            "active_operation": None, "affected": [], "mutation_started": False,
            "apply_started": False, "verbose": verbose, "diagnostics": [],
            "recovery": {"state": "not_needed"}, "cleanup": {"releases": "not_started", "workspace": "pending"}}
    durable_json(run / "run.json", data)
    try:
        source_manifest = archive_source(root, chosen["source_commit"], run / "source")
        if "deployment/cli.py" not in source_manifest:
            raise DeploymentError("deployment runner must be committed to main before starting a run")
        durable_json(run / "source-manifest.json", source_manifest)
        data["source_manifest_sha256"] = candidate.digest(run / "source-manifest.json")
        data["python_sha256"] = candidate.digest(Path(data["python"]))
        data["state"] = "prepared_source"
        durable_json(run / "run.json", data)
    except Exception as error:
        data["state"] = "stopped"
        data["detail"] = str(error)
        durable_json(run / "run.json", data)
        raise
    return run


def runtime_environment() -> dict[str, str]:
    environment = {name: os.environ[name] for name in MINIMAL_ENVIRONMENT if name in os.environ}
    for name in ("CELL_RELEASE_CACHE_DIR", "CELL_RELEASE_BUILD_JOBS"):
        if name in os.environ:
            environment[name] = os.environ[name]
    environment["HOME"] = pwd.getpwuid(os.getuid()).pw_dir
    environment["PYTHONDONTWRITEBYTECODE"] = "1"
    environment["PYTHONUNBUFFERED"] = "1"
    return environment


@contextlib.contextmanager
def deployment_lock(storage: Path) -> Iterator[int]:
    private_directory(storage)
    descriptor_fd = os.open(storage / "deployment.lock", os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    try:
        info = os.fstat(descriptor_fd)
        if (not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or info.st_nlink != 1
                or stat.S_IMODE(info.st_mode) != 0o600):
            raise DeploymentError("deployment lock is not a private owned regular file")
        try:
            fcntl.flock(descriptor_fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            raise DeploymentError("another deployment or its active child holds the deployment lock") from error
        yield descriptor_fd
    finally:
        os.close(descriptor_fd)


def process_birth(pid: int) -> str | None:
    output = subprocess.run(["ps", "-p", str(pid), "-o", "lstart="], check=False,
                            capture_output=True, text=True)
    return output.stdout.strip() or None


def cleanup_active(storage: Path) -> None:
    """Remove the inactive workspace; caller must hold the host deployment lock."""
    path = storage / "active"
    if path.is_symlink():
        raise DeploymentError("refusing symbolic active deployment workspace")
    if not path.exists():
        return
    private_directory(path)
    if (path / "run.json").exists():
        data = read_json(path / "run.json")
        repository = Path(data["repository"])
        worktree = (path / "worktree").resolve()
        registered = git(repository, "worktree", "list", "--porcelain").splitlines()
        if any(line.startswith("worktree ") and Path(line.removeprefix("worktree ")).resolve() == worktree
               for line in registered):
            git(repository, "worktree", "remove", "--force", "--force", str(worktree))
    # Sealed source/candidate directories must be writable before removal.
    for directory, _, _ in os.walk(path, followlinks=False):
        Path(directory).chmod(0o700)
    shutil.rmtree(path)


def capture_diagnostics(path: Path, data: dict[str, Any], phase: str) -> None:
    """Keep failure evidence in memory/state until the final cleanup completes."""
    active = data.get("active_operation") or {}
    paths = [(path / "run.log", active.get("stderr_offset", 0))]
    if active.get("output"):
        output = Path(active["output"])
        if output.parent == path / "steps":
            paths.append((output, 0))
    evidence = []
    for output, offset in paths:
        with contextlib.suppress(OSError, ValueError):
            if not output.is_file() or output.is_symlink():
                continue
            if output.suffix == ".out" and active.get("adapter") and output.stat().st_size > MAX_REPLY:
                continue
            # Adapter JSON is an internal protocol, not a diagnostic excerpt.
            if output.suffix == ".out" and output.stat().st_size <= MAX_REPLY:
                try:
                    json.loads(output.read_text())
                    continue
                except (UnicodeError, ValueError):
                    pass
            with output.open("rb") as stream:
                stream.seek(max(offset, output.stat().st_size - MAX_DIAGNOSTICS))
                tail = stream.read(MAX_DIAGNOSTICS).decode("utf-8", "replace").strip()
            if tail:
                evidence.append(tail)
    text = bounded_text("\n".join(evidence), MAX_DIAGNOSTICS, tail=True)
    if text:
        data.setdefault("diagnostics", []).append({"phase": phase, "text": text})


def diagnostic_text(entries: Sequence[dict[str, str]], causes: Sequence[str]) -> str:
    """One shared, rendered-byte budget, divided between distinct failures."""
    distinct = []
    for entry in entries:
        lines = [line for line in entry["text"].splitlines()
                 if not any(len(line) >= 20 and (cause.endswith(line) or line.endswith(cause))
                            for cause in causes if cause)]
        text = "\n".join(lines).strip()
        if text and text not in [item[1] for item in distinct]:
            distinct.append((entry["phase"], text))
    if not distinct:
        return ""
    allowance = MAX_DIAGNOSTICS // len(distinct)
    chunks = []
    for phase, text in distinct:
        header = f"cell-deploy: {phase} diagnostics:\n"
        chunks.append(header + bounded_text(text, allowance - len(header.encode()) - 1, tail=True) + "\n")
    return bounded_text("".join(chunks), MAX_DIAGNOSTICS)


def maintenance_result(data: dict[str, Any]) -> dict[str, Any]:
    # An attempted hold or release can take effect without a successful reply.
    # Only a subsequent captured outcome establishes its disposition.
    products: dict[str, str] = {}
    for event in data.get("events", []):
        operation = event.get("operation")
        if operation not in ("hold", "release"):
            continue
        product = event["product"]
        if event["state"] == "starting":
            products[product] = "uncertain"
        elif (event["state"] == "outcome" and event.get("returncode") == 0
              and event.get("response", {}).get("status") == EXPECTED[operation]):
            products[product] = "retained" if operation == "hold" else "released"
    outstanding = {product: state for product, state in sorted(products.items()) if state != "released"}
    if outstanding:
        return {"state": "attention_required", "owner": data["run_id"], "products": outstanding}
    return {"state": "released" if products else "not_started"}


def print_result(result: dict[str, Any]) -> None:
    result = dict(result)
    entries = result.pop("diagnostics", [])
    causes = [result.get("detail", ""), result.get("recovery", {}).get("detail", "")]
    diagnostics = diagnostic_text(entries, causes)
    if diagnostics:
        print(diagnostics, end="", file=sys.stderr)
    print(json.dumps(result, separators=(",", ":"), sort_keys=True))


class Run:
    def __init__(self, path: Path, lock_fd: int):
        self.path = path
        self.data = read_json(path / "run.json")
        if (self.data.get("schema") != SCHEMA or not RUN_ID.fullmatch(self.data.get("run_id", ""))
                or self.data.get("run_dir") != str(path)):
            raise DeploymentError("invalid deployment run identity")
        self.lock_fd = lock_fd
        self.source = Path(self.data["source_root"])
        self.worktree = Path(self.data["worktree"])
        self.last_heartbeat = time.monotonic()

    def save(self) -> None:
        self.data["updated_at"] = now()
        durable_json(self.path / "run.json", self.data)

    def event(self, state: str, **fields: Any) -> None:
        self.data["events"].append({"at": now(), "state": state, **fields})
        self.save()
        if self.data.get("verbose") and state in ("starting", "candidate_prepared", "quality_gate_passed", "installed", "recovered"):
            print(json.dumps({"event": state, **fields}, separators=(",", ":")), file=sys.stderr, flush=True)

    def heartbeat(self) -> None:
        current = time.monotonic()
        if current - self.last_heartbeat >= HEARTBEAT_SECONDS:
            active = self.data.get("active_operation") or {}
            operation = " ".join(filter(None, [active.get("product"), active.get("operation")]))
            print(f"cell-deploy: still running {operation or self.data['state']}", file=sys.stderr, flush=True)
            self.last_heartbeat = current

    def check_source(self) -> None:
        manifest_path = self.path / "source-manifest.json"
        if candidate.digest(manifest_path) != self.data["source_manifest_sha256"]:
            raise DeploymentError("sealed source manifest changed")
        if candidate.tree_files(self.source) != read_json(manifest_path):
            raise DeploymentError("sealed deployment program or packaging source changed")
        if candidate.digest(Path(self.data["python"])) != self.data["python_sha256"]:
            raise DeploymentError("the pinned Python executable changed")

    def record(self, product: str) -> dict[str, Any]:
        return self.data["records"].setdefault(product, {})

    def command(self, product: str, operation: str, command: Sequence[str],
                request: dict[str, Any] | None = None, cwd: Path | None = None) -> tuple[int, Path]:
        sequence = len(self.data["events"])
        output_path = self.path / "steps" / f"{sequence:05d}-{product}-{operation}.out"
        request_path = output_path.with_suffix(".request.json")
        if request is not None:
            durable_json(request_path, request)
        active = {"product": product, "operation": operation, "started_at": now(),
                  "output": str(output_path), "pid": None, "birth": None,
                  "adapter": request is not None,
                  "stderr_offset": (self.path / "run.log").stat().st_size if (self.path / "run.log").exists() else 0}
        self.data["active_operation"] = active
        if operation in MUTATIONS:
            self.data["mutation_started"] = True
        if operation == "apply":
            self.data["apply_started"] = True
        self.event("starting", product=product, operation=operation)
        read_fd, write_fd = os.pipe()
        process: subprocess.Popen[bytes] | None = None
        try:
            with output_path.open("xb") as output, (self.path / "run.log").open("ab") as errors:
                gate = [self.data["python"], str(self.source / "deployment" / "cli.py"),
                        "_exec", str(read_fd), str(self.lock_fd), *command]
                process = subprocess.Popen(gate, stdin=subprocess.PIPE if request is not None else subprocess.DEVNULL,
                                           stdout=output, stderr=errors, cwd=cwd or self.source,
                                           env=runtime_environment(), pass_fds=(read_fd, self.lock_fd))
                os.close(read_fd)
                read_fd = -1
                active["pid"] = process.pid
                active["birth"] = process_birth(process.pid)
                if active["birth"] is None:
                    raise DeploymentError("cannot establish child identity before starting it")
                self.save()
                os.write(write_fd, b"1")
                os.close(write_fd)
                write_fd = -1
                incoming = candidate.json_bytes(request) if request is not None else None
                while True:
                    self.heartbeat()
                    timeout = max(0.1, HEARTBEAT_SECONDS - (time.monotonic() - self.last_heartbeat))
                    try:
                        process.communicate(incoming, timeout=timeout)
                        break
                    except subprocess.TimeoutExpired:
                        incoming = None
                active["returncode"] = process.returncode
                active["finished_at"] = now()
                self.save()
                return process.returncode, output_path
        finally:
            if read_fd >= 0:
                os.close(read_fd)
            if write_fd >= 0:
                os.close(write_fd)
            if process is not None and process.poll() is None and active["pid"] is None:
                process.terminate()

    def candidate_for(self, product: str) -> tuple[Path | None, dict[str, Any] | None]:
        record = self.record(product)
        manifest = None
        directory = None
        if record.get("candidate_dir"):
            directory = Path(record["candidate_dir"])
            manifest = candidate.verify(directory, product=product, commit=self.data["source_commit"])
            source_manifest = read_json(self.path / "source-manifest.json")
            for relative, expected_hash in manifest["source_inputs"].items():
                if source_manifest.get(relative, {}).get("sha256") != expected_hash:
                    raise DeploymentError("candidate packaging differs from the pinned adapter source")
        return directory, manifest

    def sealed_adapter(self, product: str, directory: Path | None,
                       manifest: dict[str, Any] | None) -> Path:
        binary = self.data["catalog"][product]["metadata"].get("adapter_binary")
        record = self.record(product)
        if (binary != f"{product}-install"
                or directory is None or manifest is None or not record.get("prepared")):
            raise DeploymentError(f"{product} binary adapter requires its prepared candidate")
        receipt = record.get("build_receipt", {})
        if (record.get("candidate_id") != manifest["candidate_id"]
                or receipt.get("state") != "built" or receipt.get("product") != product
                or receipt.get("candidate_id") != manifest["candidate_id"]
                or receipt.get("source_key") != manifest["source_key"]
                or manifest["source_key"] != self.data.get("source_key")):
            raise DeploymentError("binary adapter does not match its admitted candidate receipt")
        if manifest["binaries"].get(binary, {}).get("path") != f"bin/{binary}":
            raise DeploymentError("candidate does not contain its declared binary adapter")
        return directory / "bin" / binary

    def adapter(self, product: str, operation: str) -> dict[str, Any]:
        self.check_source()
        item = self.data["catalog"].get(product)
        if item is None or item["metadata"] is None:
            raise DeploymentError(f"required product lacks a committed deployment adapter: {product}")
        record = self.record(product)
        directory, manifest = self.candidate_for(product)
        if item["metadata"].get("adapter_binary") is not None:
            command = [str(self.sealed_adapter(product, directory, manifest)), "adapter", operation]
        else:
            command = [self.data["python"], str(self.source / item["adapter"]), operation]
        request = {"schema": SCHEMA, "product": product, "run_id": self.data["run_id"],
                   "run_dir": str(self.path), "source_root": str(self.source),
                   "candidate_dir": str(directory) if directory else None, "candidate": manifest,
                   "prior": record.get("prior"), "selected_products": self.data["products"],
                   "recovery": record.get("recovery_context")}
        returncode, output = self.command(product, operation, command, request)
        if output.stat().st_size > MAX_REPLY:
            raise DeploymentError(f"{product} {operation} reply exceeds the protocol bound")
        try:
            reply = read_json(output)
        except (OSError, ValueError, candidate.CandidateError) as error:
            raise DeploymentError(f"{product} {operation} returned no valid outcome; state is uncertain") from error
        if reply.get("schema") != SCHEMA or not isinstance(reply.get("data"), dict):
            raise DeploymentError(f"{product} {operation} returned an unsupported outcome")
        self.event("outcome", product=product, operation=operation, response=reply, returncode=returncode)
        if returncode != 0 or reply.get("status") != EXPECTED[operation]:
            raise DeploymentError(bounded_text(f"{product} {operation} stopped: {reply.get('detail', 'outcome not proved')}", MAX_DETAIL))
        self.data["active_operation"] = None
        self.save()
        return reply

    def prepare(self) -> None:
        self.data["state"] = "preparing"
        self.save()
        self.check_source()
        repository = Path(self.data["repository"])
        if not self.worktree.exists():
            # Source generators check normal checkout modes (0755/0644). The
            # enclosing run remains private; restore 0077 before writing state.
            previous_umask = os.umask(0o022)
            try:
                git(repository, "worktree", "add", "--detach", str(self.worktree), self.data["source_commit"])
            finally:
                os.umask(previous_umask)
        if git(self.worktree, "rev-parse", "HEAD") != self.data["source_commit"]:
            raise DeploymentError("preparation worktree is on another source commit")
        if git(self.worktree, "status", "--porcelain", "--untracked-files=all"):
            raise DeploymentError("preparation worktree was modified")
        source_key = candidate.content_source_key(self.worktree)
        self.data["source_key"] = source_key
        self.save()
        preparation = self.path / "preparation"
        # Affected-only products need the same trustworthy installer boundary
        # as selected products. Prepare their declared maintenance closure once,
        # before any hold; this does not select them for installation.
        prepared_products = set(self.data["products"])
        pending = list(prepared_products)
        while pending:
            product = pending.pop()
            metadata = self.data["catalog"][product]["metadata"]
            for affected in names(metadata.get("maintenance_products", [])):
                entry = self.data["catalog"].get(affected)
                if entry is None or entry["metadata"] is None:
                    raise DeploymentError("maintenance declaration names an unavailable product")
                if entry["metadata"].get("adapter_binary") and affected not in prepared_products:
                    prepared_products.add(affected)
                    pending.append(affected)
        self.data["prepared_products"] = sorted(prepared_products)
        command = [self.data["python"], str(self.worktree / "deployment" / "build.py"),
                   "--source-root", str(self.worktree), "--output", str(preparation)]
        for product in self.data["prepared_products"]:
            command.extend(["--product", product])
        returncode, _ = self.command("cell", "release-build", command, cwd=self.worktree)
        if returncode:
            raise DeploymentError("release bundle preparation failed before installation")
        result = read_json(preparation / "result.json")
        if (result.get("schema") != 1 or result.get("state") != "built"
                or result.get("source_key") != source_key):
            raise DeploymentError("release build does not match the selected source")
        self.data["build"] = {key: result[key] for key in ("cache_hit", "build_key", "elapsed_seconds") if key in result}
        for product in self.data["prepared_products"]:
            record = self.record(product)
            output = preparation / "candidates" / product
            manifest = candidate.verify(output, product=product, commit=self.data["source_commit"])
            if manifest["source_key"] != source_key:
                raise DeploymentError("candidate evidence belongs to another source snapshot")
            prepared = result.get("candidates", {}).get(product, {})
            if prepared.get("candidate_id") != manifest["candidate_id"]:
                raise DeploymentError("candidate is missing its exact release-build record")
            receipt = {"state": "built", "product": product, "source_key": source_key,
                       "candidate_id": manifest["candidate_id"]}
            record.update(prepared=True, candidate_dir=str(output), candidate_id=manifest["candidate_id"], build_receipt=receipt)
            self.data["active_operation"] = None
            self.event("candidate_prepared", product=product, candidate_id=manifest["candidate_id"])
        if candidate.content_source_key(self.worktree) != source_key:
            raise DeploymentError("source changed while preparing deployment candidates")

    def inspect(self) -> None:
        self.data["state"] = "inspecting"
        self.save()
        pending = list(self.data["products"])
        inspected: set[str] = set()
        after: dict[str, list[str]] = {}
        while pending:
            product = pending.pop(0)
            if product in inspected:
                continue
            response = self.adapter(product, "inspect")
            data = response["data"]
            self.record(product)["prior"] = data
            metadata = self.data["catalog"][product]["metadata"]
            after[product] = [*names(metadata.get("dependencies", [])), *names(data.get("after", []))]
            if set(after[product]) - self.data["catalog"].keys():
                raise DeploymentError(f"{product} inspection named an unknown ordering prerequisite")
            inspected.add(product)
            pending.extend(name for name in names(data.get("maintenance_products", [])) if name not in inspected)
            self.data["affected"] = sorted(inspected)
            self.save()
        self.data["products"] = ordered(self.data["products"], after)
        self.data["affected"] = ordered(sorted(inspected), after)
        self.save()

    def phase(self, operation: str, products: Sequence[str]) -> None:
        self.data["state"] = {"hold": "holding", "drain": "draining", "apply": "applying",
                              "verify": "verifying", "release": "releasing"}[operation]
        self.save()
        for product in products:
            response = self.adapter(product, operation)
            record = self.record(product)
            record[operation] = response
            if operation == "hold":
                record["held"] = True
            if operation == "release":
                record["held"] = False
            self.save()

    def execute(self) -> None:
        self.prepare()
        self.inspect()
        affected = self.data["affected"]
        requesters = [product for product in affected if product != "nucleus"]
        self.phase("hold", requesters)
        self.phase("drain", requesters)
        if "nucleus" in affected:
            # Requester workflows may need continuation jobs while settling.
            # Close Nucleus admission only after those workflows have drained.
            self.phase("hold", ["nucleus"])
            self.phase("drain", ["nucleus"])
        self.phase("apply", self.data["products"])
        self.phase("verify", affected)
        release_order = [product for product in affected if product != "nucleus"]
        if "nucleus" in affected:
            release_order.append("nucleus")
        self.phase("release", release_order)
        self.data["state"] = "installed"
        self.data["detail"] = "Selected products verified; every run-owned maintenance hold released."
        self.event("installed")

    def recover(self) -> None:
        previous_state = self.data["state"]
        active = self.data["active_operation"]
        if active and active.get("pid") and process_birth(active["pid"]) == active.get("birth"):
            raise DeploymentError("the previous operation process is still active; recovery cannot race it")
        self.data["state"] = "recovering"
        self.save()
        if not self.data.get("mutation_started"):
            self.data["active_operation"] = None
            self.data["state"] = "recovered"
            self.data["detail"] = "No deployment mutation started."
            self.event("recovered")
            return
        affected = self.data["affected"]
        if active and active["product"] not in affected:
            raise DeploymentError("the uncertain operation has no captured product baseline")
        def recover_product(product: str) -> None:
            record = self.record(product)
            record["recovery_context"] = {"state": previous_state, "active_operation": active,
                                           "held": record.get("held", False),
                                           "applied": bool(record.get("apply")),
                                           "apply_started": any(
                                               event.get("product") == product and event.get("operation") == "apply"
                                               and event.get("state") == "starting" for event in self.data["events"]),
                                           "verified": bool(record.get("verify")),
                                           "any_apply_started": bool(self.data.get("apply_started") or any(
                                               event.get("operation") == "apply" and event.get("state") == "starting"
                                               for event in self.data["events"]))}
            response = self.adapter(product, "recover")
            if response["data"].get("safe_to_release") is not True:
                raise DeploymentError(f"{product} recovery did not prove release of maintenance is safe")
            if response["data"].get("installed") not in ("candidate", "prior"):
                raise DeploymentError(f"{product} recovery did not identify a coherent installed generation")
            record["recover"] = response
            self.save()
        for product in affected:
            recover_product(product)
        release_order = [product for product in affected if product != "nucleus"]
        if "nucleus" in affected:
            release_order.append("nucleus")
        self.phase("release", release_order)
        self.data["state"] = "recovered"
        self.data["detail"] = "Products proved coherent recovery; run-owned holds released. Deployment is not reported as succeeded."
        self.event("recovered")


def run_worker(path: Path, lock_fd: int | None = None) -> int:
    if lock_fd is None:
        with deployment_lock(path.parent) as owned_fd:
            return run_worker(path, owned_fd)
    run = Run(path, lock_fd)
    run.data["worker_pid"] = os.getpid()
    run.data["worker_birth"] = process_birth(os.getpid())
    run.save()
    try:
        run.execute()
    except (DeploymentError, candidate.CandidateError, OSError, ValueError, RuntimeError) as error:
        detail = bounded_text(error, MAX_DETAIL)
        capture_diagnostics(path, run.data, "failure")
        if run.data.get("mutation_started"):
            run.data["recovery"] = {"state": "running"}
            run.save()
            try:
                run.recover()
                run.data["recovery"] = {"state": "succeeded"}
            except (DeploymentError, candidate.CandidateError, OSError, ValueError, RuntimeError) as recovery_error:
                run.data["recovery"] = {"state": "failed", "detail": bounded_text(recovery_error, MAX_DETAIL)}
                capture_diagnostics(path, run.data, "recovery")
        run.data["state"] = "stopped"
        run.data["detail"] = detail
        run.event("stopped", detail=detail)
        return 1
    # Installed products are already verified and released. Cleanup failure
    # must not initiate product recovery or undo that completed deployment.
    try:
        run.check_source()
        cleanup_command = [run.data["python"], str(run.source / "deployment" / "cleanup.py")]
        for product in run.data.get("prepared_products", run.data["products"]):
            if run.data["catalog"][product]["metadata"].get("adapter_binary"):
                directory, manifest = run.candidate_for(product)
                cleanup_command.extend(["--installer", f"{product}={run.sealed_adapter(product, directory, manifest)}"])
        returncode, output = run.command("cell", "cleanup", cleanup_command)
        if returncode:
            raise DeploymentError("installed release history cleanup failed")
        run.data["release_history_cleanup"] = read_json(output)
        run.data["cleanup"]["releases"] = "succeeded"
        run.data["active_operation"] = None
        run.data["state"] = "succeeded"
        run.event("succeeded", cleanup=run.data["release_history_cleanup"])
        return 0
    except (DeploymentError, candidate.CandidateError, OSError, ValueError, RuntimeError) as error:
        run.data["state"] = "cleanup_failed"
        run.data["cleanup"]["releases"] = "failed"
        run.data["detail"] = bounded_text(f"Products verified and holds released; cleanup failed: {error}", MAX_DETAIL)
        run.event("cleanup_failed", detail=run.data["detail"])
        capture_diagnostics(path, run.data, "cleanup")
        run.save()
        return 1


def launch(path: Path, lock_fd: int) -> dict[str, Any]:
    data = read_json(path / "run.json")
    executable = Path(data["python"])
    if candidate.digest(executable) != data["python_sha256"]:
        raise DeploymentError("the pinned Python executable changed")
    script = Path(data["source_root"]) / "deployment" / "cli.py"
    expected = read_json(path / "source-manifest.json").get("deployment/cli.py", {}).get("sha256")
    if candidate.digest(script) != expected:
        raise DeploymentError("the pinned deployment runner changed")
    arguments = [str(executable), str(script), "_worker", str(path), str(lock_fd)]
    returncode = subprocess.call(arguments, env=runtime_environment(), pass_fds=(lock_fd,))
    data = read_json(path / "run.json")
    if returncode and data["state"] not in ("stopped", "cleanup_failed"):
        capture_diagnostics(path, data, "worker")
        data.update(state="stopped", detail="Deployment worker exited before completion; product holds may remain.")
        if data.get("recovery", {}).get("state") == "running":
            data["recovery"] = {"state": "uncertain"}
        elif data.get("mutation_started") and data.get("recovery", {}).get("state") == "not_needed":
            data["recovery"] = {"state": "not_attempted"}
    return {"schema": SCHEMA, "run_id": data["run_id"], "state": data["state"],
            "products": data["products"], "source_commit": data["source_commit"],
            "detail": bounded_text(data.get("detail", ""), MAX_DETAIL), "exit_code": 1 if returncode else 0,
            "recovery": data.get("recovery", {"state": "not_needed"}), "maintenance": maintenance_result(data),
            "build": data.get("build"),
            "cleanup": data.get("cleanup", {"releases": "not_started", "workspace": "pending"}),
            "diagnostics": data.get("diagnostics", [])}


def start(root: Path, products: Sequence[str], storage: Path | None = None,
          *, verbose: bool = False) -> dict[str, Any]:
    storage = storage or state_root()
    admitted = False
    result = None
    try:
        with deployment_lock(storage) as lock_fd:
            admitted = True
            cleanup_active(storage)
            path = create_run(root, products, storage, verbose=verbose)
            result = launch(path, lock_fd)
    finally:
        if admitted:
            # Close our inherited lock reference first. A surviving descendant
            # then prevents reacquisition and therefore prevents deletion.
            try:
                with deployment_lock(storage):
                    cleanup_active(storage)
                if result is not None:
                    result["cleanup"]["workspace"] = "removed"
            except (DeploymentError, candidate.CandidateError, RuntimeError, OSError, ValueError) as error:
                if result is None:
                    raise
                result["cleanup"]["workspace"] = "failed"
                result["cleanup"]["detail"] = bounded_text(error, MAX_DETAIL)
                result["exit_code"] = 1
                if result["state"] == "succeeded":
                    result["state"] = "cleanup_failed"
    assert result is not None
    return result


def main(argv: Sequence[str] | None = None) -> int:
    os.umask(0o077)
    if sys.version_info < (3, 11):
        print("cell-deploy: deployment requires Python 3.11 or newer", file=sys.stderr)
        return 1
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
    if arguments[:1] == ["_worker"]:
        try:
            return run_worker(Path(arguments[1]), int(arguments[2]))
        except (DeploymentError, OSError, ValueError) as error:
            print(f"cell-deploy: {bounded_text(error, MAX_DETAIL)}", file=sys.stderr)
            return 1
    if arguments[:1] == ["--verbose"] and arguments[1:2] in (["start"], ["plan"]):
        arguments[0], arguments[1] = arguments[1], arguments[0]
    if arguments and arguments[0] not in ("start", "plan", "-h", "--help"):
        arguments.insert(0, "start")
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    for name in ("start", "plan"):
        command = commands.add_parser(name)
        command.add_argument("products", nargs="+")
        command.add_argument("--verbose", action="store_true", help="show deployment operation progress on stderr")
    parsed = parser.parse_args(arguments)
    try:
        root = ci_client.repository_root(Path(__file__).resolve().parent.parent)
        if parsed.command == "plan":
            result = plan(root, parsed.products)
            result.pop("catalog")
        else:
            if sys.platform != "darwin":
                raise DeploymentError("live deployment is supported only for the current macOS user")
            result = start(root, parsed.products, verbose=parsed.verbose)
        print_result(result)
        return int(result.get("exit_code", 0))
    except (DeploymentError, candidate.CandidateError, RuntimeError, OSError, ValueError) as error:
        print_result({"schema": SCHEMA, "state": "stopped", "detail": bounded_text(error, MAX_DETAIL), "exit_code": 1})
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
