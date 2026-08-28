#!/usr/bin/env python3
"""Run a reproducible medium-versus-high Annals ingestion experiment."""

from __future__ import annotations

import argparse
import fcntl
import hashlib
import json
import os
import re
import shutil
import sqlite3
import subprocess
import sys
import time
from datetime import UTC, datetime
from pathlib import Path
from typing import Any


ARM_ORDER = ("medium", "high")
EXPECTED_INPUTS = 20


def fail(message: str) -> None:
    raise RuntimeError(message)


def now() -> str:
    return datetime.now(UTC).isoformat().replace("+00:00", "Z")


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def write_json(path: Path, value: Any) -> None:
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(
        json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    os.chmod(temporary, 0o600)
    temporary.replace(path)


def load_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def command(
    arguments: list[str],
    *,
    stdout_path: Path | None = None,
    stderr_path: Path | None = None,
) -> subprocess.CompletedProcess[str]:
    stdout_handle = stdout_path.open("w", encoding="utf-8") if stdout_path else None
    stderr_handle = stderr_path.open("w", encoding="utf-8") if stderr_path else None
    try:
        result = subprocess.run(
            arguments,
            check=False,
            text=True,
            stdout=stdout_handle or subprocess.PIPE,
            stderr=stderr_handle or subprocess.PIPE,
        )
    finally:
        if stdout_handle:
            stdout_handle.close()
        if stderr_handle:
            stderr_handle.close()
    return result


def checked(arguments: list[str], *, output: Path | None = None) -> str:
    result = command(arguments, stdout_path=output)
    if result.returncode != 0:
        detail = result.stderr.strip() if result.stderr else "no diagnostic output"
        fail(f"command failed ({result.returncode}): {' '.join(arguments)}\n{detail}")
    return result.stdout or ""


def json_command(arguments: list[str], *, output: Path | None = None) -> Any:
    text = checked(arguments, output=output)
    if output:
        text = output.read_text(encoding="utf-8")
    payload = json.loads(text)
    if not payload.get("ok"):
        fail(f"Annals returned an error: {payload}")
    return payload["data"]


def render_transcript(source: Path, label: str) -> tuple[str, dict[str, Any]]:
    pieces = [
        f"# {label}",
        "",
        "_Recovered from a local Codex session. This transcript includes only "
        "human-visible user and assistant messages._",
        "",
    ]
    user_messages = 0
    assistant_messages = 0
    session_id = None
    session_timestamp = None

    with source.open(encoding="utf-8") as handle:
        for line_number, line in enumerate(handle, 1):
            record = json.loads(line)
            if line_number == 1 and record.get("type") == "session_meta":
                payload = record.get("payload", {})
                session_id = payload.get("session_id") or payload.get("id")
                session_timestamp = payload.get("timestamp")
            if record.get("type") != "event_msg":
                continue
            payload = record.get("payload", {})
            message_type = payload.get("type")
            if message_type == "user_message":
                heading = f"## User — {record['timestamp']}"
                user_messages += 1
            elif message_type == "agent_message":
                suffix = " (commentary)" if payload.get("phase") == "commentary" else ""
                heading = f"## Assistant{suffix} — {record['timestamp']}"
                assistant_messages += 1
            else:
                continue
            message = payload.get("message")
            if not isinstance(message, str):
                fail(f"visible message in {source} is not text")
            pieces.extend((heading, "", message, ""))

    if user_messages == 0 or assistant_messages == 0:
        fail(f"{source} does not contain a complete visible conversation")
    text = "\n".join(pieces) + "\n"
    return text, {
        "session_id": session_id,
        "session_timestamp": session_timestamp,
        "user_messages": user_messages,
        "assistant_messages": assistant_messages,
        "visible_characters": len(text),
    }


def annals_arguments(binary: Path, database: Path, *arguments: str) -> list[str]:
    return [str(binary), "--library", str(database), "--json", *arguments]


def proposal_state(database: Path, label: str) -> dict[str, Any] | None:
    connection = sqlite3.connect(f"file:{database}?mode=ro", uri=True)
    connection.row_factory = sqlite3.Row
    try:
        row = connection.execute(
            """
            SELECT p.id, p.status, p.outcome, p.base_revision, p.uncertainties,
                   p.model_run_id
            FROM proposals AS p
            JOIN works AS w ON w.id = p.work_id
            WHERE w.label = ? AND p.model_run_id IS NOT NULL
            ORDER BY p.id DESC
            LIMIT 1
            """,
            (label,),
        ).fetchone()
    finally:
        connection.close()
    if row is None:
        return None
    value = dict(row)
    value["uncertainties"] = json.loads(value["uncertainties"])
    return value


def model_run_count(database: Path, label: str) -> int:
    connection = sqlite3.connect(f"file:{database}?mode=ro", uri=True)
    try:
        return int(
            connection.execute(
                """
                SELECT COUNT(*)
                FROM model_runs AS r
                JOIN works AS w ON w.id = r.work_id
                WHERE w.label = ?
                """,
                (label,),
            ).fetchone()[0]
        )
    finally:
        connection.close()


def running_model_runs(database: Path, label: str) -> list[int]:
    connection = sqlite3.connect(f"file:{database}?mode=ro", uri=True)
    try:
        return [
            int(row[0])
            for row in connection.execute(
                """
                SELECT r.id
                FROM model_runs AS r JOIN works AS w ON w.id = r.work_id
                WHERE w.label = ? AND r.status = 'running'
                ORDER BY r.id
                """,
                (label,),
            )
        ]
    finally:
        connection.close()


def slug(label: str) -> str:
    return re.sub(r"[^a-z0-9]+", "-", label.lower()).strip("-")[:60]


def verify_run(run_dir: Path) -> tuple[dict[str, Any], Path]:
    config = load_json(run_dir / "config.json")
    binary = run_dir / config["annals_binary"]
    if sha256(binary) != config["annals_sha256"]:
        fail("the snapshotted Annals binary no longer matches config.json")
    runner = run_dir / config["runner"]
    if sha256(runner) != config["runner_sha256"]:
        fail("the snapshotted experiment runner no longer matches config.json")
    if sha256(Path(__file__).resolve()) != config["runner_sha256"]:
        fail(f"resume with the snapshotted runner: {runner}")
    locked = load_json(run_dir / "manifest.lock.json")
    if len(locked) != EXPECTED_INPUTS:
        fail("the locked manifest no longer has exactly 20 inputs")
    for item in locked:
        path = run_dir / item["input"]
        if path.stat().st_size != item["size_bytes"] or sha256(path) != item["sha256"]:
            fail(f"snapshotted input changed: {path}")
    for arm in ARM_ORDER:
        database = run_dir / f"{arm}.db"
        if not database.is_file():
            fail(f"missing experiment database: {database}")
        connection = sqlite3.connect(f"file:{database}?mode=ro", uri=True)
        try:
            works = connection.execute(
                "SELECT label, sha256 FROM works ORDER BY id"
            ).fetchall()
        finally:
            connection.close()
        expected = [(item["label"], item["sha256"]) for item in locked]
        if works != expected:
            fail(f"{arm} work inventory differs from the locked manifest")
    return config, binary


def acquire_lock(run_dir: Path) -> Any:
    lock_path = run_dir / "runner.lock"
    handle = lock_path.open("a+", encoding="utf-8")
    os.chmod(lock_path, 0o600)
    try:
        fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        handle.close()
        fail(f"another experiment process holds {lock_path}")
    handle.seek(0)
    handle.truncate()
    handle.write(f"pid={os.getpid()} started={now()}\n")
    handle.flush()
    return handle


def setup(manifest_path: Path, run_dir: Path, annals: Path) -> None:
    if run_dir.exists():
        fail(f"run directory already exists: {run_dir}")
    source_manifest = load_json(manifest_path)
    if not isinstance(source_manifest, list) or len(source_manifest) != EXPECTED_INPUTS:
        fail("source manifest must be a JSON array containing exactly 20 entries")
    labels = [item.get("label") for item in source_manifest]
    if any(not isinstance(label, str) or not label.strip() for label in labels):
        fail("every manifest entry needs a nonempty label")
    if len(set(labels)) != len(labels):
        fail("manifest labels must be unique")
    if not annals.is_file() or not os.access(annals, os.X_OK):
        fail(f"Annals executable is not usable: {annals}")

    run_dir.mkdir(parents=True, mode=0o700)
    inputs_dir = run_dir / "inputs"
    setup_dir = run_dir / "setup"
    inputs_dir.mkdir(mode=0o700)
    setup_dir.mkdir(mode=0o700)
    (run_dir / "logs" / "medium").mkdir(parents=True, mode=0o700)
    (run_dir / "logs" / "high").mkdir(parents=True, mode=0o700)

    binary = run_dir / "annals"
    shutil.copy2(annals, binary)
    os.chmod(binary, 0o700)
    shutil.copy2(Path(__file__).resolve(), run_dir / "runner.py")
    os.chmod(run_dir / "runner.py", 0o700)
    shutil.copy2(manifest_path, run_dir / "manifest.source.json")
    os.chmod(run_dir / "manifest.source.json", 0o600)

    locked: list[dict[str, Any]] = []
    for index, item in enumerate(source_manifest, 1):
        source = Path(item["source"]).expanduser()
        if not source.is_file():
            fail(f"source session does not exist: {source}")
        label = item["label"]
        text, metadata = render_transcript(source, label)
        input_path = inputs_dir / f"{index:02d}.md"
        input_path.write_text(text, encoding="utf-8")
        os.chmod(input_path, 0o600)
        locked.append(
            {
                "index": index,
                "label": label,
                "input": str(input_path.relative_to(run_dir)),
                "sha256": sha256(input_path),
                "size_bytes": input_path.stat().st_size,
                "source": item["source"],
                "source_sha256": sha256(source),
                **metadata,
            }
        )
    write_json(run_dir / "manifest.lock.json", locked)

    config = {
        "created_at": now(),
        "annals_binary": "annals",
        "annals_sha256": sha256(binary),
        "runner": "runner.py",
        "runner_sha256": sha256(run_dir / "runner.py"),
        "qualities": list(ARM_ORDER),
        "input_count": EXPECTED_INPUTS,
        "policy": "apply only change proposals with no uncertainties",
        "methodology_notes": [
            "Inputs retain every visible event message, including visible messages from "
            "rolled-back turns, to match the earlier three-work experiments.",
            "Uncertain proposals remain unapplied and do not enter later corpus context.",
            "Later works therefore compare autonomous preset-specific corpus trajectories, "
            "not isolated independent trials.",
            "Copied-forward context in works 16 through 18 is interactional reuse, not "
            "independent corroboration.",
        ],
    }
    write_json(run_dir / "config.json", config)

    seed = run_dir / "seed.db"
    json_command(
        annals_arguments(binary, seed, "init"), output=setup_dir / "seed-init.json"
    )
    for item in locked:
        json_command(
            annals_arguments(
                binary,
                seed,
                "work",
                "add",
                str(run_dir / item["input"]),
                "--name",
                item["label"],
            ),
            output=setup_dir / f"{item['index']:02d}-work-add.json",
        )
    validation = json_command(
        annals_arguments(binary, seed, "validate"),
        output=setup_dir / "seed-validate.json",
    )
    stats = json_command(
        annals_arguments(binary, seed, "stats"), output=setup_dir / "seed-stats.json"
    )
    if not validation["valid"] or stats["revision"] != 0 or stats["work_count"] != 20:
        fail("seed library did not validate as a 20-work revision-zero library")
    for arm in ARM_ORDER:
        json_command(
            annals_arguments(binary, seed, "backup", str(run_dir / f"{arm}.db")),
            output=setup_dir / f"backup-{arm}.json",
        )
        os.chmod(run_dir / f"{arm}.db", 0o600)
    verify_run(run_dir)


def process_work(
    run_dir: Path,
    binary: Path,
    item: dict[str, Any],
    arm: str,
) -> None:
    database = run_dir / f"{arm}.db"
    label = item["label"]
    running = running_model_runs(database, label)
    if running:
        fail(
            f"{arm}/{label} has unfinished model run(s) {running}; "
            "inspect the database before resuming"
        )
    state = proposal_state(database, label)
    if state is not None:
        if state["status"] in ("applied", "no_change"):
            return
        if state["status"] == "pending" and state["uncertainties"]:
            return
        if state["status"] != "pending":
            fail(f"unexpected latest proposal state for {arm}/{label}: {state}")
        apply_pending(run_dir, binary, database, item, arm, state)
        return

    attempt = model_run_count(database, label) + 1
    work_dir = run_dir / "logs" / arm / f"{item['index']:02d}-{slug(label)}"
    work_dir.mkdir(parents=True, exist_ok=True, mode=0o700)
    stdout_path = work_dir / f"attempt-{attempt:02d}.integrate.json"
    stderr_path = work_dir / f"attempt-{attempt:02d}.integrate.stderr"
    metadata_path = work_dir / f"attempt-{attempt:02d}.meta.json"
    started_at = now()
    started = time.monotonic()
    print(f"[{item['index']:02d}/20] {arm}: examining {label}", flush=True)
    result = command(
        annals_arguments(
            binary,
            database,
            "integrate",
            "--work",
            label,
            "--quality",
            arm,
        ),
        stdout_path=stdout_path,
        stderr_path=stderr_path,
    )
    write_json(
        metadata_path,
        {
            "started_at": started_at,
            "completed_at": now(),
            "elapsed_seconds": round(time.monotonic() - started, 3),
            "exit_code": result.returncode,
        },
    )
    if result.returncode != 0:
        fail(
            f"integration failed for {arm}/{label}; inspect {stderr_path} and resume later"
        )
    state = proposal_state(database, label)
    if state is None:
        fail(f"integration produced no proposal for {arm}/{label}")
    if state["status"] == "pending" and not state["uncertainties"]:
        apply_pending(run_dir, binary, database, item, arm, state)
        print(f"[{item['index']:02d}/20] {arm}: applied", flush=True)
    elif state["status"] == "pending":
        print(
            f"[{item['index']:02d}/20] {arm}: pending with "
            f"{len(state['uncertainties'])} uncertainties",
            flush=True,
        )
    elif state["status"] == "no_change":
        print(f"[{item['index']:02d}/20] {arm}: no change", flush=True)
    else:
        fail(f"unexpected submitted proposal state for {arm}/{label}: {state}")


def apply_pending(
    run_dir: Path,
    binary: Path,
    database: Path,
    item: dict[str, Any],
    arm: str,
    state: dict[str, Any],
) -> None:
    work_dir = run_dir / "logs" / arm / f"{item['index']:02d}-{slug(item['label'])}"
    work_dir.mkdir(parents=True, exist_ok=True, mode=0o700)
    output = work_dir / f"proposal-{state['id']}.apply.json"
    json_command(
        annals_arguments(binary, database, "change", "apply", "--work", item["label"]),
        output=output,
    )


def run_experiment(run_dir: Path) -> None:
    _, binary = verify_run(run_dir)
    locked = load_json(run_dir / "manifest.lock.json")
    for offset, item in enumerate(locked):
        arms = ARM_ORDER if offset % 2 == 0 else tuple(reversed(ARM_ORDER))
        for arm in arms:
            process_work(run_dir, binary, item, arm)
    make_report(run_dir)


def arm_summary(database: Path) -> dict[str, Any]:
    connection = sqlite3.connect(f"file:{database}?mode=ro", uri=True)
    connection.row_factory = sqlite3.Row
    try:
        revision = int(
            connection.execute("SELECT revision FROM library_state").fetchone()[0]
        )
        concepts = int(
            connection.execute("SELECT COUNT(*) FROM concepts").fetchone()[0]
        )
        roots = int(
            connection.execute(
                "SELECT COUNT(*) FROM concepts WHERE parent_id IS NULL"
            ).fetchone()[0]
        )
        max_depth = int(
            connection.execute(
                """
                WITH RECURSIVE tree(id, depth) AS (
                    SELECT id, 1 FROM concepts WHERE parent_id IS NULL
                    UNION ALL
                    SELECT c.id, tree.depth + 1
                    FROM concepts AS c JOIN tree ON c.parent_id = tree.id
                )
                SELECT COALESCE(MAX(depth), 0) FROM tree
                """
            ).fetchone()[0]
        )
        evidence = int(
            connection.execute("SELECT COUNT(*) FROM evidence").fetchone()[0]
        )
        path_rows = [
            (int(row[0]), json.loads(row[1]))
            for row in connection.execute(
                """
                WITH RECURSIVE tree(id, path) AS (
                    SELECT id, json_array(label)
                    FROM concepts
                    WHERE parent_id IS NULL
                    UNION ALL
                    SELECT c.id, json_insert(tree.path, '$[#]', c.label)
                    FROM concepts AS c JOIN tree ON c.parent_id = tree.id
                )
                SELECT id, path FROM tree ORDER BY path
                """
            )
        ]
        paths = [row[1] for row in path_rows]
        concept_paths = dict(path_rows)
        evidence_quotes = [
            {"work": row[0], "quote": row[1]}
            for row in connection.execute(
                """
                SELECT w.label,
                       CAST(substr(CAST(w.text AS BLOB), e.start_byte + 1,
                                   e.end_byte - e.start_byte) AS TEXT)
                FROM evidence AS e JOIN works AS w ON w.id = e.work_id
                ORDER BY e.id
                """
            )
        ]
        evidence_links = [
            {
                "path": concept_paths[int(row[0])],
                "work": row[1],
                "start_byte": row[2],
                "end_byte": row[3],
                "quote": row[4],
            }
            for row in connection.execute(
                """
                SELECT e.concept_id, w.label, e.start_byte, e.end_byte,
                       CAST(substr(CAST(w.text AS BLOB), e.start_byte + 1,
                                   e.end_byte - e.start_byte) AS TEXT)
                FROM evidence AS e JOIN works AS w ON w.id = e.work_id
                ORDER BY e.id
                """
            )
        ]

        runs = [
            dict(row)
            for row in connection.execute(
                """
            SELECT r.id, w.label AS work, r.base_revision, r.status, r.model,
                   r.reasoning_effort, r.prompt_version,
                   ROUND((julianday(r.completed_at) - julianday(r.created_at)) * 86400, 3)
                       AS elapsed_seconds
            FROM model_runs AS r JOIN works AS w ON w.id = r.work_id
            ORDER BY r.id
            """
            )
        ]
        calls = [
            dict(row)
            for row in connection.execute(
                "SELECT model_run_id, tool_name, arguments, result, succeeded FROM tool_calls"
            )
        ]
        attempted_queries = 0
        successful_queries = 0
        attempted_read_regions = 0
        successful_read_regions = 0
        attempted_material_bytes = 0
        successful_material_bytes = 0
        for call in calls:
            arguments = json.loads(call["arguments"])
            queries = len(arguments.get("queries", []))
            regions = len(arguments.get("regions", []))
            material_bytes = len(call["arguments"].encode()) + len(
                call["result"].encode()
            )
            attempted_queries += queries
            attempted_read_regions += regions
            attempted_material_bytes += material_bytes
            if call["succeeded"]:
                successful_queries += queries
                successful_read_regions += regions
                successful_material_bytes += material_bytes

        proposals = []
        for row in connection.execute(
            """
            SELECT p.id, w.label AS work, p.base_revision, p.status, p.outcome,
                   p.summary, p.submitted_request, p.uncertainties
            FROM proposals AS p JOIN works AS w ON w.id = p.work_id
            WHERE p.model_run_id IS NOT NULL
            ORDER BY p.id
            """
        ):
            value = dict(row)
            request = json.loads(value.pop("submitted_request"))
            value["uncertainties"] = json.loads(value["uncertainties"])
            operations = request.get("operations", [])
            value["operation_count"] = len(operations)
            value["evidence_count"] = sum(
                len(operation.get("evidence", [])) for operation in operations
            )
            proposals.append(value)
        latest_by_work = {proposal["work"]: proposal for proposal in proposals}
        return {
            "revision": revision,
            "concepts": concepts,
            "roots": roots,
            "max_depth": max_depth,
            "evidence_links": evidence,
            "paths": paths,
            "evidence_quotes": evidence_quotes,
            "evidence_link_values": evidence_links,
            "model_runs": {
                "count": len(runs),
                "elapsed_seconds": round(
                    sum(run["elapsed_seconds"] or 0 for run in runs), 3
                ),
                "statuses": count_values(run["status"] for run in runs),
                "models": count_values(
                    f"{run['model']}/{run['reasoning_effort']}" for run in runs
                ),
            },
            "tools": {
                "calls": len(calls),
                "failed_calls": sum(not call["succeeded"] for call in calls),
                "attempted_query_selectors": attempted_queries,
                "successful_query_selectors": successful_queries,
                "attempted_read_regions": attempted_read_regions,
                "successful_read_regions": successful_read_regions,
                "attempted_argument_and_result_bytes": attempted_material_bytes,
                "successful_argument_and_result_bytes": successful_material_bytes,
            },
            "proposals": {
                "count": len(proposals),
                "outcomes": count_values(p["outcome"] for p in proposals),
                "statuses": count_values(p["status"] for p in proposals),
                "uncertain": sum(bool(p["uncertainties"]) for p in proposals),
                "operations": sum(p["operation_count"] for p in proposals),
                "evidence_selectors": sum(p["evidence_count"] for p in proposals),
            },
            "runs": runs,
            "per_work": latest_by_work,
        }
    finally:
        connection.close()


def count_values(values: Any) -> dict[str, int]:
    counts: dict[str, int] = {}
    for value in values:
        counts[value] = counts.get(value, 0) + 1
    return counts


def overlap(left: list[Any], right: list[Any]) -> dict[str, Any]:
    left_set = {json.dumps(value, ensure_ascii=False, sort_keys=True) for value in left}
    right_set = {
        json.dumps(value, ensure_ascii=False, sort_keys=True) for value in right
    }
    union = left_set | right_set
    return {
        "medium": len(left_set),
        "high": len(right_set),
        "shared": len(left_set & right_set),
        "jaccard": round(len(left_set & right_set) / len(union), 4) if union else 1.0,
    }


def make_report(run_dir: Path) -> None:
    _, binary = verify_run(run_dir)
    reports = run_dir / "reports"
    reports.mkdir(exist_ok=True, mode=0o700)
    summaries: dict[str, Any] = {}
    cli_commands = {
        "stats": ("stats",),
        "validate": ("validate",),
        "show": ("show",),
        "changes": ("change", "list"),
        "log": ("log", "--limit", "100"),
    }
    for arm in ARM_ORDER:
        database = run_dir / f"{arm}.db"
        arm_dir = reports / arm
        arm_dir.mkdir(exist_ok=True, mode=0o700)
        for name, arguments in cli_commands.items():
            json_command(
                annals_arguments(binary, database, *arguments),
                output=arm_dir / f"{name}.json",
            )
        stats = load_json(arm_dir / "stats.json")["data"]
        json_command(
            annals_arguments(binary, database, "diff", "0", str(stats["revision"])),
            output=arm_dir / "diff.json",
        )
        summaries[arm] = arm_summary(database)

    medium_paths = summaries["medium"]["paths"]
    high_paths = summaries["high"]["paths"]
    labels = [item["label"] for item in load_json(run_dir / "manifest.lock.json")]
    per_work = []
    for label in labels:
        medium = summaries["medium"]["per_work"].get(label)
        high = summaries["high"]["per_work"].get(label)
        per_work.append({"work": label, "medium": medium, "high": high})
    report = {
        "generated_at": now(),
        "input_count": EXPECTED_INPUTS,
        "arms": summaries,
        "comparison": {
            "path_overlap": overlap(medium_paths, high_paths),
            "exact_quotation_overlap": overlap(
                summaries["medium"]["evidence_quotes"],
                summaries["high"]["evidence_quotes"],
            ),
            "exact_evidence_link_overlap": overlap(
                summaries["medium"]["evidence_link_values"],
                summaries["high"]["evidence_link_values"],
            ),
            "per_work": per_work,
        },
    }
    write_json(reports / "comparison.json", report)
    write_markdown_report(reports / "comparison.md", report)
    print(f"Report written to {reports / 'comparison.md'}", flush=True)


def write_markdown_report(path: Path, report: dict[str, Any]) -> None:
    medium = report["arms"]["medium"]
    high = report["arms"]["high"]
    lines = [
        "# Annals preset experiment",
        "",
        f"Generated: {report['generated_at']}",
        "",
        "| Metric | Medium | High |",
        "| --- | ---: | ---: |",
    ]
    metrics = (
        ("Revision", "revision"),
        ("Concepts", "concepts"),
        ("Roots", "roots"),
        ("Maximum depth", "max_depth"),
        ("Evidence links", "evidence_links"),
    )
    for label, key in metrics:
        lines.append(f"| {label} | {medium[key]} | {high[key]} |")
    lines.extend(
        [
            f"| Model seconds | {medium['model_runs']['elapsed_seconds']} | "
            f"{high['model_runs']['elapsed_seconds']} |",
            f"| Tool calls | {medium['tools']['calls']} | {high['tools']['calls']} |",
            f"| Failed tool calls | {medium['tools']['failed_calls']} | "
            f"{high['tools']['failed_calls']} |",
            f"| Model proposals | {medium['proposals']['count']} | "
            f"{high['proposals']['count']} |",
            f"| Uncertain proposals | {medium['proposals']['uncertain']} | "
            f"{high['proposals']['uncertain']} |",
            f"| Proposed operations | {medium['proposals']['operations']} | "
            f"{high['proposals']['operations']} |",
            f"| Proposal evidence selectors | "
            f"{medium['proposals']['evidence_selectors']} | "
            f"{high['proposals']['evidence_selectors']} |",
            "",
            "## Per-work outcomes",
            "",
            "| # | Work | Medium | High |",
            "| ---: | --- | --- | --- |",
        ]
    )
    for index, item in enumerate(report["comparison"]["per_work"], 1):
        lines.append(
            f"| {index} | {item['work']} | {proposal_cell(item['medium'])} | "
            f"{proposal_cell(item['high'])} |"
        )
    lines.extend(
        [
            "",
            "## Final paths",
            "",
            "### Medium",
            "",
            *[f"- {' › '.join(value)}" for value in medium["paths"]],
            "",
            "### High",
            "",
            *[f"- {' › '.join(value)}" for value in high["paths"]],
            "",
        ]
    )
    path.write_text("\n".join(lines), encoding="utf-8")
    os.chmod(path, 0o600)


def proposal_cell(proposal: dict[str, Any] | None) -> str:
    if proposal is None:
        return "missing"
    uncertainties = len(proposal["uncertainties"])
    return (
        f"{proposal['outcome']}/{proposal['status']}; "
        f"{proposal['operation_count']} ops; {uncertainties} uncertain"
    )


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    start = subparsers.add_parser("start", help="create and run a fresh experiment")
    start.add_argument("--manifest", type=Path, required=True)
    start.add_argument("--run-dir", type=Path, required=True)
    start.add_argument("--annals", type=Path, required=True)
    resume = subparsers.add_parser("resume", help="resume an existing experiment")
    resume.add_argument("--run-dir", type=Path, required=True)
    report = subparsers.add_parser("report", help="regenerate read-only reports")
    report.add_argument("--run-dir", type=Path, required=True)
    return parser.parse_args()


def main() -> int:
    os.umask(0o077)
    arguments = parse_arguments()
    try:
        if arguments.command == "start":
            setup(
                arguments.manifest.expanduser().resolve(),
                arguments.run_dir.expanduser().resolve(),
                arguments.annals.expanduser().resolve(),
            )
            run_dir = arguments.run_dir.expanduser().resolve()
            lock = acquire_lock(run_dir)
            try:
                run_experiment(run_dir)
            finally:
                lock.close()
        elif arguments.command == "resume":
            run_dir = arguments.run_dir.expanduser().resolve()
            lock = acquire_lock(run_dir)
            try:
                run_experiment(run_dir)
            finally:
                lock.close()
        else:
            run_dir = arguments.run_dir.expanduser().resolve()
            lock = acquire_lock(run_dir)
            try:
                make_report(run_dir)
            finally:
                lock.close()
    except (
        OSError,
        ValueError,
        RuntimeError,
        sqlite3.Error,
        json.JSONDecodeError,
    ) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
