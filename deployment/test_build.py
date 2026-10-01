"""Resource-free proofs of prepared candidate reuse and release checking."""

from __future__ import annotations

import contextlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock
import uuid

from deployment import build, candidate, cli, signing


COMMIT = "a" * 40
POLICY = {"schema": 1, "macos": {"profile": "local", "certificate_sha1": "b" * 40,
          "keychain": "/Users/fixture/Library/Keychains/login.keychain-db",
          "identifier_namespace": "local.cell"}}
BUILD_CONFIGURATION = build.build_configuration


class BuildTests(unittest.TestCase):
    def setUp(self):
        self.directory = Path(self.enterContext(tempfile.TemporaryDirectory())).resolve()
        self.source = self.directory / "source"
        self.source.mkdir()
        self.output = self.directory / "output"
        self.cache = self.directory / "cache"
        self.enterContext(mock.patch.dict(build.os.environ, {"CELL_RELEASE_CACHE_DIR": str(self.cache)}))
        for name in ("require_capacity", "require_path"):
            self.enterContext(mock.patch.object(build.workspace, name))
        self.enterContext(mock.patch.object(build.workspace, "directory", return_value=self.cache))
        self.enterContext(mock.patch.object(build.workspace, "confined_command", side_effect=lambda command: command))
        self.enterContext(mock.patch.object(build, "git", return_value=COMMIT.encode()))
        self.enterContext(mock.patch.object(candidate, "source_identity", return_value=COMMIT))
        self.enterContext(mock.patch.object(signing, "load_policy", return_value=POLICY))
        self.enterContext(mock.patch.object(signing, "assert_current"))
        self.enterContext(mock.patch.object(signing, "preflight"))
        self.sign = self.enterContext(mock.patch.object(signing, "sign"))
        self.verify_signature = self.enterContext(mock.patch.object(signing, "verify"))
        self.configuration = self.enterContext(mock.patch.object(build, "build_configuration",
            return_value=({"CARGO_BUILD_WARNINGS": "deny"}, {"target": "fixture", "jobs": 2})))
        self.enterContext(mock.patch.object(build, "read_descriptor", side_effect=self.descriptor))
        metadata = {"packages": [{"name": name, "version": "1.0.0",
            "targets": [{"name": name, "kind": ["bin"]}]} for name in ("alpha", "beta")]}
        self.tools = self.enterContext(mock.patch.object(build, "tool_output", return_value=json.dumps(metadata)))
        self.cargo = self.enterContext(mock.patch.object(build.subprocess, "run", side_effect=self.compile))
        self.enterContext(mock.patch.object(candidate, "stage_build", side_effect=self.stage))

    @staticmethod
    def descriptor(source, product):
        return {"PRODUCT_DIR": product, "CARGO_PACKAGES": f"{product} {product}-support",
                "CARGO_OFFLINE": "1" if product == "alpha" else "0",
                "RELEASE_BINARY_CHECKS": f"main|target/release/{product}|{product}", "RELEASE_UNITS": "main|1"}

    def bundle(self, directory, product, *, commit=COMMIT):
        (directory / "bin").mkdir(parents=True)
        binary = directory / "bin" / product
        binary.write_bytes(f"fixture {product}".encode())
        binary.chmod(0o755)
        manifest = {"schema": 1, "product": product, "source_commit": commit, "source_key": COMMIT,
                    "signing_policy": POLICY,
                    "binaries": {product: {"path": f"bin/{product}",
                         "version": f"{product} 1.0.0", "code_identifier": signing.identifier(POLICY, product, product)}}}
        self.write_manifest(directory, manifest)
        candidate.seal_tree(directory)
        return manifest

    @staticmethod
    def write_manifest(directory, manifest):
        manifest["candidate_id"] = "uuid:" + uuid.uuid4().hex
        path = directory / "candidate.json"
        if path.exists():
            path.chmod(0o600)
        path.write_bytes(candidate.json_bytes(manifest))

    def supplied(self, products=("alpha",)):
        root = self.directory / "supplied"
        candidates = {}
        for product in products:
            directory = root / "candidates" / product
            manifest = self.bundle(directory, product)
            candidates[product] = {"candidate_dir": str(directory), "candidate_id": manifest["candidate_id"],
                                   "source_key": COMMIT}
        receipt = {"schema": 1, "state": "built", "source_key": COMMIT,
                   "signing_policy": POLICY,
                   "candidates": candidates}
        path = root / "result.json"
        path.write_bytes(candidate.json_bytes(receipt))
        return path, receipt

    def compile(self, command, **options):
        for product in ("alpha", "beta"):
            binary = self.cache / "target" / "fixture" / "release" / product
            binary.parent.mkdir(parents=True, exist_ok=True)
            binary.write_bytes(f"fixture {product}".encode())
        return subprocess.CompletedProcess(command, 0)

    def stage(self, source, product, output, *arguments, **options):
        return self.bundle(output, product)

    def test_all_supplied_candidates_are_copied_and_verified_without_cargo(self):
        path, _ = self.supplied(("alpha", "beta"))
        result = build.prepare(self.source, ["alpha", "beta"], self.output, prepared_build=path)
        self.assertEqual(result["reused_products"], ["alpha", "beta"])
        self.assertEqual(result["built_products"], [])
        self.assertEqual(result["build_seconds"], 0.0)
        self.cargo.assert_not_called()
        self.tools.assert_not_called()
        self.configuration.assert_not_called()
        self.sign.assert_not_called()
        self.assertGreaterEqual(self.verify_signature.call_count, 4)
        copied = self.output / "candidates" / "alpha" / "bin" / "alpha"
        supplied = path.parent / "candidates" / "alpha" / "bin" / "alpha"
        self.assertEqual(copied.read_bytes(), supplied.read_bytes())
        self.assertNotEqual(copied.stat().st_ino, supplied.stat().st_ino)
        self.assertEqual(copied.stat().st_mode & 0o777, 0o555)

    def test_only_missing_products_share_one_compilation(self):
        path, _ = self.supplied()
        result = build.prepare(self.source, ["alpha", "beta"], self.output, prepared_build=path)
        self.assertEqual(result["reused_products"], ["alpha"])
        self.assertEqual(result["built_products"], ["beta"])
        self.assertEqual(self.cargo.call_count, 1)
        command = self.cargo.call_args.args[0]
        self.assertEqual(command[command.index("--package") + 1], "beta")
        self.assertNotIn("alpha", command)
        self.assertNotIn("alpha-support", command)
        self.assertFalse((self.output / "missing").exists())

    def test_receipt_source_or_signer_mismatch_has_no_compile_fallback(self):
        path, receipt = self.supplied()
        other_policy = POLICY | {"macos": POLICY["macos"] | {"identifier_namespace": "other.cell"}}
        for field, replacement in (("source_key", "c" * 40), ("signing_policy", other_policy)):
            with self.subTest(field=field):
                value = {**receipt, field: replacement}
                path.write_bytes(candidate.json_bytes(value))
                with self.assertRaises(build.BuildError):
                    build.prepare(self.source, ["alpha", "beta"], self.output, prepared_build=path)
        self.cargo.assert_not_called()

    def test_candidate_source_identity_and_inventory_changes_stop_before_missing_build(self):
        path, receipt = self.supplied()
        directory = Path(receipt["candidates"]["alpha"]["candidate_dir"])
        manifest = candidate.read_manifest(directory)
        manifest["source_commit"] = "c" * 40
        self.write_manifest(directory, manifest)
        receipt["candidates"]["alpha"]["candidate_id"] = manifest["candidate_id"]
        path.write_bytes(candidate.json_bytes(receipt))
        with self.assertRaisesRegex(build.BuildError, "selected source"):
            build.prepare(self.source, ["alpha", "beta"], self.output, prepared_build=path)
        manifest["source_commit"] = COMMIT
        self.write_manifest(directory, manifest)
        receipt["candidates"]["alpha"]["candidate_id"] = manifest["candidate_id"]
        path.write_bytes(candidate.json_bytes(receipt))
        directory.chmod(0o700)
        (directory / "unexpected").write_text("unexpected payload")
        with self.assertRaisesRegex(candidate.CandidateError, "inventory changed"):
            build.prepare(self.source, ["alpha", "beta"], self.output, prepared_build=path)
        self.cargo.assert_not_called()

    def test_invalid_native_signature_stops_before_missing_build(self):
        path, _ = self.supplied()
        self.verify_signature.side_effect = signing.SigningError("signature mismatch")
        with self.assertRaisesRegex(signing.SigningError, "signature mismatch"):
            build.prepare(self.source, ["alpha", "beta"], self.output, prepared_build=path)
        self.cargo.assert_not_called()

    def test_admitted_receipt_snapshot_rejects_changed_values(self):
        path, receipt = self.supplied()
        path.write_bytes(candidate.json_bytes(receipt | {"elapsed_seconds": 42}))
        with self.assertRaisesRegex(build.BuildError, "changed after deployment admission"):
            build.prepare(self.source, ["alpha"], self.output, prepared_build=path,
                          prepared_build_snapshot=receipt)
        self.cargo.assert_not_called()

    def test_admitted_snapshot_uses_json_values_and_ignores_legacy_artifact_digests(self):
        path, receipt = self.supplied()
        receipt["signing_policy_digest"] = "obsolete"
        record = receipt["candidates"]["alpha"]
        directory = Path(record["candidate_dir"])
        manifest = candidate.read_manifest(directory)
        manifest["signing_policy_digest"] = "obsolete"
        manifest["binaries"]["alpha"]["sha256"] = "obsolete"
        self.write_manifest(directory, manifest)
        record["candidate_id"] = manifest["candidate_id"]
        path.write_text(json.dumps(receipt, indent=2))
        result = build.prepare(self.source, ["alpha"], self.output, prepared_build=path,
                               prepared_build_snapshot=receipt)
        self.assertEqual(result["reused_products"], ["alpha"])
        self.verify_signature.assert_called()
        self.cargo.assert_not_called()

    def test_supplied_reuse_cannot_claim_a_release_check_that_was_not_performed(self):
        path, _ = self.supplied()
        with self.assertRaisesRegex(build.BuildError, "no release check evidence"):
            build.prepare(self.source, ["alpha"], self.output, prepared_build=path, release_check=True)
        self.cargo.assert_not_called()

    def test_release_check_covers_all_descriptor_packages_offline_without_bin_narrowing(self):
        result = build.prepare(self.source, ["alpha", "beta"], self.output, release_check=True)
        command = self.cargo.call_args.args[0]
        self.assertTrue(result["release_check"])
        self.assertIn("--offline", command)
        self.assertNotIn("--bin", command)
        self.assertEqual([command[index + 1] for index, value in enumerate(command) if value == "--package"],
                         ["alpha", "alpha-support", "beta", "beta-support"])
        self.assertEqual(self.configuration.call_args.kwargs, {"release_check": True, "offline": True})

    def test_only_cargo_build_failure_is_compilation_error_and_preserves_output_stream(self):
        self.cargo.side_effect = subprocess.CalledProcessError(101, ["cargo", "build"])
        with self.assertRaises(build.CompilationError):
            build.prepare(self.source, ["alpha"], self.output)
        self.assertIs(self.cargo.call_args.kwargs["stdout"], sys.stderr)
        self.assertNotIn("stderr", self.cargo.call_args.kwargs)
        self.assertNotIn("capture_output", self.cargo.call_args.kwargs)
        self.tools.side_effect = subprocess.CalledProcessError(1, ["cargo", "metadata"])
        with self.assertRaises(subprocess.CalledProcessError):
            build.prepare(self.source, ["alpha"], self.output)

    def test_warning_denial_and_offline_are_build_configuration_inputs(self):
        with mock.patch.dict(build.os.environ, {"CARGO_HOME": str(self.directory / "cargo-home")}, clear=True), \
                mock.patch.object(build, "bootstrap_cargo_path"), \
                mock.patch.object(build.workspace, "environment", return_value={}), \
                mock.patch.object(build, "tool_output", side_effect=["cargo 1.97.1 (fixture)", "rustc 1.97.1 (fixture)\nhost: fixture"]):
            environment, configuration = BUILD_CONFIGURATION(self.source, self.cache, release_check=True, offline=True)
        self.assertEqual(environment["CARGO_BUILD_WARNINGS"], "deny")
        self.assertEqual(environment["CARGO_NET_OFFLINE"], "true")
        self.assertNotIn("RUSTFLAGS", environment)
        self.assertEqual(configuration["target"], "fixture")

    def test_release_check_rejects_an_unmatched_toolchain(self):
        with mock.patch.dict(build.os.environ, {"CARGO_HOME": str(self.directory / "cargo-home")}, clear=True), \
                mock.patch.object(build, "bootstrap_cargo_path"), \
                mock.patch.object(build.workspace, "environment", return_value={}), \
                mock.patch.object(build, "tool_output", side_effect=["cargo 1.96.0", "rustc 1.97.1\nhost: fixture"]):
            with self.assertRaisesRegex(build.BuildError, "cargo and rustc 1.97.1"):
                BUILD_CONFIGURATION(self.source, self.cache, release_check=True)


class PreparedRequestTests(unittest.TestCase):
    def setUp(self):
        self.directory = Path(self.enterContext(tempfile.TemporaryDirectory())).resolve()
        self.receipt = self.directory / "result.json"
        self.receipt.write_text('{}\n')
        self.enterContext(mock.patch.object(cli.workspace, "require_path"))
        self.enterContext(mock.patch.object(cli, "source_commit", return_value=COMMIT))
        self.enterContext(mock.patch.object(cli, "catalog", return_value={"alpha": {}}))
        self.enterContext(mock.patch.object(cli, "requested_products", return_value=["alpha"]))
        self.enterContext(mock.patch.object(cli.ci_client, "common_git_directory", return_value=self.directory / ".git"))

    def request(self, path=None):
        return cli.canonical_request(self.directory, ["alpha"], COMMIT, None, POLICY, path)

    def test_replay_requires_same_prepared_path_and_values_without_readmitting(self):
        request = self.request(self.receipt)
        with mock.patch.object(cli, "deployment_lock", return_value=contextlib.nullcontext(7)), \
                mock.patch.object(cli, "read_operation", return_value={"request": request}), \
                mock.patch.object(cli, "operation_status", return_value={"operation_state": "terminal"}), \
                mock.patch.object(cli, "create_run") as create:
            result = cli.start_correlated(self.directory, ["alpha"], self.directory,
                request_id="same", selected_commit=COMMIT, verbose=False, settings=None,
                signing_policy=POLICY, prepared_build=self.receipt)
            self.assertTrue(result["replayed"])
            self.receipt.write_text('{"changed":true}\n')
            with self.assertRaisesRegex(cli.DeploymentError, "different deployment request"):
                cli.start_correlated(self.directory, ["alpha"], self.directory,
                    request_id="same", selected_commit=COMMIT, verbose=False, settings=None,
                    signing_policy=POLICY, prepared_build=self.receipt)
            create.assert_not_called()
        self.assertNotEqual(request, self.request())

    def test_legacy_requests_omit_new_input_and_keep_replay_semantics(self):
        request = self.request()
        self.assertNotIn("prepared_build", request)
        self.assertTrue(cli.matching_request(request, request, explicit_policy=True))
        self.assertFalse(cli.matching_request(request, self.request(self.receipt), explicit_policy=True))

    def test_legacy_prepared_admission_keeps_its_path_and_ignores_obsolete_digest(self):
        retained = self.request(self.receipt)
        retained["prepared_build"] = {"path": str(self.receipt), "sha256": "obsolete"}
        retained["signing_policy_digest"] = "obsolete"
        self.assertTrue(cli.matching_request(retained, self.request(self.receipt), explicit_policy=True))
        other = self.directory / "other-result.json"
        other.write_text("{}")
        self.assertFalse(cli.matching_request(retained, self.request(other), explicit_policy=True))

    def test_caller_snapshot_mismatch_stops_before_admission(self):
        with mock.patch.object(cli, "deployment_lock") as lock, \
                mock.patch.object(cli, "save_operation") as save:
            with self.assertRaisesRegex(cli.DeploymentError, "caller's retained selection"):
                cli.start_correlated(self.directory, ["alpha"], self.directory,
                    request_id="same", selected_commit=COMMIT, verbose=False, settings=None,
                    signing_policy=POLICY, prepared_build=self.receipt, prepared_build_snapshot={"changed": True})
            lock.assert_not_called()
            save.assert_not_called()
        with self.assertRaisesRegex(cli.DeploymentError, "snapshot requires a prepared build"):
            cli.prepared_selection(None, {"changed": True})
        self.assertEqual(cli.prepared_selection(self.receipt, {}),
                         cli.prepared_selection(self.receipt))

    def test_run_retains_exact_prepared_selection_for_worker_and_recovery(self):
        selection = cli.prepared_selection(self.receipt)
        chosen = {"schema": 1, "source_commit": COMMIT, "products": ["alpha"],
                  "catalog": {"alpha": {"metadata": {"dependencies": []}}}}
        with mock.patch.object(signing, "assert_current"), mock.patch.object(signing, "preflight"), \
                mock.patch.object(cli, "archive_source"):
            path = cli.create_run(self.directory, ["alpha"], self.directory / "storage", chosen_plan=chosen,
                signing_policy=POLICY, prepared_build=selection)
        self.assertEqual(cli.read_json(path / "run.json")["prepared_build"], selection)
        run = cli.Run(path, 7)
        run.worktree.mkdir()
        result = {"state": "built", "source_key": COMMIT,
                  "reused_products": ["alpha"], "built_products": [], "build_seconds": 0.0}

        def prepared(product, operation, command, **options):
            preparation = path / "preparation"
            preparation.mkdir()
            cli.durable_json(preparation / "result.json", result)
            return 0, path / "output"

        manifest = {"product": "alpha", "source_commit": COMMIT, "source_key": COMMIT,
                    "candidate_id": "uuid:fixture"}
        with mock.patch.object(run, "command", side_effect=prepared) as command, \
                mock.patch.object(candidate, "verify_signatures", return_value=manifest):
            run.prepare()
        invocation = command.call_args.args[2]
        self.assertEqual(invocation[invocation.index("--prepared-build") + 1], str(self.receipt))
        snapshot_path = Path(invocation[invocation.index("--prepared-build-snapshot-file") + 1])
        self.assertEqual(cli.read_json(snapshot_path), selection["snapshot"])
        self.assertEqual(run.data["build"]["reused_products"], ["alpha"])
        self.assertEqual(run.data["records"]["alpha"]["candidate_dir"], str(path / "preparation" / "candidates" / "alpha"))


if __name__ == "__main__":
    unittest.main()
