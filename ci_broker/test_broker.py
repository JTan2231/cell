from __future__ import annotations

import contextlib
import json
import os
import signal
import sqlite3
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest import mock

from ci_broker import broker, client


BROKER = Path(broker.__file__).resolve()


class BrokerCliTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory(prefix="cell-ci-broker-test-")
        self.root = Path(self.temporary.name)
        self.state = self.root / "state"
        self.worktree = self.root / "worktree"
        self.worktree.mkdir()
        self.source = self.worktree / "source-key"
        self.source.write_text("source-1\n", encoding="utf-8")

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def base(self, lane: str = "heavy") -> list[str]:
        source_check = json.dumps(
            [
                sys.executable,
                "-c",
                "from pathlib import Path; print(Path('source-key').read_text().strip())",
            ]
        )
        return [
            sys.executable,
            str(BROKER),
            "run",
            "--repo-id",
            "cell-test",
            "--host-id",
            "host-test",
            "--state-dir",
            str(self.state),
            "--light-slots",
            "2",
            "--heartbeat-timeout",
            "1",
            "--poll-interval",
            "0.05",
            "--source-key",
            "source-1",
            "--gate",
            "root",
            "--gate-version",
            "gate-v1",
            "--toolchain-key",
            "toolchain-v1",
            "--lane",
            lane,
            "--environment-mode",
            "minimal",
            "--cwd",
            str(self.worktree),
            "--source-check-json",
            source_check,
        ]

    def parse_receipt(self, completed: subprocess.CompletedProcess[str]) -> dict:
        lines = [line for line in completed.stdout.splitlines() if line.startswith("{")]
        self.assertTrue(lines, completed)
        return json.loads(lines[-1])

    def scope_arguments(self) -> list[str]:
        return [
            "--repo-id",
            "cell-test",
            "--host-id",
            "host-test",
            "--state-dir",
            str(self.state),
            "--light-slots",
            "2",
            "--heartbeat-timeout",
            "1",
            "--poll-interval",
            "0.05",
        ]

    def test_scope_is_host_and_logical_repository_not_worktree(self) -> None:
        first = broker.Scope("host", "cell", self.state, 2, 2, 1)
        second = broker.Scope("host", "cell", self.root / "another-state", 2, 2, 1)
        other_host = broker.Scope("other", "cell", self.state, 2, 2, 1)
        other_repo = broker.Scope("host", "other", self.state, 2, 2, 1)
        self.assertEqual(first.key, second.key)
        self.assertNotEqual(first.key, other_host.key)
        self.assertNotEqual(first.key, other_repo.key)

    def test_clean_identical_calls_single_flight(self) -> None:
        count = self.root / "count"
        body = [
            sys.executable,
            "-c",
            (
                "from pathlib import Path; import time; "
                f"p=Path({str(count)!r}); "
                "p.write_text(p.read_text()+'x' if p.exists() else 'x'); "
                "time.sleep(.35)"
            ),
        ]
        command = self.base() + ["--share-clean-candidate", "--", *body]
        first = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        time.sleep(0.08)
        second = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        first_stdout, first_stderr = first.communicate(timeout=5)
        second_stdout, second_stderr = second.communicate(timeout=5)
        self.assertEqual(first.returncode, 0, first_stderr)
        self.assertEqual(second.returncode, 0, second_stderr)
        first_receipt = json.loads(first_stdout.splitlines()[-1])
        second_receipt = json.loads(second_stdout.splitlines()[-1])
        self.assertEqual(first_receipt["execution_id"], second_receipt["execution_id"])
        self.assertEqual({first_receipt["joined"], second_receipt["joined"]}, {False, True})
        self.assertEqual(count.read_text(encoding="utf-8"), "x")

    def test_heavy_lane_serializes_distinct_executions(self) -> None:
        first_marker = self.root / "first"
        second_marker = self.root / "second"

        def body(marker: Path) -> list[str]:
            return [
                sys.executable,
                "-c",
                (
                    "from pathlib import Path; import time; "
                    f"p=Path({str(marker)!r}); "
                    "p.write_text(f'{time.time()}\\n'); time.sleep(.3); "
                    "p.write_text(p.read_text()+f'{time.time()}\\n')"
                ),
            ]

        first = subprocess.Popen(
            self.base() + ["--", *body(first_marker)],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        time.sleep(0.05)
        second_command = self.base()
        gate_index = second_command.index("root")
        second_command[gate_index] = "other"
        second = subprocess.Popen(
            second_command + ["--", *body(second_marker)],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        first_output = first.communicate(timeout=5)
        second_output = second.communicate(timeout=5)
        self.assertEqual(first.returncode, 0, first_output[1])
        self.assertEqual(second.returncode, 0, second_output[1])
        first_times = [float(value) for value in first_marker.read_text().splitlines()]
        second_times = [float(value) for value in second_marker.read_text().splitlines()]
        self.assertGreaterEqual(second_times[0], first_times[1] - 0.03)

    def test_different_body_working_directories_do_not_single_flight(self) -> None:
        first_cwd = self.worktree / "one"
        second_cwd = self.worktree / "two"
        for cwd in (first_cwd, second_cwd):
            cwd.mkdir()
            (cwd / "source-key").write_text("source-1\n", encoding="utf-8")

        body = [
            sys.executable,
            "-c",
            "from pathlib import Path; import time; Path('ran').write_text('yes'); time.sleep(.2)",
        ]

        def command(cwd: Path) -> list[str]:
            values = self.base()
            values[values.index(str(self.worktree))] = str(cwd)
            source_option = values.index("--source-check-json")
            values[source_option:source_option] = [
                "--identity-root",
                str(self.worktree),
            ]
            return values + ["--share-clean-candidate", "--", *body]

        first = subprocess.Popen(
            command(first_cwd), stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True
        )
        time.sleep(0.05)
        second = subprocess.Popen(
            command(second_cwd), stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True
        )
        first_stdout, first_stderr = first.communicate(timeout=5)
        second_stdout, second_stderr = second.communicate(timeout=5)
        self.assertEqual(first.returncode, 0, first_stderr)
        self.assertEqual(second.returncode, 0, second_stderr)
        first_receipt = json.loads(first_stdout.splitlines()[-1])
        second_receipt = json.loads(second_stdout.splitlines()[-1])
        self.assertNotEqual(first_receipt["execution_id"], second_receipt["execution_id"])
        self.assertTrue((first_cwd / "ran").is_file())
        self.assertTrue((second_cwd / "ran").is_file())

    def test_expired_live_queued_request_does_not_wedge_fifo(self) -> None:
        state = self.root / "lease-state"
        scoped = broker.Broker(
            broker.Scope("lease-host", "lease-repo", state, 2, 2, 0.2),
            poll_interval=0.05,
        )
        identity = broker.Identity(
            repository_id="lease-repo",
            source_key="source-1",
            gate="root",
            gate_version="gate-v1",
            toolchain_key="toolchain-v1",
            environment_key="environment-v1",
            command_key="command-v1",
            source_check_key="source-check-v1",
            lane="heavy",
        )
        invocation = broker.Invocation(
            cwd=self.worktree,
            command=(sys.executable, "-c", "pass"),
            source_check=(sys.executable, "-c", "print('source-1')"),
            environment=dict(os.environ),
            share_clean_candidate=False,
            attribution_json="{}",
        )
        stopped = scoped.submit(identity, invocation)
        with contextlib.closing(sqlite3.connect(scoped.scope.database_path)) as connection:
            connection.execute(
                "UPDATE requests SET heartbeat_at = ? WHERE id = ?",
                (time.time() - 1, stopped.request_id),
            )
            connection.commit()

        recovered = scoped.recover()
        self.assertEqual(recovered["requests"], 1)
        self.assertEqual(scoped.execution_state(stopped.execution_id), "cancelled")

        follower = scoped.submit(identity, invocation)
        self.assertTrue(scoped._try_claim(follower, "heavy"))
        scoped.cancel_request(follower)

    def test_changed_source_is_stale_not_green(self) -> None:
        body = [
            sys.executable,
            "-c",
            "from pathlib import Path; Path('source-key').write_text('source-2\\n')",
        ]
        completed = subprocess.run(
            self.base() + ["--", *body],
            check=False,
            capture_output=True,
            text=True,
            timeout=5,
        )
        receipt = self.parse_receipt(completed)
        self.assertEqual(completed.returncode, 75, completed.stderr)
        self.assertEqual(receipt["state"], "stale")
        self.assertEqual(receipt["execution_state"], "stale")

    def test_runner_crash_recovers_to_lost_and_stops_body(self) -> None:
        body_pid = self.root / "body-pid"
        body = [
            sys.executable,
            "-c",
            (
                "from pathlib import Path; import os, time; "
                f"Path({str(body_pid)!r}).write_text(str(os.getpid())); time.sleep(30)"
            ),
        ]
        running = subprocess.Popen(
            self.base() + ["--", *body],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        deadline = time.monotonic() + 5
        execution_id = None
        database = None
        while time.monotonic() < deadline:
            databases = list(self.state.glob("*.sqlite3"))
            if databases:
                database = databases[0]
                with contextlib.closing(sqlite3.connect(database)) as connection:
                    row = connection.execute(
                        "SELECT id, state FROM executions ORDER BY created_at LIMIT 1"
                    ).fetchone()
                if row and row[1] == "running" and body_pid.exists():
                    execution_id = row[0]
                    break
            time.sleep(0.05)
        self.assertIsNotNone(execution_id)
        os.kill(running.pid, signal.SIGKILL)
        running.wait(timeout=2)
        if running.stdout is not None:
            running.stdout.close()
        if running.stderr is not None:
            running.stderr.close()
        time.sleep(1.1)
        recovered = subprocess.run(
            [sys.executable, str(BROKER), "recover", *self.scope_arguments()],
            check=False,
            capture_output=True,
            text=True,
            timeout=5,
        )
        self.assertEqual(recovered.returncode, 0, recovered.stderr)
        status = subprocess.run(
            [
                sys.executable,
                str(BROKER),
                "status",
                *self.scope_arguments(),
                execution_id,
            ],
            check=False,
            capture_output=True,
            text=True,
            timeout=5,
        )
        self.assertEqual(status.returncode, 0, status.stderr)
        self.assertEqual(json.loads(status.stdout)["state"], "lost")
        pid = int(body_pid.read_text())
        time.sleep(0.1)
        self.assertFalse(broker.same_process_alive(pid, broker.process_token(pid)))

    def test_configuration_failure_does_not_run_body(self) -> None:
        marker = self.root / "must-not-exist"
        initial = subprocess.run(
            self.base() + ["--", sys.executable, "-c", "pass"],
            check=False,
            capture_output=True,
            text=True,
            timeout=5,
        )
        self.assertEqual(initial.returncode, 0, initial.stderr)
        command = self.base()
        slots = command.index("2")
        command[slots] = "3"
        failed = subprocess.run(
            command
            + [
                "--",
                sys.executable,
                "-c",
                f"from pathlib import Path; Path({str(marker)!r}).write_text('ran')",
            ],
            check=False,
            capture_output=True,
            text=True,
            timeout=5,
        )
        self.assertEqual(failed.returncode, 78)
        self.assertFalse(marker.exists())


class RepositoryClientTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory(prefix="cell-ci-client-test-")
        self.root = Path(self.temporary.name) / "repository"
        self.root.mkdir()
        subprocess.run(("git", "init", "-q", str(self.root)), check=True)
        subprocess.run(
            ("git", "-C", str(self.root), "config", "user.email", "test@example.invalid"),
            check=True,
        )
        subprocess.run(
            ("git", "-C", str(self.root), "config", "user.name", "Test"), check=True
        )
        (self.root / "source.txt").write_text("one\n", encoding="utf-8")
        subprocess.run(("git", "-C", str(self.root), "add", "source.txt"), check=True)
        subprocess.run(
            ("git", "-C", str(self.root), "commit", "-qm", "initial"), check=True
        )

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def test_snapshot_is_exact_and_only_clean_snapshot_is_shareable(self) -> None:
        clean_key, clean = client.source_snapshot(self.root)
        self.assertTrue(clean)
        (self.root / "source.txt").write_text("two\n", encoding="utf-8")
        dirty_key, dirty_clean = client.source_snapshot(self.root)
        self.assertFalse(dirty_clean)
        self.assertNotEqual(clean_key, dirty_key)

    def test_cargo_path_bootstrap_has_no_empty_path_segment(self) -> None:
        fake_home = self.root / "home"
        cargo_bin = fake_home / ".cargo" / "bin"
        cargo_bin.mkdir(parents=True)
        for command in ("cargo", "rustc"):
            executable = cargo_bin / command
            executable.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
            executable.chmod(0o755)
        with mock.patch.dict(
            os.environ,
            {"HOME": str(fake_home), "PATH": ""},
            clear=True,
        ):
            client.bootstrap_cargo_path()
            self.assertEqual(os.environ["PATH"], str(cargo_bin))
            self.assertEqual(Path(client.shutil.which("cargo") or ""), cargo_bin / "cargo")

    def test_production_state_ignores_home_environment_override(self) -> None:
        expected = client.canonical_state_dir()
        with mock.patch.dict(os.environ, {"HOME": str(self.root / "other-home")}):
            self.assertEqual(client.canonical_state_dir(), expected)

    def test_expected_source_mismatch_is_stale_without_running_body(self) -> None:
        marker = self.root / "must-not-run"
        completed = subprocess.run(
            [
                sys.executable,
                str(Path(client.__file__).resolve()),
                "run",
                "--gate",
                "root",
                "--expected-source-key",
                "sha256:not-the-current-source",
                "--repo-root",
                str(self.root),
                "--",
                sys.executable,
                "-c",
                f"from pathlib import Path; Path({str(marker)!r}).write_text('ran')",
            ],
            check=False,
            capture_output=True,
            text=True,
            timeout=10,
        )
        self.assertEqual(completed.returncode, 75, completed.stderr)
        self.assertFalse(marker.exists())
        self.assertEqual(json.loads(completed.stdout)["state"], "stale")


if __name__ == "__main__":
    unittest.main()
