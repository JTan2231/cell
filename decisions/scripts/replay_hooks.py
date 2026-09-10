#!/usr/bin/env python3
"""Prepare historical Krisis Stop-hook inputs; replay only on explicit command."""

import argparse
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time
from zoneinfo import ZoneInfo


def run_json(command, stdin=None):
    result = subprocess.run(command, input=stdin, text=True, capture_output=True)
    if result.returncode:
        detail = result.stderr.strip()[:2000]
        raise ValueError(f"{Path(command[0]).name} exited {result.returncode}: {detail}")
    return json.loads(result.stdout)


def timestamp(value):
    return datetime.fromtimestamp(value, timezone.utc).isoformat()


def validate_hook(hook):
    if not isinstance(hook, dict) or set(hook) != {"hook_event_name", "session_id", "turn_id"}:
        raise ValueError("Expected exactly the three Stop-hook fields")
    if hook["hook_event_name"] != "Stop":
        raise ValueError("Expected a Stop event")
    for field in ("session_id", "turn_id"):
        value = hook[field]
        if (not isinstance(value, str) or not 1 <= len(value.encode("utf-8")) <= 512
                or any(ord(c) < 32 or 127 <= ord(c) <= 159 for c in value)):
            raise ValueError(f"Invalid hook {field}")


def select_turns(corpus, start, end, host_id):
    """Select by completion time, using Conversations' already-deduplicated export."""
    if not isinstance(corpus, list):
        raise ValueError("Expected a Conversations export array")
    rows, skipped, gaps, seen = [], Counter(), [], set()
    for conversation in corpus:
        thread = conversation["thread"]
        reference = thread["reference"]
        thread_id = reference["threadId"]
        if reference["hostId"] != host_id:
            raise ValueError("Export host differs from Conversations doctor")
        if (thread.get("parentThreadId") is not None or thread["sourceKind"] == "exec"
                or thread["sourceKind"].startswith("subAgent")):
            raise ValueError(f"Unexpected non-root or non-interactive task: {thread_id}")
        for turn in conversation["turns"]:
            ref = turn["reference"]
            if ref["hostId"] != host_id or ref["threadId"] != thread_id:
                raise ValueError(f"Inconsistent turn reference in {thread_id}")
            turn_id = ref["turnId"]
            identity = (thread_id, turn_id)
            if identity in seen:
                raise ValueError(f"Duplicate turn reference: {thread_id}/{turn_id}")
            seen.add(identity)
            if turn["status"] != "completed":
                skipped["not_completed"] += 1
                if turn["status"] == "unknown":
                    gaps.append({"thread_id": thread_id, "turn_id": turn_id,
                                 "reason": "unknown_turn_status"})
                continue
            completed_at = turn["completedAt"]
            if completed_at is None:
                gaps.append({"thread_id": thread_id, "turn_id": turn_id,
                             "reason": "missing_completion_time"})
                continue
            if type(completed_at) is not int:
                raise ValueError(f"Invalid completion time: {thread_id}/{turn_id}")
            if not start <= completed_at < end:
                skipped["outside_window"] += 1
                continue
            if not any(m["role"] == "user" and m["text"].strip() for m in turn["messages"]):
                skipped["no_user_after_export_deduplication"] += 1
                continue
            hook = {"hook_event_name": "Stop", "session_id": thread_id, "turn_id": turn_id}
            validate_hook(hook)
            rows.append({"hook": hook, "completed_at": completed_at,
                         "thread_id": thread_id, "turn_id": turn_id,
                         "title": thread.get("name") or thread.get("preview", ""),
                         "cwd": thread.get("cwd"), "archived": thread["archived"]})
    rows.sort(key=lambda row: (row["completed_at"], row["thread_id"], row["turn_id"]))
    return rows, dict(skipped), gaps


def write_json(path, value):
    with path.open("x", encoding="utf-8") as output:
        json.dump(value, output, ensure_ascii=False, indent=2)
        output.write("\n")


def read_histories(args, source_args, start, end, host_id):
    limit = 1024
    while True:
        selection = run_json([args.conversations, "list", *source_args,
                              "--archive", "all", "--updated-after", str(start),
                              "--limit", str(limit)])
        if selection.get("schema_version") != 2 or type(selection.get("has_more")) is not bool:
            raise ValueError("Expected a schema-two Conversations task selection")
        if not selection["has_more"]:
            break
        limit *= 2
    threads = selection["threads"]
    if len({(t["reference"]["hostId"], t["reference"]["threadId"]) for t in threads}) != len(threads):
        raise ValueError("Task listing contains duplicate references")
    print(f"Reading {len(threads)} tasks with {args.workers} readers...", file=sys.stderr, flush=True)

    def read_thread(thread):
        try:
            conversation = run_json([args.conversations, "show", thread["reference"]["threadId"], *source_args])
            if conversation["thread"]["reference"] != thread["reference"]:
                raise ValueError("History reference differs from selected task")
            return conversation, None
        except (ValueError, KeyError, TypeError, OSError) as error:
            return None, {"thread_id": thread["reference"]["threadId"],
                          "title": thread.get("title", ""),
                          "reason": "history_read_failed", "error": str(error)}

    rows, skipped, gaps, seen_items = [], Counter(), [], set()
    read_count, copied_messages = 0, 0
    with ThreadPoolExecutor(max_workers=args.workers) as readers:
        # Consume in listing order, so copied items belong to the same newest
        # task as Conversations export, independently of read completion order.
        for index, (conversation, error) in enumerate(readers.map(read_thread, threads), 1):
            if error:
                gaps.append(error)
            else:
                read_count += 1
                for turn in conversation["turns"]:
                    retained = []
                    for message in turn["messages"]:
                        item_id = message["reference"]["itemId"]
                        if item_id in seen_items:
                            copied_messages += 1
                        else:
                            seen_items.add(item_id)
                            retained.append(message)
                    turn["messages"] = retained
                selected, excluded, unknown = select_turns([conversation], start, end, host_id)
                rows.extend(selected)
                skipped.update(excluded)
                gaps.extend(unknown)
            if index % 10 == 0 or index == len(threads):
                print(f"Read {index}/{len(threads)} tasks; {len(rows)} hooks; {len(gaps)} gaps",
                      file=sys.stderr, flush=True)
    rows.sort(key=lambda row: (row["completed_at"], row["thread_id"], row["turn_id"]))
    return rows, dict(skipped), gaps, len(threads), read_count, copied_messages


def generate(args):
    end = args.until if args.until is not None else int(time.time())
    start = end - args.days * 86400
    zone = ZoneInfo(args.timezone)
    directory = args.output.resolve()
    directory.mkdir(parents=True, exist_ok=False, mode=0o700)
    source_args = ["--codex", args.codex, "--app-server-stderr", "suppress", "--json"]
    summary = {"version": 1, "window_start": start, "window_end_exclusive": end,
               "window_start_utc": timestamp(start), "window_end_utc": timestamp(end),
               "timezone": args.timezone, "conversations_cli": args.conversations,
               "codex": args.codex, "scope": "active and archived interactive root tasks",
               "coverage_complete": False, "ingested": 0}
    try:
        print("Checking Conversations readiness...", file=sys.stderr, flush=True)
        doctor = run_json([args.conversations, "doctor", *source_args])
        if doctor.get("ok") is not True:
            raise ValueError("Conversations doctor is not ready")
        summary["host_id"] = doctor["hostId"]
        summary["source_warnings"] = doctor.get("warnings", [])
        rows, skipped, gaps, total, read_count, copied = read_histories(
            args, source_args, start, end, doctor["hostId"])
        summary.update({"coverage_complete": not gaps, "conversations_enumerated": total,
                        "conversations_read": read_count, "copied_messages_removed": copied,
                        "conversations_with_hooks": len({row["thread_id"] for row in rows}),
                        "hook_count": len(rows), "skipped_turns": skipped, "gaps": gaps})
        hooks = "".join(json.dumps(row["hook"], separators=(",", ":"), ensure_ascii=False)
                        + "\n" for row in rows).encode("utf-8")
        summary["hooks_sha256"] = hashlib.sha256(hooks).hexdigest()
        by_day, by_thread = Counter(), {}
        for row in rows:
            by_day[datetime.fromtimestamp(row["completed_at"], zone).date().isoformat()] += 1
            entry = by_thread.setdefault(row["thread_id"], {
                "thread_id": row["thread_id"], "title": row["title"], "cwd": row["cwd"],
                "archived": row["archived"], "hooks": 0})
            entry["hooks"] += 1
        summary["by_day"] = dict(sorted(by_day.items()))
        summary["by_conversation"] = sorted(by_thread.values(), key=lambda row: (-row["hooks"], row["thread_id"]))
        with (directory / "hooks.jsonl").open("xb") as output:
            output.write(hooks)
        write_json(directory / "turns.json", rows)
    except (ValueError, KeyError, TypeError, OSError) as error:
        summary["error"] = str(error)
        write_json(directory / "summary.json", summary)
        raise
    write_json(directory / "summary.json", summary)
    print(json.dumps({key: value for key, value in summary.items()
                      if key not in {"source_warnings", "by_conversation", "gaps"}}, indent=2))
    print(f"Prepared files: {directory}", file=sys.stderr)
    if gaps:
        raise ValueError(f"Incomplete coverage: {len(gaps)} source/timing gaps; see summary.json")


def replay(args):
    directory = args.input.resolve()
    summary = json.loads((directory / "summary.json").read_text(encoding="utf-8"))
    if summary.get("version") != 1 or summary.get("error"):
        raise ValueError("Replay requires a version-one generation without a generation error")
    excluded = set(getattr(args, "exclude_unreadable_task", []))
    gaps = summary.get("gaps", [])
    unreadable = {gap.get("thread_id") for gap in gaps
                  if gap.get("reason") == "history_read_failed"}
    if excluded != unreadable or any(gap.get("reason") != "history_read_failed" for gap in gaps):
        raise ValueError("Replay requires explicit exclusions for every unreadable task and no other gaps")
    if summary.get("coverage_complete") is not True and not gaps:
        raise ValueError("Replay requires a complete generation or explicitly excluded unreadable tasks")
    hooks = (directory / "hooks.jsonl").read_bytes()
    if hashlib.sha256(hooks).hexdigest() != summary["hooks_sha256"]:
        raise ValueError("Hook file differs from the generated manifest")
    records = [json.loads(line) for line in hooks.decode("utf-8").splitlines()]
    if len(records) != summary["hook_count"]:
        raise ValueError("Hook count differs from the generated manifest")
    # Validate the whole file before making the first live admission.
    for hook in records:
        validate_hook(hook)
        if hook["session_id"] in excluded:
            raise ValueError("Hook file contains an excluded unreadable task")
    submitted = 0
    try:
        for hook in records:
            receipt = run_json([args.krisis, "observe", "ingest"], json.dumps(hook) + "\n")
            if not isinstance(receipt, dict):
                raise ValueError("Invalid Krisis ingest receipt")
            submitted += 1
            print(f"Admitted {submitted}/{len(records)}", file=sys.stderr, flush=True)
    finally:
        print(json.dumps({"ingest_calls_succeeded": submitted,
                          "total_hook_calls": len(records),
                          "excluded_unreadable_tasks": sorted(excluded),
                          "classification_results_checked": False}))


def positive_int(value):
    parsed = int(value)
    if parsed < 1:
        raise argparse.ArgumentTypeError("must be positive")
    return parsed


def main():
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    prepare = commands.add_parser("generate", help="read history and create a local preview; no ingestion")
    prepare.add_argument("--days", type=positive_int, default=30)
    prepare.add_argument("--until", type=int, help="exclusive ending Unix second; default is now")
    prepare.add_argument("--output", type=Path, required=True, help="new private output directory")
    prepare.add_argument("--timezone", default="UTC", help="IANA timezone for daily counts")
    prepare.add_argument("--workers", type=positive_int, default=4, help="concurrent read-only history calls")
    prepare.add_argument("--codex", required=True, help="same Codex executable used by Krisis")
    prepare.add_argument("--conversations", default=str(Path.home() / ".local/bin/conversations"))
    prepare.set_defaults(action=generate)
    admit = commands.add_parser("replay", help="submit a previously generated hook file to live Krisis")
    admit.add_argument("--input", type=Path, required=True)
    admit.add_argument("--krisis", default=str(Path.home() / ".local/bin/krisis"))
    admit.add_argument("--exclude-unreadable-task", action="append", default=[], metavar="THREAD_ID",
                       help="exclude one reviewed history-read failure; repeat for each unreadable task")
    admit.set_defaults(action=replay)
    args = parser.parse_args()
    try:
        args.action(args)
    except (ValueError, KeyError, TypeError, OSError) as error:
        print(f"replay_hooks: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
