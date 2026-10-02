"""Resource-free proofs of prepared candidate reuse and release checking."""

from __future__ import annotations

import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock
import uuid

from deployment import build, candidate, signing


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

    def test_all_supplied_candidates_are_copied_without_cargo_or_audits(self):
        path, _ = self.supplied(("alpha", "beta"))
        result = build.prepare(self.source, ["alpha", "beta"], self.output, prepared_build=path)
        self.assertEqual(result["reused_products"], ["alpha", "beta"])
        self.assertEqual(result["built_products"], [])
        self.assertEqual(result["build_seconds"], 0.0)
        self.cargo.assert_not_called()
        self.tools.assert_not_called()
        self.configuration.assert_not_called()
        self.sign.assert_not_called()
        self.verify_signature.assert_not_called()
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

    def test_candidate_source_identity_mismatch_stops_before_missing_build(self):
        path, receipt = self.supplied()
        directory = Path(receipt["candidates"]["alpha"]["candidate_dir"])
        manifest = candidate.read_manifest(directory)
        manifest["source_commit"] = "c" * 40
        self.write_manifest(directory, manifest)
        receipt["candidates"]["alpha"]["candidate_id"] = manifest["candidate_id"]
        path.write_bytes(candidate.json_bytes(receipt))
        with self.assertRaisesRegex(build.BuildError, "selected source"):
            build.prepare(self.source, ["alpha", "beta"], self.output, prepared_build=path)
        self.cargo.assert_not_called()

    def test_supplied_payload_is_opaque_and_native_signature_is_not_a_gate(self):
        path, receipt = self.supplied()
        directory = Path(receipt["candidates"]["alpha"]["candidate_dir"])
        directory.chmod(0o700)
        (directory / "unexpected").write_bytes(b"opaque payload")
        self.verify_signature.side_effect = signing.SigningError("signature mismatch")
        result = build.prepare(self.source, ["alpha"], self.output, prepared_build=path)
        self.assertEqual(result["reused_products"], ["alpha"])
        self.assertEqual((self.output / "candidates/alpha/unexpected").read_bytes(), b"opaque payload")
        self.verify_signature.assert_not_called()
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
        self.verify_signature.assert_not_called()
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


if __name__ == "__main__":
    unittest.main()
