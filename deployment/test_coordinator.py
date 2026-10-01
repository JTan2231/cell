"""Offline deployment proof: fake products, disposable Git, no live services."""

from __future__ import annotations

import contextlib
import hashlib
import io
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import tomllib
from types import SimpleNamespace
import unittest
from unittest import mock

sys.dont_write_bytecode = True

from deployment import candidate, cli, signing
from deployment.inventory import descriptor


ROOT = Path(__file__).resolve().parent.parent
FAKE_ADAPTER = '''import json, pathlib, sys, time
sys.dont_write_bytecode = True
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[2]))
op = sys.argv[1]
r = json.load(sys.stdin)
run = pathlib.Path(r["run_dir"])
name = r["product"]
with (run / "observed.jsonl").open("a") as output:
    output.write(json.dumps([name, op]) + "\\n")
fault_path = run / "fault.json"
fault = json.loads(fault_path.read_text()) if fault_path.exists() else {}
if fault.get("stderr", {}).get(op):
    print(fault["stderr"][op], file=sys.stderr)
if fault.get("sleep") == [name, op]:
    (run / "child-started").write_text("yes")
    time.sleep(30)
if fault.get("spawn_sleep") == [name, op]:
    import os, subprocess
    child_code = "import os,pathlib,time;pathlib.Path(" + repr(str(run / "descendant-started")) + ").write_text(str(os.getpid()));time.sleep(30)"
    subprocess.run([sys.executable, "-c", child_code], check=True, pass_fds=(int(os.environ["CELL_DEPLOYMENT_LOCK_FD"]),))
if op == "inspect":
    data = {"maintenance_products": ["beta"] if name == "alpha" else [], "after": []}
else:
    data = {}
if op == "apply":
    (run / ("installed-" + name)).write_text(r["candidate"]["candidate_id"])
if op == "hold":
    (run / ("held-" + name)).touch()
if op == "release":
    (run / ("held-" + name)).unlink(missing_ok=True)
if op == "recover":
    data = {"safe_to_release": fault.get("recovery_safe", True), "installed": "candidate" if (run / ("installed-" + name)).exists() else "prior"}
if fault.get("invalid") == [name, op]:
    print("uncertain output")
    sys.exit(1)
status = {"inspect":"ready","hold":"held","drain":"drained","apply":"applied","release":"released","recover":"recovered","configure":"configured","activate":"activated"}[op]
if op == "drain" and fault.get("wait") == name:
    marker = run / ("waited-" + name)
    if not marker.exists():
        marker.touch()
        status = "waiting"
if fault.get("fail") == [name, op]:
    status = "stopped"
print(json.dumps({"schema":1,"status":status,"data":data,"detail":fault.get("detail", "isolated fixture")}))
'''

FAKE_BUILD = '''import argparse, json, pathlib, sys
sys.dont_write_bytecode = True
parser = argparse.ArgumentParser()
parser.add_argument("--source-root", type=pathlib.Path)
parser.add_argument("--output", type=pathlib.Path)
parser.add_argument("--product", action="append")
parser.add_argument("--signing-policy-file")
args = parser.parse_args()
root = args.source_root
sys.path.insert(0, str(root))
from deployment import candidate
source_key = candidate.source_identity(root)
records = {}
for name in args.product:
    binary = root / "target" / "release" / name
    binary.parent.mkdir(parents=True, exist_ok=True)
    binary.write_text("#!/bin/sh\\nprintf '%s\\\\n' '" + name + " 1.0.0'\\n")
    binary.chmod(0o755)
    spec = name + "|target/release/" + name + "|" + name
    versions = {name: name + " 1.0.0"}
    installer = binary.with_name(name + "-install")
    if (root / name / "packaging/installer-fixture").exists():
        installer.write_text((root / name / "packaging/installer-fixture").read_text())
        installer.chmod(0o755)
        if not (root / name / "packaging/skip-installer").exists():
            spec += "\\n" + name + "|target/release/" + name + "-install|" + name + "-install"
            versions[name + "-install"] = name + "-install 1.0.0"
    records[name] = candidate.stage_build(root, name, args.output / "candidates" / name, spec,
        target=root / "target", source_key=source_key)
    if (root / name / "packaging/installer-fixture").exists():
        installer.write_text("later target contents must never execute")
(args.output / "result.json").write_text(json.dumps({"schema":1, "state":"built",
    "source_key":source_key, "candidates":records, "cache_hit":False}))
'''


class Fixture:
    def __init__(self, path: Path):
        self.base = path
        self.repo = path / "repository"
        self.repo.mkdir()
        self.storage = path / "private"
        self.storage.mkdir(mode=0o700)
        self.git("init", "-q", "-b", "main")
        self.git("config", "user.name", "Offline deployment fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        self.write(".gitignore", "target/\n__pycache__/\n")
        for relative in ("deployment/__init__.py", "deployment/cli.py", "deployment/candidate.py", "deployment/inventory.py",
                         "ci_broker/__init__.py", "ci_broker/client.py", "ci_broker/broker.py",
                         "ci_manager/__init__.py", "ci_manager/workspace.py"):
            self.write(relative, (ROOT / relative).read_text())
        with (self.repo / "ci_manager/workspace.py").open("a") as stream:
            stream.write(f"\n# Isolated fixture storage; never inspect the live configuration.\ndef root():\n    return Path({str(self.base)!r})\n")
        # The pinned cleanup subprocess never touches actual installations in
        # this disposable fixture, including when run_worker is called directly.
        self.write("deployment/cleanup.py", "print('{}')\n")
        self.write("deployment/signing.py", '''import hashlib,json
def load_policy(): return None
def validate_policy(value): return value
def policy_digest(value): return hashlib.sha256((json.dumps(value,sort_keys=True,separators=(",",":"))+"\\n").encode()).hexdigest()
def assert_current(value): pass
def preflight(value): pass
def identifier(value,product,key): return None
def sign(*args): pass
def verify(*args): pass
''')
        self.write("deployment/build.py", FAKE_BUILD)
        # Quality-gate admission is simulated in this isolated repository. It
        # never invokes the production broker, Cargo, or a real product body.
        client_path = self.repo / "ci_broker/client.py"
        client_path.write_text(client_path.read_text().replace(
            'if __name__ == "__main__":',
            'if __name__ == "__main__" and sys.argv[1:2] == ["run"]:\n    raise SystemExit(0)\n\nif __name__ == "__main__":'))
        self.write("pipeline/test.sh", "#!/bin/sh\nexit 99\n", executable=True)
        for name in ("alpha", "beta"):
            self.write(f"pipeline/products/{name}.sh", f"PRODUCT_ID={name}\nPRODUCT_DIR={name}\nDEPLOY_PROFILE=selector-only-v1\n")
            self.write(f"{name}/deployment/adapter.json", json.dumps({"schema": 1, "product": name, "dependencies": []}))
            self.write(f"{name}/deployment/adapter.py", FAKE_ADAPTER)
            self.write(f"{name}/packaging/manifest.txt", "owned packaging\n")
            self.write(f"{name}/ci.sh", "#!/bin/sh\nexit 99\n", executable=True)
        self.commit()

    def write(self, relative: str, content: str, executable: bool = False) -> None:
        path = self.repo / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)
        if executable:
            path.chmod(0o755)

    def git(self, *arguments: str) -> str:
        result = subprocess.run(["git", "-C", str(self.repo), *arguments], check=True,
                                capture_output=True, text=True)
        return result.stdout.strip()

    def commit(self) -> None:
        self.git("add", ".")
        self.git("commit", "-qm", "isolated fixture")

    def create(self, products: tuple[str, ...] = ("alpha",)) -> Path:
        return cli.create_run(self.repo, products, self.storage)

    def add_binary_adapter(self, *, stage_installer: bool = True, name: str = "usher", affected: tuple[str, ...] = ()) -> None:
        self.write(f"pipeline/products/{name}.sh", f"PRODUCT_ID={name}\nPRODUCT_DIR={name}\nDEPLOY_PROFILE=rust-install-v1\n")
        self.write(f"{name}/deployment/adapter.json", json.dumps({
            "schema": 1, "product": name, "dependencies": [], "adapter_binary": f"{name}-install",
            "maintenance_products": list(affected)}))
        installer = f"#!{sys.executable}\n" + FAKE_ADAPTER.replace(
            "op = sys.argv[1]", f'if sys.argv[1:] == ["--version"]:\n    print("{name}-install 1.0.0")\n    sys.exit(0)\nassert sys.argv[1] == "adapter"\nop = sys.argv[2]')
        installer = installer.replace('["beta"] if name == "alpha" else []', repr(list(affected)))
        self.write(f"{name}/packaging/installer-fixture", installer)
        if not stage_installer:
            self.write(f"{name}/packaging/skip-installer", "yes")
        self.write(f"{name}/ci.sh", "#!/bin/sh\nexit 99\n", executable=True)
        self.write("deployment/cleanup.py", 'import json,sys\nprint(json.dumps({"arguments":sys.argv[1:]}))\n')
        self.commit()


@unittest.skipIf(sys.version_info < (3, 11), "deployment runtime requires Python 3.11")
class DeploymentTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.base = Path(self.temporary.name).resolve()
        for name in ("load_policy", "assert_current", "preflight"):
            patch = mock.patch.object(signing, name, return_value=None)
            patch.start()
            self.addCleanup(patch.stop)
        storage_patch = mock.patch.object(cli.workspace, "root", return_value=self.base)
        storage_patch.start()
        self.addCleanup(storage_patch.stop)
        home = self.base / "home"
        home.mkdir()
        home_patch = mock.patch.object(cli.pwd, "getpwuid", return_value=SimpleNamespace(pw_dir=str(home)))
        home_patch.start()
        self.addCleanup(home_patch.stop)
        self.fixture = Fixture(self.base)

    def tearDown(self) -> None:
        # Sealed source and candidate trees are intentionally read-only.
        for path in self.base.rglob("*"):
            if path.is_dir() and not path.is_symlink():
                path.chmod(0o700)
        self.temporary.cleanup()

    def observed(self, path: Path) -> list[list[str]]:
        return [json.loads(line) for line in (path / "observed.jsonl").read_text().splitlines()]

    def retain_correlated(self, *, policy=None, legacy=False, phase="terminal"):
        commit = self.fixture.git("rev-parse", "HEAD")
        request = cli.canonical_request(self.fixture.repo, ["alpha"], commit, {}, policy)
        if legacy:
            request.pop("signing_policy")
            request.pop("signing_policy_digest")
        identity = "fixture:retained"
        result = {"schema": 1, "state": "succeeded", "request_id": identity,
                  "source_commit": commit, "products": ["alpha"], "exit_code": 0}
        record = {"schema": 1, "request_id": identity, "request": request,
                  "request_hash": hashlib.sha256(candidate.json_bytes(request)).hexdigest(),
                  "run_id": "a" * 32, "products": ["alpha"], "phase": phase,
                  "result": result}
        cli.save_operation(self.fixture.storage, record)
        return identity, commit, record

    def test_correlated_replay_preserves_legacy_or_signed_request_without_current_key(self):
        policy = {"schema": 1, "macos": {"profile": "local", "certificate_sha1": "a" * 40,
                                          "keychain": "/example/login.keychain-db",
                                          "identifier_namespace": "local.cell"}}
        for legacy, retained_policy in ((True, None), (False, policy)):
            with self.subTest(legacy=legacy):
                identity, commit, record = self.retain_correlated(policy=retained_policy, legacy=legacy)
                with mock.patch.object(signing, "load_policy", side_effect=signing.SigningError("missing config")) as load, \
                        mock.patch.object(signing, "assert_current", side_effect=signing.SigningError("changed policy")) as current, \
                        mock.patch.object(cli, "create_run") as create:
                    result = cli.start(self.fixture.repo, ["alpha"], self.fixture.storage,
                                       request_id=identity, selected_commit=commit)
                self.assertTrue(result["replayed"])
                self.assertEqual(result["operation_state"], "terminal")
                self.assertEqual(cli.read_operation(self.fixture.storage, identity)["request"], record["request"])
                load.assert_not_called()
                current.assert_not_called()
                create.assert_not_called()

    def test_correlated_replay_accepts_same_explicit_snapshot_and_rejects_changed_signer(self):
        policy = {"schema": 1, "macos": {"profile": "local", "certificate_sha1": "a" * 40,
                                          "keychain": "/example/login.keychain-db",
                                          "identifier_namespace": "local.cell"}}
        identity, commit, _ = self.retain_correlated(policy=policy)
        with mock.patch.object(signing, "assert_current", side_effect=signing.SigningError("changed host policy")) as current:
            result = cli.start(self.fixture.repo, ["alpha"], self.fixture.storage,
                               request_id=identity, selected_commit=commit, signing_policy=policy)
            changed = {"schema": 1, "macos": {**policy["macos"], "certificate_sha1": "b" * 40}}
            with self.assertRaisesRegex(cli.DeploymentError, "different deployment request"):
                cli.start(self.fixture.repo, ["alpha"], self.fixture.storage,
                          request_id=identity, selected_commit=commit, signing_policy=changed)
        self.assertTrue(result["replayed"])
        current.assert_not_called()

    def test_correlated_legacy_replay_cannot_acquire_an_explicit_new_signer(self):
        identity, commit, _ = self.retain_correlated(legacy=True)
        policy = {"schema": 1, "macos": {"profile": "local", "certificate_sha1": "a" * 40,
                                          "keychain": "/example/login.keychain-db",
                                          "identifier_namespace": "local.cell"}}
        with self.assertRaisesRegex(cli.DeploymentError, "different deployment request"):
            cli.start(self.fixture.repo, ["alpha"], self.fixture.storage,
                      request_id=identity, selected_commit=commit, signing_policy=policy)

    def test_correlated_busy_replay_does_not_probe_signing_or_admit_work(self):
        identity, commit, _ = self.retain_correlated(legacy=True, phase="running")
        with mock.patch.object(cli, "deployment_lock", side_effect=cli.DeploymentBusy("busy")), \
                mock.patch.object(signing, "load_policy", side_effect=signing.SigningError("missing")) as load, \
                mock.patch.object(cli, "create_run") as create:
            result = cli.start(self.fixture.repo, ["alpha"], self.fixture.storage,
                               request_id=identity, selected_commit=commit)
        self.assertTrue(result["replayed"])
        self.assertEqual(result["exit_code"], 75)
        load.assert_not_called()
        create.assert_not_called()

    def test_correlated_new_admission_requires_current_signing_policy(self):
        commit = self.fixture.git("rev-parse", "HEAD")
        policy = {"schema": 1, "macos": {"profile": "local", "certificate_sha1": "a" * 40,
                                          "keychain": "/example/login.keychain-db",
                                          "identifier_namespace": "local.cell"}}
        with mock.patch.object(cli, "create_run") as create:
            with mock.patch.object(signing, "load_policy", side_effect=signing.SigningError("missing config")), \
                    self.assertRaisesRegex(signing.SigningError, "missing config"):
                cli.start(self.fixture.repo, ["alpha"], self.fixture.storage,
                          request_id="fixture:new", selected_commit=commit)
            with mock.patch.object(signing, "assert_current", side_effect=signing.SigningError("drifted policy")), \
                    self.assertRaisesRegex(signing.SigningError, "drifted policy"):
                cli.start(self.fixture.repo, ["alpha"], self.fixture.storage,
                          request_id="fixture:new", selected_commit=commit, signing_policy=policy)
        self.assertIsNone(cli.read_operation(self.fixture.storage, "fixture:new"))
        create.assert_not_called()

    def test_private_policy_file_allows_terminal_replay_after_host_policy_changes(self):
        policy = {"schema": 1, "macos": {"profile": "local", "certificate_sha1": "a" * 40,
                                          "keychain": "/example/login.keychain-db",
                                          "identifier_namespace": "local.cell"}}
        identity, commit, _ = self.retain_correlated(policy=policy)
        policy_path = self.base / "frozen-policy.json"
        policy_path.write_text(json.dumps(policy))
        with mock.patch.object(cli.sys, "platform", "darwin"), \
                mock.patch.object(cli, "state_root", return_value=self.fixture.storage), \
                mock.patch.object(cli.ci_client, "repository_root", return_value=self.fixture.repo), \
                mock.patch.object(signing, "assert_current", side_effect=signing.SigningError("changed policy")) as current, \
                contextlib.redirect_stdout(io.StringIO()) as output:
            code = cli.main(["start", "alpha", "--request-id", identity, "--source-commit", commit,
                             "--signing-policy-file", str(policy_path)])
        self.assertEqual(code, 0, output.getvalue())
        self.assertEqual(json.loads(output.getvalue())["operation_state"], "terminal")
        current.assert_not_called()

    def test_plan_uses_main_and_ignores_uncommitted_adapter_or_manifest_edits(self) -> None:
        expected = self.fixture.git("rev-parse", "HEAD")
        self.fixture.write("alpha/deployment/adapter.json", "malformed edited metadata")
        result = cli.plan(self.fixture.repo, ["alpha"])
        self.assertEqual(result["source_commit"], expected)
        self.assertEqual(result["products"], ["alpha"])
        self.assertEqual(self.fixture.git("status", "--porcelain"), "M alpha/deployment/adapter.json")

    def declare_dependency(self, *, bounded=False) -> None:
        metadata = {"schema": 1, "product": "alpha", "dependencies": ["beta"]}
        if bounded:
            metadata["runtime_versions"] = {"beta": {"minimum": "1.2.0", "before": "2.0.0"}}
        self.fixture.write("alpha/deployment/adapter.json", json.dumps(metadata))
        self.fixture.write("pipeline/products/beta.sh", "PRODUCT_ID=beta\nPRODUCT_DIR=beta\nDEPLOY_PROFILE=selector-only-v1\nRELEASE_UNITS='beta|Beta|package|beta/Cargo.toml|beta-|1'\n")
        self.fixture.write("beta/Cargo.toml", '[package]\nname="beta"\nversion="1.3.0"\n')
        self.fixture.commit()

    def test_plan_installs_missing_runtime_dependency(self):
        self.declare_dependency()
        with mock.patch.object(cli, "installed_product", return_value=False):
            plan = cli.plan(self.fixture.repo, ["alpha"])
        self.assertEqual(plan["products"], ["beta", "alpha"])
        self.assertEqual(plan["selection_reasons"]["beta"], "missing runtime dependency of alpha")

    def test_plan_reuses_compatible_dependency_and_replaces_unproved_or_incompatible_one(self):
        self.declare_dependency(bounded=True)
        with mock.patch.object(cli, "installed_product", return_value=True):
            for version, expected in [("1.2.0", ["alpha"]), ("1.9.2", ["alpha"]),
                                      ("1.1.9", ["beta", "alpha"]), ("2.0.0", ["beta", "alpha"]),
                                      (None, ["beta", "alpha"])]:
                with self.subTest(version=version), mock.patch.object(cli, "installed_version", return_value=version):
                    self.assertEqual(cli.plan(self.fixture.repo, ["alpha"])["products"], expected)

    def test_same_version_dependency_needs_its_explicitly_indexed_interface(self):
        self.declare_dependency(bounded=True)
        path = self.fixture.repo / "alpha/deployment/adapter.json"
        metadata = json.loads(path.read_text())
        metadata["runtime_contracts"] = {"beta": {"beta.setup": {"minimum": 1, "before": 2}}}
        path.write_text(json.dumps(metadata))
        path = self.fixture.repo / "pipeline/products/beta.sh"
        path.write_text(path.read_text() + "PROVIDERS='beta|beta|beta/chancery|3'\n")
        bundle = {"schema_version": 3, "provider": {"id": "beta", "release": "1.3.0"},
                  "entries": ["entries/setup.json"]}
        entry = {"id": "beta.setup", "contract_version": 1, "support": "supported"}
        self.fixture.write("beta/chancery/provider.json", json.dumps(bundle))
        self.fixture.write("beta/chancery/entries/setup.json", json.dumps(entry))
        self.fixture.commit()
        home = self.base / "home"
        installed = home / "Library/Application Support/Chancery/providers/beta"
        (installed / "entries").mkdir(parents=True)
        (installed / "entries/setup.json").write_text(json.dumps(entry))
        with mock.patch.object(cli, "installed_product", return_value=True), \
                mock.patch.object(cli, "installed_version", return_value="1.3.0"), \
                mock.patch.object(cli.pwd, "getpwuid", return_value=SimpleNamespace(pw_dir=str(home))):
            # A loose draft is not a supported installed interface.
            (installed / "provider.json").write_text(json.dumps({**bundle, "entries": []}))
            self.assertEqual(cli.plan(self.fixture.repo, ["alpha"])["products"], ["beta", "alpha"])
            (installed / "provider.json").write_text(json.dumps(bundle))
            self.assertEqual(cli.plan(self.fixture.repo, ["alpha"])["products"], ["alpha"])
            (installed / "entries/setup.json").write_text(json.dumps({**entry, "support": "unsupported"}))
            self.assertEqual(cli.plan(self.fixture.repo, ["alpha"])["products"], ["beta", "alpha"])

    def test_plan_rejects_incompatible_committed_candidates_before_maintenance(self):
        self.declare_dependency(bounded=True)
        self.fixture.write("beta/Cargo.toml", '[package]\nname="beta"\nversion="2.0.0"\n')
        self.fixture.commit()
        with mock.patch.object(cli, "installed_product", return_value=False):
            with self.assertRaisesRegex(cli.DeploymentError, "runtime releases are incompatible"):
                self.fixture.create()
        self.assertFalse((self.fixture.storage / "active").exists())

    def test_retained_dependency_is_inspected_without_holds_or_configuration(self):
        self.declare_dependency(bounded=True)
        adapter = FAKE_ADAPTER.replace('["beta"] if name == "alpha" else []', '[]')
        self.fixture.write("alpha/deployment/adapter.py", adapter)
        self.fixture.write("beta/deployment/adapter.py", adapter.replace(
            'else:\n    data = {}',
            '    data["installation"] = {"current":{"versions":{"beta":"1.3.0"},"release_id":"retained"}}\nelse:\n    data = {}'))
        self.fixture.commit()
        with mock.patch.object(cli, "installed_product", return_value=True), \
                mock.patch.object(cli, "installed_version", return_value="1.3.0"):
            path = self.fixture.create()
        outcome = cli.run_worker(path)
        self.assertEqual(outcome, 0, cli.read_json(path / "run.json"))
        self.assertEqual([item for item in self.observed(path) if item[0] == "beta"],
                         [["beta", "inspect"]])
        state = cli.read_json(path / "run.json")
        self.assertEqual(state["products"], ["alpha"])
        self.assertEqual(state["affected"], ["alpha"])

    def test_installation_remnants_are_not_treated_as_absent(self):
        home = self.base / "home"
        install = home / "Library/Application Support/Beta/install"
        install.mkdir(parents=True)
        (install / "transaction.json").write_text("{}")
        with mock.patch.object(cli.pwd, "getpwuid", return_value=SimpleNamespace(pw_dir=str(home))):
            self.assertTrue(cli.installed_product("beta", {"application": "Beta"}))
            self.assertFalse(cli.installed_product("alpha", {"application": "Alpha"}))
            path = home / ".local/bin/alpha"
            path.parent.mkdir(parents=True)
            path.symlink_to("missing-release")
            self.assertTrue(cli.installed_product("alpha", {"application": "Alpha"}))

    def test_settings_reach_only_the_owning_adapter_and_its_declared_consumer(self):
        self.declare_dependency()
        settings = {"alpha": {"enabled": False}, "beta": {"credential_file": "/private/key"}}
        with mock.patch.object(cli, "installed_product", return_value=False):
            path = cli.create_run(self.fixture.repo, ["alpha"], self.fixture.storage, settings=settings)
        self.assertEqual(cli.run_worker(path), 0)
        requests = [cli.read_json(item) for item in (path / "steps").glob("*-inspect.request.json")]
        alpha = next(item for item in requests if item["product"] == "alpha")
        beta = next(item for item in requests if item["product"] == "beta")
        self.assertEqual(alpha["settings"], {"enabled": False})
        self.assertEqual(alpha["dependency_settings"], {"beta": settings["beta"]})
        self.assertEqual(beta["settings"], settings["beta"])
        self.assertEqual(beta["dependency_settings"], {})
        self.assertIn("beta", alpha["dependency_candidates"])

    def test_activation_failure_reholds_and_drains_before_configuration_recovery(self):
        path = self.fixture.create()
        cli.durable_json(path / "fault.json", {"fail": ["alpha", "activate"]})
        self.assertEqual(cli.run_worker(path), 1)
        operations = self.observed(path)
        first_activation = operations.index(["alpha", "activate"])
        recovery = operations.index(["alpha", "recover"])
        self.assertEqual(operations[first_activation + 1:recovery],
                         [["beta", "hold"], ["beta", "drain"], ["alpha", "hold"], ["alpha", "drain"]])
        state = cli.read_json(path / "run.json")
        self.assertTrue(state["records"]["alpha"]["recovery_context"]["activation_started"])

    def test_admitted_consumer_can_finish_using_provider_before_provider_is_held(self):
        self.declare_dependency()
        adapter = FAKE_ADAPTER.replace('print(json.dumps({"schema":1,"status":status',
            'if name == "alpha" and op == "drain" and (run / "held-beta").exists():\n'
            '    status = "stopped"\n'
            'print(json.dumps({"schema":1,"status":status')
        self.fixture.write("alpha/deployment/adapter.py", adapter)
        self.fixture.commit()
        with mock.patch.object(cli, "installed_product", return_value=False):
            path = self.fixture.create()
        self.assertEqual(cli.run_worker(path), 0)
        observed = self.observed(path)
        self.assertLess(observed.index(["alpha", "drain"]), observed.index(["beta", "hold"]))

    def test_declarations_cover_every_embedded_product_library(self):
        crate_products = {}
        metadata = {}
        for path in (ROOT / "pipeline/products").glob("*.sh"):
            values = descriptor(path.read_text())
            name = "krisis" if values["PRODUCT_ID"] == "decisions" else values["PRODUCT_ID"]
            metadata[name] = json.loads((ROOT / values["PRODUCT_DIR"] / "deployment/adapter.json").read_text())
            for crate in values["CARGO_PACKAGES"].split():
                crate_products[crate] = name
        workspace = tomllib.loads((ROOT / "Cargo.toml").read_text())
        for member in workspace["workspace"]["members"]:
            manifest = tomllib.loads((ROOT / member / "Cargo.toml").read_text())
            consumer = crate_products.get(manifest["package"]["name"])
            if consumer is None:
                continue
            for key, dependency in manifest.get("dependencies", {}).items():
                provider = crate_products.get(dependency.get("package", key) if isinstance(dependency, dict) else key)
                if provider is not None and provider != consumer:
                    self.assertIn(consumer, metadata[provider].get("companions", []),
                                  f"{provider} must rebuild its embedded consumer {consumer}")

    def test_committed_candidates_satisfy_declared_runtime_contracts(self):
        revision = cli.git(ROOT, "rev-parse", "HEAD")
        products = cli.catalog(ROOT, revision)
        for consumer, item in products.items():
            for dependency, required in (item["metadata"] or {}).get("runtime_contracts", {}).items():
                with self.subTest(consumer=consumer, dependency=dependency):
                    available = cli.contract_versions(products[dependency], root=ROOT, revision=revision)
                    self.assertTrue(cli.compatible_contracts(available, required),
                                    f"{consumer} requires {required}; committed {dependency} provides {available}")

    def test_binary_adapter_uses_the_prepared_candidate(self):
        self.fixture.add_binary_adapter()
        planned = cli.plan(self.fixture.repo, ["usher"])
        self.assertIsNone(planned["catalog"]["usher"]["adapter"])
        self.assertFalse((self.fixture.repo / "usher/deployment/adapter.py").exists())
        path = self.fixture.create(("usher",))
        self.assertEqual(cli.run_worker(path), 0)
        self.assertEqual(self.observed(path), [["usher", operation] for operation in
                                             ("inspect", "hold", "drain", "apply", "configure", "release", "activate")])
        state = cli.read_json(path / "run.json")
        self.assertEqual(state["release_history_cleanup"]["arguments"], ["--product", "usher"])
        self.assertEqual((path / "worktree/target/release/usher-install").read_text(),
                         "later target contents must never execute")

    def test_binary_adapter_requires_its_selected_prepared_candidate(self):
        self.fixture.add_binary_adapter()
        path = self.fixture.create(("usher",))
        with cli.deployment_lock(self.fixture.storage) as lock_fd:
            run = cli.Run(path, lock_fd)
            with self.assertRaisesRegex(cli.DeploymentError, "prepared candidate"):
                run.adapter("usher", "inspect")
        self.assertFalse((path / "observed.jsonl").exists())

    def test_signed_prior_bundle_cannot_replace_the_retained_candidate(self):
        path = self.fixture.create()
        run = cli.Run(path, -1)
        policy = {"schema": 1, "macos": {"profile": "local", "certificate_sha1": "a" * 40,
                                          "keychain": "/example/login.keychain-db",
                                          "identifier_namespace": "local.cell"}}
        run.data.update(signing_policy=policy, source_key="sha256:selected-source")
        directory = path / "preparation/candidates/alpha"
        run.record("alpha").update(prepared=True, candidate_dir=str(directory),
                                   candidate_id="sha256:selected-candidate")
        selected = {"candidate_id": "sha256:selected-candidate", "product": "alpha",
                    "source_commit": run.data["source_commit"], "source_key": run.data["source_key"]}
        variants = ({"candidate_id": "sha256:prior-candidate", "source_commit": "b" * 40},
                    {"source_commit": "b" * 40}, {"source_key": "sha256:prior-source"},
                    {"product": "beta"})
        with mock.patch.object(candidate, "read_manifest", side_effect=AssertionError("must use verified metadata")), \
                mock.patch.object(run, "command") as command:
            for changed in variants:
                with self.subTest(changed=changed), mock.patch.object(
                        candidate, "verify_signatures", return_value=selected | changed) as verify:
                    with self.assertRaisesRegex(cli.DeploymentError, "retained deployment selection"):
                        run.adapter("alpha", "apply")
                    verify.assert_called_once_with(directory, policy)
            command.assert_not_called()
        self.assertEqual(run.record("alpha")["candidate_id"], selected["candidate_id"])

    def test_preparation_rejects_verified_candidate_from_another_source(self):
        path = self.fixture.create()
        with cli.deployment_lock(self.fixture.storage) as lock_fd:
            run = cli.Run(path, lock_fd)
            def other_source(directory, policy):
                return candidate.verify(directory) | {"source_commit": "b" * 40}
            with mock.patch.object(candidate, "verify_signatures", side_effect=other_source), \
                    self.assertRaisesRegex(cli.DeploymentError, "prepared source selection"):
                run.prepare()
        self.assertFalse(run.record("alpha").get("prepared"))
        self.assertFalse((path / "observed.jsonl").exists())

    def test_affected_binary_installer_is_prepared_without_selecting_upgrade(self):
        self.fixture.add_binary_adapter(name="nucleus")
        self.fixture.add_binary_adapter(name="requester", affected=("nucleus",))
        path = self.fixture.create(("requester",))
        self.assertEqual(cli.run_worker(path), 0)
        state = cli.read_json(path / "run.json")
        self.assertEqual(state["products"], ["requester"])
        self.assertEqual(state["prepared_products"], ["nucleus", "requester"])
        observed = self.observed(path)
        self.assertNotIn(["nucleus", "apply"], observed)
        self.assertLess(observed.index(["requester", "drain"]), observed.index(["nucleus", "hold"]))
        self.assertEqual([event for event in observed if event[1] == "release"][-1], ["nucleus", "release"])

    def test_binary_adapter_rejects_candidate_without_declared_installer(self):
        self.fixture.add_binary_adapter(stage_installer=False)
        path = self.fixture.create(("usher",))
        self.assertEqual(cli.run_worker(path), 1)
        self.assertIn("declared binary adapter", cli.read_json(path / "run.json")["detail"])
        self.assertFalse((path / "observed.jsonl").exists())


    def test_binary_adapter_uncertain_apply_uses_recovery_without_replaying_apply(self):
        self.fixture.add_binary_adapter()
        path = self.fixture.create(("usher",))
        cli.durable_json(path / "fault.json", {"invalid": ["usher", "apply"]})
        self.assertEqual(cli.run_worker(path), 1)
        self.assertEqual(self.observed(path).count(["usher", "apply"]), 1)
        self.assertEqual(self.observed(path)[-3:], [["usher", "recover"], ["usher", "release"], ["usher", "activate"]])
        self.assertEqual(cli.read_json(path / "run.json")["recovery"]["state"], "succeeded")

    def test_binary_adapter_replies_keep_the_existing_protocol_bound(self):
        self.fixture.add_binary_adapter()
        path = self.fixture.create(("usher",))
        cli.durable_json(path / "fault.json", {"detail": "x" * (cli.MAX_REPLY + 1)})
        self.assertEqual(cli.run_worker(path), 1)
        state = cli.read_json(path / "run.json")
        self.assertIn("reply exceeds the protocol bound", state["detail"])
        self.assertFalse(state["mutation_started"])
        self.assertEqual(self.observed(path), [["usher", "inspect"]])

    def test_binary_adapter_declaration_cannot_select_an_arbitrary_command(self):
        self.fixture.add_binary_adapter()
        self.fixture.write("usher/deployment/adapter.json", json.dumps({
            "schema": 1, "product": "usher", "dependencies": [], "adapter_binary": "../other"}))
        self.fixture.commit()
        with self.assertRaisesRegex(cli.DeploymentError, "unsupported committed binary adapter"):
            cli.plan(self.fixture.repo, ["usher"])

    def test_candidate_survives_later_target_overwrite(self) -> None:
        binary = self.fixture.repo / "target" / "release" / "alpha"
        binary.parent.mkdir(parents=True)
        binary.write_text("#!/bin/sh\nprintf 'alpha 1.0.0\\n'\n")
        binary.chmod(0o755)
        output = self.base / "candidate"
        with mock.patch.dict(os.environ, {"CARGO_TARGET_DIR": str(self.fixture.repo / "target")}):
            manifest = candidate.stage(self.fixture.repo, "alpha", output, "alpha|target/release/alpha|alpha")
        binary.write_text("later unrelated gate replaced this executable")
        self.assertEqual(candidate.read_manifest(output), manifest)
        staged = output / "bin" / "alpha"
        staged.chmod(0o755)

    def test_staged_bytes_from_a_failed_build_are_not_admitted(self) -> None:
        self.fixture.write("deployment/build.py", FAKE_BUILD + "\nsys.exit(75)\n")
        self.fixture.commit()
        path = self.fixture.create()
        self.assertEqual(cli.run_worker(path), 1)
        self.assertTrue((path / "preparation/candidates" / "alpha" / "candidate.json").exists())
        self.assertFalse(cli.read_json(path / "run.json")["records"].get("alpha", {}).get("prepared", False))
        self.assertFalse((path / "observed.jsonl").exists())

    def test_private_runner_checks_out_normal_source_modes_without_exposing_run_state(self) -> None:
        self.fixture.write("deployment/build.py", FAKE_BUILD.replace(
            "records = {}", 'assert (root / "alpha/ci.sh").stat().st_mode & 0o777 == 0o755\nassert (root / "alpha/packaging/manifest.txt").stat().st_mode & 0o777 == 0o644\nrecords = {}'))
        self.fixture.commit()
        previous_umask = os.umask(0o077)
        try:
            path = self.fixture.create()
            self.assertEqual(cli.run_worker(path), 0)
            self.assertEqual(os.umask(0o077), 0o077)
            self.assertEqual(path.stat().st_mode & 0o777, 0o700)
            self.assertEqual((path / "steps").stat().st_mode & 0o777, 0o700)
            self.assertEqual((path / "run.json").stat().st_mode & 0o777, 0o600)
            self.assertTrue(all(file.stat().st_mode & 0o777 == 0o600 for file in (path / "steps").iterdir()))
        finally:
            os.umask(previous_umask)

    def test_preparation_keeps_the_builders_source_identity(self) -> None:
        self.fixture.write("deployment/build.py", FAKE_BUILD.replace(
            "source_key = candidate.source_identity(root)",
            'root.joinpath("alpha/packaging/manifest.txt").write_text("dirty preparation\\n")\n'
            "source_key = candidate.source_identity(root)"))
        self.fixture.commit()
        path = self.fixture.create()
        self.assertEqual(cli.run_worker(path), 0)
        result = cli.read_json(path / "preparation/result.json")
        run = cli.read_json(path / "run.json")
        self.assertTrue(result["source_key"].startswith("dirty:"))
        self.assertEqual(run["source_key"], result["source_key"])
        for record in run["records"].values():
            if record.get("prepared"):
                self.assertEqual(record["build_receipt"]["source_key"], result["source_key"])

    def test_all_maintenance_holds_precede_draining_or_cutover(self) -> None:
        path = self.fixture.create()
        self.assertEqual(cli.run_worker(path), 0)
        observed = self.observed(path)
        self.assertEqual(observed, [["alpha", "inspect"], ["beta", "inspect"],
                                    ["beta", "hold"], ["beta", "drain"],
                                    ["alpha", "hold"], ["alpha", "drain"],
                                    ["alpha", "apply"], ["alpha", "configure"], ["beta", "configure"],
                                    ["alpha", "release"], ["beta", "release"],
                                    ["alpha", "activate"], ["beta", "activate"]])
        data = cli.read_json(path / "run.json")
        self.assertEqual(data["state"], "succeeded")
        self.assertNotIn("candidate_dir", data["records"]["beta"])
        self.assertTrue(data["records"]["alpha"]["build_receipt"])


    def test_internal_recovery_never_replays_an_uncertain_apply(self) -> None:
        path = self.fixture.create()
        cli.durable_json(path / "fault.json", {"invalid": ["alpha", "apply"]})
        self.assertEqual(cli.run_worker(path), 1)
        self.assertEqual(self.observed(path).count(["alpha", "apply"]), 1)
        records = cli.read_json(path / "run.json")["records"]
        self.assertTrue(records["alpha"]["recovery_context"]["apply_started"])
        self.assertFalse(records["alpha"]["recovery_context"]["applied"])
        self.assertFalse(records["beta"]["recovery_context"]["apply_started"])
        self.assertFalse((path / "held-alpha").exists())

    def test_lost_hold_reply_is_released_even_without_a_held_ledger_bit(self) -> None:
        path = self.fixture.create()
        cli.durable_json(path / "fault.json", {"invalid": ["alpha", "hold"]})
        self.assertEqual(cli.run_worker(path), 1)
        self.assertFalse((path / "held-alpha").exists())
        self.assertFalse((path / "held-beta").exists())
        data = cli.read_json(path / "run.json")
        self.assertEqual(data["state"], "stopped")
        self.assertEqual(data["recovery"]["state"], "succeeded")
        self.assertEqual(cli.maintenance_result(data), {"state": "released"})


    def test_cycle_is_rejected_and_not_silently_ordered(self) -> None:
        with self.assertRaisesRegex(cli.DeploymentError, "cycle"):
            cli.ordered(["alpha", "beta"], {"alpha": ["beta"], "beta": ["alpha"]})

    def test_foreground_success_removes_stale_workspace_and_current_artifacts(self) -> None:
        stale = self.fixture.create()
        self.assertEqual(cli.run_worker(stale), 0)
        self.assertTrue((stale / "worktree").exists())
        result = cli.start(self.fixture.repo, ["alpha"], self.fixture.storage)
        self.assertEqual(result["exit_code"], 0)
        self.assertEqual(result["state"], "succeeded")
        self.assertEqual(result["maintenance"], {"state": "released"})
        self.assertEqual(result["recovery"], {"state": "not_needed"})
        self.assertEqual(result["cleanup"], {"releases": "succeeded", "workspace": "removed"})
        self.assertFalse((self.fixture.storage / "active").exists())
        self.assertEqual(self.fixture.git("worktree", "list", "--porcelain").count("worktree "), 1)
        self.assertEqual([path.name for path in self.fixture.storage.iterdir()], ["deployment.lock"])

    def test_foreground_failure_removes_sealed_candidates_logs_and_worktree(self) -> None:
        self.fixture.write("deployment/build.py", FAKE_BUILD + "\nsys.exit(75)\n")
        self.fixture.commit()
        result = cli.start(self.fixture.repo, ["alpha"], self.fixture.storage)
        self.assertEqual(result["exit_code"], 1)
        self.assertEqual(result["state"], "stopped")
        self.assertEqual(result["maintenance"], {"state": "not_started"})
        self.assertEqual(result["recovery"], {"state": "not_needed"})
        self.assertFalse((self.fixture.storage / "active").exists())
        self.assertEqual(self.fixture.git("worktree", "list", "--porcelain").count("worktree "), 1)

    def test_cleanup_failure_does_not_recover_or_reapply_installed_products(self) -> None:
        self.fixture.write("deployment/cleanup.py", "import sys\nprint('cleanup blocked', file=sys.stderr)\nsys.exit(1)\n")
        self.fixture.commit()
        path = self.fixture.create()
        self.assertEqual(cli.run_worker(path), 1)
        self.assertEqual(cli.read_json(path / "run.json")["state"], "cleanup_failed")
        observed = self.observed(path)
        self.assertFalse(any(operation == "recover" for _, operation in observed))
        self.assertEqual(observed.count(["alpha", "apply"]), 1)
        self.assertFalse((path / "held-alpha").exists())
        self.assertFalse((path / "held-beta").exists())

    def foreground(self, *, verbose=False):
        script = (f"import sys\nsys.path.insert(0, {str(self.fixture.repo)!r})\n"
                  "from pathlib import Path\nfrom deployment import cli\n"
                  f"cli.workspace.root=lambda: Path({str(self.base)!r})\n"
                  f"result=cli.start(Path({str(self.fixture.repo)!r}), ['alpha'], "
                  f"Path({str(self.fixture.storage)!r}), verbose={verbose!r})\n"
                  "cli.print_result(result)\nraise SystemExit(result['exit_code'])\n")
        return subprocess.run([sys.executable, "-B", "-c", script], cwd=ROOT,
                              capture_output=True, text=True, timeout=30)

    def test_foreground_default_is_one_result_and_verbose_only_adds_stderr_progress(self):
        quiet = self.foreground()
        self.assertEqual(quiet.returncode, 0, quiet.stderr)
        self.assertEqual(len(quiet.stdout.splitlines()), 1)
        self.assertEqual(quiet.stderr, "")
        verbose = self.foreground(verbose=True)
        self.assertEqual(verbose.returncode, 0, verbose.stderr)
        self.assertEqual(len(verbose.stdout.splitlines()), 1)
        self.assertIn('"event":"starting"', verbose.stderr)
        self.assertNotIn('"response"', verbose.stderr)
        self.assertEqual(json.loads(verbose.stdout)["maintenance"], {"state": "released"})

    def test_original_and_recovery_evidence_survive_cleanup_with_one_shared_budget(self):
        fault = {"fail": ["alpha", "apply"], "invalid": ["alpha", "recover"],
                 "detail": "D" * 30000,
                 "stderr": {"apply": "original-evidence\n" + "A" * 6000 + "\noriginal-tail",
                            "recover": "recovery-evidence\n" + "R" * 6000 + "\nrecovery-tail"}}
        self.fixture.write("alpha/deployment/adapter.py", FAKE_ADAPTER.replace(
            'fault = json.loads(fault_path.read_text()) if fault_path.exists() else {}', f'fault = {fault!r}'))
        self.fixture.commit()
        result = self.foreground()
        self.assertEqual(result.returncode, 1)
        self.assertEqual(len(result.stdout.splitlines()), 1)
        self.assertLessEqual(len(result.stderr.encode()), cli.MAX_DIAGNOSTICS)
        self.assertIn("failure diagnostics", result.stderr)
        self.assertIn("recovery diagnostics", result.stderr)
        self.assertIn("original-tail", result.stderr)
        self.assertIn("recovery-tail", result.stderr)
        self.assertIn("uncertain output", result.stderr)
        self.assertLess(len(result.stdout.encode()), 3000)
        final = json.loads(result.stdout)
        self.assertEqual(final["recovery"]["state"], "failed")
        self.assertEqual(final["maintenance"]["products"], {"alpha": "retained", "beta": "retained"})
        self.assertEqual(final["cleanup"]["workspace"], "retained_for_recovery")
        self.assertNotIn("DDDD", result.stderr)
        self.assertTrue((self.fixture.storage / "active").exists())

    def test_lost_hold_and_release_outcomes_are_uncertain_until_a_proved_release(self):
        for operation in ("hold", "release"):
            with self.subTest(operation=operation):
                events = [{"state": "starting", "operation": "hold", "product": "alpha"}]
                if operation == "release":
                    events += [{"state": "outcome", "operation": "hold", "product": "alpha", "returncode": 0,
                                "response": {"status": "held"}},
                               {"state": "starting", "operation": "release", "product": "alpha"}]
                data = {"run_id": "owner", "events": events}
                self.assertEqual(cli.maintenance_result(data)["products"], {"alpha": "uncertain"})
                events.append({"state": "outcome", "operation": "release", "product": "alpha", "returncode": 0,
                               "response": {"status": "released"}})
                self.assertEqual(cli.maintenance_result(data), {"state": "released"})

    def test_workspace_cleanup_failure_preserves_installation_and_hold_result(self):
        original = cli.cleanup_active
        def cleanup(storage):
            if (storage / "active" / "run.json").exists():
                raise OSError("fixture cleanup refused")
            original(storage)
        with mock.patch.object(cli, "cleanup_active", side_effect=cleanup):
            result = cli.start(self.fixture.repo, ["alpha"], self.fixture.storage)
        self.assertEqual(result["exit_code"], 1)
        self.assertEqual(result["state"], "cleanup_failed")
        self.assertEqual(result["maintenance"], {"state": "released"})
        self.assertEqual(result["cleanup"]["workspace"], "failed")
        self.assertEqual(result["cleanup"]["releases"], "succeeded")
        with cli.deployment_lock(self.fixture.storage):
            original(self.fixture.storage)

    def test_heartbeat_is_delayed_and_limited_to_once_per_minute(self):
        path = self.fixture.create()
        with mock.patch.object(cli.time, "monotonic", return_value=100):
            run = cli.Run(path, -1)
        output = io.StringIO()
        with contextlib.redirect_stderr(output):
            for current in (100, 159, 160, 160.1, 219, 220):
                with mock.patch.object(cli.time, "monotonic", return_value=current):
                    run.heartbeat()
        self.assertEqual(len(output.getvalue().splitlines()), 2)


    def add_nucleus(self) -> None:
        fixture = self.fixture
        fixture.write("pipeline/products/nucleus.sh", "PRODUCT_ID=nucleus\nPRODUCT_DIR=nucleus\nDEPLOY_PROFILE=custom\n")
        fixture.write("nucleus/deployment/adapter.json", json.dumps({"schema": 1, "product": "nucleus", "dependencies": []}))
        fixture.write("nucleus/deployment/adapter.py", FAKE_ADAPTER)
        fixture.write("alpha/deployment/adapter.py", FAKE_ADAPTER.replace('["beta"] if name == "alpha"', '["beta", "nucleus"] if name == "alpha"'))
        fixture.commit()

    def test_internal_recovery_proves_all_products_and_releases_nucleus_last(self) -> None:
        self.add_nucleus()
        path = self.fixture.create()
        cli.durable_json(path / "fault.json", {"fail": ["nucleus", "configure"]})
        self.assertEqual(cli.run_worker(path), 1)
        observed = self.observed(path)
        first_release = min(index for index, event in enumerate(observed) if event[1] == "release")
        self.assertTrue(all(index < first_release for index, event in enumerate(observed) if event[1] == "recover"))
        self.assertEqual([event for event in observed if event[1] == "release"][-1], ["nucleus", "release"])

    def test_waiting_drain_stays_in_the_same_deployment(self):
        path = self.fixture.create()
        cli.durable_json(path / "fault.json", {"wait": "alpha"})
        self.assertEqual(cli.run_worker(path), 0)
        observed = self.observed(path)
        self.assertEqual(observed.count(["alpha", "drain"]), 2)
        self.assertEqual(observed.count(["alpha", "hold"]), 1)
        self.assertEqual(observed.count(["alpha", "apply"]), 1)
        self.assertNotIn(["alpha", "recover"], observed)

    def test_unfinished_transaction_is_retained_and_reconciled_before_next_start(self):
        path = self.fixture.create()
        cli.durable_json(path / "fault.json", {"fail": ["alpha", "configure"], "recovery_safe": False})
        self.assertEqual(cli.run_worker(path), 1)
        with self.assertRaisesRegex(cli.DeploymentError, "retained"):
            cli.cleanup_active(self.fixture.storage)
        (path / "fault.json").unlink()
        with cli.deployment_lock(self.fixture.storage) as lock_fd:
            cli.reconcile_active(self.fixture.storage, lock_fd)
        state = cli.read_json(path / "run.json")
        self.assertEqual(state["recovery"]["state"], "succeeded")
        self.assertEqual(self.observed(path).count(["alpha", "apply"]), 1)
        cli.cleanup_active(self.fixture.storage)
        self.assertFalse(path.exists())

    def test_activation_failure_is_recovered_after_configuration(self):
        path = self.fixture.create()
        cli.durable_json(path / "fault.json", {"fail": ["alpha", "activate"]})
        self.assertEqual(cli.run_worker(path), 1)
        state = cli.read_json(path / "run.json")
        self.assertEqual(state["recovery"]["state"], "failed")
        self.assertTrue(cli.unresolved(state))
        observed = self.observed(path)
        first_activation = observed.index(["alpha", "activate"])
        self.assertTrue(all(index < first_activation for index, event in enumerate(observed) if event[1] == "configure"))

    def test_competing_start_cannot_create_or_delete_an_active_workspace(self) -> None:
        path = self.fixture.create()
        with cli.deployment_lock(self.fixture.storage):
            with self.assertRaisesRegex(cli.DeploymentError, "holds the deployment lock"):
                cli.start(self.fixture.repo, ["alpha"], self.fixture.storage)
        self.assertTrue((path / "run.json").exists())
        self.assertFalse((path / "observed.jsonl").exists())

    def worker_process(self, path: Path, log):
        with cli.deployment_lock(self.fixture.storage) as lock_fd:
            return subprocess.Popen([sys.executable, str(path / "source" / "deployment" / "cli.py"),
                                     "_worker", str(path), str(lock_fd)], stdout=log, stderr=log,
                                    pass_fds=(lock_fd,))

    def test_active_adapter_retains_global_lock_if_runner_dies(self) -> None:
        path = self.fixture.create()
        cli.durable_json(path / "fault.json", {"sleep": ["alpha", "apply"]})
        log = (path / "worker-test.log").open("wb")
        process = self.worker_process(path, log)
        child_pid = None
        try:
            deadline = time.monotonic() + 15
            while not (path / "child-started").exists() and time.monotonic() < deadline:
                if process.poll() is not None:
                    self.fail((path / "worker-test.log").read_text())
                time.sleep(0.02)
            self.assertTrue((path / "child-started").exists())
            child_pid = cli.read_json(path / "run.json")["active_operation"]["pid"]
            process.kill()
            process.wait(timeout=5)
            with self.assertRaisesRegex(cli.DeploymentError, "holds the deployment lock"):
                with cli.deployment_lock(self.fixture.storage):
                    pass
            self.assertNotIn(["alpha", "release"], self.observed(path))
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)
            if child_pid:
                with contextlib.suppress(ProcessLookupError):
                    os.kill(child_pid, signal.SIGKILL)
            log.close()

    def test_deployer_descendant_retains_lock_after_both_supervisors_die(self) -> None:
        path = self.fixture.create()
        cli.durable_json(path / "fault.json", {"spawn_sleep": ["alpha", "apply"]})
        with (path / "worker-test.log").open("wb") as log:
            process = self.worker_process(path, log)
            child_pid = descendant_pid = None
            try:
                deadline = time.monotonic() + 15
                marker = path / "descendant-started"
                while not marker.exists() and time.monotonic() < deadline:
                    if process.poll() is not None:
                        self.fail((path / "worker-test.log").read_text())
                    time.sleep(0.02)
                self.assertTrue(marker.exists())
                descendant_pid = int(marker.read_text())
                child_pid = cli.read_json(path / "run.json")["active_operation"]["pid"]
                process.kill()
                process.wait(timeout=5)
                os.kill(child_pid, signal.SIGKILL)
                with self.assertRaisesRegex(cli.DeploymentError, "holds the deployment lock"):
                    with cli.deployment_lock(self.fixture.storage):
                        pass
                self.assertNotIn(["alpha", "release"], self.observed(path))
                with self.assertRaisesRegex(cli.DeploymentError, "holds the deployment lock"):
                    cli.start(self.fixture.repo, ["alpha"], self.fixture.storage)
                self.assertTrue((path / "source").exists())
                self.assertTrue((path / "worktree").exists())
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait(timeout=5)
                for pid in (child_pid, descendant_pid):
                    if pid:
                        with contextlib.suppress(ProcessLookupError):
                            os.kill(pid, signal.SIGKILL)


if __name__ == "__main__":
    unittest.main()
