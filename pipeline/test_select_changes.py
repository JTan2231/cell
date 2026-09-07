"""Exercise root CI selection in local Git fixtures without running real gates."""

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
                     "GIT_INDEX_FILE", "GIT_COMMON_DIR"):
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
                                ("integrated.sh", "integrated")):
            self.write(f"pipeline/{filename}", self.wrapper(label), executable=True)
        self.write("pipeline/ci.sh", '''#!/bin/sh
set -eu
ROOT=$(CDPATH='' cd "$(dirname "$0")/.." && pwd)
exec python3 "$ROOT/fixture_gate.py" "$@"
''', executable=True)
        self.write("pipeline/platform.sh", '''#!/bin/sh
set -eu
ROOT=$(CDPATH='' cd "$(dirname "$0")/.." && pwd)
if [ "$1" = catalog ]; then
    exec "$ROOT/pipeline/integrated.sh"
fi
exec python3 "$ROOT/fixture_gate.py" "shared-$1"
''', executable=True)

        # Root infrastructure gates use this adapter. Only source hashing calls
        # the real client; fake gate bodies never enter the production broker.
        self.write("ci_broker/client.py", f"""import os
import subprocess
import sys

if sys.argv[1] == "source-key":
    command = [sys.executable, {str(SOURCE / 'ci_broker/client.py')!r}, *sys.argv[1:]]
elif sys.argv[1] == "run":
    command = sys.argv[sys.argv.index("--") + 1:]
else:
    raise SystemExit("unexpected fixture broker command")
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
        return subprocess.run(["sh", "ci.sh", *arguments], cwd=self.root,
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

    def test_shared_changes_are_reported_without_selecting_projects(self):
        self.write("Cargo.lock", "changed shared lockfile\n")
        self.write("unowned notes.txt", "new shared input\n")
        tokens, result = self.plan()
        self.assertEqual(tokens[5:], [])
        self.assertIn("Cargo.lock", result.stderr)
        self.assertIn("unowned notes.txt", result.stderr)

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

    def test_snapshot_check_accepts_unchanged_checkout(self):
        tokens, _ = self.plan()
        self.assert_passed(self.helper("check", tokens[2], tokens[3]))

    def test_snapshot_check_rejects_source_change(self):
        tokens, _ = self.plan()
        self.write("beta/tracked.txt", "changed after planning\n")
        result = self.helper("check", tokens[2], tokens[3])
        self.assertEqual(result.returncode, 75, result.stdout + result.stderr)

    def test_snapshot_check_rejects_index_only_change(self):
        self.write("alpha/tracked.txt", "already dirty source\n")
        tokens, _ = self.plan()
        self.git("add", "alpha/tracked.txt")
        later, _ = self.plan()
        self.assertEqual(tokens[2], later[2], "staging alone should not change source bytes")
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
                         ["preflight", "recognition", "beta"])
        self.assertEqual(gates[-1]["args"], ["--tests", "product"])
        self.assertTrue(gates[-1]["source"].startswith("sha256:"))

    def test_explicit_alias_uses_descriptor_directory(self):
        result = self.ci("decisions")
        self.assert_passed(result)
        self.assertEqual([gate["gate"] for gate in self.gates()],
                         ["preflight", "recognition", "krisis"])

    def test_all_root_run_includes_integrated_check(self):
        result = self.ci("--all")
        self.assert_passed(result)
        gates = [gate["gate"] for gate in self.gates()]
        self.assertEqual(gates[:2], ["preflight", "recognition"])
        self.assertEqual(set(gates[2:-1]), {
            "alpha", "beta", "krisis", "shared-pipeline", "shared-broker",
            "shared-deployment", "shared-build", "shared-cleanup", "shared-install",
            "shared-maintenance",
        })
        self.assertEqual(len(gates), 13)
        self.assertEqual(gates[-1], "integrated")

    def test_explicitly_listing_every_project_does_not_request_integrated_check(self):
        result = self.ci("alpha", "beta", "krisis")
        self.assert_passed(result)
        gates = [gate["gate"] for gate in self.gates()]
        self.assertEqual(gates[:2], ["preflight", "recognition"])
        self.assertEqual(set(gates[2:]), {"alpha", "beta", "krisis"})
        self.assertEqual(len(gates), 5)

    def test_project_failure_propagates_and_stops_later_gates(self):
        result = self.ci("--all", FIXTURE_FAIL_AT="alpha")
        self.assertEqual(result.returncode, 23, result.stdout + result.stderr)
        self.assertEqual([gate["gate"] for gate in self.gates()],
                         ["preflight", "recognition", "shared-pipeline", "shared-broker",
                          "shared-deployment", "shared-build", "shared-cleanup", "shared-install",
                          "shared-maintenance", "alpha"])

    def test_preflight_failure_stops_before_recognition(self):
        result = self.ci("--all", FIXTURE_FAIL_AT="preflight")
        self.assertEqual(result.returncode, 23, result.stdout + result.stderr)
        self.assertEqual([gate["gate"] for gate in self.gates()], ["preflight"])

    def test_root_run_rejects_source_change_during_gate(self):
        result = self.ci("alpha", FIXTURE_CHANGE_AT="alpha")
        self.assertEqual(result.returncode, 75, result.stdout + result.stderr)

    def test_root_run_rejects_index_only_change_during_gate(self):
        self.write("alpha/tracked.txt", "already dirty source\n")
        result = self.ci(FIXTURE_INDEX_AT="alpha")
        self.assertEqual(result.returncode, 75, result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
