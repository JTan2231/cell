"""Exercise release manifests using local Git repositories and a fake Cargo."""

import os
from pathlib import Path
import subprocess
import tempfile
import unittest


SOURCE = Path(__file__).resolve().parent


class CompanionReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="cell-release-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name) / "workspace"
        self.root.mkdir()
        self.environment = os.environ.copy()
        self.environment.pop("CELL_CI_EXPECTED_SOURCE_KEY", None)
        self.run_git("init", "-q", "--initial-branch=main")
        self.run_git("config", "user.name", "Release Fixture")
        self.run_git("config", "user.email", "release@example.invalid")
        self.run_git("config", "commit.gpgsign", "false")
        self.run_git("config", "tag.gpgsign", "false")
        origin = Path(self.temporary.name) / "origin.git"
        self.run_git("init", "-q", "--bare", str(origin))
        self.run_git("remote", "add", "origin", str(origin))
        for name in ("lib.sh", "release.sh"):
            self.write(f"pipeline/{name}", (SOURCE / name).read_text(), executable=True)
        self.write("pipeline/products/fixture.sh", """PIPELINE_SCHEMA=1
PRODUCT_ID=fixture
PRODUCT_NAME=Fixture
PRODUCT_DIR=fixture
CI_RESOURCE_CLASS=light
RELEASE_BRANCH=main
DEPLOY_PROFILE=custom
DEPLOY_CONFLICT_KEYS=fixture
CARGO_PACKAGES='fixture fixture-api'
RELEASE_UNITS='fixture|Fixture|package|fixture/Cargo.toml|fixture-|1
fixture-usage|Fixture Usage|package|fixture/usage/Cargo.toml|fixture-usage-|0'
RELEASE_ALLOW_EXPLICIT_UNIT=1
RELEASE_COMPANION_MANIFESTS='fixture|fixture/api/Cargo.toml'
PROVIDERS='fixture|fixture|fixture/chancery|0
fixture-usage|fixture-usage|fixture/usage-chancery|0'
""")
        self.write("Cargo.toml", '[workspace]\nmembers = ["fixture", "fixture/api"]\n')
        for path, name in (("fixture/Cargo.toml", "fixture"),
                           ("fixture/api/Cargo.toml", "fixture-api")):
            self.write(path, f'[package]\nname = "{name}"\nversion = "1.2.3"\n')
        self.write("fixture/usage/Cargo.toml", '[package]\nname = "fixture-usage"\nversion = "7.8.9"\n')
        self.write("fixture/chancery/provider.json", '{\n  "release": "1.2.3"\n}\n')
        self.write("fixture/usage-chancery/provider.json", '{\n  "release": "7.8.9"\n}\n')
        self.write("fixture/ci.sh", '#!/bin/sh\nexit "${FIXTURE_CI_EXIT:-0}"\n', executable=True)
        fake_bin = Path(self.temporary.name) / "bin"
        fake_bin.mkdir()
        cargo = fake_bin / "cargo"
        cargo.write_text('''#!/bin/sh
case " $* " in *" --locked "*) exit 0 ;; esac
cat fixture/Cargo.toml fixture/api/Cargo.toml fixture/usage/Cargo.toml > Cargo.lock
''')
        cargo.chmod(0o755)
        self.environment["PATH"] = str(fake_bin) + os.pathsep + self.environment["PATH"]
        self.refresh_lock()
        self.run_git("add", ".")
        self.run_git("commit", "-qm", "Fixture")
        self.run_git("push", "-q", "origin", "main")

    def write(self, relative, contents, executable=False):
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(contents)
        if executable:
            path.chmod(0o755)

    def run_git(self, *arguments):
        return subprocess.run(["git", *arguments], cwd=self.root, env=self.environment,
                              text=True, capture_output=True, check=True).stdout.strip()

    def refresh_lock(self):
        self.write("Cargo.lock", (self.root / "fixture/Cargo.toml").read_text()
                   + (self.root / "fixture/api/Cargo.toml").read_text()
                   + (self.root / "fixture/usage/Cargo.toml").read_text())

    def release(self, unit=None, **extra_environment):
        arguments = ["sh", "pipeline/release.sh", "fixture"]
        if unit is not None:
            arguments.append(unit)
        return subprocess.run([*arguments, "--patch"],
                              cwd=self.root, env={**self.environment, **extra_environment},
                              text=True, capture_output=True)

    def test_companion_is_committed_with_owner_and_tag(self):
        result = self.release()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        for path in ("fixture/Cargo.toml", "fixture/api/Cargo.toml"):
            self.assertIn('version = "1.2.4"', (self.root / path).read_text())
        self.assertIn('"release": "1.2.4"',
                      (self.root / "fixture/chancery/provider.json").read_text())
        self.assertEqual(self.run_git("status", "--porcelain"), "")
        self.assertEqual(self.run_git("rev-parse", "HEAD"),
                         self.run_git("rev-parse", "fixture-v1.2.4^{}"))
        changed = set(self.run_git("diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD").splitlines())
        self.assertEqual(changed, {"fixture/Cargo.toml", "fixture/api/Cargo.toml",
                                   "fixture/chancery/provider.json", "Cargo.lock"})
        remote_head = self.run_git("ls-remote", "origin", "refs/heads/main").split()[0]
        self.assertEqual(remote_head, self.run_git("rev-parse", "HEAD"))

    def test_independent_unit_does_not_bump_other_units_companion(self):
        result = self.release(unit="fixture-usage")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        for path in ("fixture/Cargo.toml", "fixture/api/Cargo.toml"):
            self.assertIn('version = "1.2.3"', (self.root / path).read_text())
        self.assertIn('version = "7.8.10"',
                      (self.root / "fixture/usage/Cargo.toml").read_text())
        self.assertEqual(self.run_git("status", "--porcelain"), "")

    def test_failed_ci_restores_companion_and_all_version_files(self):
        before = self.run_git("rev-parse", "HEAD")
        result = self.release(FIXTURE_CI_EXIT="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("restored version files", result.stderr)
        self.assertEqual(self.run_git("status", "--porcelain"), "")
        self.assertEqual(self.run_git("rev-parse", "HEAD"), before)
        self.assertEqual(self.run_git("tag", "--list"), "")

    def test_mismatched_companion_fails_before_mutation(self):
        path = self.root / "fixture/api/Cargo.toml"
        path.write_text(path.read_text().replace("1.2.3", "9.9.9"))
        self.refresh_lock()
        self.run_git("add", ".")
        self.run_git("commit", "-qm", "Mismatched fixture")
        self.run_git("push", "-q", "origin", "main")
        before = self.run_git("rev-parse", "HEAD")
        result = self.release()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("does not match", result.stderr)
        self.assertEqual(self.run_git("status", "--porcelain"), "")
        self.assertEqual(self.run_git("rev-parse", "HEAD"), before)

    def test_invalid_companion_descriptors_are_rejected(self):
        for value in ("unknown|fixture/api/Cargo.toml",
                      "fixture|fixture/api/Cargo.toml\nfixture|fixture/api/Cargo.toml",
                      "fixture|fixture/Cargo.toml",
                      "fixture|Cargo.toml",
                      "fixture|fixture/missing/Cargo.toml"):
            with self.subTest(value=value):
                result = subprocess.run(
                    ["sh", "-c", 'PIPELINE_ROOT=$PWD; . pipeline/lib.sh; '
                     'pipeline_load_descriptor fixture; RELEASE_COMPANION_MANIFESTS=$1; '
                     'pipeline_validate_descriptor', "sh", value],
                    cwd=self.root, env=self.environment, text=True, capture_output=True)
                self.assertNotEqual(result.returncode, 0)


if __name__ == "__main__":
    unittest.main()
