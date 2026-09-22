#!/usr/bin/env python3
"""Submission and operation controls for the installed serial CI manager."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time
import uuid

sys.dont_write_bytecode = True
if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from ci_manager import VERSION
from ci_manager import git_ops as git
from ci_manager.storage import ManagerError, Store, TERMINAL, lock, state_root


def positive(value: str) -> int:
    result = int(value)
    if result < 1:
        raise argparse.ArgumentTypeError("must be a positive integer")
    return result


def initialize(args) -> dict:
    root = Path(args.repo).resolve()
    repository = Path(git.value(root, "rev-parse", "--show-toplevel")).resolve()
    baseline = git.commit(repository, args.accepted_baseline)
    with lock(state_root() / "admission.lock"):
        store = Store(state_root(), create=True)
        config = {"repository": str(repository), "common_git_dir": str(git.common(repository)),
                  "policy": {"luna_attempts": args.luna_attempts, "terra_attempts": args.terra_attempts,
                             "model_timeout_seconds": args.model_timeout_seconds}}
        previous = store.get("config")
        if previous is not None:
            if previous != config:
                raise ManagerError("CI manager already has another repository or policy; initialization does not replace it")
            if git.commit(repository, git.ACCEPTED) != baseline:
                raise ManagerError("accepted already advanced; initialization cannot move it")
            return {"state": "initialized", "paused": store.get("paused"), "config": previous}
        existing = git.git(repository, "rev-parse", "--verify", git.ACCEPTED, check=False)
        if existing.returncode == 0 and existing.stdout.decode().strip() != baseline:
            raise ManagerError("the existing accepted ref differs from the selected baseline")
        if existing.returncode:
            git.git(repository, "update-ref", git.ACCEPTED, baseline, "0" * len(baseline))
        with store.transaction():
            store.set("config", config)
            store.set("bootstrap", {"commit": baseline, "recorded_at": time.time(),
                                    "basis": "operator-selected previously validated baseline"})
            store.set("paused", True)
        return {"state": "initialized", "paused": True, "accepted": baseline, "config": config}


def submit(store: Store, args) -> dict:
    root = Path(args.repo or os.getcwd()).resolve()
    config = store.get("config")
    if not config or str(git.common(root)) != config["common_git_dir"]:
        raise ManagerError("submit from a worktree of the configured repository")
    revision = git.commit(root, args.commit)
    products = sorted(set(args.deploy)) if args.deploy is not None else None
    if products is not None and any(not re.fullmatch(r"[a-z][a-z0-9-]*", item) for item in products):
        raise ManagerError("invalid deployment product name")
    request_key = args.request_id or uuid.uuid4().hex
    if not 1 <= len(request_key) <= 256:
        raise ManagerError("submission request ID must contain 1 to 256 characters")
    with lock(store.root / "admission.lock"), store.transaction():
        previous = store.db.execute("SELECT * FROM jobs WHERE submission_key=?", (request_key,)).fetchone()
        if previous:
            job = store.decode(previous)
            if job["input_commit"] != revision or job["deploy_products"] != products:
                raise ManagerError("submission request ID belongs to different inputs")
            return job
        identity = uuid.uuid4().hex
        # Pin before acknowledgement. An interrupted, unacknowledged orphan pin
        # is harmless and cannot be mistaken for an admitted queue item.
        git.git(root, "update-ref", git.private_ref(identity, "input"), revision, "0" * len(revision))
        now = time.time()
        data = {"input_commit": revision, "deploy_products": products,
                "policy": config["policy"], "submission_key": request_key}
        store.db.execute("INSERT INTO jobs(id,submission_key,phase,created,updated,data) VALUES (?,?,'queued',?,?,?)",
                         (identity, request_key, now, now, json.dumps(data)))
        return store.job(identity)


def maintenance(store: Store, action: str, owner: str | None) -> dict:
    if action != "status" and (not owner or len(owner) > 256):
        raise ManagerError("maintenance mutation requires an owner of 1 to 256 characters")
    with lock(store.root / "admission.lock"), store.transaction():
        if action == "hold":
            store.db.execute("INSERT OR IGNORE INTO holds VALUES (?,?)", (owner, time.time()))
        elif action == "release":
            store.db.execute("DELETE FROM holds WHERE owner=?", (owner,))
        owners = store.owners()
        active = store.active()
        drained = not active or not active.get("model_unresolved", False)
        return {"held": bool(owners), "drained": drained, "owners": owners,
                "owner_held": owner in owners if owner else None,
                "active_job": active["id"] if active else None}


def status(store: Store, identity: str | None) -> dict:
    if identity:
        return store.job(identity)
    active = store.active()
    return {"version": VERSION, "schema_version": 1, "paused": store.get("paused", True),
            "config": store.get("config"), "active": active, "maintenance_owners": store.owners(),
            "queue": [store.decode(row) for row in store.db.execute(
                "SELECT * FROM jobs WHERE phase='queued' ORDER BY sequence")]}


def main(argv: list[str] | None = None) -> int:
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", action="version", version=VERSION)
    commands = parser.add_subparsers(dest="command", required=True)
    init = commands.add_parser("init", help="select a previously validated bootstrap baseline; start paused")
    init.add_argument("--repo", required=True)
    init.add_argument("--accepted-baseline", required=True)
    init.add_argument("--luna-attempts", type=positive, default=3)
    init.add_argument("--terra-attempts", type=positive, default=1)
    init.add_argument("--model-timeout-seconds", type=positive, default=600)
    submission = commands.add_parser("submit")
    submission.add_argument("commit")
    submission.add_argument("--repo")
    submission.add_argument("--request-id")
    submission.add_argument("--deploy", action="append")
    inspection = commands.add_parser("status")
    inspection.add_argument("job", nargs="?")
    wait = commands.add_parser("wait")
    wait.add_argument("job")
    wait.add_argument("--timeout", type=positive)
    for name in ("cancel", "recover"):
        commands.add_parser(name).add_argument("job")
    for name in ("pause", "resume", "worker", "install"):
        commands.add_parser(name)
    upkeep = commands.add_parser("maintenance")
    upkeep.add_argument("action", choices=("hold", "status", "release"))
    upkeep.add_argument("--owner")
    service = commands.add_parser("service")
    service.add_argument("action", choices=("start", "stop", "status"))
    args = parser.parse_args(argv)
    try:
        if args.command == "init":
            result = initialize(args)
        elif args.command in {"install", "service"}:
            from ci_manager import installation
            result = installation.install() if args.command == "install" else installation.service(args.action)
        else:
            store = Store(state_root())
            if args.command == "worker":
                from ci_manager.manager import Worker
                with lock(store.root / "worker.lock", blocking=False) as descriptor:
                    Worker(store, descriptor).run()
                return 0
            if args.command == "submit":
                result = submit(store, args)
            elif args.command == "status":
                result = status(store, args.job)
            elif args.command == "maintenance":
                result = maintenance(store, args.action, args.owner)
            elif args.command in {"pause", "resume"}:
                with lock(store.root / "admission.lock"), store.transaction():
                    active = store.active()
                    if args.command == "resume" and active and active["phase"] == "blocked":
                        raise ManagerError("recover the blocked job before resuming admission")
                    store.set("paused", args.command == "pause")
                result = status(store, None)
            elif args.command == "cancel":
                with store.transaction():
                    job = store.job(args.job)
                    if job["phase"] not in TERMINAL:
                        store.db.execute("UPDATE jobs SET cancel_requested=1,updated=? WHERE id=?", (time.time(), args.job))
                        if job["phase"] == "queued":
                            job["outcome_message"] = "Cancelled before admission."
                            store.save(job, "cancelled")
                result = store.job(args.job)
            elif args.command == "recover":
                with store.transaction():
                    job = store.job(args.job)
                    if job["phase"] != "blocked":
                        raise ManagerError("recover selects one blocked active job")
                    store.set("recovery_request", args.job)
                result = {"state": "recovery_requested", "job": args.job}
            else:
                deadline = time.monotonic() + args.timeout if args.timeout else None
                while True:
                    result = store.job(args.job)
                    if result["phase"] in TERMINAL or result["phase"] == "blocked":
                        break
                    if deadline is not None and time.monotonic() >= deadline:
                        result = {"state": "timeout", "job": result}
                        break
                    time.sleep(1)
        print(json.dumps(result, sort_keys=True, indent=2))
        return 0
    except (ManagerError, OSError, ValueError, subprocess.SubprocessError) as exception:
        print(json.dumps({"state": "error", "message": str(exception)}))
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
