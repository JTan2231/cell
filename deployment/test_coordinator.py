"""Offline deployment proof: fake products, disposable Git, no live services."""

from __future__ import annotations

import contextlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest

sys.dont_write_bytecode = True

from deployment import candidate, cli


ROOT = Path(__file__).resolve().parent.parent
FAKE_ADAPTER = '''import json, pathlib, sys, time
sys.dont_write_bytecode = True
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[2]))
op = sys.argv[1]
r = json.load(sys.stdin)
run = pathlib.Path(r["run_dir"])
name = r["product"]
with (run / "observed.jsonl").open("a") as output:
    output.write(json.dumps([name, op]) + "\\n")
fault_path = run / "fault.json"
fault = json.loads(fault_path.read_text()) if fault_path.exists() else {}
if fault.get("sleep") == [name, op]:
    (run / "child-started").write_text("yes")
    time.sleep(30)
if fault.get("spawn_sleep") == [name, op]:
    from deployment.adapter_support import command
    child_code = "import os,pathlib,time;pathlib.Path(" + repr(str(run / "descendant-started")) + ").write_text(str(os.getpid()));time.sleep(30)"
    command([sys.executable, "-c", child_code])
if op == "inspect":
    data = {"maintenance_products": ["beta"] if name == "alpha" else [], "after": []}
else:
    data = {}
if op == "apply":
    (run / ("installed-" + name)).write_text(r["candidate"]["candidate_id"])
if op == "hold":
    (run / ("held-" + name)).touch()
if op == "release":
    (run / ("held-" + name)).unlink(missing_ok=True)
if op == "recover":
    data = {"safe_to_release": fault.get("recovery_safe", True), "installed": "candidate" if (run / ("installed-" + name)).exists() else "prior"}
if fault.get("invalid") == [name, op]:
    print("uncertain output")
    sys.exit(1)
status = {"inspect":"ready","hold":"held","drain":"drained","apply":"applied","verify":"verified","release":"released","recover":"recovered"}[op]
if fault.get("fail") == [name, op]:
    status = "stopped"
print(json.dumps({"schema":1,"status":status,"data":data,"detail":"isolated fixture"}))
'''

FAKE_CI = '''#!PYTHON
import json, os, pathlib, sys
sys.dont_write_bytecode = True
root = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(root))
from deployment import candidate
from ci_broker.client import source_snapshot
name = pathlib.Path(__file__).resolve().parent.name
binary = root / "target" / "release" / name
binary.parent.mkdir(parents=True, exist_ok=True)
binary.write_text("#!/bin/sh\\nprintf '%s\\\\n' '" + name + " 1.0.0'\\n")
binary.chmod(0o755)
result = candidate.stage(root, name, pathlib.Path(sys.argv[2]), name + "|target/release/" + name + "|" + name)
print(json.dumps({"execution_id":"isolated-fixture", "state":"passed", "gate":name, "source_key":source_snapshot(root)[0]}))
'''


class Fixture:
    def __init__(self, path: Path):
        self.base = path
        self.repo = path / "repository"
        self.repo.mkdir()
        self.storage = path / "private"
        self.storage.mkdir(mode=0o700)
        self.git("init", "-q", "-b", "main")
        self.git("config", "user.name", "Offline deployment fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        self.write(".gitignore", "target/\n__pycache__/\n")
        for relative in ("deployment/__init__.py", "deployment/cli.py", "deployment/candidate.py", "deployment/adapter_support.py",
                         "ci_broker/__init__.py", "ci_broker/client.py", "ci_broker/broker.py"):
            self.write(relative, (ROOT / relative).read_text())
        # The pinned cleanup subprocess never touches actual installations in
        # this disposable fixture, including when run_worker is called directly.
        self.write("deployment/cleanup.py", "print('{}')\n")
        # Quality-gate admission is simulated in this isolated repository. It
        # never invokes the production broker, Cargo, or a real product body.
        client_path = self.repo / "ci_broker/client.py"
        client_path.write_text(client_path.read_text().replace(
            'if __name__ == "__main__":',
            'if __name__ == "__main__" and sys.argv[1:2] == ["run"]:\n    raise SystemExit(0)\n\nif __name__ == "__main__":'))
        self.write("pipeline/test.sh", "#!/bin/sh\nexit 0\n", executable=True)
        for name in ("alpha", "beta"):
            self.write(f"pipeline/products/{name}.sh", f"PRODUCT_ID={name}\nPRODUCT_DIR={name}\nDEPLOY_PROFILE=selector-only-v1\n")
            self.write(f"{name}/deployment/adapter.json", json.dumps({"schema": 1, "product": name, "dependencies": []}))
            self.write(f"{name}/deployment/adapter.py", FAKE_ADAPTER)
            self.write(f"{name}/packaging/manifest.txt", "owned packaging\n")
            self.write(f"{name}/ci.sh", FAKE_CI.replace("PYTHON", sys.executable), executable=True)
        self.commit()

    def write(self, relative: str, content: str, executable: bool = False) -> None:
        path = self.repo / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)
        if executable:
            path.chmod(0o755)

    def git(self, *arguments: str) -> str:
        result = subprocess.run(["git", "-C", str(self.repo), *arguments], check=True,
                                capture_output=True, text=True)
        return result.stdout.strip()

    def commit(self) -> None:
        self.git("add", ".")
        self.git("commit", "-qm", "isolated fixture")

    def create(self, products: tuple[str, ...] = ("alpha",)) -> Path:
        return cli.create_run(self.repo, products, self.storage)


@unittest.skipIf(sys.version_info < (3, 11), "deployment runtime requires Python 3.11")
class DeploymentTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.base = Path(self.temporary.name)
        self.fixture = Fixture(self.base)

    def tearDown(self) -> None:
        # Sealed source and candidate trees are intentionally read-only.
        for path in self.base.rglob("*"):
            if path.is_dir() and not path.is_symlink():
                path.chmod(0o700)
        self.temporary.cleanup()

    def observed(self, path: Path) -> list[list[str]]:
        return [json.loads(line) for line in (path / "observed.jsonl").read_text().splitlines()]

    def test_plan_uses_main_and_ignores_uncommitted_adapter_or_manifest_edits(self) -> None:
        expected = self.fixture.git("rev-parse", "HEAD")
        self.fixture.write("alpha/deployment/adapter.json", "malformed edited metadata")
        result = cli.plan(self.fixture.repo, ["alpha"])
        self.assertEqual(result["source_commit"], expected)
        self.assertEqual(result["products"], ["alpha"])
        self.assertEqual(self.fixture.git("status", "--porcelain"), "M alpha/deployment/adapter.json")

    def test_candidate_survives_later_target_overwrite_and_detects_tampering(self) -> None:
        binary = self.fixture.repo / "target" / "release" / "alpha"
        binary.parent.mkdir(parents=True)
        binary.write_text("#!/bin/sh\nprintf 'alpha 1.0.0\\n'\n")
        binary.chmod(0o755)
        output = self.base / "candidate"
        manifest = candidate.stage(self.fixture.repo, "alpha", output, "alpha|target/release/alpha|alpha")
        binary.write_text("later unrelated gate replaced this executable")
        self.assertEqual(candidate.verify(output), manifest)
        staged = output / "bin" / "alpha"
        staged.chmod(0o755)
        staged.write_text("tampered")
        with self.assertRaisesRegex(candidate.CandidateError, "tree changed"):
            candidate.verify(output)

    def test_dirty_source_is_not_a_deployable_candidate(self) -> None:
        self.fixture.write("alpha/packaging/manifest.txt", "uncommitted")
        with self.assertRaisesRegex(candidate.CandidateError, "committed worktree"):
            candidate.stage(self.fixture.repo, "alpha", self.base / "candidate", "alpha|target/release/alpha|alpha")
        self.assertFalse((self.base / "candidate").exists())

    def test_staged_bytes_from_a_stale_gate_are_not_admitted(self) -> None:
        self.fixture.write("alpha/ci.sh", FAKE_CI.replace("PYTHON", sys.executable).replace(
            '"state":"passed"', '"state":"stale"') + "\nsys.exit(75)\n", executable=True)
        self.fixture.commit()
        path = self.fixture.create()
        self.assertEqual(cli.run_worker(path), 1)
        self.assertTrue((path / "candidates" / "alpha" / "candidate.json").exists())
        self.assertFalse(cli.read_json(path / "run.json")["records"]["alpha"].get("prepared", False))
        self.assertFalse((path / "observed.jsonl").exists())

    def test_private_runner_checks_out_normal_source_modes_without_exposing_run_state(self) -> None:
        self.fixture.write("pipeline/test.sh", f'''#!{sys.executable}
from pathlib import Path
root = Path(__file__).resolve().parent.parent
assert (root / "alpha/ci.sh").stat().st_mode & 0o777 == 0o755
assert (root / "alpha/packaging/manifest.txt").stat().st_mode & 0o777 == 0o644
''', executable=True)
        self.fixture.commit()
        previous_umask = os.umask(0o077)
        try:
            path = self.fixture.create()
            self.assertEqual(cli.run_worker(path), 0)
            self.assertEqual(os.umask(0o077), 0o077)
            self.assertEqual(path.stat().st_mode & 0o777, 0o700)
            self.assertEqual((path / "steps").stat().st_mode & 0o777, 0o700)
            self.assertEqual((path / "run.json").stat().st_mode & 0o777, 0o600)
            self.assertTrue(all(file.stat().st_mode & 0o777 == 0o600 for file in (path / "steps").iterdir()))
        finally:
            os.umask(previous_umask)

    def test_all_maintenance_holds_precede_draining_or_cutover(self) -> None:
        path = self.fixture.create()
        self.assertEqual(cli.run_worker(path), 0)
        observed = self.observed(path)
        self.assertEqual(observed, [["alpha", "inspect"], ["beta", "inspect"],
                                    ["alpha", "hold"], ["beta", "hold"],
                                    ["alpha", "drain"], ["beta", "drain"],
                                    ["alpha", "apply"], ["alpha", "verify"], ["beta", "verify"],
                                    ["alpha", "release"], ["beta", "release"]])
        data = cli.read_json(path / "run.json")
        self.assertEqual(data["state"], "succeeded")
        self.assertNotIn("candidate_dir", data["records"]["beta"])
        self.assertTrue(data["records"]["alpha"]["ci_receipt"])

    def test_failed_verify_keeps_holds_when_internal_recovery_is_not_safe(self) -> None:
        path = self.fixture.create()
        cli.durable_json(path / "fault.json", {"fail": ["beta", "verify"], "recovery_safe": False})
        self.assertEqual(cli.run_worker(path), 1)
        self.assertNotIn(["alpha", "release"], self.observed(path))
        data = cli.read_json(path / "run.json")
        self.assertEqual(data["state"], "stopped")
        self.assertTrue(all(data["records"][name]["held"] for name in ("alpha", "beta")))
        self.assertIn("recovery stopped", data["detail"])

    def test_internal_recovery_never_replays_an_uncertain_apply(self) -> None:
        path = self.fixture.create()
        cli.durable_json(path / "fault.json", {"invalid": ["alpha", "apply"]})
        self.assertEqual(cli.run_worker(path), 1)
        self.assertEqual(self.observed(path).count(["alpha", "apply"]), 1)
        records = cli.read_json(path / "run.json")["records"]
        self.assertTrue(records["alpha"]["recovery_context"]["apply_started"])
        self.assertFalse(records["alpha"]["recovery_context"]["applied"])
        self.assertFalse(records["beta"]["recovery_context"]["apply_started"])
        self.assertFalse((path / "held-alpha").exists())

    def test_lost_hold_reply_is_released_even_without_a_held_ledger_bit(self) -> None:
        path = self.fixture.create()
        cli.durable_json(path / "fault.json", {"invalid": ["alpha", "hold"]})
        self.assertEqual(cli.run_worker(path), 1)
        self.assertFalse((path / "held-alpha").exists())
        self.assertFalse((path / "held-beta").exists())
        self.assertEqual(cli.read_json(path / "run.json")["state"], "stopped")

    def test_pinned_source_change_stops_before_any_adapter_action(self) -> None:
        path = self.fixture.create()
        source = path / "source" / "alpha" / "deployment" / "adapter.py"
        source.chmod(0o644)
        source.write_text("raise SystemExit('changed')\n")
        self.assertEqual(cli.run_worker(path), 1)
        self.assertFalse((path / "observed.jsonl").exists())

    def test_cycle_is_rejected_and_not_silently_ordered(self) -> None:
        with self.assertRaisesRegex(cli.DeploymentError, "cycle"):
            cli.ordered(["alpha", "beta"], {"alpha": ["beta"], "beta": ["alpha"]})

    def test_foreground_success_removes_stale_workspace_and_current_artifacts(self) -> None:
        stale = self.fixture.create()
        self.assertEqual(cli.run_worker(stale), 0)
        self.assertTrue((stale / "worktree").exists())
        result = cli.start(self.fixture.repo, ["alpha"], self.fixture.storage)
        self.assertEqual(result["exit_code"], 0)
        self.assertEqual(result["state"], "succeeded")
        self.assertFalse((self.fixture.storage / "active").exists())
        self.assertEqual(self.fixture.git("worktree", "list", "--porcelain").count("worktree "), 1)
        self.assertEqual([path.name for path in self.fixture.storage.iterdir()], ["deployment.lock"])

    def test_foreground_failure_removes_sealed_candidates_logs_and_worktree(self) -> None:
        self.fixture.write("alpha/ci.sh", FAKE_CI.replace("PYTHON", sys.executable) + "\nsys.exit(75)\n", executable=True)
        self.fixture.commit()
        result = cli.start(self.fixture.repo, ["alpha"], self.fixture.storage)
        self.assertEqual(result["exit_code"], 1)
        self.assertEqual(result["state"], "stopped")
        self.assertFalse((self.fixture.storage / "active").exists())
        self.assertEqual(self.fixture.git("worktree", "list", "--porcelain").count("worktree "), 1)

    def test_cleanup_failure_does_not_recover_or_reapply_installed_products(self) -> None:
        self.fixture.write("deployment/cleanup.py", "import sys\nprint('cleanup blocked', file=sys.stderr)\nsys.exit(1)\n")
        self.fixture.commit()
        path = self.fixture.create()
        self.assertEqual(cli.run_worker(path), 1)
        self.assertEqual(cli.read_json(path / "run.json")["state"], "cleanup_failed")
        observed = self.observed(path)
        self.assertFalse(any(operation == "recover" for _, operation in observed))
        self.assertEqual(observed.count(["alpha", "apply"]), 1)
        self.assertFalse((path / "held-alpha").exists())
        self.assertFalse((path / "held-beta").exists())

    def test_all_readiness_proofs_precede_release_and_nucleus_releases_last(self) -> None:
        self.add_nucleus()
        path = self.fixture.create()
        self.assertEqual(cli.run_worker(path), 0)
        observed = self.observed(path)
        first_release = min(index for index, event in enumerate(observed) if event[1] == "release")
        self.assertTrue(all(index < first_release for index, event in enumerate(observed) if event[1] == "verify"))
        self.assertEqual(observed[-1], ["nucleus", "release"])
        self.assertEqual(observed.count(["nucleus", "release"]), 1)

    def add_nucleus(self) -> None:
        fixture = self.fixture
        fixture.write("pipeline/products/nucleus.sh", "PRODUCT_ID=nucleus\nPRODUCT_DIR=nucleus\nDEPLOY_PROFILE=custom\n")
        fixture.write("nucleus/deployment/adapter.json", json.dumps({"schema": 1, "product": "nucleus", "dependencies": []}))
        fixture.write("nucleus/deployment/adapter.py", FAKE_ADAPTER)
        fixture.write("alpha/deployment/adapter.py", FAKE_ADAPTER.replace('["beta"] if name == "alpha"', '["beta", "nucleus"] if name == "alpha"'))
        fixture.commit()

    def test_internal_recovery_proves_all_products_and_releases_nucleus_last(self) -> None:
        self.add_nucleus()
        path = self.fixture.create()
        cli.durable_json(path / "fault.json", {"fail": ["nucleus", "verify"]})
        self.assertEqual(cli.run_worker(path), 1)
        observed = self.observed(path)
        first_release = min(index for index, event in enumerate(observed) if event[1] == "release")
        self.assertTrue(all(index < first_release for index, event in enumerate(observed) if event[1] == "recover"))
        self.assertEqual(observed[-1], ["nucleus", "release"])

    def test_competing_start_cannot_create_or_delete_an_active_workspace(self) -> None:
        path = self.fixture.create()
        with cli.deployment_lock(self.fixture.storage):
            with self.assertRaisesRegex(cli.DeploymentError, "holds the deployment lock"):
                cli.start(self.fixture.repo, ["alpha"], self.fixture.storage)
        self.assertTrue((path / "run.json").exists())
        self.assertFalse((path / "observed.jsonl").exists())

    def worker_process(self, path: Path, log):
        with cli.deployment_lock(self.fixture.storage) as lock_fd:
            return subprocess.Popen([sys.executable, str(path / "source" / "deployment" / "cli.py"),
                                     "_worker", str(path), str(lock_fd)], stdout=log, stderr=log,
                                    pass_fds=(lock_fd,))

    def test_active_adapter_retains_global_lock_if_runner_dies(self) -> None:
        path = self.fixture.create()
        cli.durable_json(path / "fault.json", {"sleep": ["alpha", "apply"]})
        log = (path / "worker-test.log").open("wb")
        process = self.worker_process(path, log)
        child_pid = None
        try:
            deadline = time.monotonic() + 15
            while not (path / "child-started").exists() and time.monotonic() < deadline:
                if process.poll() is not None:
                    self.fail((path / "worker-test.log").read_text())
                time.sleep(0.02)
            self.assertTrue((path / "child-started").exists())
            child_pid = cli.read_json(path / "run.json")["active_operation"]["pid"]
            process.kill()
            process.wait(timeout=5)
            with self.assertRaisesRegex(cli.DeploymentError, "holds the deployment lock"):
                with cli.deployment_lock(self.fixture.storage):
                    pass
            self.assertNotIn(["alpha", "release"], self.observed(path))
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)
            if child_pid:
                with contextlib.suppress(ProcessLookupError):
                    os.kill(child_pid, signal.SIGKILL)
            log.close()

    def test_deployer_descendant_retains_lock_after_both_supervisors_die(self) -> None:
        path = self.fixture.create()
        cli.durable_json(path / "fault.json", {"spawn_sleep": ["alpha", "apply"]})
        with (path / "worker-test.log").open("wb") as log:
            process = self.worker_process(path, log)
            child_pid = descendant_pid = None
            try:
                deadline = time.monotonic() + 15
                marker = path / "descendant-started"
                while not marker.exists() and time.monotonic() < deadline:
                    if process.poll() is not None:
                        self.fail((path / "worker-test.log").read_text())
                    time.sleep(0.02)
                self.assertTrue(marker.exists())
                descendant_pid = int(marker.read_text())
                child_pid = cli.read_json(path / "run.json")["active_operation"]["pid"]
                process.kill()
                process.wait(timeout=5)
                os.kill(child_pid, signal.SIGKILL)
                with self.assertRaisesRegex(cli.DeploymentError, "holds the deployment lock"):
                    with cli.deployment_lock(self.fixture.storage):
                        pass
                self.assertNotIn(["alpha", "release"], self.observed(path))
                with self.assertRaisesRegex(cli.DeploymentError, "holds the deployment lock"):
                    cli.start(self.fixture.repo, ["alpha"], self.fixture.storage)
                self.assertTrue((path / "source").exists())
                self.assertTrue((path / "worktree").exists())
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait(timeout=5)
                for pid in (child_pid, descendant_pid):
                    if pid:
                        with contextlib.suppress(ProcessLookupError):
                            os.kill(pid, signal.SIGKILL)


if __name__ == "__main__":
    unittest.main()
