"""Check signing boundaries using in-memory policy and retained evidence."""

from contextlib import ExitStack, nullcontext
import copy
import json
from pathlib import Path
import sys
from types import SimpleNamespace
import unittest
from unittest import mock

from ci_manager import client, production
from ci_manager.manager import Worker
from ci_manager.process import validation_exited
from ci_manager.storage import ManagerError
from deployment import candidate, signing


POLICY = {"schema": 1, "macos": {"profile": "local", "certificate_sha1": "a" * 40,
                                    "keychain": "/example/login.keychain-db",
                                    "identifier_namespace": "local.cell"}}


class SubmissionSigningTests(unittest.TestCase):
    def setUp(self):
        self.root = Path("/evidence/repository")
        self.store = mock.Mock()
        self.store.root = Path("/evidence/state")
        self.store.get.return_value = {"common_git_dir": str(self.root / "git"),
                                       "policy": {"luna_attempts": 3, "terra_attempts": 1,
                                                  "model_timeout_seconds": 600}}
        self.store.transaction.side_effect = nullcontext
        self.store.db.execute.return_value.fetchone.return_value = None
        self.store.job.return_value = {"id": "new-job"}
        self.args = SimpleNamespace(repo=str(self.root), commit="HEAD", deploy=["alpha"],
                                    skip_tests=True, request_id="request")
        stack = ExitStack()
        self.addCleanup(stack.close)
        stack.enter_context(mock.patch.object(Path, "resolve", return_value=self.root))
        stack.enter_context(mock.patch.object(client, "lock", side_effect=lambda *args: nullcontext()))
        stack.enter_context(mock.patch.object(client.uuid, "uuid4", return_value=SimpleNamespace(hex="new-job")))
        self.git = stack.enter_context(mock.patch.object(client.git, "git"))
        stack.enter_context(mock.patch.object(client.git, "common", return_value=self.root / "git"))
        stack.enter_context(mock.patch.object(client.git, "commit", return_value="a" * 40))
        self.load = stack.enter_context(mock.patch.object(signing, "load_policy", return_value=POLICY))
        self.preflight = stack.enter_context(mock.patch.object(signing, "preflight"))

    def test_new_submission_freezes_signer_and_retry_keeps_it(self):
        job = client.submit(self.store, self.args)
        inserts = [call for call in self.store.db.execute.call_args_list
                   if call.args[0].startswith("INSERT INTO jobs")]
        self.assertEqual(len(inserts), 1)
        snapshot = json.loads(inserts[0].args[1][-1])
        self.assertEqual(snapshot["signing_policy"], POLICY)
        self.assertNotIn("signing_policy_digest", snapshot)
        self.assertTrue(snapshot["release_builds_deferred"])
        self.assertEqual(snapshot["policy"]["manifest_executor"], 1)
        retained = {**snapshot, "id": job["id"]}
        self.store.db.execute.return_value.fetchone.return_value = retained
        self.store.decode.return_value = retained
        self.load.side_effect = signing.SigningError("configuration was removed")
        self.assertEqual(client.submit(self.store, self.args), retained)
        self.load.assert_called_once()
        self.preflight.assert_called_once_with(POLICY)

    def test_unusable_signer_rejects_admission_without_pin_or_alternate_identity(self):
        self.preflight.side_effect = signing.SigningError("private key unavailable")
        with self.assertRaisesRegex(signing.SigningError, "private key unavailable"):
            client.submit(self.store, self.args)
        self.git.assert_not_called()
        self.load.assert_called_once()
        self.preflight.assert_called_once_with(POLICY)
        self.assertFalse(any(call.args[0].startswith("INSERT")
                             for call in self.store.db.execute.call_args_list))

    def test_signing_cli_routes_without_opening_queue(self):
        with mock.patch.object(client.os, "umask"), mock.patch.object(
                signing, "run_cli", return_value=0) as run, mock.patch.object(client, "Store") as store:
            self.assertEqual(client.main(["signing", "status"]), 0)
        run.assert_called_once_with(["status"])
        store.assert_not_called()


class WorkerSigningTests(unittest.TestCase):
    def setUp(self):
        self.directory = Path("/evidence/job")
        self.worker = Worker.__new__(Worker)
        self.worker.store = mock.Mock()
        self.worker.store.job.return_value = {"cancel_requested": False}
        self.worker.directory = mock.Mock(return_value=self.directory)
        self.worker.worktree = mock.Mock(return_value=self.directory / "worktree")
        self.worker.repository = Path("/evidence/repository")
        self.worker.process = mock.Mock()
        self.worker.finish = mock.Mock()
        self.job = {"id": "fixture", "base_commit": "a" * 40, "candidate_commit": "b" * 40,
                    "validations": [{"state": "passed"}], "deploy_products": ["beta"],
                    "signing_policy": POLICY, "policy": {"manifest_executor": 1},
                    "last_receipt": {"state": "passed", "candidate_commit": "b" * 40,
                                     "selection": {"product_tests": ["alpha"], "platform_products": []}}}
        stack = ExitStack()
        self.addCleanup(stack.close)
        self.current = stack.enter_context(mock.patch.object(signing, "assert_current"))
        stack.enter_context(mock.patch.object(signing, "preflight"))
        self.writes = stack.enter_context(mock.patch("ci_manager.manager.atomic_json"))
        self.bytes_writes = stack.enter_context(mock.patch("ci_manager.manager.atomic_bytes"))
        stack.enter_context(mock.patch.object(Path, "exists", return_value=False))
        self.descriptors = stack.enter_context(mock.patch.object(production.build, "read_descriptor",
            side_effect=lambda source, product: {"RELEASE_BINARY_CHECKS": f"main|target/release/{product}|{product}"}))

    def receipt(self):
        records = {product: {"candidate_dir": str(self.directory / "production-0/candidates" / product),
                             "candidate_id": f"candidate:{product}"} for product in ("alpha", "beta")}
        return {"schema_version": 1, "state": "passed", "source_commit": self.job["candidate_commit"],
                "signing_policy": POLICY, "products": ["alpha", "beta"],
                "preparation": {"source_key": self.job["candidate_commit"], "candidates": records}}

    def manifest(self, path, *, signing_policy):
        self.assertEqual(signing_policy, POLICY)
        return {"source_commit": self.job["candidate_commit"], "source_key": self.job["candidate_commit"],
                "product": path.name, "candidate_id": f"candidate:{path.name}", "binaries": {path.name: {}}}

    def test_prepare_covers_checked_and_explicit_products_with_installed_host_code(self):
        self.worker.process.return_value = ({"exit_code": 0}, json.dumps(self.receipt()).encode(), b"")
        with mock.patch("deployment.signing.verify", side_effect=self.manifest) as verify:
            self.worker.preparing(self.job)
        command = self.worker.process.call_args.args[2]
        self.assertEqual(command[:4], [sys.executable, "-I", "-B", str(Path(production.__file__))])
        self.assertEqual([command[index + 1] for index, item in enumerate(command) if item == "--product"],
                         ["alpha", "beta"])
        self.writes.assert_any_call(self.directory / "signing-policy.json", POLICY)
        verify.assert_not_called()
        self.worker.store.save.assert_called_once_with(self.job, "accepting")

    def test_signing_failure_stops_before_acceptance_without_repair(self):
        receipt = {"schema_version": 1, "state": "error", "failure_kind": "signing_configuration",
                   "message": "configured certificate is unavailable"}
        self.worker.process.return_value = ({"exit_code": 78}, json.dumps(receipt).encode(), b"")
        self.worker.preparing(self.job)
        self.assertEqual(self.job["configuration_error"]["kind"], "signing_configuration")
        self.worker.finish.assert_called_once()
        self.worker.store.save.assert_not_called()

    def test_cancellation_after_failed_preparation_keeps_receipt(self):
        receipt = {"schema_version": 1, "state": "error", "failure_kind": "signing_configuration",
                   "message": "configured certificate is unavailable"}
        self.worker.store.job.side_effect = [{"cancel_requested": False}, {"cancel_requested": True}]
        self.worker.process.return_value = ({"exit_code": 78}, json.dumps(receipt).encode(), b"")
        self.worker.preparing(self.job)
        self.assertEqual(self.job["production_receipt"], receipt)
        self.writes.assert_any_call(self.directory / "production-0.json", receipt)
        self.worker.finish.assert_called_once_with(
            self.job, "cancelled", "Cancelled after production preparation drained.")

    def test_skipped_tests_still_require_production_preparation(self):
        self.job.update(skip_tests=True, validations=[])
        receipt = {"schema_version": 1, "state": "passed", "base_commit": self.job["base_commit"],
                   "candidate_commit": self.job["candidate_commit"], "gates": [],
                   "selection": {"tests_skipped": True, "product_tests": ["alpha"], "platform_products": []}}
        self.worker.process.return_value = ({"exit_code": 0}, json.dumps(receipt).encode(), b"")
        with mock.patch("ci_manager.manager.git.ensure_worktree"), mock.patch(
                "ci_manager.manager.git.clean_candidate"):
            self.worker.checking(self.job)
        self.worker.store.save.assert_called_once_with(self.job, "preparing")

    def test_only_new_signed_jobs_with_supported_source_defer_release_builds(self):
        self.job.update(release_builds_deferred=True, validations=[])
        receipt = {"schema_version": 1, "state": "passed", "base_commit": self.job["base_commit"],
                   "candidate_commit": self.job["candidate_commit"], "gates": [],
                   "selection": {"release_builds_deferred": True, "product_tests": ["alpha"],
                                 "platform_products": []}}
        self.worker.process.return_value = ({"exit_code": 0}, json.dumps(receipt).encode(), b"")
        with mock.patch.object(Path, "is_file", return_value=True), mock.patch(
                "ci_manager.manager.git.ensure_worktree"), mock.patch("ci_manager.manager.git.clean_candidate"):
            self.worker.checking(self.job)
        self.assertIn("--defer-release-builds", self.worker.process.call_args.args[2])
        self.worker.store.save.assert_called_once_with(self.job, "preparing")
        for legacy in ({"release_builds_deferred": False}, {"signing_policy": None}):
            job = self.job | legacy
            if legacy.get("signing_policy", POLICY) is None:
                job.pop("signing_policy")
            with mock.patch.object(Path, "is_file", return_value=True):
                self.assertFalse(self.worker.release_builds_deferred(job))
        with mock.patch.object(Path, "is_file", return_value=False):
            self.assertFalse(self.worker.release_builds_deferred(self.job))

    def test_validator_cannot_defer_without_manager_release_build_policy(self):
        self.job["validations"] = []
        receipt = {"schema_version": 1, "state": "passed", "base_commit": self.job["base_commit"],
                   "candidate_commit": self.job["candidate_commit"], "gates": [],
                   "selection": {"release_builds_deferred": True}}
        self.worker.process.return_value = ({"exit_code": 0}, json.dumps(receipt).encode(), b"")
        with mock.patch("ci_manager.manager.git.ensure_worktree"), self.assertRaisesRegex(
                ManagerError, "release build policy"):
            self.worker.checking(self.job)
        self.worker.store.save.assert_not_called()

    def test_combined_release_failure_retains_compiler_diagnostics_for_repair(self):
        self.job["last_receipt"]["selection"]["release_builds_deferred"] = True
        receipt = {"schema_version": 1, "state": "error", "failure_kind": "release_build",
                   "message": "cargo build failed"}
        self.worker.process.return_value = ({"exit_code": 1}, json.dumps(receipt).encode(), b"compiler error")
        self.worker.preparing(self.job)
        self.assertIn("--release-check", self.worker.process.call_args.args[2])
        self.assertEqual(self.job["production_diagnostics"], str(self.directory / "production-0.log"))
        self.bytes_writes.assert_called_once_with(self.directory / "production-0.log",
                                                 b"compiler error\n" + json.dumps(receipt).encode())
        self.worker.store.save.assert_called_once_with(self.job, "repair_prepare")
        self.worker.finish.assert_not_called()

    def test_legacy_preparation_failure_does_not_change_its_repair_policy(self):
        receipt = {"schema_version": 1, "state": "error", "failure_kind": "release_build",
                   "message": "cargo build failed"}
        self.worker.process.return_value = ({"exit_code": 1}, json.dumps(receipt).encode(), b"compiler error")
        self.worker.preparing(self.job)
        self.worker.finish.assert_called_once()
        self.worker.store.save.assert_not_called()

    def test_deferred_release_requires_check_evidence_before_acceptance(self):
        self.job["last_receipt"]["selection"]["release_builds_deferred"] = True
        self.job["production_receipt"] = self.receipt()
        with mock.patch("ci_manager.manager.git.advance") as advance, self.assertRaisesRegex(
                ManagerError, "release check evidence"):
            self.worker.accepting(self.job)
        advance.assert_not_called()

    def test_changed_preparation_receipt_stops_before_acceptance(self):
        self.job["last_receipt"]["selection"]["release_builds_deferred"] = True
        receipt = self.receipt()
        receipt["preparation"]["release_check"] = True
        self.job["production_receipt"] = receipt
        with mock.patch("ci_manager.manager.production_candidate.regular"), mock.patch.object(
                Path, "read_text", return_value=json.dumps(receipt["preparation"] | {"elapsed_seconds": 42})), mock.patch(
                "ci_manager.manager.git.advance") as advance, self.assertRaisesRegex(
                ManagerError, "receipt changed"):
            self.worker.accepting(self.job)
        advance.assert_not_called()

    def test_missing_malformed_or_mismatched_receipts_cannot_advance_accepted(self):
        valid = self.receipt()
        variants = (None, [], valid | {"source_commit": "c" * 40},
                    valid | {"signing_policy": {}}, valid | {"products": ["alpha"]},
                    valid | {"preparation": []}, valid | {"preparation": {"candidates": []}})
        with mock.patch("ci_manager.manager.git.advance") as advance, mock.patch(
                "deployment.signing.verify") as verify:
            for receipt in variants:
                with self.subTest(receipt=receipt):
                    self.job["production_receipt"] = receipt
                    with self.assertRaises(ManagerError):
                        self.worker.accepting(self.job)
            advance.assert_not_called()
            verify.assert_not_called()

    def test_manifest_job_accepts_preparation_without_candidate_audits(self):
        self.job["policy"] = {"manifest_executor": 1}
        self.job["production_receipt"] = self.receipt()
        with mock.patch("deployment.signing.verify") as verify, mock.patch(
                "ci_manager.manager.git.clean_candidate"), mock.patch(
                "ci_manager.manager.git.advance") as advance:
            self.worker.accepting(self.job)
        verify.assert_not_called()
        self.descriptors.assert_not_called()
        advance.assert_called_once()
        self.assertEqual(self.job["deployment_request"]["manifest_executor"], 1)

    def deployment_receipt(self, **changes):
        return {"schema": 2, "manifest_executor": 1, "state": "succeeded", "exit_code": 0,
                "operation_state": "terminal", "source_commit": "b" * 40,
                "request_id": "deployment", "products": ["alpha"],
                "steps": [{"id": "build", "state": "succeeded"},
                          {"id": "alpha.deploy", "state": "succeeded"}], **changes}

    def manifest_deployment_job(self):
        return {**self.job, "policy": {"manifest_executor": 1}, "cancel_requested": False,
                "deployment_request": {"source_commit": "b" * 40, "request_id": "deployment",
                                       "products": ["alpha"], "manifest_executor": 1}}

    def test_manifest_success_requires_only_instruction_completion(self):
        job = self.manifest_deployment_job()
        self.worker.deployment_read = mock.Mock(return_value=self.deployment_receipt())
        self.worker.deploying(job)
        self.assertTrue(job["installation_completed"])
        self.worker.finish.assert_called_once_with(
            job, "succeeded", "Deployment instructions completed for the accepted candidate.")

    def test_manifest_incomplete_or_legacy_success_cannot_finish_job(self):
        variants = ({"schema": 1}, {"manifest_executor": 0}, {"exit_code": 1}, {"steps": []},
                    {"steps": [{"state": "failed"}]}, {"products": ["beta"]},
                    {"source_commit": "c" * 40}, {"request_id": "other"})
        for changed in variants:
            with self.subTest(changed=changed):
                self.worker.deployment_read = mock.Mock(return_value=self.deployment_receipt(**changed))
                with self.assertRaises(ManagerError):
                    self.worker.deploying(self.manifest_deployment_job())
        self.worker.finish.assert_not_called()

    def test_manifest_interruption_reports_failure_without_application_recovery(self):
        job = self.manifest_deployment_job()
        self.worker.deployment_read = mock.Mock(return_value=self.deployment_receipt(
            state="interrupted", exit_code=1, operation_state="interrupted",
            steps=[{"id": "alpha.deploy", "state": "interrupted"}]))
        self.worker.deploying(job)
        self.assertFalse(job["installation_completed"])
        self.worker.finish.assert_called_once_with(job, "failed", "Deployment instructions interrupted.")

    def test_manifest_interruption_before_first_instruction_is_terminal_failure(self):
        job = self.manifest_deployment_job()
        self.worker.deployment_read = mock.Mock(return_value=self.deployment_receipt(
            state="interrupted", exit_code=1, operation_state="interrupted", steps=[]))
        self.worker.deploying(job)
        self.assertFalse(job["installation_completed"])
        self.worker.process.assert_not_called()
        self.worker.finish.assert_called_once_with(job, "failed", "Deployment instructions interrupted.")

    def test_policy_drift_blocks_model_repair_and_legacy_jobs_keep_their_policy(self):
        self.current.side_effect = signing.SigningError("signing selection changed")
        with self.assertRaisesRegex(signing.SigningError, "selection changed"):
            self.worker.repair_prepare(self.job)
        self.worker.store.save.assert_not_called()
        self.current.reset_mock()
        legacy = {key: value for key, value in self.job.items() if not key.startswith("signing_policy")}
        self.worker.assert_signing_policy(legacy)
        self.current.assert_not_called()

    def test_legacy_policy_digest_is_ignored_when_policy_matches(self):
        self.job["signing_policy_digest"] = "obsolete-digest"
        self.worker.assert_signing_policy(self.job)
        self.current.assert_called_once_with(POLICY)

    def test_deployment_carries_only_the_frozen_signing_policy(self):
        self.job["deployment_request"] = {"request_id": "deployment", "source_commit": "b" * 40,
                                           "products": ["alpha"]}
        command = self.worker.deployment_command(self.job, "start")
        self.assertIn("--signing-policy-file", command)
        self.writes.assert_called_once_with(self.directory / "signing-policy.json", POLICY)
        self.assertNotIn("--signing-policy-file", self.worker.deployment_command(self.job, "status"))

    def test_deployment_reuses_the_exact_deferred_production_preparation(self):
        self.job["last_receipt"]["selection"]["release_builds_deferred"] = True
        self.job["production_receipt"] = self.receipt()
        self.job["deployment_request"] = {"request_id": "deployment", "source_commit": "b" * 40,
                                           "products": ["alpha"]}
        command = self.worker.deployment_command(self.job, "start")
        self.assertEqual(command[command.index("--prepared-build") + 1],
                         str(self.directory / "production-0/result.json"))
        self.assertEqual(command[command.index("--prepared-build-snapshot-file") + 1],
                         str(self.directory / "production-0.snapshot.json"))
        self.writes.assert_any_call(self.directory / "production-0.snapshot.json",
                                   self.job["production_receipt"]["preparation"])
        self.assertNotIn("--prepared-build", self.worker.deployment_command(self.job, "status"))


class ProductionHelperTests(unittest.TestCase):
    def setUp(self):
        self.source = Path("/evidence/source")
        self.output = Path("/evidence/output")
        self.commit = "b" * 40
        self.result = {"schema": 1, "state": "built", "source_key": self.commit, "signing_policy": POLICY,
                       "candidates": {"alpha": {"candidate_dir": str(self.output / "candidates/alpha"),
                                                 "candidate_id": "candidate:alpha"}}}
        self.manifest = {"source_commit": self.commit, "source_key": self.commit, "product": "alpha",
                         "candidate_id": "candidate:alpha", "binaries": {"alpha": {}}}
        stack = ExitStack()
        self.addCleanup(stack.close)
        stack.enter_context(mock.patch.object(signing, "assert_current"))
        stack.enter_context(mock.patch.object(signing, "preflight"))
        stack.enter_context(mock.patch.object(production, "source_commit", return_value=self.commit))
        self.clean = stack.enter_context(mock.patch.object(production.git_ops, "clean_candidate"))
        self.exists = stack.enter_context(mock.patch.object(Path, "exists", return_value=False))
        stack.enter_context(mock.patch.object(Path, "read_text", return_value=json.dumps(self.result)))
        self.build = stack.enter_context(mock.patch.object(production.build, "prepare", return_value=self.result))
        self.verify = stack.enter_context(mock.patch.object(signing, "verify", return_value=self.manifest))
        self.descriptors = stack.enter_context(mock.patch.object(production.build, "read_descriptor",
            return_value={"RELEASE_BINARY_CHECKS": "main|target/release/alpha|alpha"}))

    def test_fresh_preparation_uses_frozen_policy_without_auditing_outputs(self):
        receipt = production.prepare(self.source, ["alpha"], self.output, POLICY)
        self.build.assert_called_once_with(self.source, ["alpha"], self.output, signing_policy=POLICY)
        self.verify.assert_not_called()
        self.assertEqual(self.clean.call_args_list, [mock.call(self.source, self.commit)] * 2)
        self.assertEqual(receipt["signing_policy"], POLICY)
        self.assertEqual(receipt["source_commit"], self.commit)

    def test_retained_preparation_reuses_command_result_without_artifact_audit(self):
        self.exists.return_value = True
        production.prepare(self.source, ["alpha"], self.output, POLICY)
        self.build.assert_not_called()
        self.verify.assert_not_called()

    def test_retained_preparation_does_not_interpret_executable_inventory(self):
        self.exists.return_value = True
        self.descriptors.return_value = {"RELEASE_BINARY_CHECKS":
            "main|target/release/alpha|alpha\nmain|target/release/alpha-helper|alpha-helper"}
        production.prepare(self.source, ["alpha"], self.output, POLICY)
        self.build.assert_not_called()
        self.descriptors.assert_not_called()

    def test_deferred_release_uses_strict_combined_check_and_rejects_other_policy(self):
        self.build.return_value = self.result | {"release_check": True}
        production.prepare(self.source, ["alpha"], self.output, POLICY, release_check=True)
        self.build.assert_called_once_with(self.source, ["alpha"], self.output,
                                          signing_policy=POLICY, release_check=True)
        self.exists.return_value = True
        with self.assertRaisesRegex(production.candidate.CandidateError, "release check policy"):
            production.prepare(self.source, ["alpha"], self.output, POLICY, release_check=True)

    def test_initial_dirty_or_changed_head_stops_before_production_build(self):
        self.clean.side_effect = ManagerError("candidate worktree changed")
        with self.assertRaisesRegex(ManagerError, "worktree changed"):
            production.prepare(self.source, ["alpha"], self.output, POLICY)
        self.build.assert_not_called()
        self.verify.assert_not_called()

    def test_dirty_or_changed_head_after_preparation_cannot_produce_success(self):
        self.clean.side_effect = [None, ManagerError("candidate worktree changed")]
        with self.assertRaisesRegex(ManagerError, "worktree changed"):
            production.prepare(self.source, ["alpha"], self.output, POLICY)
        self.verify.assert_not_called()
        self.assertEqual(self.clean.call_args_list, [mock.call(self.source, self.commit)] * 2)

    def test_preparation_receipt_must_have_the_frozen_source_key(self):
        self.build.return_value = self.result | {"source_key": "dirty:fresh-build"}
        with self.assertRaisesRegex(production.candidate.CandidateError, "source identity"):
            production.prepare(self.source, ["alpha"], self.output, POLICY)
        self.verify.assert_not_called()

    def test_other_source_location_or_malformed_scope_is_rejected(self):
        variants = ({"source_key": "c" * 40}, {"candidates": []},
                    {"candidates": {"alpha": {"candidate_dir": "/other/candidate",
                                              "candidate_id": "candidate:alpha"}}})
        for changed in variants:
            with self.subTest(changed=changed):
                self.build.return_value = self.result | changed
                with self.assertRaises(production.candidate.CandidateError):
                    production.prepare(self.source, ["alpha"], self.output, POLICY)


class ValidationEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.directory = Path("/evidence/job")
        self.job = {"candidate_commit": "b" * 40, "base_commit": "a" * 40, "validations": []}
        worktree = self.directory / "worktree"
        self.request_path = self.directory / "validation-0.request.json"
        request = {"command": ["/python", str(worktree / "pipeline/select_changes.py"), "run",
                               "--base", self.job["base_commit"], "--candidate", self.job["candidate_commit"], "--json"],
                   "cwd": str(worktree), **{key: str(self.directory / f"validation-0.{suffix}")
                       for key, suffix in (("stdout", "stdout"), ("stderr", "stderr"),
                                           ("started", "started.json"), ("result", "result.json"))}}
        self.evidence = {str(self.request_path): json.dumps(request),
                         str(self.directory / "validation-0.result.json"):
                            json.dumps({"request": str(self.request_path), "exit_code": 75})}
        stack = ExitStack()
        self.addCleanup(stack.close)
        stack.enter_context(mock.patch.object(Path, "exists", lambda path: str(path) in self.evidence))
        stack.enter_context(mock.patch.object(Path, "read_text", lambda path: self.evidence[str(path)]))

    def test_current_and_recorded_terminal_validation_supply_the_same_proof(self):
        self.assertTrue(validation_exited(self.directory, self.job))
        self.job["validations"] = [{"candidate": self.job["candidate_commit"],
                                    "receipt": str(self.directory / "validation-0.json")}]
        self.assertTrue(validation_exited(self.directory, self.job))

    def test_newer_or_mismatched_evidence_never_uses_an_older_result(self):
        self.job["validations"] = [{"candidate": self.job["candidate_commit"],
                                    "receipt": str(self.directory / "validation-0.json")}]
        original = copy.deepcopy(self.evidence)
        for newer in ("request.json", "json", "log"):
            with self.subTest(newer=newer):
                self.evidence[str(self.directory / f"validation-1.{newer}")] = "{}"
                self.assertFalse(validation_exited(self.directory, self.job))
                self.evidence = copy.deepcopy(original)
        self.job["validations"][0]["candidate"] = "c" * 40
        self.assertFalse(validation_exited(self.directory, self.job))


if __name__ == "__main__":
    unittest.main()
