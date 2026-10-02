"""Execution proofs for opaque commands, interruption and exact request replay."""

from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock

from deployment import cli, manifest


class ExecutorTests(unittest.TestCase):
    def setUp(self):
        self.directory = Path(self.enterContext(tempfile.TemporaryDirectory())).resolve()
        self.storage = self.directory / "storage"
        self.path = self.directory / "run"
        self.path.mkdir(mode=0o700)
        (self.path / "worktree").mkdir()
        self.record = {"schema": 2, "manifest_executor": 1, "request_id": "fixture",
            "run_id": "a" * 32, "run_dir": str(self.path), "repository_root": str(self.directory),
            "source_commit": "b" * 40, "products": ["alpha"], "steps": [],
            "phase": "active", "state": "active", "request": {"settings": {}}}
        self.enterContext(mock.patch.object(cli, "runtime_environment", return_value={
            "HOME": str(self.directory), "PATH": os.environ["PATH"], "PYTHONDONTWRITEBYTECODE": "1"}))

    def test_exit_zero_is_success_even_when_output_claims_domain_failure(self):
        with cli.deployment_lock(self.storage) as descriptor_fd:
            run = cli.Run(self.storage, self.record, descriptor_fd)
            with mock.patch.object(run, "save", wraps=run.save) as save:
                run.command({"id": "opaque", "kind": "run", "product": "alpha"},
                    [sys.executable, "-c", 'print("application is unhealthy")'])
        self.assertEqual(self.record["steps"][0]["state"], "succeeded")
        self.assertEqual(save.call_count, 2)
        self.assertGreaterEqual(self.record["steps"][0]["elapsed_seconds"], 0)
        self.assertIn("application is unhealthy", Path(self.record["steps"][0]["stdout"]).read_text())

    def test_child_cannot_execute_before_attempt_is_saved(self):
        marker = self.directory / "effect"
        with cli.deployment_lock(self.storage) as descriptor_fd:
            run = cli.Run(self.storage, self.record, descriptor_fd)
            original = run.save

            def save():
                step = self.record["steps"][-1]
                if step["state"] == "running":
                    self.assertFalse(marker.exists())
                    self.assertIsNotNone(step["birth"])
                original()

            with mock.patch.object(run, "save", side_effect=save):
                run.command({"id": "effect", "kind": "run", "product": "alpha"},
                    [sys.executable, "-c", "from pathlib import Path; Path(__import__('sys').argv[1]).touch()", str(marker)])
        self.assertTrue(marker.exists())

    def test_descendant_retains_host_lock_after_direct_child_and_executor_exit(self):
        release = self.directory / "release-descendant"
        script = """import os, sys, time
from pathlib import Path
if os.fork():
    os._exit(0)
deadline = time.monotonic() + 3
while not Path(sys.argv[1]).exists() and time.monotonic() < deadline:
    time.sleep(0.01)
os._exit(0)
"""
        try:
            with cli.deployment_lock(self.storage) as descriptor_fd:
                run = cli.Run(self.storage, self.record, descriptor_fd)
                run.command({"id": "fork", "kind": "run", "product": "alpha"},
                    [sys.executable, "-c", script, str(release)])
            self.assertEqual(self.record["steps"][-1]["returncode"], 0)
            self.assertTrue(cli.deployment_busy(self.storage))
        finally:
            release.touch()
        deadline = time.monotonic() + 3
        while cli.deployment_busy(self.storage) and time.monotonic() < deadline:
            time.sleep(0.01)
        self.assertFalse(cli.deployment_busy(self.storage))

    def test_literal_input_is_transport_and_timeout_retains_unknown_effects(self):
        with cli.deployment_lock(self.storage) as descriptor_fd:
            run = cli.Run(self.storage, self.record, descriptor_fd)
            run.command({"id": "input", "kind": "run", "product": "alpha"},
                [sys.executable, "-c", "import sys; sys.stdout.write(sys.stdin.read())"], stdin="literal {opaque} text")
            self.assertEqual(Path(self.record["steps"][-1]["stdout"]).read_text(), "literal {opaque} text")
            with self.assertRaises(cli.ExecutionInterrupted):
                run.command({"id": "timeout", "kind": "run", "product": "alpha"},
                    [sys.executable, "-c", "import time; time.sleep(30)"], timeout_seconds=0.05)
        self.assertEqual(self.record["steps"][-1]["state"], "unknown")
        self.assertIsNotNone(self.record["steps"][-1]["returncode"])

    def test_nonzero_command_stops_remaining_manifest_instructions(self):
        candidates = self.path / "candidate"
        candidates.mkdir()
        (candidates / "candidate.json").write_text(json.dumps({"opaque": "metadata"}))
        preparation = self.path / "preparation"
        preparation.mkdir()
        (preparation / "result.json").write_text(json.dumps({"candidates": {
            "alpha": {"candidate_dir": str(candidates)}}}))
        marker = self.directory / "must-not-run"
        self.record["manifest"] = {"alpha": {"steps": [
            {"id": "stop", "kind": "run", "argv": [sys.executable, "-c", "raise SystemExit(9)"]},
            {"id": "next", "kind": "run", "argv": [sys.executable, "-c",
                "from pathlib import Path; Path(__import__('sys').argv[1]).touch()", str(marker)]}]}}
        with cli.deployment_lock(self.storage) as descriptor_fd:
            run = cli.Run(self.storage, self.record, descriptor_fd)
            original = run.command

            def command(step, argv, **options):
                if step["id"] == "build":
                    self.record["steps"].append({**step, "state": "succeeded"})
                else:
                    original(step, argv, **options)

            with mock.patch.object(cli, "git"), mock.patch.object(run, "command", side_effect=command):
                with self.assertRaisesRegex(cli.DeploymentError, "exited 9"):
                    run.execute()
        self.assertFalse(marker.exists())
        self.assertEqual([step["id"] for step in self.record["steps"]], ["build", "alpha:stop"])

    def test_interruption_reconciliation_executes_nothing_and_needs_acknowledgement(self):
        self.record["steps"] = [{"id": "uncertain", "kind": "run", "state": "running"}]
        cli.save_operation(self.storage, self.record)
        cli.private_directory(self.storage / "active")
        cli.durable_json(self.storage / "active/run.json", {"run_id": self.record["run_id"]})
        with mock.patch.object(cli.Run, "execute") as execute:
            result = cli.reconcile_request(self.storage, "fixture")
            self.assertEqual(result["state"], "interrupted")
            self.assertEqual(result["steps"][0]["state"], "unknown")
            self.assertTrue((self.storage / "active").exists())
            acknowledged = cli.reconcile_request(self.storage, "fixture", acknowledge=True)
            self.assertEqual(acknowledged["state"], "interrupted")
            self.assertFalse((self.storage / "active").exists())
            execute.assert_not_called()

    def test_admission_marker_creation_and_removal_sync_the_parent(self):
        cli.private_directory(self.storage)
        active = self.storage / "active"
        with mock.patch.object(cli, "sync_directory", wraps=cli.sync_directory) as sync:
            cli.private_directory(active)
            cli.durable_json(active / "run.json", {"run_id": self.record["run_id"]})
            cli.clear_marker(self.storage, self.record)
        self.assertEqual([call.args[0] for call in sync.call_args_list], [self.storage, active, self.storage])

    def test_copy_and_link_place_opaque_bytes_and_explicit_target(self):
        source, destination = self.directory / "source", self.directory / "placed"
        source.write_bytes(b"opaque application bytes\x00")
        link = self.directory / "selector"
        context = {"product": "alpha", "home": str(self.directory)}
        with cli.deployment_lock(self.storage) as descriptor_fd:
            run = cli.Run(self.storage, self.record, descriptor_fd)
            run.file_step({"id": "copy", "kind": "copy", "source": str(source),
                "destination": str(destination), "mode": 0o640}, context)
            run.file_step({"id": "link", "kind": "link", "target": str(destination),
                "destination": str(link)}, context)
        self.assertEqual(link.read_bytes(), source.read_bytes())
        self.assertEqual(destination.stat().st_mode & 0o777, 0o640)

    def test_failed_path_expansion_records_file_step_failure(self):
        with cli.deployment_lock(self.storage) as descriptor_fd:
            run = cli.Run(self.storage, self.record, descriptor_fd)
            with self.assertRaises(manifest.ManifestError):
                run.file_step({"id": "copy", "kind": "copy", "source": "/unused",
                    "destination": "{missing}/file"}, {"product": "alpha"})
        retained = cli.read_operation(self.storage, "fixture")
        self.assertEqual(retained["steps"][-1]["state"], "failed")

    def test_only_explicit_selection_is_ordered_without_installed_inspection(self):
        inventory = {name: {"aliases": [], "manifest": {"order": index}}
            for index, name in enumerate(("beta", "alpha", "unselected"))}
        self.assertEqual(cli.requested_products(inventory, ["alpha", "beta"]), ["beta", "alpha"])

    def test_catalog_reads_flat_and_nested_declarations_from_the_selected_commit(self):
        source = self.directory / "source"
        source.mkdir()
        subprocess.run(["git", "init", "--quiet", str(source)], check=True, capture_output=True)
        descriptors = source / "pipeline/products"
        descriptors.mkdir(parents=True)
        for product, directory, order in (("alpha", "alpha", 9), ("beta", "infrastructure/beta", 4)):
            (descriptors / f"{product}.sh").write_text(f"PRODUCT_ID={product}\nPRODUCT_DIR={directory}\n")
            declaration = source / directory / "deployment/manifest.json"
            declaration.parent.mkdir(parents=True)
            declaration.write_text(json.dumps({"schema": 1, "product": product, "order": order,
                "steps": [{"id": "install", "kind": "run", "argv": [f"{{candidate_dir}}/bin/{product}-install"]}]}))
        cli.git(source, "add", ".")
        cli.git(source, "-c", "user.name=Fixture", "-c", "user.email=fixture@example.test",
                "commit", "--quiet", "--no-gpg-sign", "-m", "fixture")
        revision = cli.git(source, "rev-parse", "HEAD")
        (descriptors / "beta.sh").write_text("PRODUCT_ID=beta\nPRODUCT_DIR=missing\n")
        (source / "infrastructure/beta/deployment/manifest.json").write_text("invalid checkout declaration")
        inventory = cli.catalog(source, revision)
        self.assertEqual(inventory["alpha"]["directory"], "alpha")
        self.assertEqual(inventory["beta"]["directory"], "infrastructure/beta")
        self.assertEqual(cli.requested_products(inventory, ["alpha", "beta"]), ["beta", "alpha"])

    def test_replayed_request_cannot_change_settings_or_prepared_input(self):
        request = {"source_commit": "b" * 40, "products": ["alpha"], "settings": {}, "signing_policy": None}
        self.record.update(state="succeeded", phase="terminal", request=request)
        cli.save_operation(self.storage, self.record)
        with mock.patch.object(cli, "canonical_request", return_value=request), \
                mock.patch.object(cli.Run, "execute") as execute:
            result = cli.start(self.directory, ["alpha"], self.storage,
                selected_commit="b" * 40, request_id="fixture")
            self.assertTrue(result["replayed"])
            execute.assert_not_called()
        with mock.patch.object(cli, "canonical_request", return_value={**request, "settings": {"alpha": {"enabled": False}}}):
            with self.assertRaisesRegex(cli.DeploymentError, "different instruction request"):
                cli.start(self.directory, ["alpha"], self.storage,
                    selected_commit="b" * 40, request_id="fixture")

    def test_manifest_rejects_unknown_instruction_fields(self):
        with self.assertRaises(manifest.ManifestError):
            manifest.instruction({"id": "unexpected", "kind": "run", "argv": ["/bin/true"], "health": "required"})


if __name__ == "__main__":
    unittest.main()
