"""Offline product artifact, pause preservation and interrupted recovery proofs."""
import hashlib
import importlib.util
import json
from pathlib import Path
import shutil
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

from deployment.adapter_support import Stopped, digest, tree_digest
from deployment.stateful_adapter import StatefulAdapter

ROOT = Path(__file__).resolve().parents[1]


def product_module(directory):
    spec = importlib.util.spec_from_file_location("fixture_" + directory, ROOT / directory / "deployment/adapter.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class StatefulProofTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.base = Path(self.temporary.name)

    def tearDown(self):
        self.temporary.cleanup()

    def fixture(self, directory, *, changed_bundle=None, changed_source=False):
        module = product_module(directory)
        spec = module.SPEC
        home = self.base / directory / "home"
        candidate = self.base / directory / "candidate"
        (candidate / "bin").mkdir(parents=True)
        binaries = {}
        for binary in spec["candidate_proofs"].values():
            path = candidate / "bin" / binary
            path.write_text("tested " + binary + "\n")
            binaries[binary] = {"sha256": digest(path)}
        source_inputs = {source: digest(ROOT / source) for source in spec["source_proofs"].values()}
        for source in spec["bundle_sources"].values():
            source_inputs.update({path.relative_to(ROOT).as_posix(): digest(path)
                                  for path in (ROOT / source).rglob("*") if path.is_file()})
        request = {"schema": 1, "product": spec["product"], "run_id": "run-a", "source_root": str(ROOT),
                   "run_dir": str(self.base), "candidate_dir": str(candidate),
                   "candidate": {"schema": 1, "product": spec["product"], "binaries": binaries,
                                 "source_inputs": source_inputs}}
        stage = self.base / directory / "stage"
        stage.mkdir()
        hashes = {}
        altered_key = next(iter(spec["source_proofs"])) if changed_source else None
        for key, paths in spec["proofs"].items():
            source = candidate / "bin" / spec["candidate_proofs"][key] if key in spec["candidate_proofs"] else ROOT / spec["source_proofs"][key]
            for relative in paths:
                target = stage / relative
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(source, target)
                if key == altered_key:
                    with target.open("a") as stream:
                        stream.write("\n# another independently valid release\n")
            hashes[key] = digest(stage / paths[0])
        for key, name in spec["bundles"].items():
            target = stage / "share/chancery" / name
            shutil.copytree(ROOT / spec["bundle_sources"]["share/chancery/" + name], target)
            if name == changed_bundle:
                (target / "additional.md").write_text("Another contract release.\n")
            hashes[key] = tree_digest(target)
        identity = hashlib.sha256("".join(value + "\n" for value in hashes.values()).encode()).hexdigest()
        metadata = {key: "fixture" for key in spec.get("manifest_metadata", ["version"])}
        manifest = {"format": int(spec["format"]) if spec.get("manifest") == "manifest.json" else spec["format"],
                    "release_id": identity, **metadata, **hashes}
        path = stage / spec.get("manifest", "manifest.txt")
        path.write_text(json.dumps(manifest) if path.suffix == ".json" else "".join(f"{key}={value}\n" for key, value in manifest.items()))
        install = home / "Library/Application Support" / spec["application"] / "install"
        (install / "releases").mkdir(parents=True)
        stage.rename(install / "releases" / identity)
        (install / "current").symlink_to("releases/" + identity)
        cli = home / ".local/bin" / spec["product"]
        cli.parent.mkdir(parents=True)
        cli.symlink_to(install / "current/bin" / spec["product"])
        if spec["product"] == "annals":
            (cli.parent / "annals-usage").symlink_to(install / "current/libexec/annals-usage")
        for name in spec["bundles"].values():
            link = home / "Library/Application Support/Chancery/providers" / name
            link.parent.mkdir(parents=True, exist_ok=True)
            link.symlink_to(install / "current/share/chancery" / name)
        with mock.patch.object(Path, "home", return_value=home):
            return module.Adapter(request, spec)

    def test_all_stateful_layouts_bind_every_artifact_to_sealed_source(self):
        for directory in ("annals", "decisions", "semantics"):
            with self.subTest(product=directory):
                adapter = self.fixture(directory)
                adapter.prove_candidate_release(adapter.installed())

    def test_secondary_provider_cannot_change_with_same_candidate_binary(self):
        for directory, bundle in (("annals", "annals-usage"), ("decisions", "decisions")):
            with self.subTest(product=directory):
                adapter = self.fixture(directory, changed_bundle=bundle)
                with self.assertRaisesRegex(Stopped, "provider differs"):
                    adapter.prove_candidate_release(adapter.installed())

    def test_self_consistent_other_packaging_is_not_the_candidate(self):
        adapter = self.fixture("semantics", changed_source=True)
        with self.assertRaisesRegex(Stopped, "differs from the selected candidate"):
            adapter.prove_candidate_release(adapter.installed())

    def test_unfinished_installer_never_releases_or_claims_recovery(self):
        adapter = self.fixture("semantics")
        (adapter.install.parent / ".clockwork-maintenance").touch()
        with mock.patch.object(adapter, "runtime_verify") as verify, mock.patch.object(adapter, "maintenance") as maintenance:
            for operation in (adapter.recover, adapter.release):
                with self.assertRaisesRegex(Stopped, "unfinished transaction"):
                    operation()
            verify.assert_not_called()
            maintenance.assert_not_called()

    def test_operator_disabled_schedule_is_restored_before_release(self):
        adapter = self.fixture("semantics")
        adapter.prior = {"controls": {"semantics/worker": {"enabled": False, "definition_digest": "old"}}}
        before = {"semantics/worker": {"enabled": True, "definition_digest": "candidate"}}
        after = {"semantics/worker": {"enabled": False, "definition_digest": "candidate"}}
        with mock.patch.object(adapter, "controls", side_effect=[before, after]), mock.patch("deployment.stateful_adapter.command") as command:
            self.assertEqual(adapter.restore_controls(), after)
            self.assertEqual(command.call_args.args[0][-3:], ["binding", "disable", "semantics/worker"])

    def test_unsupported_installed_admission_stops_inspection_without_holding(self):
        adapter = self.fixture("semantics")
        with mock.patch("deployment.stateful_adapter.command", return_value={"unrelated": True}) as command:
            with self.assertRaisesRegex(Stopped, "maintenance result"):
                adapter.runtime_inspect()
            self.assertEqual(command.call_count, 1)
            self.assertEqual(command.call_args.args[0][-2:], ["maintenance", "status"])

    def test_orphan_execution_stops_drain_after_process_lock_is_empty(self):
        adapter = self.fixture("semantics")
        with mock.patch.object(adapter, "maintenance", return_value={"holds": ["run-a"], "drained": True}), \
             mock.patch("deployment.stateful_adapter.command", return_value={"version": 1, "jobs": [{"state": "running"}]}) as command:
            with self.assertRaisesRegex(Stopped, "durable requester work"):
                adapter.drain()
            self.assertIn("--compact", command.call_args.args[0])

    def test_pre_cutover_recovery_needs_no_new_hold_or_domain_canary(self):
        adapter = self.fixture("semantics")
        adapter.prior = adapter.installed()
        adapter.request["recovery"] = {"any_apply_started": False}
        with mock.patch.object(adapter, "restore_controls"), mock.patch.object(adapter, "runtime_verify") as canary, \
             mock.patch.object(adapter, "maintenance") as maintenance:
            self.assertTrue(adapter.recover()["data"]["safe_to_release"])
            canary.assert_not_called()
            maintenance.assert_not_called()

    def test_lost_release_reply_preserves_captured_verification_without_new_canary(self):
        adapter = self.fixture("semantics")
        adapter.prior = adapter.installed()
        adapter.request["recovery"] = {"any_apply_started": True, "verified": True}
        with mock.patch.object(adapter, "restore_controls"), \
             mock.patch.object(adapter, "maintenance", return_value={"holds": [], "drained": False}), \
             mock.patch.object(adapter, "runtime_readiness") as readiness, \
             mock.patch.object(adapter, "runtime_verify") as canary:
            self.assertTrue(adapter.recover()["data"]["safe_to_release"])
            readiness.assert_called_once()
            canary.assert_not_called()


class AnnalsCanaryTests(unittest.TestCase):
    def setUp(self):
        from annals.deployment import canary
        self.canary = canary
        self.temporary = tempfile.TemporaryDirectory()
        self.base = Path(self.temporary.name).resolve()
        self.adapter = SimpleNamespace(run_dir=self.base, run_id="run-a", home=self.base,
                                       socket=lambda: self.base / "nucleus.sock",
                                       payload=lambda: self.base / "annals",
                                       environment=lambda: {"CELL_DEPLOYMENT_RUN_ID": "run-a"})
        self.applied = False
        self.integrations = 0
        self.fail_integration = False
        self.output = {"threadId": "thread", "turnId": "turn", "finalMessage": "Recorded."}
        self.coverage = "cumulative"
        self.patch = mock.patch.object(canary, "command", side_effect=self.command)
        self.patch.start()

    def tearDown(self):
        self.patch.stop()
        self.temporary.cleanup()

    def command(self, argv, **_):
        args = [str(value) for value in argv]
        if args[-1] == "init":
            (self.base / "annals-canary/annals.db").touch()
            return {"data": {}}
        if args[-1] == "stats":
            count = int(self.applied)
            return {"data": {"revision": count, "work_count": count, "model_run_count": count,
                             "commit_count": count, "pending_reconciliation_count": 0,
                             "concept_count": count, "evidence_count": count}}
        if "integrate" in args:
            self.integrations += 1
            if self.fail_integration:
                raise Stopped("interrupted model invocation")
            self.applied = True
            return {"data": {"applied_revision": 1}}
        if "change" in args:
            return {"data": {"work": self.canary.LABEL, "status": "applied",
                             "base_revision": 0, "applied_revision": 1}}
        if "report" in args:
            return {"deliveries": [], "unattributedRuns": [
                {"annalsModelRunId": None, "jobId": "unrelated-library-job"},
                {"annalsModelRunId": 1, "jobId": "job", "modelRunToken": "token",
                 "workLabel": self.canary.LABEL, "baseRevision": 0, "status": "completed",
                 "coverage": self.coverage, "threadId": "thread", "turnId": "turn",
                 "usage": {"inputTokens": 20, "outputTokens": 10, "totalTokens": 30}}]}
        if "jobs" in args:
            return {"summary": {"state": "completed", "requester": {"program": "annals", "id": "token"}},
                    "attempts": [{"state": "completed", "output": self.output}]}
        raise AssertionError(f"unexpected command {args}")

    def test_completed_replay_reads_same_domain_job_and_usage_without_new_examination(self):
        first = self.canary.verify(self.adapter)
        self.assertEqual(first, self.canary.verify(self.adapter))
        self.assertEqual(self.integrations, 1)
        self.assertTrue(first["canary"]["verified"])
        self.assertEqual(first["canary"]["usage_coverage"], "cumulative")

    def test_domain_commit_does_not_mask_missing_or_foreign_runtime_output(self):
        for output in ({}, {"threadId": "foreign", "turnId": "turn", "finalMessage": "Recorded."}):
            self.output = output
            with self.assertRaisesRegex(Stopped, "structured final output"):
                self.canary.verify(self.adapter)
        self.assertEqual(self.integrations, 1)
        self.assertTrue(self.applied)

    def test_usage_gap_does_not_become_deployment_success(self):
        self.coverage = "gap"
        with self.assertRaisesRegex(Stopped, "token coverage"):
            self.canary.verify(self.adapter)
        self.assertTrue(self.applied)

    def test_interrupted_start_cannot_create_a_replacement_attempt(self):
        self.fail_integration = True
        with self.assertRaisesRegex(Stopped, "interrupted"):
            self.canary.verify(self.adapter)
        with self.assertRaisesRegex(Stopped, "applied evidence-grounded"):
            self.canary.verify(self.adapter)
        self.assertEqual(self.integrations, 1)
        # If the same admitted examination later committed before the caller
        # disappeared, recovery can prove that result without another attempt.
        self.applied = True
        self.assertTrue(self.canary.verify(self.adapter)["canary"]["verified"])
        self.assertEqual(self.integrations, 1)

    def test_replay_refuses_changed_synthetic_source(self):
        self.canary.verify(self.adapter)
        (self.base / "annals-canary/work.txt").write_text("different source")
        with self.assertRaisesRegex(Stopped, "input or configuration changed"):
            self.canary.verify(self.adapter)
        self.assertEqual(self.integrations, 1)


if __name__ == "__main__":
    unittest.main()
