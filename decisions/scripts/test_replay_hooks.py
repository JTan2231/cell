#!/usr/bin/env python3
"""Offline checks for selection boundaries and the explicit replay boundary."""

import argparse
import contextlib
import hashlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import replay_hooks


def turn(turn_id, completed_at, role="user", status="completed"):
    return {"reference": {"hostId": "host", "threadId": "thread", "turnId": turn_id},
            "completedAt": completed_at, "status": status,
            "messages": [{"role": role, "text": "Example message.",
                          "reference": {"itemId": "message-" + turn_id}}]}


def corpus(turns):
    return [{"thread": {"reference": {"hostId": "host", "threadId": "thread"},
                        "sourceKind": "cli", "parentThreadId": None,
                        "name": "Example", "archived": True}, "turns": turns}]


class ReplayHooksTests(unittest.TestCase):
    def test_completion_window_order_and_copied_empty_turns(self):
        history = corpus([turn("last", 199), turn("outside", 200), turn("first", 100),
                          turn("old", 99), turn("assistant-only", 150, "assistant"),
                          turn("interrupted", 150, status="interrupted")])
        copied = turn("copied", 150)
        copied["messages"] = []
        history[0]["turns"].append(copied)
        rows, skipped, gaps = replay_hooks.select_turns(history, 100, 200, "host")
        self.assertEqual([row["turn_id"] for row in rows], ["first", "last"])
        self.assertEqual(skipped, {"outside_window": 2,
                                  "no_user_after_export_deduplication": 2, "not_completed": 1})
        self.assertEqual(gaps, [])

    def test_unknown_dates_are_gaps_and_inconsistent_identity_fails(self):
        rows, _, gaps = replay_hooks.select_turns(corpus([turn("undated", None)]), 100, 200, "host")
        self.assertEqual(rows, [])
        self.assertEqual(gaps[0]["reason"], "missing_completion_time")
        with self.assertRaisesRegex(ValueError, "host differs"):
            replay_hooks.select_turns(corpus([]), 100, 200, "another-host")
        with self.assertRaisesRegex(ValueError, "Duplicate turn"):
            replay_hooks.select_turns(corpus([turn("same", 150), turn("same", 150)]), 100, 200, "host")

    def test_generate_never_invokes_krisis_and_source_failure_is_incomplete(self):
        with tempfile.TemporaryDirectory() as temporary:
            args = argparse.Namespace(until=86401, days=1, timezone="UTC",
                                      output=Path(temporary) / "ready", codex="/fixture/codex",
                                      conversations="/fixture/conversations", workers=1)
            doctor = {"ok": True, "hostId": "host"}
            selection = {"schema_version": 2, "has_more": False,
                         "threads": [{"reference": {"hostId": "host", "threadId": "thread"}}]}
            with patch.object(replay_hooks, "run_json", side_effect=[doctor, selection, corpus([turn("one", 100)])[0]]) as run:
                with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                    replay_hooks.generate(args)
            self.assertEqual([call.args[0][1] for call in run.call_args_list], ["doctor", "list", "show"])
            summary = json.loads((args.output / "summary.json").read_text())
            self.assertTrue(summary["coverage_complete"])
            self.assertEqual(summary["ingested"], 0)
            self.assertEqual(summary["hook_count"], 1)
            args.output = Path(temporary) / "incomplete"
            with patch.object(replay_hooks, "run_json", side_effect=[doctor, ValueError("source read failed")]):
                with contextlib.redirect_stderr(io.StringIO()), self.assertRaisesRegex(ValueError, "source read"):
                    replay_hooks.generate(args)
            summary = json.loads((args.output / "summary.json").read_text())
            self.assertFalse(summary["coverage_complete"])
            self.assertFalse((args.output / "hooks.jsonl").exists())

    def test_replay_validates_every_line_before_admitting_any(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            hooks = b'{"hook_event_name":"Stop","session_id":"thread","turn_id":"one"}\n{}\n'
            (directory / "hooks.jsonl").write_bytes(hooks)
            (directory / "summary.json").write_text(json.dumps({
                "version": 1, "coverage_complete": True, "hook_count": 2,
                "hooks_sha256": hashlib.sha256(hooks).hexdigest()}))
            with patch.object(replay_hooks, "run_json") as run:
                with self.assertRaisesRegex(ValueError, "three Stop-hook"):
                    replay_hooks.replay(argparse.Namespace(input=directory, krisis="/fixture/krisis"))
                run.assert_not_called()

    def test_per_task_errors_are_reported_and_copied_messages_have_one_owner(self):
        tasks = [{"reference": {"hostId": "host", "threadId": name}, "title": name}
                 for name in ("newest", "unreadable", "older")]
        def fake_run(command, stdin=None):
            if command[1] == "list":
                # Exercise the CLI's has_more boundary instead of accepting a cap.
                limited = command[command.index("--limit") + 1] == "1024"
                return {"schema_version": 2, "has_more": limited, "threads": tasks[:1] if limited else tasks}
            if command[2] == "unreadable":
                raise ValueError("unsupported history")
            name = command[2]
            value = corpus([turn("shared", 150)])[0]
            value["thread"]["reference"]["threadId"] = name
            value["turns"][0]["reference"]["threadId"] = name
            return value
        args = argparse.Namespace(conversations="/fixture/conversations", workers=1)
        with patch.object(replay_hooks, "run_json", side_effect=fake_run), contextlib.redirect_stderr(io.StringIO()):
            rows, skipped, gaps, total, read, copied = replay_hooks.read_histories(args, [], 100, 200, "host")
        self.assertEqual((total, read, copied), (3, 2, 1))
        self.assertEqual([row["thread_id"] for row in rows], ["newest"])
        self.assertEqual(skipped["no_user_after_export_deduplication"], 1)
        self.assertEqual(gaps[0]["thread_id"], "unreadable")

    def test_replay_stops_on_failed_admission(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            hooks = b"".join(json.dumps({"hook_event_name": "Stop", "session_id": "thread",
                                        "turn_id": str(index)}).encode() + b"\n" for index in range(3))
            (directory / "hooks.jsonl").write_bytes(hooks)
            (directory / "summary.json").write_text(json.dumps({
                "version": 1, "coverage_complete": True, "hook_count": 3,
                "hooks_sha256": hashlib.sha256(hooks).hexdigest()}))
            output = io.StringIO()
            with patch.object(replay_hooks, "run_json", side_effect=[{}, ValueError("admission failed")]) as run:
                with contextlib.redirect_stdout(output), contextlib.redirect_stderr(io.StringIO()):
                    with self.assertRaisesRegex(ValueError, "admission failed"):
                        replay_hooks.replay(argparse.Namespace(input=directory, krisis="/fixture/krisis"))
            self.assertEqual(run.call_count, 2)
            self.assertEqual(json.loads(output.getvalue())["ingest_calls_succeeded"], 1)

    def test_replay_excludes_only_explicitly_named_unreadable_tasks(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            hooks = b'{"hook_event_name":"Stop","session_id":"thread","turn_id":"one"}\n'
            (directory / "hooks.jsonl").write_bytes(hooks)
            summary = {"version": 1, "coverage_complete": False, "hook_count": 1,
                       "hooks_sha256": hashlib.sha256(hooks).hexdigest(),
                       "gaps": [{"thread_id": "unreadable", "reason": "history_read_failed"}]}
            args = argparse.Namespace(input=directory, krisis="/fixture/krisis",
                                      exclude_unreadable_task=[])
            with patch.object(replay_hooks, "run_json", return_value={}) as run:
                for exclusions in ([], ["another-task"]):
                    args.exclude_unreadable_task = exclusions
                    (directory / "summary.json").write_text(json.dumps(summary))
                    with self.assertRaisesRegex(ValueError, "explicit exclusions"):
                        replay_hooks.replay(args)
                    run.assert_not_called()
                args.exclude_unreadable_task = ["unreadable"]
                summary["gaps"].append({"thread_id": "thread", "reason": "missing_completion_time"})
                (directory / "summary.json").write_text(json.dumps(summary))
                with self.assertRaisesRegex(ValueError, "no other gaps"):
                    replay_hooks.replay(args)
                run.assert_not_called()
                summary["gaps"].pop()
                (directory / "summary.json").write_text(json.dumps(summary))
                output = io.StringIO()
                with contextlib.redirect_stdout(output), contextlib.redirect_stderr(io.StringIO()):
                    replay_hooks.replay(args)
                run.assert_called_once()
                receipt = json.loads(output.getvalue())
                self.assertEqual(receipt["ingest_calls_succeeded"], 1)
                self.assertEqual(receipt["excluded_unreadable_tasks"], ["unreadable"])


if __name__ == "__main__":
    unittest.main()
