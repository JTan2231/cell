"""Check Rust test selection with in-memory metadata."""

import unittest
from argparse import Namespace
from pathlib import Path
from unittest.mock import patch

from cargo_tests import run_tests
from parallel_tests import TestPlan, cargo_target_args, filterset, make_plan, run_plan


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
            package("bazaar", [target("bazaar", "lib", doctest=True),
                               target("bazaar", "bin"), target("bazaar-install", "bin"),
                               target("prompt_resolution")]),
        ]}
        self.configs = {
            "alpha": {"packages": ["alpha", "cell-install"], "offline": True},
            "beta": {"packages": ["beta"], "offline": False},
            "bazaar": {"packages": ["bazaar"], "offline": True},
        }

    def test_product_coverage_excludes_lifecycle_disabled_and_incidental_shared(self):
        plan = make_plan(self.workspace, self.configs, ["alpha"], [], [])
        self.assertEqual(set(plan.targets), {
            ("alpha", "lib", "alpha_lib"), ("alpha", "bin", "alpha"),
            ("alpha", "test", "ordinary"), ("alpha", "example", "sample"),
            ("alpha", "bench", "timing"),
        })
        self.assertTrue(plan.offline)

    def test_platform_only_coverage_includes_only_lifecycle_targets(self):
        plan = make_plan(self.workspace, {"alpha": self.configs["alpha"]}, [], ["alpha"], [])
        self.assertEqual(set(plan.targets), {
            ("alpha", "bin", "alpha-install"), ("alpha", "test", "install"),
            ("alpha", "test", "maintenance"),
        })

    def test_product_cargo_selectors_exclude_lifecycle_and_disabled_targets(self):
        plan = make_plan(self.workspace, self.configs, ["alpha"], [], [])
        self.assertEqual(cargo_target_args(plan), [
            "--lib", "--bin", "alpha", "--test", "ordinary",
            "--example", "sample", "--bench", "timing",
        ])

    def test_library_and_proc_macro_share_one_library_selector(self):
        plan = make_plan(self.workspace, self.configs, ["alpha", "beta"], [], [])
        arguments = cargo_target_args(plan)
        self.assertEqual(arguments.count("--lib"), 1)
        self.assertNotIn("alpha_lib", arguments)
        self.assertNotIn("beta", arguments)
        self.assertIn("(package(=beta) & kind(=proc-macro) & binary(=beta))", filterset(plan))

    def test_platform_cargo_selectors_do_not_add_library_tests(self):
        plan = make_plan(self.workspace, self.configs, [], ["alpha"], [])
        self.assertEqual(cargo_target_args(plan), [
            "--bin", "alpha-install", "--test", "install", "--test", "maintenance",
        ])

    def test_named_selectors_deduplicate_across_packages_but_filter_keeps_identity(self):
        plan = make_plan(self.workspace, self.configs, ["alpha", "beta"], ["alpha"], [])
        arguments = cargo_target_args(plan)
        self.assertEqual(arguments.count("ordinary"), 1)
        self.assertEqual(arguments.count("install"), 1)
        expression = filterset(plan)
        self.assertIn("(package(=alpha) & kind(=test) & binary(=install))", expression)
        self.assertNotIn("(package(=beta) & kind(=test) & binary(=install))", expression)

    def test_target_kinds_with_same_name_keep_separate_selectors_and_filters(self):
        plan = TestPlan(targets={
            ("alpha", "bin", "same"): {"product:alpha"},
            ("beta", "test", "same"): {"product:beta"},
            ("alpha", "example", "same"): {"product:alpha"},
            ("beta", "bench", "same"): {"product:beta"},
        })
        self.assertEqual(cargo_target_args(plan), [
            "--bin", "same", "--test", "same", "--example", "same", "--bench", "same",
        ])
        for package_name, kind, name in plan.targets:
            self.assertIn(
                f"(package(={package_name}) & kind(={kind}) & binary(={name}))", filterset(plan))

    def test_shared_suite_is_explicit_and_deduplicated(self):
        plan = make_plan(self.workspace, self.configs, ["alpha", "alpha"], ["alpha"],
                         ["install", "install"])
        self.assertIn(("cell-install", "lib", "cell_install"), plan.targets)
        self.assertNotIn("cell-maintenance", {key[0] for key in plan.targets})
        self.assertEqual(plan.targets[("cell-install", "lib", "cell_install")], {"shared:install"})

    def test_bazaar_product_includes_prompt_library_command_and_integration_tests(self):
        plan = make_plan(self.workspace, self.configs, ["bazaar"], [], [])
        self.assertEqual(set(plan.targets), {
            ("bazaar", "lib", "bazaar"), ("bazaar", "bin", "bazaar"),
            ("bazaar", "test", "prompt_resolution"),
        })
        self.assertTrue(all(owners == {"product:bazaar"} for owners in plan.targets.values()))

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

    def test_doctest_only_library_runs_no_tests_in_either_runner(self):
        workspace = {"packages": [package("docs", [target("docs", "lib", False, True)])]}
        config = {"docs": {"packages": ["docs"], "offline": False}}
        plan = make_plan(workspace, config, ["docs"], [], [])
        self.assertEqual(plan.targets, {})
        self.assertEqual(cargo_target_args(plan), [])
        args = Namespace(packages=["docs"], group="product", no_fail_fast=False)
        with patch("subprocess.run") as run:
            self.assertEqual(run_plan(Path("/cell"), plan, Path("/nextest"), 4), 0)
            self.assertEqual(run_tests(args, [], {"docs": workspace["packages"][0]}, {}), 0)
        run.assert_not_called()

    def test_serial_runner_runs_library_tests_without_doctests(self):
        args = Namespace(packages=["alpha"], group="product", no_fail_fast=False)
        packages = {"alpha": package("alpha", [target("alpha", "lib", doctest=True)])}
        with patch("subprocess.run") as run:
            run.return_value.returncode = 0
            self.assertEqual(run_tests(args, [], packages, {}), 0)
        run.assert_called_once_with(
            ["cargo", "test", "--package", "alpha", "--lib"], check=False, env={})


if __name__ == "__main__":
    unittest.main()
