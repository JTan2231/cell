"""Check exact Rust coverage and failure draining without invoking Cargo."""

import contextlib
import io
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from parallel_tests import (
    TestPlan, absolute_path, filterset, load_products, make_plan, positive_integer,
    report_failures, run_plan,
)


SOURCE = Path(__file__).resolve().parent


def target(name, kind="test", test=True, doctest=False):
    return {"name": name, "kind": [kind], "test": test, "doctest": doctest}


def package(name, targets):
    return {"name": name, "targets": targets}


class SelectionTests(unittest.TestCase):
    def setUp(self):
        self.workspace = {"packages": [
            package("alpha", [target("alpha_lib", "rlib", doctest=True),
                              target("alpha", "bin"), target("alpha-install", "bin"),
                              target("ordinary"), target("install"), target("maintenance"),
                              target("disabled", test=False),
                              target("sample", "example"), target("timing", "bench")]),
            package("beta", [target("beta", "proc-macro", doctest=True),
                             target("ordinary"), target("install")]),
            package("cell-install", [target("cell_install", "lib", doctest=True)]),
            package("cell-maintenance", [target("cell_maintenance", "lib", doctest=True)]),
            package("cell-prompts", [target("cell_prompts", "lib", doctest=True),
                                    target("cell-prompts", "bin")]),
        ]}
        self.configs = {
            "alpha": {"packages": ["alpha", "cell-install", "cell-prompts"], "offline": True},
            "beta": {"packages": ["beta"], "offline": False},
        }

    def test_product_coverage_excludes_lifecycle_disabled_and_incidental_shared(self):
        plan = make_plan(self.workspace, self.configs, ["alpha"], [], [])
        self.assertEqual(set(plan.targets), {
            ("alpha", "lib", "alpha_lib"), ("alpha", "bin", "alpha"),
            ("alpha", "test", "ordinary"), ("alpha", "example", "sample"),
            ("alpha", "bench", "timing"),
        })
        self.assertEqual(plan.doctests, {"alpha": True})
        self.assertTrue(plan.offline)

    def test_platform_only_coverage_includes_only_lifecycle_targets(self):
        plan = make_plan(self.workspace, {"alpha": self.configs["alpha"]}, [], ["alpha"], [])
        self.assertEqual(set(plan.targets), {
            ("alpha", "bin", "alpha-install"), ("alpha", "test", "install"),
            ("alpha", "test", "maintenance"),
        })
        self.assertEqual(plan.doctests, {})

    def test_shared_suite_is_explicit_and_deduplicated(self):
        plan = make_plan(self.workspace, self.configs, ["alpha", "alpha"], ["alpha"],
                         ["install", "install", "prompts"])
        self.assertIn(("cell-install", "lib", "cell_install"), plan.targets)
        self.assertIn(("cell-prompts", "bin", "cell-prompts"), plan.targets)
        self.assertNotIn("cell-maintenance", {key[0] for key in plan.targets})
        self.assertEqual(plan.targets[("cell-install", "lib", "cell_install")], {"shared:install"})
        self.assertEqual(plan.doctests, {"alpha": True, "cell-install": False,
                                        "cell-prompts": False})

    def test_filter_disambiguates_same_target_name_in_different_packages(self):
        plan = make_plan(self.workspace, self.configs, ["alpha", "beta"], ["alpha"], [])
        expression = filterset(plan)
        self.assertIn("(package(=alpha) & kind(=test) & binary(=install))", expression)
        self.assertNotIn("(package(=beta) & kind(=test) & binary(=install))", expression)
        self.assertIn("(package(=beta) & kind(=proc-macro) & binary(=beta))", expression)
        self.assertNotIn("deps(", expression)

    def test_filter_escapes_dsl_delimiters(self):
        expression = filterset(TestPlan(targets={("some-package", "test", "a),b\\c"): {"p"}}))
        self.assertIn(r"binary(=a\)\,b\\c)", expression)
        self.assertEqual(filterset(TestPlan()), "none()")

    def test_unknown_package_fails_instead_of_losing_coverage(self):
        config = {"missing": {"packages": ["missing"], "offline": False}}
        with self.assertRaisesRegex(ValueError, "absent from metadata: missing"):
            make_plan(self.workspace, config, ["missing"], [], [])

    def test_test_false_library_can_still_require_doctests(self):
        workspace = {"packages": [package("docs", [target("docs", "lib", False, True)])]}
        config = {"docs": {"packages": ["docs"], "offline": False}}
        plan = make_plan(workspace, config, ["docs"], [], [])
        self.assertEqual(plan.targets, {})
        self.assertEqual(plan.doctests, {"docs": False})


class DescriptorTests(unittest.TestCase):
    def test_uses_sourceable_descriptor_loader_and_keeps_offline_policy(self):
        with tempfile.TemporaryDirectory(prefix="cell-parallel-descriptors-") as temporary:
            root = Path(temporary)
            descriptors = root / "pipeline/products"
            descriptors.mkdir(parents=True)
            (root / "pipeline/lib.sh").write_text((SOURCE / "lib.sh").read_text())
            (descriptors / "fixture.sh").write_text(
                "PIPELINE_SCHEMA=1\nPRODUCT_ID=fixture\n"
                "CARGO_PACKAGES='fixture\nfixture-api'\nCARGO_OFFLINE=1\n")
            self.assertEqual(load_products(root, ["fixture", "fixture"]), {
                "fixture": {"packages": ["fixture", "fixture-api"], "offline": True},
            })
            with self.assertRaises(subprocess.CalledProcessError):
                load_products(root, ["fixture; touch unexpected"])
            self.assertFalse((root / "unexpected").exists())


class ExecutionTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="cell-parallel-execution-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.nextest = self.root / "cargo-nextest"

    def run_with_mock(self, plan, callback, threads=4):
        output = io.StringIO()
        with patch("parallel_tests.subprocess.run", side_effect=callback) as runner:
            with contextlib.redirect_stdout(output):
                status = run_plan(self.root, plan, self.nextest, threads)
        return status, runner, output.getvalue()

    def test_one_pool_has_exact_selection_and_fixed_concurrency_policy(self):
        plan = TestPlan(targets={
            ("alpha", "test", "integration"): {"product:alpha"},
            ("beta", "lib", "beta"): {"product:beta"},
        }, offline=True)

        def run(command, **kwargs):
            self.assertEqual(command[:3], [str(self.nextest), "nextest", "run"])
            self.assertEqual(command[command.index("--test-threads") + 1], "7")
            self.assertEqual(command[command.index("--build-jobs") + 1], "2")
            self.assertEqual(command[command.index("--retries") + 1], "0")
            self.assertIn("--no-fail-fast", command)
            self.assertIn("--ignore-default-filter", command)
            self.assertIn("--all-targets", command)
            self.assertIn("--offline", command)
            self.assertEqual(command[command.index("--filterset") + 1], filterset(plan))
            selected = [command[i + 1] for i, argument in enumerate(command) if argument == "--package"]
            self.assertEqual(selected, ["alpha", "beta"])
            self.assertFalse(any(key.startswith("NEXTEST_") for key in kwargs["env"]))
            config = Path(command[command.index("--config-file") + 1]).read_text()
            self.assertIn("[profile.default.junit]", config)
            self.assertNotIn("overrides", config)
            self.assertEqual(Path(command[command.index("--user-config-file") + 1]).read_text(), "")
            return subprocess.CompletedProcess(command, 0)

        with patch.dict(os.environ, {"CARGO_BUILD_JOBS": "2", "NEXTEST_PROFILE": "wrong",
                                     "NEXTEST_RETRIES": "9", "NEXTEST_TEST_THREADS": "1"}):
            status, runner, output = self.run_with_mock(plan, run, threads=7)
        self.assertEqual(status, 0)
        self.assertEqual(runner.call_count, 1)
        self.assertIn("product:alpha alpha/test/integration", output)
        self.assertIn("7 test workers", output)

    def test_drains_doctests_after_failed_pool_and_retains_all_failures(self):
        plan = TestPlan(targets={("alpha", "lib", "alpha"): {"product:alpha"}},
                        doctests={"alpha": True, "beta": False})
        commands = []

        def run(command, **kwargs):
            commands.append(command)
            if command[0] == str(self.nextest):
                config = Path(command[command.index("--config-file") + 1])
                report = config.parent / "nextest/default/report.xml"
                report.parent.mkdir(parents=True)
                report.write_text('<testsuites><testsuite><testcase classname="alpha" '
                                  'name="tests::failure"><failure/></testcase>'
                                  '<testcase classname="alpha::other" name="crash"><error/>'
                                  '</testcase><testcase name="pass"/></testsuite></testsuites>')
                return subprocess.CompletedProcess(command, 100)
            return subprocess.CompletedProcess(command, 42 if "alpha" in command else 0)

        status, runner, output = self.run_with_mock(plan, run)
        self.assertEqual(status, 100)
        self.assertEqual(runner.call_count, 3)
        self.assertIn("--offline", commands[1])
        self.assertNotIn("--offline", commands[2])
        for command in commands[1:]:
            self.assertIn("--doc", command)
            self.assertIn("--no-fail-fast", command)
        self.assertIn("test alpha::tests::failure ... FAILED", output)
        self.assertIn("test alpha::other::crash ... FAILED", output)
        self.assertNotIn("test ::pass", output)

    def test_doctest_failure_fails_the_stage_after_successful_pool(self):
        plan = TestPlan(targets={("alpha", "lib", "alpha"): {"p"}}, doctests={"alpha": False})
        status, runner, _ = self.run_with_mock(
            plan, lambda command, **kwargs: subprocess.CompletedProcess(
                command, 3 if "--doc" in command else 0))
        self.assertEqual(status, 3)
        self.assertEqual(runner.call_count, 2)

    def test_bad_failure_report_does_not_skip_required_doctests(self):
        plan = TestPlan(targets={("alpha", "lib", "alpha"): {"p"}}, doctests={"alpha": False})

        def run(command, **kwargs):
            if command[0] == str(self.nextest):
                config = Path(command[command.index("--config-file") + 1])
                report = config.parent / "nextest/default/report.xml"
                report.parent.mkdir(parents=True)
                report.write_text("truncated report")
            return subprocess.CompletedProcess(command, 0)

        with contextlib.redirect_stderr(io.StringIO()):
            status, runner, _ = self.run_with_mock(plan, run)
        self.assertEqual(status, 1)
        self.assertEqual(runner.call_count, 2)
        self.assertIn("--doc", runner.call_args.args[0])

    def test_prompt_seed_is_job_local_and_survives_pool_and_doctests(self):
        plan = TestPlan(targets={("cell-prompts", "lib", "cell_prompts"): {"shared:prompts"}},
                        doctests={"cell-prompts": False})
        databases = []

        def run(command, **kwargs):
            if command[:2] == ["cargo", "run"]:
                database = Path(command[command.index("--") + 1])
                database.parent.mkdir(parents=True)
                database.write_text("private seed")
                databases.append(database)
                self.assertIn(str(self.root / "prompting/seed.json"), command)
            else:
                self.assertEqual(Path(kwargs["env"]["CELL_BAZAAR_DATABASE"]), databases[0])
                self.assertTrue(databases[0].is_file())
            return subprocess.CompletedProcess(command, 0)

        status, runner, _ = self.run_with_mock(plan, run)
        self.assertEqual(status, 0)
        self.assertEqual(runner.call_count, 3)
        self.assertFalse(databases[0].exists())

    def test_empty_plan_does_not_run_an_unqualified_cargo_command(self):
        status, runner, _ = self.run_with_mock(TestPlan(), lambda *args, **kwargs: None)
        self.assertEqual(status, 0)
        runner.assert_not_called()

    def test_doctest_only_plan_does_not_invoke_nextest(self):
        status, runner, _ = self.run_with_mock(TestPlan(doctests={"docs": False}),
                                             lambda command, **kwargs: subprocess.CompletedProcess(command, 0))
        self.assertEqual(status, 0)
        command = runner.call_args.args[0]
        self.assertEqual(command[:2], ["cargo", "test"])
        self.assertIn("--doc", command)
        self.assertIn("--package", command)

    def test_missing_junit_after_build_failure_has_no_fake_test_names(self):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            report_failures(self.root / "missing.xml")
        self.assertEqual(output.getvalue(), "")

    def test_cli_rejects_zero_threads_and_relative_tool_path(self):
        with self.assertRaises(ValueError):
            positive_integer("not-an-integer")
        with self.assertRaisesRegex(Exception, "positive"):
            positive_integer("0")
        with self.assertRaisesRegex(Exception, "absolute"):
            absolute_path("cargo-nextest")
        self.assertEqual(positive_integer("8"), 8)
        self.assertEqual(absolute_path("/tools/cargo-nextest"), Path("/tools/cargo-nextest"))


if __name__ == "__main__":
    unittest.main()
