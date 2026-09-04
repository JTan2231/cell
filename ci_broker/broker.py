#!/usr/bin/env python3
"""Durable, host-scoped admission control for Cell CI commands.

The broker is deliberately daemonless.  Every synchronous caller journals its
request in the same SQLite scope, competes for a lane transactionally, and, if
admitted, supervises its own command.  No command is started unless the journal
is writable and the caller owns a recorded lane slot.

The ``_exec-body`` subcommand is private.  It gates the actual exec on one byte
from the supervising process so a body cannot start in the small window between
``Popen`` and recording its PID in SQLite.
"""

from __future__ import annotations

import argparse
import contextlib
import hashlib
import json
import os
import platform
import signal
import socket
import sqlite3
import subprocess
import sys
import time
import uuid
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable, Mapping, Sequence


SCHEMA_VERSION = 1
PROTOCOL_VERSION = 1
DEFAULT_LIGHT_SLOTS = 2
DEFAULT_CARGO_JOBS = 2
DEFAULT_HEARTBEAT_TIMEOUT = 15.0
DEFAULT_POLL_INTERVAL = 0.25
SOURCE_CHECK_TIMEOUT = 30.0
MAX_TERMINAL_EXECUTIONS = 256
TERMINAL_MAX_AGE_SECONDS = 14 * 24 * 60 * 60

STATES = (
    "queued",
    "running",
    "passed",
    "failed",
    "stale",
    "lost",
    "cancelled",
)
TERMINAL_STATES = frozenset(("passed", "failed", "stale", "lost", "cancelled"))
LANE_CAPACITY = {"heavy": 1}

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

# These values describe the invoking shell rather than the body environment.
# Removing them also keeps otherwise identical worktrees from receiving
# different environment identities solely because their working paths differ.
EPHEMERAL_ENVIRONMENT = frozenset(("OLDPWD", "PWD", "SHLVL", "_"))


class BrokerError(RuntimeError):
    """A fail-closed broker error."""


@dataclass(frozen=True)
class Scope:
    host_id: str
    repository_id: str
    state_dir: Path
    light_slots: int
    cargo_jobs: int
    heartbeat_timeout: float

    @property
    def key(self) -> str:
        return digest_json(
            {
                "host_id": self.host_id,
                "repository_id": self.repository_id,
            }
        )

    @property
    def database_path(self) -> Path:
        return self.state_dir / f"scope-{self.key}.sqlite3"


@dataclass(frozen=True)
class Identity:
    repository_id: str
    source_key: str
    gate: str
    gate_version: str
    toolchain_key: str
    environment_key: str
    command_key: str
    source_check_key: str
    lane: str

    def execution_key(self, host_id: str) -> str:
        return digest_json(
            {
                "protocol_version": PROTOCOL_VERSION,
                "host_id": host_id,
                "repository_id": self.repository_id,
                "source_key": self.source_key,
                "gate": self.gate,
                "gate_version": self.gate_version,
                "toolchain_key": self.toolchain_key,
                "environment_key": self.environment_key,
                "command_key": self.command_key,
                "source_check_key": self.source_check_key,
                "lane": self.lane,
            }
        )


@dataclass(frozen=True)
class Invocation:
    cwd: Path
    command: tuple[str, ...]
    source_check: tuple[str, ...]
    environment: Mapping[str, str]
    share_clean_candidate: bool
    attribution_json: str


@dataclass(frozen=True)
class Submission:
    execution_id: str
    request_id: str
    joined: bool
    runner_nonce: str


def digest_json(value: Any) -> str:
    encoded = json.dumps(
        value, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    ).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def default_host_id() -> str:
    return f"{socket.gethostname()}:{uuid.getnode():012x}"


def default_state_dir() -> Path:
    override = os.environ.get("CELL_CI_BROKER_STATE_DIR")
    if override:
        return Path(override).expanduser()
    if sys.platform == "darwin":
        return Path.home() / "Library" / "Application Support" / "Cell" / "ci-broker"
    xdg_state = os.environ.get("XDG_STATE_HOME")
    if xdg_state:
        return Path(xdg_state).expanduser() / "cell" / "ci-broker"
    return Path.home() / ".local" / "state" / "cell" / "ci-broker"


def require_text(label: str, value: str, maximum: int = 1024) -> str:
    if not value or not value.strip():
        raise BrokerError(f"{label} must not be empty")
    if len(value) > maximum:
        raise BrokerError(f"{label} is longer than {maximum} characters")
    if "\x00" in value:
        raise BrokerError(f"{label} contains a NUL byte")
    return value


def canonical_attribution(raw: str | None) -> str:
    if raw is None:
        return "{}"
    try:
        value = json.loads(raw)
    except json.JSONDecodeError as error:
        raise BrokerError(f"invalid --attribution-json: {error}") from error
    if not isinstance(value, dict):
        raise BrokerError("--attribution-json must be a JSON object")
    encoded = json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
    if len(encoded.encode("utf-8")) > 16 * 1024:
        raise BrokerError("--attribution-json is larger than 16 KiB")
    return encoded


def parse_json_argv(label: str, raw: str) -> tuple[str, ...]:
    try:
        value = json.loads(raw)
    except json.JSONDecodeError as error:
        raise BrokerError(f"invalid {label}: {error}") from error
    if not isinstance(value, list) or not value:
        raise BrokerError(f"{label} must be a non-empty JSON string array")
    if not all(isinstance(item, str) and item and "\x00" not in item for item in value):
        raise BrokerError(f"{label} must be a non-empty JSON string array")
    return tuple(value)


def parse_environment_assignments(assignments: Iterable[str]) -> dict[str, str]:
    parsed: dict[str, str] = {}
    for assignment in assignments:
        name, separator, value = assignment.partition("=")
        if not separator or not name or "\x00" in name or "=" in name:
            raise BrokerError(f"invalid --env assignment: {assignment!r}")
        if "\x00" in value:
            raise BrokerError(f"environment value for {name!r} contains a NUL byte")
        parsed[name] = value
    return parsed


def build_environment(
    mode: str,
    inherit_names: Iterable[str],
    assignments: Iterable[str],
    unset_names: Iterable[str],
) -> dict[str, str]:
    if mode == "full":
        environment = {
            name: value
            for name, value in os.environ.items()
            if name not in EPHEMERAL_ENVIRONMENT
            and not name.startswith("CELL_CI_EXECUTION_")
        }
    elif mode == "minimal":
        environment = {
            name: os.environ[name]
            for name in MINIMAL_ENVIRONMENT
            if name in os.environ
        }
    else:
        raise BrokerError(f"unsupported environment mode: {mode}")

    for name in inherit_names:
        require_text("--inherit-env name", name, 255)
        if "=" in name:
            raise BrokerError(f"invalid --inherit-env name: {name!r}")
        if name in os.environ:
            environment[name] = os.environ[name]
        else:
            environment.pop(name, None)

    environment.update(parse_environment_assignments(assignments))
    for name in unset_names:
        environment.pop(name, None)

    return environment


def normalize_argv(argv: Sequence[str], cwd: Path) -> list[str]:
    """Remove only a leading worktree path from command identity arguments."""

    normalized: list[str] = []
    cwd_text = str(cwd.resolve())
    prefix = cwd_text + os.sep
    for argument in argv:
        if argument == cwd_text:
            normalized.append("<worktree>")
        elif argument.startswith(prefix):
            normalized.append("<worktree>/" + argument[len(prefix) :])
        else:
            normalized.append(argument)
    return normalized


def process_token(pid: int) -> str | None:
    """Return a PID-reuse-resistant process start token when inspectable."""

    if pid <= 0:
        return None
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


def same_process_alive(pid: int | None, expected_token: str | None) -> bool:
    if pid is None or expected_token is None:
        return False
    return process_token(pid) == expected_token


def terminate_process_group(pid: int, expected_token: str | None) -> None:
    if not same_process_alive(pid, expected_token):
        return
    try:
        os.killpg(pid, signal.SIGTERM)
    except (ProcessLookupError, PermissionError):
        return
    deadline = time.monotonic() + 2.0
    while time.monotonic() < deadline:
        if not same_process_alive(pid, expected_token):
            return
        time.sleep(0.05)
    if same_process_alive(pid, expected_token):
        with contextlib.suppress(ProcessLookupError, PermissionError):
            os.killpg(pid, signal.SIGKILL)


class Broker:
    def __init__(self, scope: Scope, poll_interval: float = DEFAULT_POLL_INTERVAL):
        if scope.light_slots < 1:
            raise BrokerError("light slot count must be at least one")
        if scope.cargo_jobs < 1:
            raise BrokerError("Cargo job count must be at least one")
        if scope.heartbeat_timeout <= 0:
            raise BrokerError("heartbeat timeout must be positive")
        if poll_interval <= 0 or poll_interval >= scope.heartbeat_timeout:
            raise BrokerError("poll interval must be positive and less than heartbeat timeout")
        self.scope = scope
        self.poll_interval = poll_interval
        self._initialize()
        self.prune()

    def _connect(self) -> sqlite3.Connection:
        try:
            connection = sqlite3.connect(
                self.scope.database_path,
                timeout=5.0,
                isolation_level=None,
            )
            connection.row_factory = sqlite3.Row
            connection.execute("PRAGMA foreign_keys = ON")
            connection.execute("PRAGMA busy_timeout = 5000")
            return connection
        except sqlite3.Error as error:
            raise BrokerError(f"cannot open broker journal: {error}") from error

    def _initialize(self) -> None:
        try:
            self.scope.state_dir.mkdir(mode=0o700, parents=True, exist_ok=True)
            os.chmod(self.scope.state_dir, 0o700)
        except OSError as error:
            raise BrokerError(f"cannot create broker state directory: {error}") from error

        try:
            with contextlib.closing(self._connect()) as connection:
                connection.execute("PRAGMA journal_mode = WAL")
                connection.execute("PRAGMA synchronous = FULL")
                connection.executescript(
                    """
                    CREATE TABLE IF NOT EXISTS meta (
                        key TEXT PRIMARY KEY,
                        value TEXT NOT NULL
                    );

                    CREATE TABLE IF NOT EXISTS executions (
                        id TEXT PRIMARY KEY,
                        execution_key TEXT NOT NULL,
                        shareable INTEGER NOT NULL CHECK (shareable IN (0, 1)),
                        lane TEXT NOT NULL CHECK (lane IN ('heavy', 'light')),
                        state TEXT NOT NULL CHECK (
                            state IN ('queued', 'running', 'passed', 'failed',
                                      'stale', 'lost', 'cancelled')
                        ),
                        source_key TEXT NOT NULL,
                        gate TEXT NOT NULL,
                        gate_version TEXT NOT NULL,
                        toolchain_key TEXT NOT NULL,
                        environment_key TEXT NOT NULL,
                        command_key TEXT NOT NULL,
                        source_check_key TEXT NOT NULL,
                        created_at REAL NOT NULL,
                        queued_at REAL NOT NULL,
                        started_at REAL,
                        finished_at REAL,
                        runner_request_id TEXT,
                        runner_pid INTEGER,
                        runner_process_token TEXT,
                        runner_nonce TEXT,
                        heartbeat_at REAL,
                        child_pid INTEGER,
                        child_process_token TEXT,
                        exit_code INTEGER,
                        detail TEXT
                    );

                    CREATE UNIQUE INDEX IF NOT EXISTS executions_active_shareable
                        ON executions(execution_key)
                        WHERE shareable = 1 AND state IN ('queued', 'running');

                    CREATE INDEX IF NOT EXISTS executions_lane_queue
                        ON executions(lane, state, queued_at, id);

                    CREATE TABLE IF NOT EXISTS requests (
                        id TEXT PRIMARY KEY,
                        execution_id TEXT NOT NULL REFERENCES executions(id),
                        pid INTEGER NOT NULL,
                        process_token TEXT NOT NULL,
                        nonce TEXT NOT NULL,
                        submitted_at REAL NOT NULL,
                        heartbeat_at REAL NOT NULL,
                        finished_at REAL,
                        result_state TEXT CHECK (
                            result_state IS NULL OR result_state IN
                                ('passed', 'failed', 'stale', 'lost', 'cancelled')
                        ),
                        joined INTEGER NOT NULL CHECK (joined IN (0, 1)),
                        attribution_json TEXT NOT NULL
                    );

                    CREATE INDEX IF NOT EXISTS requests_execution_active
                        ON requests(execution_id, finished_at, heartbeat_at);

                    CREATE TABLE IF NOT EXISTS events (
                        sequence INTEGER PRIMARY KEY AUTOINCREMENT,
                        execution_id TEXT NOT NULL REFERENCES executions(id),
                        state TEXT NOT NULL CHECK (
                            state IN ('queued', 'running', 'passed', 'failed',
                                      'stale', 'lost', 'cancelled')
                        ),
                        occurred_at REAL NOT NULL,
                        detail TEXT
                    );
                    """
                )
                expected = {
                    "schema_version": str(SCHEMA_VERSION),
                    "host_id": self.scope.host_id,
                    "repository_id": self.scope.repository_id,
                    "light_slots": str(self.scope.light_slots),
                    "cargo_jobs": str(self.scope.cargo_jobs),
                    "heartbeat_timeout": repr(self.scope.heartbeat_timeout),
                }
                connection.execute("BEGIN IMMEDIATE")
                try:
                    existing = {
                        row["key"]: row["value"]
                        for row in connection.execute("SELECT key, value FROM meta")
                    }
                    if not existing:
                        connection.executemany(
                            "INSERT INTO meta(key, value) VALUES (?, ?)",
                            expected.items(),
                        )
                    elif existing != expected:
                        differences = sorted(
                            key
                            for key in set(existing) | set(expected)
                            if existing.get(key) != expected.get(key)
                        )
                        raise BrokerError(
                            "broker scope configuration differs for: "
                            + ", ".join(differences)
                        )
                    connection.execute("COMMIT")
                except Exception:
                    connection.execute("ROLLBACK")
                    raise
            os.chmod(self.scope.database_path, 0o600)
        except BrokerError:
            raise
        except (OSError, sqlite3.Error) as error:
            raise BrokerError(f"cannot initialize broker journal: {error}") from error

    def _transaction(self, connection: sqlite3.Connection) -> None:
        try:
            connection.execute("BEGIN IMMEDIATE")
        except sqlite3.Error as error:
            raise BrokerError(f"cannot lock broker journal: {error}") from error

    @staticmethod
    def _event(
        connection: sqlite3.Connection,
        execution_id: str,
        state: str,
        occurred_at: float,
        detail: str | None,
    ) -> None:
        connection.execute(
            "INSERT INTO events(execution_id, state, occurred_at, detail) "
            "VALUES (?, ?, ?, ?)",
            (execution_id, state, occurred_at, detail),
        )

    def recover(self) -> dict[str, int]:
        """Recover abandoned work without ever converting it to success."""

        now = time.time()
        cutoff = now - self.scope.heartbeat_timeout
        expired_request_ids: list[str] = []
        stale_runners: list[sqlite3.Row] = []
        try:
            with contextlib.closing(self._connect()) as connection:
                expired_request_ids = [
                    row["id"]
                    for row in connection.execute(
                        "SELECT id FROM requests "
                        "WHERE finished_at IS NULL AND heartbeat_at < ?",
                        (cutoff,),
                    )
                ]
                stale_runners = list(
                    connection.execute(
                        "SELECT id, runner_pid, runner_process_token, child_pid, "
                        "child_process_token FROM executions "
                        "WHERE state = 'running' AND heartbeat_at < ?",
                        (cutoff,),
                    )
                )
        except sqlite3.Error as error:
            raise BrokerError(f"cannot inspect broker journal for recovery: {error}") from error

        # Request ownership is a renewable lease, not a PID liveness claim.  A
        # live-but-stopped queued requester must expire or it can wedge FIFO
        # forever.  Running executions have the same heartbeat rule below.
        # A runner that has not renewed its lease is lost even if its process is
        # suspended or wedged.  It must never later publish a green result.
        stale_execution_ids = {row["id"] for row in stale_runners}
        children_to_stop: list[tuple[int, str | None]] = []
        recovered = {"lost": 0, "cancelled": 0, "requests": 0}

        try:
            with contextlib.closing(self._connect()) as connection:
                self._transaction(connection)
                try:
                    for request_id in expired_request_ids:
                        cursor = connection.execute(
                            "UPDATE requests SET finished_at = ?, result_state = 'cancelled' "
                            "WHERE id = ? AND finished_at IS NULL AND heartbeat_at < ?",
                            (now, request_id, cutoff),
                        )
                        recovered["requests"] += cursor.rowcount

                    for execution_id in stale_execution_ids:
                        row = connection.execute(
                            "SELECT child_pid, child_process_token FROM executions "
                            "WHERE id = ? AND state = 'running' AND heartbeat_at < ?",
                            (execution_id, cutoff),
                        ).fetchone()
                        if row is None:
                            continue
                        connection.execute(
                            "UPDATE executions SET state = 'lost', finished_at = ?, "
                            "detail = 'runner heartbeat expired' WHERE id = ?",
                            (now, execution_id),
                        )
                        self._event(
                            connection,
                            execution_id,
                            "lost",
                            now,
                            "runner heartbeat expired",
                        )
                        connection.execute(
                            "UPDATE requests SET finished_at = ?, result_state = 'lost' "
                            "WHERE execution_id = ? AND finished_at IS NULL",
                            (now, execution_id),
                        )
                        if row["child_pid"] is not None:
                            children_to_stop.append(
                                (row["child_pid"], row["child_process_token"])
                            )
                        recovered["lost"] += 1

                    orphaned = list(
                        connection.execute(
                            "SELECT e.id FROM executions e "
                            "WHERE e.state = 'queued' AND NOT EXISTS ("
                            "  SELECT 1 FROM requests r "
                            "  WHERE r.execution_id = e.id AND r.finished_at IS NULL"
                            ")"
                        )
                    )
                    for row in orphaned:
                        execution_id = row["id"]
                        connection.execute(
                            "UPDATE executions SET state = 'cancelled', finished_at = ?, "
                            "detail = 'no live requesters' WHERE id = ?",
                            (now, execution_id),
                        )
                        self._event(
                            connection,
                            execution_id,
                            "cancelled",
                            now,
                            "no live requesters",
                        )
                        recovered["cancelled"] += 1
                    connection.execute("COMMIT")
                except Exception:
                    connection.execute("ROLLBACK")
                    raise
        except sqlite3.Error as error:
            raise BrokerError(f"cannot recover broker journal: {error}") from error

        for child_pid, child_token in children_to_stop:
            terminate_process_group(child_pid, child_token)
        return recovered

    def prune(self) -> int:
        """Bound old terminal journal data after a requester grace period."""

        now = time.time()
        grace_cutoff = now - self.scope.heartbeat_timeout
        age_cutoff = now - TERMINAL_MAX_AGE_SECONDS
        try:
            with contextlib.closing(self._connect()) as connection:
                terminal = list(
                    connection.execute(
                        "SELECT id, finished_at FROM executions "
                        "WHERE state IN ('passed', 'failed', 'stale', 'lost', 'cancelled') "
                        "AND finished_at IS NOT NULL AND finished_at < ? "
                        "ORDER BY finished_at DESC, id DESC",
                        (grace_cutoff,),
                    )
                )
                doomed = [
                    row["id"]
                    for index, row in enumerate(terminal)
                    if index >= MAX_TERMINAL_EXECUTIONS
                    or row["finished_at"] < age_cutoff
                ]
                if not doomed:
                    return 0
                self._transaction(connection)
                try:
                    placeholders = ",".join("?" for _ in doomed)
                    connection.execute(
                        f"DELETE FROM requests WHERE execution_id IN ({placeholders})",
                        doomed,
                    )
                    connection.execute(
                        f"DELETE FROM events WHERE execution_id IN ({placeholders})",
                        doomed,
                    )
                    connection.execute(
                        f"DELETE FROM executions WHERE id IN ({placeholders})",
                        doomed,
                    )
                    connection.execute("COMMIT")
                except Exception:
                    connection.execute("ROLLBACK")
                    raise
                return len(doomed)
        except sqlite3.Error as error:
            raise BrokerError(f"cannot prune broker journal: {error}") from error

    def submit(self, identity: Identity, invocation: Invocation) -> Submission:
        now = time.time()
        execution_id = uuid.uuid4().hex
        request_id = uuid.uuid4().hex
        runner_nonce = uuid.uuid4().hex
        pid = os.getpid()
        token = process_token(pid)
        if token is None:
            raise BrokerError("cannot establish a process identity for broker ownership")
        execution_key = identity.execution_key(self.scope.host_id)
        joined = False

        try:
            with contextlib.closing(self._connect()) as connection:
                self._transaction(connection)
                try:
                    if invocation.share_clean_candidate:
                        active = connection.execute(
                            "SELECT id FROM executions WHERE execution_key = ? "
                            "AND shareable = 1 AND state IN ('queued', 'running')",
                            (execution_key,),
                        ).fetchone()
                    else:
                        active = None

                    if active is not None:
                        execution_id = active["id"]
                        joined = True
                    else:
                        try:
                            connection.execute(
                                "INSERT INTO executions("
                                "id, execution_key, shareable, lane, state, source_key, "
                                "gate, gate_version, toolchain_key, environment_key, "
                                "command_key, source_check_key, created_at, queued_at"
                                ") VALUES (?, ?, ?, ?, 'queued', ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                                (
                                    execution_id,
                                    execution_key,
                                    int(invocation.share_clean_candidate),
                                    identity.lane,
                                    identity.source_key,
                                    identity.gate,
                                    identity.gate_version,
                                    identity.toolchain_key,
                                    identity.environment_key,
                                    identity.command_key,
                                    identity.source_check_key,
                                    now,
                                    now,
                                ),
                            )
                            self._event(connection, execution_id, "queued", now, None)
                        except sqlite3.IntegrityError:
                            if not invocation.share_clean_candidate:
                                raise
                            active = connection.execute(
                                "SELECT id FROM executions WHERE execution_key = ? "
                                "AND shareable = 1 AND state IN ('queued', 'running')",
                                (execution_key,),
                            ).fetchone()
                            if active is None:
                                raise
                            execution_id = active["id"]
                            joined = True

                    connection.execute(
                        "INSERT INTO requests("
                        "id, execution_id, pid, process_token, nonce, submitted_at, "
                        "heartbeat_at, joined, attribution_json"
                        ") VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                        (
                            request_id,
                            execution_id,
                            pid,
                            token,
                            runner_nonce,
                            now,
                            now,
                            int(joined),
                            invocation.attribution_json,
                        ),
                    )
                    connection.execute("COMMIT")
                except Exception:
                    connection.execute("ROLLBACK")
                    raise
        except sqlite3.Error as error:
            raise BrokerError(f"cannot submit broker request: {error}") from error

        return Submission(execution_id, request_id, joined, runner_nonce)

    def record_initial_stale(
        self,
        identity: Identity,
        invocation: Invocation,
        detail: str,
    ) -> dict[str, Any]:
        """Journal a candidate that was already stale before admission."""

        now = time.time()
        execution_id = uuid.uuid4().hex
        request_id = uuid.uuid4().hex
        pid = os.getpid()
        token = process_token(pid)
        if token is None:
            raise BrokerError("cannot establish a process identity for broker ownership")
        try:
            with contextlib.closing(self._connect()) as connection:
                self._transaction(connection)
                try:
                    connection.execute(
                        "INSERT INTO executions("
                        "id, execution_key, shareable, lane, state, source_key, gate, "
                        "gate_version, toolchain_key, environment_key, command_key, "
                        "source_check_key, created_at, queued_at, finished_at, detail"
                        ") VALUES (?, ?, 0, ?, 'stale', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                        (
                            execution_id,
                            identity.execution_key(self.scope.host_id),
                            identity.lane,
                            identity.source_key,
                            identity.gate,
                            identity.gate_version,
                            identity.toolchain_key,
                            identity.environment_key,
                            identity.command_key,
                            identity.source_check_key,
                            now,
                            now,
                            now,
                            detail,
                        ),
                    )
                    self._event(connection, execution_id, "stale", now, detail)
                    connection.execute(
                        "INSERT INTO requests("
                        "id, execution_id, pid, process_token, nonce, submitted_at, "
                        "heartbeat_at, finished_at, result_state, joined, attribution_json"
                        ") VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'stale', 0, ?)",
                        (
                            request_id,
                            execution_id,
                            pid,
                            token,
                            uuid.uuid4().hex,
                            now,
                            now,
                            now,
                            invocation.attribution_json,
                        ),
                    )
                    connection.execute("COMMIT")
                except Exception:
                    connection.execute("ROLLBACK")
                    raise
        except sqlite3.Error as error:
            raise BrokerError(f"cannot journal stale request: {error}") from error
        return self.receipt(execution_id, request_id, False, local_state="stale")

    def _try_claim(self, submission: Submission, lane: str) -> bool:
        now = time.time()
        capacity = LANE_CAPACITY.get(lane, self.scope.light_slots)
        try:
            with contextlib.closing(self._connect()) as connection:
                self._transaction(connection)
                try:
                    request = connection.execute(
                        "SELECT finished_at FROM requests WHERE id = ?",
                        (submission.request_id,),
                    ).fetchone()
                    if request is None or request["finished_at"] is not None:
                        connection.execute("COMMIT")
                        return False
                    connection.execute(
                        "UPDATE requests SET heartbeat_at = ? WHERE id = ?",
                        (now, submission.request_id),
                    )
                    execution = connection.execute(
                        "SELECT state, lane FROM executions WHERE id = ?",
                        (submission.execution_id,),
                    ).fetchone()
                    if execution is None or execution["state"] != "queued":
                        connection.execute("COMMIT")
                        return False
                    first = connection.execute(
                        "SELECT id FROM executions WHERE lane = ? AND state = 'queued' "
                        "ORDER BY queued_at, id LIMIT 1",
                        (lane,),
                    ).fetchone()
                    running = connection.execute(
                        "SELECT COUNT(*) AS count FROM executions "
                        "WHERE lane = ? AND state = 'running'",
                        (lane,),
                    ).fetchone()["count"]
                    if (
                        first is None
                        or first["id"] != submission.execution_id
                        or running >= capacity
                    ):
                        connection.execute("COMMIT")
                        return False
                    cursor = connection.execute(
                        "UPDATE executions SET state = 'running', started_at = ?, "
                        "runner_request_id = ?, runner_pid = ?, runner_process_token = ?, "
                        "runner_nonce = ?, heartbeat_at = ? "
                        "WHERE id = ? AND state = 'queued'",
                        (
                            now,
                            submission.request_id,
                            os.getpid(),
                            process_token(os.getpid()),
                            submission.runner_nonce,
                            now,
                            submission.execution_id,
                        ),
                    )
                    if cursor.rowcount == 1:
                        self._event(
                            connection, submission.execution_id, "running", now, None
                        )
                    connection.execute("COMMIT")
                    return cursor.rowcount == 1
                except Exception:
                    connection.execute("ROLLBACK")
                    raise
        except sqlite3.Error as error:
            raise BrokerError(f"cannot acquire broker lane: {error}") from error

    def _heartbeat(self, submission: Submission) -> str:
        now = time.time()
        try:
            with contextlib.closing(self._connect()) as connection:
                self._transaction(connection)
                try:
                    connection.execute(
                        "UPDATE requests SET heartbeat_at = ? "
                        "WHERE id = ? AND finished_at IS NULL",
                        (now, submission.request_id),
                    )
                    connection.execute(
                        "UPDATE executions SET heartbeat_at = ? "
                        "WHERE id = ? AND state = 'running' "
                        "AND runner_request_id = ? AND runner_nonce = ?",
                        (
                            now,
                            submission.execution_id,
                            submission.request_id,
                            submission.runner_nonce,
                        ),
                    )
                    row = connection.execute(
                        "SELECT state FROM executions WHERE id = ?",
                        (submission.execution_id,),
                    ).fetchone()
                    connection.execute("COMMIT")
                except Exception:
                    connection.execute("ROLLBACK")
                    raise
        except sqlite3.Error as error:
            raise BrokerError(f"cannot renew broker ownership: {error}") from error
        if row is None:
            raise BrokerError("broker execution disappeared")
        return row["state"]

    def _record_child(
        self,
        submission: Submission,
        child_pid: int,
        child_token: str,
    ) -> None:
        now = time.time()
        try:
            with contextlib.closing(self._connect()) as connection:
                self._transaction(connection)
                try:
                    cursor = connection.execute(
                        "UPDATE executions SET child_pid = ?, child_process_token = ?, "
                        "heartbeat_at = ? WHERE id = ? AND state = 'running' "
                        "AND runner_request_id = ? AND runner_nonce = ?",
                        (
                            child_pid,
                            child_token,
                            now,
                            submission.execution_id,
                            submission.request_id,
                            submission.runner_nonce,
                        ),
                    )
                    if cursor.rowcount != 1:
                        raise BrokerError("broker ownership was lost before body start")
                    connection.execute("COMMIT")
                except Exception:
                    connection.execute("ROLLBACK")
                    raise
        except sqlite3.Error as error:
            raise BrokerError(f"cannot record broker worker: {error}") from error

    def _finish(
        self,
        submission: Submission,
        state: str,
        exit_code: int | None,
        detail: str | None,
    ) -> bool:
        if state not in TERMINAL_STATES:
            raise BrokerError(f"invalid terminal state: {state}")
        now = time.time()
        try:
            with contextlib.closing(self._connect()) as connection:
                self._transaction(connection)
                try:
                    cursor = connection.execute(
                        "UPDATE executions SET state = ?, finished_at = ?, exit_code = ?, "
                        "detail = ?, heartbeat_at = ? WHERE id = ? AND state = 'running' "
                        "AND runner_request_id = ? AND runner_nonce = ?",
                        (
                            state,
                            now,
                            exit_code,
                            detail,
                            now,
                            submission.execution_id,
                            submission.request_id,
                            submission.runner_nonce,
                        ),
                    )
                    if cursor.rowcount == 1:
                        self._event(
                            connection,
                            submission.execution_id,
                            state,
                            now,
                            detail,
                        )
                        connection.execute(
                            "UPDATE requests SET finished_at = ?, result_state = ? "
                            "WHERE execution_id = ? AND finished_at IS NULL",
                            (now, state, submission.execution_id),
                        )
                    connection.execute("COMMIT")
                    return cursor.rowcount == 1
                except Exception:
                    connection.execute("ROLLBACK")
                    raise
        except sqlite3.Error as error:
            raise BrokerError(f"cannot finish broker execution: {error}") from error

    def _finish_local_request(self, request_id: str, state: str) -> None:
        now = time.time()
        try:
            with contextlib.closing(self._connect()) as connection:
                connection.execute(
                    "UPDATE requests SET finished_at = COALESCE(finished_at, ?), "
                    "result_state = ? WHERE id = ?",
                    (now, state, request_id),
                )
        except sqlite3.Error as error:
            raise BrokerError(f"cannot finish broker request: {error}") from error

    def cancel_request(self, submission: Submission) -> None:
        now = time.time()
        child_to_stop: tuple[int, str | None] | None = None
        try:
            with contextlib.closing(self._connect()) as connection:
                self._transaction(connection)
                try:
                    connection.execute(
                        "UPDATE requests SET finished_at = ?, result_state = 'cancelled' "
                        "WHERE id = ? AND finished_at IS NULL",
                        (now, submission.request_id),
                    )
                    execution = connection.execute(
                        "SELECT state, runner_request_id, runner_nonce, child_pid, "
                        "child_process_token FROM executions WHERE id = ?",
                        (submission.execution_id,),
                    ).fetchone()
                    should_cancel = False
                    if execution is not None and execution["state"] == "running":
                        should_cancel = (
                            execution["runner_request_id"] == submission.request_id
                            and execution["runner_nonce"] == submission.runner_nonce
                        )
                        if should_cancel and execution["child_pid"] is not None:
                            child_to_stop = (
                                execution["child_pid"],
                                execution["child_process_token"],
                            )
                    elif execution is not None and execution["state"] == "queued":
                        live = connection.execute(
                            "SELECT COUNT(*) AS count FROM requests "
                            "WHERE execution_id = ? AND finished_at IS NULL",
                            (submission.execution_id,),
                        ).fetchone()["count"]
                        should_cancel = live == 0
                    if should_cancel:
                        connection.execute(
                            "UPDATE executions SET state = 'cancelled', finished_at = ?, "
                            "detail = 'request cancelled' WHERE id = ? "
                            "AND state IN ('queued', 'running')",
                            (now, submission.execution_id),
                        )
                        self._event(
                            connection,
                            submission.execution_id,
                            "cancelled",
                            now,
                            "request cancelled",
                        )
                        connection.execute(
                            "UPDATE requests SET finished_at = ?, result_state = 'cancelled' "
                            "WHERE execution_id = ? AND finished_at IS NULL",
                            (now, submission.execution_id),
                        )
                    connection.execute("COMMIT")
                except Exception:
                    connection.execute("ROLLBACK")
                    raise
        except sqlite3.Error as error:
            raise BrokerError(f"cannot cancel broker request: {error}") from error
        if child_to_stop is not None:
            terminate_process_group(*child_to_stop)

    def execution_state(self, execution_id: str) -> str:
        try:
            with contextlib.closing(self._connect()) as connection:
                row = connection.execute(
                    "SELECT state FROM executions WHERE id = ?", (execution_id,)
                ).fetchone()
        except sqlite3.Error as error:
            raise BrokerError(f"cannot read broker execution: {error}") from error
        if row is None:
            raise BrokerError(f"unknown execution: {execution_id}")
        return row["state"]

    def receipt(
        self,
        execution_id: str,
        request_id: str | None,
        joined: bool | None,
        local_state: str | None = None,
        include_events: bool = False,
    ) -> dict[str, Any]:
        try:
            with contextlib.closing(self._connect()) as connection:
                row = connection.execute(
                    "SELECT * FROM executions WHERE id = ?", (execution_id,)
                ).fetchone()
                if row is None:
                    raise BrokerError(f"unknown execution: {execution_id}")
                receipt = {
                    "protocol_version": PROTOCOL_VERSION,
                    "execution_id": row["id"],
                    "request_id": request_id,
                    "state": local_state or row["state"],
                    "execution_state": row["state"],
                    "joined": joined,
                    "lane": row["lane"],
                    "source_key": row["source_key"],
                    "gate": row["gate"],
                    "gate_version": row["gate_version"],
                    "toolchain_key": row["toolchain_key"],
                    "environment_key": row["environment_key"],
                    "command_key": row["command_key"],
                    "execution_key": row["execution_key"],
                    "created_at": row["created_at"],
                    "started_at": row["started_at"],
                    "finished_at": row["finished_at"],
                    "exit_code": row["exit_code"],
                    "detail": row["detail"],
                }
                if include_events:
                    receipt["events"] = [
                        {
                            "sequence": event["sequence"],
                            "state": event["state"],
                            "occurred_at": event["occurred_at"],
                            "detail": event["detail"],
                        }
                        for event in connection.execute(
                            "SELECT sequence, state, occurred_at, detail FROM events "
                            "WHERE execution_id = ? ORDER BY sequence",
                            (execution_id,),
                        )
                    ]
                return receipt
        except sqlite3.Error as error:
            raise BrokerError(f"cannot build broker receipt: {error}") from error

    def wait_and_run(
        self,
        submission: Submission,
        identity: Identity,
        invocation: Invocation,
    ) -> dict[str, Any]:
        announced_running = False
        try:
            while True:
                self.recover()
                state = self.execution_state(submission.execution_id)
                if state in TERMINAL_STATES:
                    local_state = state
                    if state == "passed":
                        source_ok, _ = check_source(
                            invocation.cwd,
                            invocation.source_check,
                            invocation.environment,
                            identity.source_key,
                        )
                        if not source_ok:
                            local_state = "stale"
                    self._finish_local_request(submission.request_id, local_state)
                    return self.receipt(
                        submission.execution_id,
                        submission.request_id,
                        submission.joined,
                        local_state=local_state,
                    )

                if state == "queued" and self._try_claim(submission, identity.lane):
                    if not announced_running:
                        print(
                            f"cell-ci: running {identity.gate} "
                            f"({submission.execution_id})",
                            file=sys.stderr,
                            flush=True,
                        )
                        announced_running = True
                    self._run_body(submission, identity, invocation)
                    continue

                self._heartbeat(submission)
                time.sleep(self.poll_interval)
        except KeyboardInterrupt:
            self.cancel_request(submission)
            return self.receipt(
                submission.execution_id,
                submission.request_id,
                submission.joined,
                local_state="cancelled",
            )

    def _run_body(
        self,
        submission: Submission,
        identity: Identity,
        invocation: Invocation,
    ) -> None:
        source_ok, source_detail = check_source(
            invocation.cwd,
            invocation.source_check,
            invocation.environment,
            identity.source_key,
        )
        if not source_ok:
            self._finish(submission, "stale", None, source_detail)
            return

        if os.name != "posix":
            self._finish(
                submission,
                "failed",
                None,
                "the fail-closed body gate requires a POSIX host",
            )
            return

        read_fd, write_fd = os.pipe()
        child: subprocess.Popen[Any] | None = None
        cancelled = False
        previous_handlers: dict[int, Any] = {}

        def request_cancel(_signum: int, _frame: Any) -> None:
            nonlocal cancelled
            cancelled = True

        for signum in (signal.SIGINT, signal.SIGTERM):
            previous_handlers[signum] = signal.getsignal(signum)
            signal.signal(signum, request_cancel)

        try:
            helper = (
                sys.executable,
                str(Path(__file__).resolve()),
                "_exec-body",
                "--start-fd",
                str(read_fd),
                "--",
                *invocation.command,
            )
            try:
                child = subprocess.Popen(
                    helper,
                    cwd=invocation.cwd,
                    env=dict(invocation.environment),
                    pass_fds=(read_fd,),
                    start_new_session=True,
                )
            except OSError as error:
                self._finish(
                    submission,
                    "failed",
                    127,
                    f"body spawn failed: {error.__class__.__name__}",
                )
                return
            finally:
                os.close(read_fd)

            child_token = process_token(child.pid)
            if child_token is None:
                with contextlib.suppress(ProcessLookupError):
                    child.terminate()
                self._finish(
                    submission,
                    "failed",
                    None,
                    "cannot establish body process identity",
                )
                return
            try:
                self._record_child(submission, child.pid, child_token)
                os.write(write_fd, b"1")
            except (BrokerError, OSError):
                terminate_process_group(child.pid, child_token)
                raise
            finally:
                os.close(write_fd)
                write_fd = -1

            while child.poll() is None:
                if cancelled:
                    terminate_process_group(child.pid, child_token)
                    child.wait()
                    self._finish(submission, "cancelled", child.returncode, "request cancelled")
                    return
                try:
                    state = self._heartbeat(submission)
                except BrokerError:
                    terminate_process_group(child.pid, child_token)
                    child.wait()
                    raise
                if state != "running":
                    terminate_process_group(child.pid, child_token)
                    child.wait()
                    return
                time.sleep(self.poll_interval)

            exit_code = child.returncode
            source_ok, source_detail = check_source(
                invocation.cwd,
                invocation.source_check,
                invocation.environment,
                identity.source_key,
            )
            if not source_ok:
                state = "stale"
                detail = source_detail
            elif exit_code == 0:
                state = "passed"
                detail = None
            else:
                state = "failed"
                detail = "body exited nonzero"
            self._finish(submission, state, exit_code, detail)
        finally:
            if write_fd >= 0:
                with contextlib.suppress(OSError):
                    os.close(write_fd)
            for signum, handler in previous_handlers.items():
                signal.signal(signum, handler)


def check_source(
    cwd: Path,
    command: Sequence[str],
    environment: Mapping[str, str],
    expected_source_key: str,
) -> tuple[bool, str | None]:
    try:
        result = subprocess.run(
            command,
            cwd=cwd,
            env=dict(environment),
            check=False,
            capture_output=True,
            text=True,
            timeout=SOURCE_CHECK_TIMEOUT,
        )
    except (OSError, subprocess.SubprocessError) as error:
        return False, f"source check failed: {error.__class__.__name__}"
    if result.returncode != 0:
        return False, "source check exited nonzero"
    if result.stdout.strip() != expected_source_key:
        return False, "source identity changed"
    return True, None


def receipt_exit_code(receipt: Mapping[str, Any]) -> int:
    state = receipt["state"]
    if state == "passed":
        return 0
    if state == "failed":
        body_code = receipt.get("exit_code")
        if isinstance(body_code, int) and 1 <= body_code <= 125:
            return body_code
        return 1
    if state == "stale":
        return 75
    if state == "cancelled":
        return 130
    return 70


def build_scope(arguments: argparse.Namespace) -> Scope:
    repository_id = require_text("--repo-id", arguments.repo_id, 255)
    host_id = require_text("--host-id", arguments.host_id, 255)
    return Scope(
        host_id=host_id,
        repository_id=repository_id,
        state_dir=Path(arguments.state_dir).expanduser().resolve(),
        light_slots=arguments.light_slots,
        cargo_jobs=arguments.cargo_jobs,
        heartbeat_timeout=arguments.heartbeat_timeout,
    )


def add_scope_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--repo-id", required=True, help="stable logical repository ID")
    parser.add_argument("--host-id", default=default_host_id())
    parser.add_argument("--state-dir", default=str(default_state_dir()))
    parser.add_argument("--light-slots", type=int, default=DEFAULT_LIGHT_SLOTS)
    parser.add_argument("--cargo-jobs", type=int, default=DEFAULT_CARGO_JOBS)
    parser.add_argument(
        "--heartbeat-timeout", type=float, default=DEFAULT_HEARTBEAT_TIMEOUT
    )
    parser.add_argument("--poll-interval", type=float, default=DEFAULT_POLL_INTERVAL)


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(description=__doc__)
    commands = root.add_subparsers(dest="subcommand", required=True)

    run = commands.add_parser("run", help="submit, wait for, and report one CI body")
    add_scope_arguments(run)
    run.add_argument("--source-key", required=True)
    run.add_argument("--gate", required=True)
    run.add_argument("--gate-version", required=True)
    run.add_argument("--toolchain-key", required=True)
    run.add_argument("--lane", choices=("heavy", "light"), required=True)
    run.add_argument(
        "--share-clean-candidate",
        action="store_true",
        help="single-flight this exact, immutable clean candidate while in flight",
    )
    run.add_argument("--cwd", default=os.getcwd())
    run.add_argument(
        "--identity-root",
        help="normalize worktree-local command paths against this root",
    )
    run.add_argument(
        "--source-check-json",
        required=True,
        help="JSON argv that prints the current source key",
    )
    run.add_argument(
        "--environment-mode", choices=("full", "minimal"), default="full"
    )
    run.add_argument("--inherit-env", action="append", default=[])
    run.add_argument("--env", action="append", default=[])
    run.add_argument("--unset-env", action="append", default=[])
    run.add_argument("--attribution-json")
    run.add_argument(
        "--quiet-success",
        action="store_true",
        help="omit the JSON receipt only when the final result is passed",
    )
    run.add_argument("command", nargs=argparse.REMAINDER)

    status = commands.add_parser("status", help="read one durable execution receipt")
    add_scope_arguments(status)
    status.add_argument("--events", action="store_true")
    status.add_argument("execution_id")

    recover = commands.add_parser("recover", help="mark abandoned work lost/cancelled")
    add_scope_arguments(recover)

    private = commands.add_parser("_exec-body", help=argparse.SUPPRESS)
    private.add_argument("--start-fd", type=int, required=True)
    private.add_argument("command", nargs=argparse.REMAINDER)
    return root


def strip_separator(command: Sequence[str]) -> tuple[str, ...]:
    values = tuple(command)
    if values and values[0] == "--":
        values = values[1:]
    if not values:
        raise BrokerError("a body command is required after --")
    return values


def run_command(arguments: argparse.Namespace) -> int:
    scope = build_scope(arguments)
    broker = Broker(scope, poll_interval=arguments.poll_interval)
    cwd = Path(arguments.cwd).expanduser().resolve()
    if not cwd.is_dir():
        raise BrokerError(f"working directory does not exist: {cwd}")
    identity_root = (
        Path(arguments.identity_root).expanduser().resolve()
        if arguments.identity_root is not None
        else cwd
    )
    if not identity_root.is_dir():
        raise BrokerError(f"identity root does not exist: {identity_root}")
    try:
        identity_cwd = cwd.relative_to(identity_root)
    except ValueError as error:
        raise BrokerError("working directory must be inside the identity root") from error
    command = strip_separator(arguments.command)
    source_check = parse_json_argv("--source-check-json", arguments.source_check_json)
    environment = build_environment(
        arguments.environment_mode,
        arguments.inherit_env,
        arguments.env,
        arguments.unset_env,
    )
    environment["CARGO_BUILD_JOBS"] = str(scope.cargo_jobs)
    identity = Identity(
        repository_id=scope.repository_id,
        source_key=require_text("--source-key", arguments.source_key),
        gate=require_text("--gate", arguments.gate, 255),
        gate_version=require_text("--gate-version", arguments.gate_version),
        toolchain_key=require_text("--toolchain-key", arguments.toolchain_key),
        environment_key=digest_json(sorted(environment.items())),
        command_key=digest_json(
            {
                "argv": normalize_argv(command, identity_root),
                "cwd": identity_cwd.as_posix() or ".",
            }
        ),
        source_check_key=digest_json(normalize_argv(source_check, identity_root)),
        lane=arguments.lane,
    )
    invocation = Invocation(
        cwd=cwd,
        command=command,
        source_check=source_check,
        environment=environment,
        share_clean_candidate=arguments.share_clean_candidate,
        attribution_json=canonical_attribution(arguments.attribution_json),
    )

    source_ok, source_detail = check_source(
        invocation.cwd,
        invocation.source_check,
        invocation.environment,
        identity.source_key,
    )
    if not source_ok:
        receipt = broker.record_initial_stale(
            identity, invocation, source_detail or "source identity changed"
        )
        print(json.dumps(receipt, sort_keys=True), flush=True)
        return receipt_exit_code(receipt)

    broker.recover()
    submission = broker.submit(identity, invocation)
    action = "joined" if submission.joined else "queued"
    print(
        f"cell-ci: {action} {identity.gate} ({submission.execution_id})",
        file=sys.stderr,
        flush=True,
    )
    receipt = broker.wait_and_run(submission, identity, invocation)
    if not (arguments.quiet_success and receipt["state"] == "passed"):
        print(json.dumps(receipt, sort_keys=True), flush=True)
    return receipt_exit_code(receipt)


def status_command(arguments: argparse.Namespace) -> int:
    broker = Broker(build_scope(arguments), poll_interval=arguments.poll_interval)
    broker.recover()
    receipt = broker.receipt(
        arguments.execution_id,
        request_id=None,
        joined=None,
        include_events=arguments.events,
    )
    print(json.dumps(receipt, sort_keys=True), flush=True)
    return 0


def recover_command(arguments: argparse.Namespace) -> int:
    broker = Broker(build_scope(arguments), poll_interval=arguments.poll_interval)
    result = broker.recover()
    print(json.dumps(result, sort_keys=True), flush=True)
    return 0


def exec_body_command(arguments: argparse.Namespace) -> int:
    command = strip_separator(arguments.command)
    try:
        start = os.read(arguments.start_fd, 1)
    finally:
        os.close(arguments.start_fd)
    if start != b"1":
        return 125
    try:
        os.execvpe(command[0], command, os.environ)
    except OSError:
        return 127
    return 127


def main(argv: Sequence[str] | None = None) -> int:
    arguments = parser().parse_args(argv)
    try:
        if arguments.subcommand == "run":
            return run_command(arguments)
        if arguments.subcommand == "status":
            return status_command(arguments)
        if arguments.subcommand == "recover":
            return recover_command(arguments)
        if arguments.subcommand == "_exec-body":
            return exec_body_command(arguments)
        raise BrokerError(f"unsupported subcommand: {arguments.subcommand}")
    except BrokerError as error:
        print(f"cell-ci: broker error: {error}", file=sys.stderr)
        return 78


if __name__ == "__main__":
    raise SystemExit(main())
