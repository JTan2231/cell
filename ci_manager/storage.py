"""Private durable records. Git, Nucleus and deployment retain their own authority."""

from __future__ import annotations

import contextlib
import fcntl
import json
import os
from pathlib import Path
import pwd
import sqlite3
import stat
import sys
import time
import uuid

from ci_manager.budget import repair_budget

SCHEMA = 1
TERMINAL = {"succeeded", "failed", "cancelled", "already_included"}


class ManagerError(RuntimeError):
    pass


def home() -> Path:
    return Path(pwd.getpwuid(os.getuid()).pw_dir)


def state_root() -> Path:
    if sys.platform == "darwin":
        return home() / "Library/Application Support/Cell/ci-manager"
    return home() / ".local/state/cell/ci-manager"


def private_directory(path: Path) -> None:
    if path.is_symlink():
        raise ManagerError(f"symbolic state directory: {path}")
    path.mkdir(parents=True, exist_ok=True, mode=0o700)
    info = path.stat()
    if info.st_uid != os.getuid() or stat.S_IMODE(info.st_mode) != 0o700:
        raise ManagerError(f"state directory must be user-owned and mode 0700: {path}")


def atomic_bytes(path: Path, value: bytes) -> None:
    private_directory(path.parent)
    if path.is_symlink():
        raise ManagerError(f"symbolic state file: {path}")
    temporary = path.with_name(f".{path.name}.{uuid.uuid4().hex}")
    try:
        with temporary.open("xb") as stream:
            os.chmod(temporary, 0o600)
            stream.write(value)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
        descriptor = os.open(path.parent, os.O_RDONLY)
        try:
            os.fsync(descriptor)
        finally:
            os.close(descriptor)
    finally:
        temporary.unlink(missing_ok=True)


def atomic_json(path: Path, value: object) -> None:
    atomic_bytes(path, (json.dumps(value, sort_keys=True, indent=2) + "\n").encode())


@contextlib.contextmanager
def lock(path: Path, *, blocking: bool = True):
    private_directory(path.parent)
    descriptor = os.open(path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    try:
        fcntl.flock(descriptor, fcntl.LOCK_EX | (0 if blocking else fcntl.LOCK_NB))
        yield descriptor
    finally:
        os.close(descriptor)


class Store:
    def __init__(self, root: Path, *, create: bool = False):
        self.root = root
        path = root / "queue.sqlite3"
        if not create and not path.is_file():
            raise ManagerError("CI manager is not initialized; run cell-ci init")
        private_directory(root)
        if path.is_symlink():
            raise ManagerError("symbolic queue database")
        self.db = sqlite3.connect(path, timeout=30, isolation_level=None)
        self.db.row_factory = sqlite3.Row
        os.chmod(path, 0o600)
        self.db.execute("PRAGMA synchronous=FULL")
        version = self.db.execute("PRAGMA user_version").fetchone()[0]
        if version == 0 and create:
            with self.transaction():
                self.db.execute("CREATE TABLE control (key TEXT PRIMARY KEY, value TEXT NOT NULL)")
                self.db.execute("""CREATE TABLE jobs (
                    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
                    id TEXT NOT NULL UNIQUE, submission_key TEXT NOT NULL UNIQUE,
                    phase TEXT NOT NULL, cancel_requested INTEGER NOT NULL DEFAULT 0,
                    created REAL NOT NULL, updated REAL NOT NULL, data TEXT NOT NULL)""")
                self.db.execute("CREATE TABLE holds (owner TEXT PRIMARY KEY, created REAL NOT NULL)")
                self.db.execute(f"PRAGMA user_version={SCHEMA}")
                self.set("paused", True)
        elif version != SCHEMA:
            raise ManagerError(f"unsupported CI journal schema {version}; expected {SCHEMA}")

    @contextlib.contextmanager
    def transaction(self):
        self.db.execute("BEGIN IMMEDIATE")
        try:
            yield
            self.db.execute("COMMIT")
        except BaseException:
            self.db.execute("ROLLBACK")
            raise

    def get(self, key: str, default=None):
        row = self.db.execute("SELECT value FROM control WHERE key=?", (key,)).fetchone()
        return json.loads(row[0]) if row else default

    def set(self, key: str, value) -> None:
        self.db.execute("INSERT INTO control VALUES (?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                        (key, json.dumps(value)))

    def job(self, identity: str) -> dict:
        row = self.db.execute("SELECT * FROM jobs WHERE id=?", (identity,)).fetchone()
        if row is None:
            raise ManagerError(f"unknown CI job: {identity}")
        return self.decode(row)

    @staticmethod
    def decode(row) -> dict:
        data = json.loads(row["data"])
        data.update(id=row["id"], sequence=row["sequence"], phase=row["phase"],
                    cancel_requested=bool(row["cancel_requested"]), created=row["created"], updated=row["updated"])
        data["repair_budget"] = repair_budget(data)
        return data

    def save(self, job: dict, phase: str | None = None) -> None:
        if phase is not None:
            job["phase"] = phase
        data = dict(job)
        data.pop("repair_budget", None)
        self.db.execute("UPDATE jobs SET phase=?,data=?,updated=? WHERE id=?",
                        (job["phase"], json.dumps(data, sort_keys=True), time.time(), job["id"]))

    def active(self) -> dict | None:
        rows = self.db.execute("SELECT * FROM jobs WHERE phase NOT IN ('queued','succeeded','failed','cancelled','already_included') ORDER BY sequence").fetchall()
        if len(rows) > 1:
            raise ManagerError("multiple active CI jobs; admission is stopped")
        return self.decode(rows[0]) if rows else None

    def owners(self) -> list[str]:
        return [row[0] for row in self.db.execute("SELECT owner FROM holds ORDER BY owner")]
