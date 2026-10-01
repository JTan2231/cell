"""Check Rust test selection with in-memory metadata."""

import unittest

from parallel_tests import TestPlan, filterset, make_plan


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



if __name__ == "__main__":
    unittest.main()
