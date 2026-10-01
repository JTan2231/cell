"""Release preparation with disposable Git and fake compilers only."""

from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

sys.dont_write_bytecode = True

from deployment import build, candidate


FAKE_CARGO = r'''#!PYTHON
import json, pathlib, sys
base = pathlib.Path(BASE)
args = sys.argv[1:]
if args == ["--version"]:
    print("cargo fixture 1.0")
elif args[0] == "metadata":
    print(pathlib.Path("metadata.json").read_text())
elif args[0] == "build":
    with (base / "calls.jsonl").open("a") as stream:
        stream.write(json.dumps(args) + "\n")
    fault = json.loads((base / "fault.json").read_text()) if (base / "fault.json").exists() else {}
    if fault.get("source_change"):
        pathlib.Path("source.txt").write_text("changed during build\n")
    target = pathlib.Path(args[args.index("--target-dir") + 1]) / args[args.index("--target") + 1] / "release"
    target.mkdir(parents=True, exist_ok=True)
    for index, value in enumerate(args):
        if value == "--bin":
            name = args[index + 1]
            binary = target / name
            version = "9.9.9" if fault.get("wrong_version") else "1.0.0"
            binary.write_text("#!/bin/sh\nprintf '%s\\n' '" + name + " " + version + "'\n")
            binary.chmod(0o755)
else:
    raise SystemExit("unexpected Cargo command: " + repr(args))
'''


class BuildTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.base = Path(self.temporary.name).resolve()
        self.signing = {}
        for name in ("load_policy", "assert_current", "preflight", "identifier", "policy_digest", "sign", "verify"):
            patch = mock.patch.object(build.signing, name)
            self.signing[name] = patch.start()
            self.addCleanup(patch.stop)
        self.signing["load_policy"].return_value = None
        self.signing["policy_digest"].side_effect = lambda policy: build.hashlib.sha256(candidate.json_bytes(policy)).hexdigest()
        self.signing["identifier"].side_effect = lambda policy, product, key: f"local.cell.{product}.{key}"
        self.policy = {"schema": 1, "macos": {"profile": "local", "certificate_sha1": "a" * 40,
                                              "keychain": "/fixture/signing.keychain-db", "identifier_namespace": "local.cell"}}
        storage_patch = mock.patch.object(build.workspace, "root", return_value=self.base)
        storage_patch.start()
        self.addCleanup(storage_patch.stop)
        # Fake compilers are confined by the enclosing CI gate. Storage's
        # subprocess tests separately prove the production filesystem sandbox.
        confinement_patch = mock.patch.object(build.workspace, "confined_command", side_effect=lambda command: command)
        confinement_patch.start()
        self.addCleanup(confinement_patch.stop)
        self.source = self.base / "source"
        self.source.mkdir()
        self.git("init", "-q", "-b", "main")
        self.git("config", "user.name", "Build fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        self.write("Cargo.toml", "[workspace]\nmembers = []\n")
        self.write("Cargo.lock", "version = 4\n")
        self.write("source.txt", "source\n")
        self.write("deployment/crates/cell-install/src/lib.rs", "// shared adapter\n")
        packages = []
        for name in ("alpha", "beta"):
            directory = "beta-source" if name == "beta" else name
            binaries = [name, name + "-helper"] if name == "alpha" else [name]
            binary_rows = "\n".join(f"{binary}|target/release/{binary}|{binary}" for binary in binaries)
            self.write(f"pipeline/products/{name}.sh", f"PRODUCT_ID={name}\nPRODUCT_DIR={directory}\n"
                       f"CARGO_PACKAGES={name}-package\nRELEASE_UNITS='" + "\n".join(
                           f"{binary}|Fixture|package|Cargo.toml|fixture-|1" for binary in binaries)
                       + f"'\nRELEASE_BINARY_CHECKS='{binary_rows}'\n")
            self.write(f"{directory}/packaging/manifest.txt", "owned packaging\n")
            self.write(f"{directory}/chancery/provider.json", '{"release":"1.0.0"}\n')
            self.write(f"{directory}/deployment/adapter.py", "# owned adapter\n")
            packages.append({"name": name + "-package", "version": "1.0.0",
                             "targets": [{"name": binary, "kind": ["bin"]} for binary in binaries]})
        self.write("metadata.json", json.dumps({"packages": packages}))
        self.commit()
        tools = self.base / "tools"
        tools.mkdir()
        (tools / "cargo").write_text(FAKE_CARGO.replace("PYTHON", sys.executable).replace("BASE", repr(str(self.base))))
        (tools / "rustc").write_text("#!/bin/sh\nprintf '%s\\n' 'rustc fixture 1.0' 'host: fixture-host'\n")
        for tool in tools.iterdir():
            tool.chmod(0o755)
        self.cache = self.base / "cache"
        self.environment = mock.patch.dict(os.environ, {
            "PATH": str(tools) + os.pathsep + os.environ.get("PATH", ""),
            "CELL_RELEASE_CACHE_DIR": str(self.cache), "CELL_RELEASE_BUILD_JOBS": "3",
        })
        self.environment.start()

    def tearDown(self) -> None:
        self.environment.stop()
        for path in self.base.rglob("*"):
            if path.is_dir() and not path.is_symlink():
                path.chmod(0o700)
        self.temporary.cleanup()

    def write(self, relative: str, content: str) -> None:
        path = self.source / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)

    def git(self, *arguments: str) -> str:
        result = subprocess.run(["git", "-C", str(self.source), *arguments], check=True,
                                capture_output=True, text=True)
        return result.stdout.strip()

    def commit(self) -> None:
        self.git("add", ".")
        self.git("commit", "-qm", "fixture")

    def calls(self) -> list[list[str]]:
        path = self.base / "calls.jsonl"
        return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []

    def prepare(self, name: str, products: list[str] | None = None, unit: str | None = None) -> dict:
        return build.prepare(self.source, products or ["alpha"], self.base / name, unit)

    def test_one_selected_batch_and_complete_cached_materials(self) -> None:
        result = self.prepare("output", ["beta", "alpha"])
        self.assertEqual(len(self.calls()), 1)
        command = self.calls()[0]
        self.assertEqual(command.count("--package"), 2)
        self.assertEqual(command.count("--bin"), 3)
        self.assertIn("--release", command)
        self.assertIn("--locked", command)
        self.assertNotIn("--workspace", command)
        self.assertEqual(command[command.index("--jobs") + 1], "3")
        self.assertFalse(result["cache_hit"])
        self.assertEqual(result["source_key"], self.git("rev-parse", "HEAD"))
        for product in ("alpha", "beta"):
            manifest = candidate.read_manifest(self.base / "output" / "candidates" / product)
            self.assertEqual(manifest["source_key"], result["source_key"])
            self.assertNotIn("source_inputs", manifest)
        self.assertEqual(json.loads((self.base / "output/result.json").read_text()), result)

    def test_clean_commit_reuses_build_in_another_worktree(self) -> None:
        first = self.prepare("before")
        worktree = self.base / "worktree"
        self.git("worktree", "add", "--detach", str(worktree), "HEAD")
        second = build.prepare(worktree, ["alpha"], self.base / "after")
        self.assertTrue(second["cache_hit"])
        self.assertEqual(first["source_key"], second["source_key"])
        self.assertEqual(len(self.calls()), 1)
        manifest = candidate.read_manifest(self.base / "after/candidates/alpha")
        self.assertEqual(first["candidates"]["alpha"]["candidate_id"], manifest["candidate_id"])

    def test_dirty_builds_do_not_reuse_cache(self) -> None:
        clean = self.prepare("clean")
        self.write("source.txt", "release edits before commit\n")
        first = self.prepare("dirty-first")
        second = self.prepare("dirty-second")
        self.assertFalse(first["cache_hit"])
        self.assertFalse(second["cache_hit"])
        self.assertTrue(first["source_key"].startswith("dirty:"))
        self.assertNotEqual(clean["source_key"], first["source_key"])
        self.assertNotEqual(first["source_key"], second["source_key"])
        self.commit()
        committed = self.prepare("committed")
        self.assertFalse(committed["cache_hit"])
        self.assertEqual(committed["source_key"], self.git("rev-parse", "HEAD"))
        self.assertEqual(len(self.calls()), 4)

    def test_new_commit_does_not_reuse_previous_build(self) -> None:
        first = self.prepare("first")
        self.write("source.txt", "committed source change\n")
        self.commit()
        second = self.prepare("second")
        self.assertFalse(second["cache_hit"])
        self.assertNotEqual(first["source_key"], second["source_key"])
        self.assertEqual(len(self.calls()), 2)

    def test_unit_build_excludes_other_product_binaries(self) -> None:
        self.prepare("one-unit", unit="alpha-helper")
        command = self.calls()[0]
        self.assertEqual(command.count("--bin"), 1)
        self.assertEqual(command[command.index("--bin") + 1], "alpha-helper")
        self.assertEqual(set(candidate.read_manifest(self.base / "one-unit/candidates/alpha")["binaries"]), {"alpha-helper"})




    def test_configuration_change_causes_new_build(self) -> None:
        self.prepare("first")
        with mock.patch.dict(os.environ, {"RUSTFLAGS": "-C opt-level=2"}):
            result = self.prepare("second")
        self.assertFalse(result["cache_hit"])
        self.assertEqual(len(self.calls()), 2)
        manifest = json.loads((Path(result["cache_entry"]) / "manifest.json").read_text())
        environment = manifest["identity"]["configuration"]["environment"]
        self.assertEqual(environment["RUSTFLAGS"], build.hashlib.sha256(b"-C opt-level=2").hexdigest())
        self.assertNotIn("-C opt-level=2", json.dumps(manifest))

    def test_unit_selection_and_untracked_source_use_separate_builds(self) -> None:
        self.prepare("all")
        self.prepare("one", unit="alpha")
        self.write("untracked.txt", "untracked source also matters\n")
        self.prepare("new-source", unit="alpha")
        self.assertEqual(len(self.calls()), 3)

    def enable_signing(self) -> None:
        self.signing["load_policy"].return_value = self.policy

        def sign(path, policy, product, key):
            with path.open("a") as stream:
                stream.write("# signed " + policy["macos"]["certificate_sha1"] + "\n")

        self.signing["sign"].side_effect = sign

    def test_signing_covers_every_declared_binary_without_changing_cargo_outputs(self) -> None:
        self.enable_signing()
        result = self.prepare("signed", ["alpha", "beta"])
        self.assertEqual(self.signing["sign"].call_count, 3)
        self.signing["preflight"].assert_called_once_with(self.policy)
        self.assertEqual(result["signing_policy"], self.policy)
        for call in self.signing["sign"].call_args_list:
            path, policy, product, key = call.args
            self.assertNotIn(self.cache / "target", path.parents)
            self.assertEqual(policy, self.policy)
            self.assertEqual(path.name, key)
            unsigned = self.cache / "target/fixture-host/release" / key
            self.assertNotIn("# signed", unsigned.read_text())
            manifest = candidate.read_manifest(self.base / "signed/candidates" / product)
            record = manifest["binaries"][key]
            self.assertEqual(record["code_identifier"], f"local.cell.{product}.{key}")
            self.assertEqual(record["sha256"], candidate.digest(self.base / "signed/candidates" / product / record["path"]))
            self.assertEqual(manifest["signing_policy"], self.policy)
            self.assertEqual(manifest["signing_policy_digest"], result["signing_policy_digest"])

    def test_signed_cache_reuse_verifies_binaries_without_resigning(self) -> None:
        self.enable_signing()
        self.prepare("first-signed")
        before = self.signing["verify"].call_count
        second = self.prepare("second-signed")
        self.assertTrue(second["cache_hit"])
        self.assertEqual(len(self.calls()), 1)
        self.assertEqual(self.signing["sign"].call_count, 2)
        calls = self.signing["verify"].call_args_list[before:]
        cached = Path(second["cache_entry"]) / "binaries/release"
        self.assertEqual({call.args[0].name for call in calls if cached in call.args[0].parents},
                         {"alpha", "alpha-helper"})
        self.assertTrue(any(".candidate-" in str(call.args[0]) for call in calls))

    def test_changed_signing_identity_has_a_separate_cache_and_candidate(self) -> None:
        self.enable_signing()
        first = self.prepare("first-identity")
        self.policy = {"schema": 1, "macos": {**self.policy["macos"], "certificate_sha1": "b" * 40}}
        self.signing["load_policy"].return_value = self.policy
        second = self.prepare("second-identity")
        self.assertFalse(second["cache_hit"])
        self.assertNotEqual(first["build_key"], second["build_key"])
        self.assertNotEqual(first["candidates"]["alpha"]["candidate_id"], second["candidates"]["alpha"]["candidate_id"])
        self.assertEqual(len(self.calls()), 2)

    def test_preflight_and_frozen_policy_failure_stop_before_build(self) -> None:
        self.enable_signing()
        for operation in ("assert_current", "preflight"):
            with self.subTest(operation=operation):
                self.signing[operation].side_effect = RuntimeError("signing is unavailable")
                with self.assertRaisesRegex(RuntimeError, "signing is unavailable"):
                    build.prepare(self.source, ["alpha"], self.base / operation, signing_policy=self.policy)
                self.signing[operation].side_effect = None
        self.assertEqual(self.calls(), [])

    def test_signed_cache_tampering_fails_before_assembly(self) -> None:
        self.enable_signing()
        first = self.prepare("sealed-cache")
        path = Path(first["cache_entry"]) / "binaries/release/alpha"
        path.chmod(0o755)
        path.write_text(path.read_text() + "# changed\n")
        with self.assertRaisesRegex(build.BuildError, "release cache executable changed"):
            self.prepare("changed-cache")
        self.assertFalse((self.base / "changed-cache").exists())
        self.assertEqual(len(self.calls()), 1)

    def test_nonobject_cache_metadata_fails_before_assembly(self) -> None:
        first = self.prepare("cache-object")
        path = Path(first["cache_entry"]) / "manifest.json"
        path.chmod(0o600)
        path.write_text("[]\n")
        with self.assertRaisesRegex(build.BuildError, "cache metadata"):
            self.prepare("bad-cache-object")
        self.assertFalse((self.base / "bad-cache-object").exists())

    def test_direct_staging_signs_only_the_copy_and_verifies_it(self) -> None:
        self.prepare("unsigned-fixture")
        self.enable_signing()
        target = self.cache / "target/fixture-host"
        before = candidate.digest(target / "release/alpha")
        output = self.base / "direct-candidate"
        with mock.patch.dict(os.environ, {"CARGO_TARGET_DIR": str(target)}):
            manifest = candidate.stage(self.source, "alpha", output, "alpha|target/release/alpha|alpha",
                                       signing_policy=self.policy)
        self.assertEqual(candidate.digest(target / "release/alpha"), before)
        self.assertEqual(self.signing["sign"].call_count, 1)
        self.assertEqual(self.signing["sign"].call_args.args[0].name, "alpha")
        self.assertEqual(candidate.verify(output, signing_policy=self.policy), manifest)

    def test_candidate_verification_requires_the_external_policy_and_unchanged_bytes(self) -> None:
        self.enable_signing()
        self.prepare("verified")
        root = self.base / "verified/candidates/alpha"
        candidate.verify_signatures(root, self.policy)
        other = {"schema": 1, "macos": {**self.policy["macos"], "certificate_sha1": "b" * 40}}
        with self.assertRaisesRegex(candidate.CandidateError, "signing policy does not match"):
            candidate.verify(root, signing_policy=other)
        path = root / "bin/alpha"
        path.chmod(0o755)
        path.write_text(path.read_text() + "# changed\n")
        with self.assertRaisesRegex(candidate.CandidateError, "executable changed"):
            candidate.verify(root, signing_policy=self.policy)

    def test_nonobject_candidate_metadata_is_a_reportable_failure(self) -> None:
        self.enable_signing()
        self.prepare("candidate-object")
        root = self.base / "candidate-object/candidates/alpha"
        path = root / "candidate.json"
        path.chmod(0o600)
        path.write_text("null\n")
        with self.assertRaisesRegex(candidate.CandidateError, "metadata must be an object"):
            candidate.verify(root, signing_policy=self.policy)

    def test_signed_build_staging_refuses_an_unverifiable_cache_binary(self) -> None:
        first = self.prepare("unsigned-cache")
        self.enable_signing()
        self.signing["verify"].side_effect = RuntimeError("wrong certificate")
        with self.assertRaisesRegex(RuntimeError, "wrong certificate"):
            candidate.stage_build(self.source, "alpha", self.base / "bad-signed-copy",
                                  "alpha|target/release/alpha|alpha",
                                  target=Path(first["cache_entry"]) / "binaries", source_key=first["source_key"],
                                  signing_policy=self.policy)
        self.signing["sign"].assert_not_called()
        self.assertFalse((self.base / "bad-signed-copy").exists())

    def run_main_with_policy(self, value):
        path = self.base / "frozen-signing.json"
        path.write_text(json.dumps(value))
        arguments = ["cell-build", "--source-root", str(self.source), "--product", "alpha",
                     "--output", str(self.base / "cli-output"), "--signing-policy-file", str(path)]
        with mock.patch.object(sys, "argv", arguments), mock.patch("builtins.print"), \
                mock.patch.object(build, "prepare", return_value={"state": "built"}) as prepare:
            status = build.main()
        return status, prepare

    def test_cli_preserves_and_validates_frozen_signing_policy(self) -> None:
        value = {"schema": 1, "macos": {**self.policy["macos"], "certificate_sha1": "A" * 40}}
        status, prepare = self.run_main_with_policy(value)
        self.assertEqual(status, 0)
        self.signing["assert_current"].assert_called_once_with(self.policy)
        prepare.assert_called_once_with(self.source, ["alpha"], self.base / "cli-output", None,
                                        signing_policy=self.policy)

    def test_cli_policy_drift_stops_before_preparation(self) -> None:
        self.signing["assert_current"].side_effect = RuntimeError("policy changed after admission")
        status, prepare = self.run_main_with_policy(self.policy)
        self.assertEqual(status, 1)
        prepare.assert_not_called()

    def test_cli_rejects_missing_mac_policy(self) -> None:
        with mock.patch.object(sys, "platform", "darwin"):
            status, prepare = self.run_main_with_policy(None)
        self.assertEqual(status, 1)
        prepare.assert_not_called()

    def test_cli_allows_explicit_unsigned_nonmac_fixture_policy(self) -> None:
        with mock.patch.object(sys, "platform", "linux"):
            status, prepare = self.run_main_with_policy(None)
        self.assertEqual(status, 0)
        self.signing["assert_current"].assert_called_once_with(None)
        self.assertIsNone(prepare.call_args.kwargs["signing_policy"])


if __name__ == "__main__":
    unittest.main()
