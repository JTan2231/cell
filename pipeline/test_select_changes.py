"""Exercise internal CI selection in local Git fixtures without running real gates."""

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


SOURCE = Path(__file__).resolve().parent.parent


class ChangedProjectTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="cell-ci-selection-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name) / "workspace with spaces"
        self.root.mkdir()
        self.log = Path(self.temporary.name) / "gates.jsonl"
        self.environment = os.environ.copy()
        for name in ("CELL_CI_EXPECTED_SOURCE_KEY", "GIT_DIR", "GIT_WORK_TREE",
                     "GIT_INDEX_FILE", "GIT_COMMON_DIR", "CELL_CI_TEST_THREADS"):
            self.environment.pop(name, None)
        self.environment.update({
            "PYTHONDONTWRITEBYTECODE": "1",
            "FIXTURE_GATE_LOG": str(self.log),
            "FIXTURE_ROOT": str(self.root),
        })
        self.git("init", "-q", "--initial-branch=main")
        self.git("config", "user.name", "CI Selection Fixture")
        self.git("config", "user.email", "ci-selection@example.invalid")
        self.git("config", "commit.gpgsign", "false")
        hooks = Path(self.temporary.name) / "empty-hooks"
        hooks.mkdir()
        self.git("config", "core.hooksPath", str(hooks))

        for relative in ("ci.sh", "pipeline/select_changes.py", "pipeline/lib.sh",
                         "pipeline/platform_inputs.py"):
            self.write(relative, (SOURCE / relative).read_text(), executable=True)
        self.write(".gitignore", "ignored/\n")
        self.write("Cargo.lock", "shared lockfile\n")
        for product, directory, aliases in (("alpha", "alpha", ""),
                                             ("beta", "beta", ""),
                                             ("krisis", "decisions", "decisions")):
            self.write(f"pipeline/products/{product}.sh", f"""PIPELINE_SCHEMA=1
PRODUCT_ID={product}
PRODUCT_NAME={product}
PRODUCT_DIR={directory}
PRODUCT_ALIASES='{aliases}'
CI_RESOURCE_CLASS=light
RELEASE_BRANCH=main
DEPLOY_PROFILE=custom
DEPLOY_CONFLICT_KEYS={product}
CARGO_MANIFEST={directory}/Cargo.toml
CARGO_PACKAGES={product}
RELEASE_UNITS='{product}|{product}|package|{directory}/Cargo.toml|{product}-|1'
""")
            self.write(f"{directory}/Cargo.toml",
                       f'[package]\nname = "{product}"\nversion = "1.0.0"\n')
            self.write(f"{directory}/tracked.txt", f"original {product} source\n")
            self.write(f"{directory}/ci.sh", self.wrapper(product), executable=True)
        for filename, label in (("check.sh", "preflight"),
                                ("recognition.sh", "recognition"),
                                ("clippy.sh", "clippy"),
                                ("integrated.sh", "integrated")):
            self.write(f"pipeline/{filename}", self.wrapper(label), executable=True)
        self.write("pipeline/ci.sh", '''#!/bin/sh
set -eu
ROOT=$(CDPATH='' cd "$(dirname "$0")/.." && pwd)
exec python3 "$ROOT/fixture_product.py" "$@"
''', executable=True)
        self.write("fixture_product.py", '''import os
from pathlib import Path
import subprocess
import sys

root = Path(os.environ["FIXTURE_ROOT"])
arguments = sys.argv[2:]
phase = arguments[arguments.index("--phase") + 1]
raise SystemExit(subprocess.run([sys.executable, str(root / "fixture_gate.py"),
                                sys.argv[1] + "." + phase, *arguments]).returncode)
''')
        self.write("pipeline/nextest_tool.py", '''from pathlib import Path

def runner():
    return Path(__file__).resolve().parent / "fixture-nextest"
''')
        self.write("pipeline/parallel_tests.py", '''import os
from pathlib import Path
import subprocess
import sys

root = Path(os.environ["FIXTURE_ROOT"])
raise SystemExit(subprocess.run([sys.executable, str(root / "fixture_gate.py"),
                                "parallel-rust", *sys.argv[1:]]).returncode)
''')
        self.write("pipeline/platform.sh", '''#!/bin/sh
set -eu
ROOT=$(CDPATH='' cd "$(dirname "$0")/.." && pwd)
if [ "$1" = catalog ]; then
    exec "$ROOT/pipeline/integrated.sh"
fi
suite=$1
shift
exec python3 "$ROOT/fixture_gate.py" "shared-$suite" "$@"
''', executable=True)

        # Fake gate bodies never enter the production broker. The dispatcher
        # reads commit identity directly from Git.
        self.write("ci_broker/client.py", f"""import json
import os
import subprocess
import sys

if sys.argv[1] == "run":
    command = sys.argv[sys.argv.index("--") + 1:]
else:
    raise SystemExit("unexpected fixture broker command")
if "--verbose-receipt" in sys.argv:
    child = subprocess.run(command, env=os.environ, stdout=subprocess.PIPE)
    gate = sys.argv[sys.argv.index("--gate") + 1]
    print(json.dumps({{"protocol_version": 1, "gate": gate,
                      "source_key": os.environ["CELL_CI_EXPECTED_SOURCE_KEY"],
                      "state": "passed" if child.returncode == 0 else "failed",
                      "exit_code": child.returncode, "execution_id": gate + ".fixture"}}))
    raise SystemExit(child.returncode)
raise SystemExit(subprocess.run(command, env=os.environ).returncode)
""")
        self.write("fixture_gate.py", """import json
import os
from pathlib import Path
import subprocess
import sys

label = sys.argv[1]
root = Path(os.environ["FIXTURE_ROOT"])
with Path(os.environ["FIXTURE_GATE_LOG"]).open("a") as log:
    log.write(json.dumps({"gate": label, "args": sys.argv[2:],
                          "source": os.environ.get("CELL_CI_EXPECTED_SOURCE_KEY")}) + "\\n")
if os.environ.get("FIXTURE_CHANGE_AT") == label:
    with (root / "alpha/tracked.txt").open("a") as source:
        source.write("changed during gate\\n")
if os.environ.get("FIXTURE_INDEX_AT") == label:
    subprocess.run(["git", "add", "alpha/tracked.txt"], cwd=root, check=True)
if os.environ.get("FIXTURE_FAIL_AT") == label:
    raise SystemExit(23)
""")
        self.git("add", ".")
        self.git("commit", "-qm", "Fixture")

    @staticmethod
    def wrapper(label):
        return f'''#!/bin/sh
set -eu
ROOT=$(CDPATH='' cd "$(dirname "$0")/.." && pwd)
exec python3 "$ROOT/fixture_gate.py" {label} "$@"
'''

    def write(self, relative, contents, executable=False):
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(contents)
        if executable:
            path.chmod(0o755)

    def git(self, *arguments):
        return subprocess.run(["git", *arguments], cwd=self.root,
                              env=self.environment, text=True,
                              capture_output=True, check=True).stdout

    def helper(self, *arguments):
        return subprocess.run([sys.executable, "pipeline/select_changes.py", *arguments],
                              cwd=self.root, env=self.environment,
                              text=True, capture_output=True)

    def plan(self, *arguments):
        result = self.helper("plan", *arguments)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        tokens = result.stdout.split()
        self.assertGreaterEqual(len(tokens), 5, result.stdout)
        self.assertEqual(tokens[4], "3")
        return tokens, result

    def ci(self, *arguments, **environment):
        return subprocess.run([sys.executable, "pipeline/select_changes.py", "run", *arguments], cwd=self.root,
                              env={**self.environment, **environment},
                              text=True, capture_output=True)

    def gates(self):
        if not self.log.exists():
            return []
        return [json.loads(line) for line in self.log.read_text().splitlines()]

    def assert_passed(self, result):
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_clean_checkout_selects_no_projects(self):
        tokens, _ = self.plan()
        self.assertEqual(tokens[:2], ["changed", "quiet"])
        self.assertEqual(tokens[5:], [])

    def test_staged_change_selects_its_project(self):
        self.write("alpha/tracked.txt", "staged source\n")
        self.git("add", "alpha/tracked.txt")
        tokens, _ = self.plan()
        self.assertEqual(tokens[5:], ["alpha"])

    def test_unstaged_change_selects_its_project(self):
        self.write("beta/tracked.txt", "unstaged source\n")
        tokens, _ = self.plan()
        self.assertEqual(tokens[5:], ["beta"])

    def test_untracked_paths_with_spaces_and_newlines_select_their_project(self):
        self.write("alpha/new file\nwith newline.txt", "untracked\n")
        tokens, _ = self.plan()
        self.assertEqual(tokens[5:], ["alpha"])

    def test_ignored_files_do_not_select_projects(self):
        self.write("alpha/ignored/generated.txt", "ignored\n")
        tokens, _ = self.plan()
        self.assertEqual(tokens[5:], [])

    def test_deleted_file_selects_its_project(self):
        (self.root / "decisions/tracked.txt").unlink()
        tokens, _ = self.plan()
        self.assertEqual(tokens[5:], ["krisis"])

    def test_rename_between_projects_selects_both_ends(self):
        self.git("mv", "alpha/tracked.txt", "beta/moved file\nwith newline.txt")
        tokens, _ = self.plan()
        self.assertEqual(set(tokens[5:]), {"alpha", "beta"})

    def test_rename_from_project_to_shared_path_keeps_source_project(self):
        self.git("mv", "alpha/tracked.txt", "shared file\nwith newline.txt")
        tokens, result = self.plan()
        self.assertEqual(tokens[5:], ["alpha"])
        self.assertIn("shared", result.stderr)

    def test_descriptor_change_selects_its_project(self):
        descriptor = self.root / "pipeline/products/beta.sh"
        descriptor.write_text(descriptor.read_text() + "# changed descriptor\n")
        tokens, _ = self.plan()
        self.assertEqual(tokens[5:], ["beta"])

    def test_retired_product_checks_pipeline_and_remaining_catalog(self):
        self.git("rm", "-r", "pipeline/products/krisis.sh", "decisions")
        result = self.ci()
        self.assert_passed(result)
        self.assertEqual([gate["gate"] for gate in self.gates()],
                         ["preflight", "recognition", "shared-pipeline", "integrated"])

    def test_shared_changes_are_reported_without_selecting_projects(self):
        self.write("Cargo.lock", "changed shared lockfile\n")
        self.write("unowned notes.txt", "new shared input\n")
        tokens, result = self.plan()
        self.assertEqual(tokens[5:], [])
        self.assertIn("Cargo.lock", result.stderr)
        self.assertIn("unowned notes.txt", result.stderr)

    def test_manager_change_selects_pipeline_regressions(self):
        base = self.git("rev-parse", "HEAD").strip()
        self.write("ci_manager/manager.py", "# changed manager source\n")
        self.git("add", "ci_manager/manager.py")
        self.git("commit", "-qm", "Change manager")
        candidate = self.git("rev-parse", "HEAD").strip()
        result = self.ci("--base", base, "--candidate", candidate)
        self.assert_passed(result)
        self.assertEqual([gate["gate"] for gate in self.gates()],
                         ["preflight", "recognition", "shared-pipeline"])

    def test_alias_selects_canonical_project_even_when_clean(self):
        tokens, _ = self.plan("decisions")
        self.assertEqual(tokens[0], "explicit")
        self.assertEqual(tokens[5:], ["krisis"])

    def test_all_selects_every_project_when_clean(self):
        tokens, _ = self.plan("--all", "--verbose")
        self.assertEqual(tokens[:2], ["all", "verbose"])
        self.assertEqual(set(tokens[5:]), {"alpha", "beta", "krisis"})

    def test_unknown_and_conflicting_arguments_fail_before_gates(self):
        for arguments in (("unknown",), ("--unknown",), ("--all", "alpha")):
            with self.subTest(arguments=arguments):
                result = self.ci(*arguments)
                self.assertNotEqual(result.returncode, 0)
                self.assertTrue(result.stderr)
                self.assertEqual(self.gates(), [])

    def test_invalid_descriptor_is_not_treated_as_empty_selection(self):
        self.write("pipeline/products/alpha.sh", "PIPELINE_SCHEMA=999\nPRODUCT_ID=alpha\n")
        result = self.ci()
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue(result.stderr)
        self.assertEqual(self.gates(), [])

    def test_git_discovery_failure_stops_before_gates(self):
        (self.root / ".git").rename(Path(self.temporary.name) / "hidden-git")
        result = self.ci()
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue(result.stderr)
        self.assertEqual(self.gates(), [])

    def test_git_state_check_accepts_unchanged_checkout(self):
        tokens, _ = self.plan()
        self.assert_passed(self.helper("check", tokens[2], tokens[3]))

    def test_git_state_check_rejects_dirty_checkout(self):
        tokens, _ = self.plan()
        self.write("beta/tracked.txt", "changed after planning\n")
        result = self.helper("check", tokens[2], tokens[3])
        self.assertEqual(result.returncode, 75, result.stdout + result.stderr)

    def test_git_state_check_rejects_index_only_change(self):
        self.write("alpha/tracked.txt", "already dirty source\n")
        tokens, _ = self.plan()
        self.git("add", "alpha/tracked.txt")
        later, _ = self.plan()
        self.assertEqual(tokens[2], later[2], "staging alone should not change the commit")
        result = self.helper("check", tokens[2], tokens[3])
        self.assertEqual(result.returncode, 75, result.stdout + result.stderr)

    def test_clean_root_run_keeps_common_checks(self):
        result = self.ci()
        self.assert_passed(result)
        self.assertEqual([gate["gate"] for gate in self.gates()],
                         ["preflight", "recognition"])

    def test_default_root_run_executes_only_changed_project(self):
        self.write("beta/tracked.txt", "changed beta source\n")
        result = self.ci("--verbose")
        self.assert_passed(result)
        gates = self.gates()
        self.assertEqual([gate["gate"] for gate in gates],
                         ["preflight", "recognition", "beta.pre", "clippy", "parallel-rust", "beta.post"])
        self.assertEqual(gates[2]["args"], ["--tests", "product", "--phase", "pre"])
        self.assertEqual(gates[-1]["args"], ["--tests", "product", "--phase", "post"])
        self.assertEqual(gates[3]["args"], ["--product", "beta"])
        self.assertIn("--product", gates[4]["args"])
        self.assertIn("beta", gates[4]["args"])
        self.assertNotIn("--platform-product", gates[4]["args"])
        self.assertEqual(gates[-1]["source"], self.git("rev-parse", "HEAD").strip())

    def test_explicit_alias_uses_descriptor_directory(self):
        result = self.ci("decisions")
        self.assert_passed(result)
        self.assertEqual([gate["gate"] for gate in self.gates()],
                         ["preflight", "recognition", "krisis.pre", "clippy", "parallel-rust", "krisis.post"])

    def test_all_root_run_includes_integrated_check(self):
        result = self.ci("--all")
        self.assert_passed(result)
        gates = [gate["gate"] for gate in self.gates()]
        self.assertEqual(gates[:2], ["preflight", "recognition"])
        self.assertEqual(set(gates[2:-1]), {
            "alpha.pre", "beta.pre", "krisis.pre", "alpha.post", "beta.post", "krisis.post",
            "clippy", "parallel-rust", "shared-pipeline", "shared-broker",
            "shared-deployment", "shared-build", "shared-cleanup", "shared-install",
            "shared-maintenance", "shared-prompts",
        })
        self.assertEqual(len(gates), 19)
        self.assertEqual(gates.count("clippy"), 1)
        self.assertEqual(gates[-1], "integrated")

    def test_skip_tests_retains_checks_builds_and_deployment_scope(self):
        self.write("pipeline/nextest_tool.py", "raise RuntimeError('must not load nextest')\n")
        result = self.ci("--all", "--skip-tests", "--json", CELL_CI_TEST_THREADS="invalid")
        self.assert_passed(result)
        gates = self.gates()
        self.assertEqual([gate["gate"] for gate in gates], [
            "preflight", "recognition", "shared-install", "shared-maintenance", "shared-prompts",
            "alpha.pre", "beta.pre", "krisis.pre", "clippy", "alpha.post", "beta.post", "krisis.post",
            "integrated",
        ])
        self.assertTrue(all(gate["args"] == ["--checks-only"] for gate in gates
                            if gate["gate"].startswith("shared-")))
        self.assertTrue(all(gate["args"][1] == "none" for gate in gates
                            if gate["gate"].endswith((".pre", ".post"))))
        clippy = [gate for gate in gates if gate["gate"] == "clippy"]
        self.assertEqual([gate["args"] for gate in clippy], [[
            "--product", "alpha", "--product", "beta", "--product", "krisis",
            "--shared-suite", "install", "--shared-suite", "maintenance",
            "--shared-suite", "prompts",
        ]])
        receipt = json.loads(result.stdout)
        self.assertTrue(receipt["selection"]["tests_skipped"])
        self.assertEqual(receipt["selection"]["product_tests"], ["alpha", "beta", "krisis"])
        self.assertEqual(receipt["selection"]["platform_products"], ["alpha", "beta", "krisis"])
        self.assertEqual([gate["gate"] for gate in receipt["selection"]["required_gates"]],
                         [gate["gate"] for gate in receipt["gates"]])

    def test_explicitly_listing_every_project_does_not_request_integrated_check(self):
        result = self.ci("alpha", "beta", "krisis")
        self.assert_passed(result)
        gates = [gate["gate"] for gate in self.gates()]
        self.assertEqual(gates[:2], ["preflight", "recognition"])
        self.assertEqual(gates[2:], ["alpha.pre", "beta.pre", "krisis.pre", "clippy", "parallel-rust",
                                     "alpha.post", "beta.post", "krisis.post"])
        self.assertEqual(len(gates), 10)

    def test_project_failure_propagates_and_stops_later_gates(self):
        result = self.ci("--all", FIXTURE_FAIL_AT="alpha.pre")
        self.assertEqual(result.returncode, 23, result.stdout + result.stderr)
        self.assertEqual([gate["gate"] for gate in self.gates()],
                         ["preflight", "recognition", "shared-pipeline", "shared-broker",
                          "shared-deployment", "shared-build", "shared-cleanup", "shared-install",
                          "shared-maintenance", "shared-prompts", "alpha.pre"])

    def test_shared_test_failure_stops_all_post_test_gates(self):
        result = self.ci("alpha", "beta", FIXTURE_FAIL_AT="parallel-rust")
        self.assertEqual(result.returncode, 23, result.stdout + result.stderr)
        self.assertEqual([gate["gate"] for gate in self.gates()],
                         ["preflight", "recognition", "alpha.pre", "beta.pre", "clippy", "parallel-rust"])

    def test_clippy_failure_stops_tests_and_all_post_test_gates(self):
        result = self.ci("alpha", "beta", "--json", FIXTURE_FAIL_AT="clippy")
        self.assertEqual(result.returncode, 23, result.stdout + result.stderr)
        self.assertEqual([gate["gate"] for gate in self.gates()],
                         ["preflight", "recognition", "alpha.pre", "beta.pre", "clippy"])
        receipt = json.loads(result.stdout)
        self.assertEqual(receipt["state"], "failed")
        self.assertEqual(receipt["gates"][-1]["gate"], "cell.clippy")
        self.assertEqual(receipt["gates"][-1]["exit_code"], 23)

    def test_shared_rust_checks_and_platform_targets_join_one_test_gate(self):
        result = self.ci("--platform", "alpha", "beta")
        self.assert_passed(result)
        gates = self.gates()
        self.assertEqual([gate["gate"] for gate in gates],
                         ["preflight", "recognition", "shared-install", "alpha.pre", "beta.pre",
                          "clippy", "parallel-rust", "alpha.post", "beta.post"])
        self.assertEqual(gates[2]["args"], ["--checks-only"])
        self.assertEqual(gates[5]["args"], ["--product", "alpha", "--product", "beta",
                                              "--shared-suite", "install"])
        arguments = gates[6]["args"]
        self.assertEqual(arguments.count("--product"), 2)
        self.assertEqual(arguments.count("--platform-product"), 2)
        self.assertEqual(arguments[-2:], ["--shared-suite", "install"])

    def test_worker_count_is_explicit_and_invalid_values_fail_before_admission(self):
        for value in ("0", "-1", "1.5", "many", ""):
            with self.subTest(value=value):
                result = self.ci("alpha", CELL_CI_TEST_THREADS=value)
                self.assertEqual(result.returncode, 78, result.stdout + result.stderr)
                self.assertIn("positive integer", result.stderr)
                self.assertEqual(self.gates(), [])
        result = self.ci("alpha", CELL_CI_TEST_THREADS="7")
        self.assert_passed(result)
        arguments = next(gate["args"] for gate in self.gates() if gate["gate"] == "parallel-rust")
        self.assertEqual(arguments[arguments.index("--test-threads") + 1], "7")
        path = arguments[arguments.index("--nextest-path") + 1]
        self.assertTrue(Path(path).is_absolute())

    def test_non_rust_plan_does_not_require_the_nextest_tool(self):
        self.write("pipeline/nextest_tool.py", "raise RuntimeError('must not load nextest')\n")
        self.git("add", "pipeline/nextest_tool.py")
        self.git("commit", "-qm", "Unavailable optional runner fixture")
        result = self.ci()
        self.assert_passed(result)
        self.assertEqual([gate["gate"] for gate in self.gates()],
                         ["preflight", "recognition"])

    def test_missing_nextest_stops_required_test_plan_before_admission(self):
        self.write("pipeline/nextest_tool.py", "def runner():\n    raise RuntimeError('provision nextest')\n")
        result = self.ci("alpha")
        self.assertEqual(result.returncode, 78, result.stdout + result.stderr)
        self.assertIn("provision nextest", result.stderr)
        self.assertEqual(self.gates(), [])

    def test_common_rust_executor_change_selects_all_product_and_platform_tests(self):
        driver = self.root / "pipeline/parallel_tests.py"
        self.write("pipeline/parallel_tests.py", driver.read_text() + "# changed executor\n")
        tokens, result = self.plan()
        self.assertEqual(tokens[5:], ["alpha", "beta", "krisis"])
        self.assertIn("global Rust runner changed", result.stderr)
        result = self.ci()
        self.assert_passed(result)
        gates = self.gates()
        arguments = next(gate["args"] for gate in gates if gate["gate"] == "parallel-rust")
        self.assertEqual(arguments.count("--product"), 3)
        self.assertEqual(arguments.count("--platform-product"), 3)
        self.assertEqual(arguments.count("--shared-suite"), 3)
        self.assertTrue(all(gate["args"][1] == "all" for gate in gates
                            if gate["gate"].endswith(".pre")))

    def test_test_only_edits_do_not_expand_product_coverage(self):
        self.write("pipeline/test_parallel_tests.py", "# changed executor tests\n")
        tokens, result = self.plan()
        self.assertEqual(tokens[5:], [])
        self.assertNotIn("global Rust runner changed", result.stderr)

    def test_clippy_executor_change_selects_all_products_and_shared_rust_suites(self):
        driver = self.root / "pipeline/clippy.sh"
        self.write("pipeline/clippy.sh", driver.read_text() + "# changed executor\n", executable=True)
        tokens, result = self.plan()
        self.assertEqual(tokens[5:], ["alpha", "beta", "krisis"])
        self.assertIn("global Rust runner changed", result.stderr)
        result = self.ci()
        self.assert_passed(result)
        clippy = [gate for gate in self.gates() if gate["gate"] == "clippy"]
        self.assertEqual([gate["args"] for gate in clippy], [[
            "--product", "alpha", "--product", "beta", "--product", "krisis",
            "--shared-suite", "install", "--shared-suite", "maintenance",
            "--shared-suite", "prompts",
        ]])



    def test_direct_shared_rust_selection_uses_one_parallel_gate(self):
        result = self.helper("shared", "install", "prompts")
        self.assert_passed(result)
        gates = self.gates()
        self.assertEqual([gate["gate"] for gate in gates],
                         ["shared-install", "shared-prompts", "clippy", "parallel-rust"])
        self.assertEqual(gates[2]["args"], ["--shared-suite", "install", "--shared-suite", "prompts"])
        self.assertEqual(gates[-1]["args"][-4:], ["--shared-suite", "install",
                                                  "--shared-suite", "prompts"])
        self.assertNotIn("--product", gates[-1]["args"])

    def test_aggregate_receipt_requires_the_phases_and_real_shared_gate(self):
        result = self.ci("alpha", "beta", "--json")
        self.assert_passed(result)
        receipt = json.loads(result.stdout)
        self.assertEqual(receipt["schema_version"], 1)
        self.assertEqual(receipt["state"], "passed")
        self.assertFalse(receipt["selection"]["tests_skipped"])
        required = receipt["selection"]["required_gates"]
        expected = ["cell.structure", "cell.recognition", "alpha.pre", "beta.pre",
                    "cell.clippy", "cell.tests.rust", "alpha.post", "beta.post"]
        self.assertEqual([gate["gate"] for gate in required], expected)
        self.assertEqual([gate["gate"] for gate in receipt["gates"]], expected)
        self.assertTrue(all(gate["source_key"] == receipt["source_key"] for gate in receipt["gates"]))
        self.assertEqual(required[4]["lane"], "heavy")
        self.assertEqual(required[4]["command"].count("--product"), 2)
        self.assertEqual(required[5]["lane"], "heavy")
        self.assertEqual(required[5]["command"].count("--product"), 2)

    def test_preflight_failure_stops_before_recognition(self):
        result = self.ci("--all", FIXTURE_FAIL_AT="preflight")
        self.assertEqual(result.returncode, 23, result.stdout + result.stderr)
        self.assertEqual([gate["gate"] for gate in self.gates()], ["preflight"])

    def test_root_run_rejects_source_change_during_gate(self):
        result = self.ci("alpha", FIXTURE_CHANGE_AT="alpha.pre")
        self.assertEqual(result.returncode, 75, result.stdout + result.stderr)

    def test_root_run_rejects_index_only_change_during_gate(self):
        self.write("alpha/tracked.txt", "already dirty source\n")
        result = self.ci(FIXTURE_INDEX_AT="alpha.pre")
        self.assertEqual(result.returncode, 75, result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
