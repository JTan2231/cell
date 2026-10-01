"""Check release build scope and exact-source deferral with in-memory inputs."""

from contextlib import redirect_stderr, redirect_stdout
from io import StringIO
import json
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest import mock

import release_build
import select_changes as selection


class ReleaseBuildTests(unittest.TestCase):
    def setUp(self):
        self.root = Path("/evidence/worktree")
        self.configs = {
            "alpha": {"packages": ["alpha", "shared"], "offline": False},
            "beta": {"packages": ["beta", "shared"], "offline": True},
        }

    def execute(self, configs=None, build_status=0, rustc_version="rustc 1.97.1 (fixture)"):
        results = [SimpleNamespace(stdout=rustc_version),
                   SimpleNamespace(stdout="cargo 1.97.1 (fixture)"),
                   SimpleNamespace(returncode=build_status)]
        with mock.patch.object(release_build.subprocess, "run", side_effect=results) as runner, \
                mock.patch.dict(release_build.os.environ, {"RUSTFLAGS": "fixture"}, clear=True), \
                redirect_stdout(StringIO()):
            code = release_build.run_build(self.root, configs or self.configs, "/cargo/bin:/bin")
        return code, runner

    def test_combines_packages_once_with_offline_and_warning_rules(self):
        code, runner = self.execute(build_status=7)
        self.assertEqual(code, 7)
        command = runner.call_args.args[0]
        self.assertEqual(command, ["cargo", "build", "--manifest-path", str(self.root / "Cargo.toml"),
                                   "--package", "alpha", "--package", "beta", "--package", "shared",
                                   "--release", "--locked", "--offline"])
        environment = runner.call_args.kwargs["env"]
        self.assertEqual(environment["CARGO_BUILD_WARNINGS"], "deny")
        self.assertEqual(environment["CARGO_NET_OFFLINE"], "true")
        self.assertEqual(environment["PATH"], "/cargo/bin:/bin")
        self.assertEqual(environment["RUSTFLAGS"], "fixture")
        self.assertEqual(runner.call_args.kwargs["cwd"], self.root)

    def test_online_scope_keeps_locked_release_build(self):
        _, runner = self.execute({"alpha": self.configs["alpha"]})
        self.assertNotIn("--offline", runner.call_args.args[0])
        self.assertNotIn("CARGO_NET_OFFLINE", runner.call_args.kwargs["env"])

    def test_wrong_toolchain_stops_before_compilation(self):
        with self.assertRaisesRegex(ValueError, "rustc 1.97.1 is required"):
            self.execute(rustc_version="rustc 1.96.0 (fixture)")

    def test_descriptor_loader_retains_bootstrapped_path(self):
        values = "\0".join(("alpha", "alpha shared", "0", "beta", "beta shared", "1",
                              "/cargo/bin:/bin", ""))
        with mock.patch.object(release_build.subprocess, "run",
                               return_value=SimpleNamespace(stdout=values)):
            configs, cargo_path = release_build.load_configuration(self.root, ["beta", "alpha"])
        self.assertEqual(configs, self.configs)
        self.assertEqual(cargo_path, "/cargo/bin:/bin")


class ReleaseSelectionTests(unittest.TestCase):
    def setUp(self):
        self.root = Path("/evidence/worktree")
        self.plan = selection.Plan(
            mode="changed", verbose=False, quiet=False, source="b" * 40, status="status:",
            head="b" * 40, base="a" * 40, committed=True,
            products={"alpha": ("alpha", ["alpha"]), "beta": ("beta", ["beta"])},
            selected=["alpha", "beta"], platform={"alpha": [], "beta": []},
            shared={suite: [] for suite in selection.SHARED_INPUTS}, tests_skipped=True,
        )

    def gates(self):
        with mock.patch.object(selection, "output", return_value=b"product\nheavy\n"):
            return selection.gate_plan(self.root, self.plan)

    def test_legacy_validator_batches_releases_even_when_tests_skipped(self):
        gates = self.gates()
        builds = [gate for gate in gates if gate[0] == "cell.build.release"]
        self.assertEqual(builds, [("cell.build.release", "heavy",
                                 [selection.sys.executable, str(self.root / "pipeline/release_build.py"),
                                  "--product", "alpha", "--product", "beta"])])
        self.assertEqual([body[-1] for gate, _, body in gates if gate.endswith(".post")],
                         ["post", "post"])

    def test_deferral_skips_only_release_gate(self):
        ordinary = self.gates()
        self.plan.release_builds_deferred = True
        self.assertEqual(self.gates(), [gate for gate in ordinary if gate[0] != "cell.build.release"])

    def test_empty_selection_has_no_release_gate(self):
        self.plan.selected = []
        self.assertNotIn("cell.build.release", [gate for gate, _, _ in self.gates()])

    def test_defer_flag_requires_exact_committed_json_validation(self):
        valid = ["--base", "a" * 40, "--candidate", "b" * 40, "--json", "--defer-release-builds"]
        self.assertTrue(selection.parse_arguments(valid).defer_release_builds)
        with self.assertRaises(selection.SelectionError):
            selection.parse_arguments(["--json", "--defer-release-builds"])

    def test_receipt_records_deferral(self):
        for deferred in (False, True):
            with self.subTest(deferred=deferred):
                self.plan.release_builds_deferred = deferred
                stream = StringIO()
                with mock.patch.object(selection, "make_plan", return_value=self.plan), \
                        mock.patch.object(selection, "gate_plan", return_value=[]), \
                        mock.patch.object(selection, "output", return_value=b"b" * 40), \
                        mock.patch.object(selection, "check_plan"), redirect_stdout(stream):
                    code = selection.run_json(self.root, ["--base", "a" * 40,
                                                          "--candidate", "b" * 40, "--json"])
                self.assertEqual(code, 0)
                self.assertIs(json.loads(stream.getvalue())["selection"]["release_builds_deferred"], deferred)

    def test_common_release_and_selection_changes_expand_product_coverage(self):
        for changed in ("pipeline/release_build.py", "pipeline/select_changes.py"):
            with self.subTest(changed=changed), \
                    mock.patch.object(selection, "output", return_value=b"b" * 40), \
                    mock.patch.object(selection, "git_status", return_value=b"fixture"), \
                    mock.patch.object(selection, "changed_paths", return_value={changed}), \
                    mock.patch.object(selection, "inventory", return_value=self.plan.products), \
                    mock.patch.object(selection, "at_revision", return_value=b"present"), \
                    mock.patch.object(selection, "platform_change", return_value=True), \
                    mock.patch.object(selection, "check"), redirect_stderr(StringIO()):
                plan = selection.make_plan(self.root, [])
            self.assertEqual(plan.selected, ["alpha", "beta"])
            self.assertTrue(all(plan.platform.values()))
            self.assertTrue(all(plan.shared[suite] for suite in ("pipeline", "install", "maintenance", "prompts")))


if __name__ == "__main__":
    unittest.main()
