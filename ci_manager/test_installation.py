"""Check manager release inventories and compatible selection without file hashes."""

from contextlib import ExitStack
import copy
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

from ci_manager import installation
from ci_manager.integrations import TransportError
from ci_manager.storage import ManagerError, Store


class InstallationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.programs = self.root / "programs"
        self.source = self.root / "source/ci_manager"
        files = ["ci_manager/client.py", "ci_manager/chancery/provider.json",
                 "deployment/__init__.py", "deployment/signing.py", "deployment/build.py",
                 "deployment/candidate.py", "deployment/inventory.py", "ci_broker/__init__.py",
                 "ci_broker/client.py", "ci_broker/broker.py"]
        for name in files:
            path = self.source.parent / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("{}\n" if path.suffix == ".json" else "# fixture\n")
        self.python = Path(sys.executable).resolve()
        self.addCleanup(mock.patch.stopall)
        mock.patch.object(installation, "program_root", return_value=self.programs).start()

    def tearDown(self):
        # Published releases are read-only. Make fixture directories removable.
        for directory in [self.root, *self.root.rglob("*")]:
            if directory.is_dir() and not directory.is_symlink():
                directory.chmod(0o700)

    def release(self):
        return installation._prepare_release(self.source, self.python)

    def replace_manifest(self, release, manifest):
        release.chmod(0o700)
        installation._write_file(release / "manifest.json", installation._json(manifest), 0o444)
        release.chmod(0o555)

    def legacy_release(self):
        release = self.release()
        legacy = release.with_name("a" * 64)
        release.rename(legacy)
        manifest = json.loads((legacy / "manifest.json").read_bytes())
        manifest["recipe"]["schema_version"] = 1
        manifest["recipe"].pop("runtime_root", None)
        for record in manifest["recipe"]["files"].values():
            record["sha256"] = "retained legacy metadata"
        manifest["wrapper_sha256"] = "retained legacy metadata"
        self.replace_manifest(legacy, manifest)
        wrapper = legacy / "bin/cell-ci"
        wrapper.chmod(0o755)
        wrapper.write_bytes(installation._launcher(str(self.python), legacy))
        wrapper.chmod(0o555)
        return legacy

    def installed_paths(self):
        previous = self.release()
        paths = {"programs": self.programs, "current": self.programs / "current", "runtime": self.programs / "runtime",
                 "wrapper": self.root / "bin/cell-ci", "provider": self.root / "providers/ci-manager",
                 "plist": self.root / "agents/manager.plist"}
        installation._link(paths["current"], f"releases/{previous.name}")
        installation._link(paths["wrapper"], str(paths["current"] / "bin/cell-ci"))
        installation._link(paths["provider"], str(paths["current"] / "ci_manager/chancery"))
        installation._write_file(paths["plist"], installation._plist(previous))
        return previous, paths

    def prepared_repair(self):
        store = Store(self.root / "state", create=True)
        self.addCleanup(store.db.close)
        directory = store.root / "jobs/fixture"
        directory.mkdir(parents=True)
        worktree = directory / "worktree"
        worktree.mkdir()
        candidate = "b" * 40
        request_path = directory / "repair-1.request.json"
        request = {"version": 1, "id": "ci-fixture-repair-1",
                   "requester": {"program": "ci-manager", "id": "fixture"},
                   "prompt": "Frozen prompt from the previous worker.",
                   "invocation": {"cwd": str(worktree), "workspaceAccess": "read-only"}}
        request_path.write_text(json.dumps(request))
        validator = directory / "validation-0.request.json"
        validator.write_text(json.dumps({
            "command": [str(self.python), str(worktree / "pipeline/select_changes.py"), "run",
                        "--base", "a" * 40, "--candidate", candidate, "--json"],
            "cwd": str(worktree), **{key: str(directory / f"validation-0.{suffix}")
                for key, suffix in (("stdout", "stdout"), ("stderr", "stderr"),
                                    ("started", "started.json"), ("result", "result.json"))}}))
        (directory / "validation-0.result.json").write_text(json.dumps(
            {"request": str(validator), "exit_code": 1}))
        data = {"base_commit": "a" * 40, "candidate_commit": candidate,
                "stopped_phase": "repair_wait", "model_unresolved": False,
                "policy": {"luna_attempts": 3, "terra_attempts": 1},
                "attempts": [{"number": 1, "nucleus_job_id": request["id"],
                              "request": str(request_path), "parent": candidate}],
                "validations": [{"candidate": candidate,
                                 "receipt": str(directory / "validation-0.json")}]}
        for identity, phase in (("fixture", "blocked"), ("queued", "queued")):
            store.db.execute("""INSERT INTO jobs
                (id,submission_key,phase,created,updated,data) VALUES (?,?,?,?,?,?)""",
                (identity, identity, phase, 1.0, 1.0, json.dumps(data if phase == "blocked" else {})))
        return store, store.job("fixture"), request_path

    def host_fixture(self, stack, store, paths):
        stack.enter_context(mock.patch.object(installation.sys, "platform", "darwin"))
        stack.enter_context(mock.patch.object(installation, "__file__", str(self.source / "installation.py")))
        stack.enter_context(mock.patch.object(installation, "_paths", return_value=paths))
        stack.enter_context(mock.patch.object(installation, "state_root", return_value=store.root))

    def journal(self, store):
        return [tuple(row) for row in store.db.execute("SELECT * FROM jobs ORDER BY sequence")]

    def test_prepared_repair_requires_authoritative_absence_and_retains_frozen_request(self):
        store, job, request_path = self.prepared_repair()
        retained = request_path.read_bytes()
        with mock.patch.object(installation, "NucleusClient") as client:
            client.return_value.get.return_value = None
            installation._require_idle(store, allow_install=True)
            client.return_value.get.assert_called_once_with("ci-fixture-repair-1")
            with self.assertRaises(ManagerError):
                installation._require_idle(store)
        self.assertEqual(request_path.read_bytes(), retained)
        self.assertEqual(store.job("fixture"), job)

    def test_install_refuses_unresolved_or_uncorrelated_repair_evidence(self):
        store, job, request_path = self.prepared_repair()
        original = copy.deepcopy(job)
        variants = ({"stopped_phase": "checking"}, {"model_unresolved": True},
                    {"model_unresolved": None}, {"accepted": True}, {"acceptance_intent": {"candidate": "x"}},
                    {"deployment_request": {"id": "x"}}, {"deployment_result": {"state": "lost"}},
                    {"attempts": []}, {"candidate_commit": "c" * 40})
        with mock.patch.object(installation, "NucleusClient") as client:
            for changed in variants:
                with self.subTest(changed=changed):
                    self.assertFalse(installation._prepared_repair_not_admitted(store, original | changed))
            for changed in ({"number": 2}, {"number": True}, {"nucleus_job_id": "other"},
                            {"request": str(request_path.parent / "other.json")}, {"parent": "c" * 40},
                            {"state": "completed"}, {"terminal": {"attempt_state": "lost"}},
                            {"patch": "/retained.patch"}):
                with self.subTest(attempt=changed):
                    changed_job = copy.deepcopy(original)
                    changed_job["attempts"][-1].update(changed)
                    self.assertFalse(installation._prepared_repair_not_admitted(store, changed_job))
            store.set("recovery_request", "fixture")
            self.assertFalse(installation._prepared_repair_not_admitted(store, job))
            store.set("recovery_request", None)
            (request_path.parent / "validation-0.result.json").unlink()
            self.assertFalse(installation._prepared_repair_not_admitted(store, job))
            client.assert_not_called()

    def test_install_refuses_changed_frozen_request_or_missing_request(self):
        store, job, request_path = self.prepared_repair()
        request = json.loads(request_path.read_bytes())
        variants = ({"id": "another"}, {"requester": {"program": "ci-manager", "id": "other"}},
                    {"invocation": {"cwd": "/other", "workspaceAccess": "read-only"}},
                    {"invocation": {"cwd": str(request_path.parent / "worktree"), "workspaceAccess": "write"}})
        with mock.patch.object(installation, "NucleusClient") as client:
            for changed in variants:
                with self.subTest(changed=changed):
                    request_path.write_text(json.dumps(request | changed))
                    self.assertFalse(installation._prepared_repair_not_admitted(store, job))
            request_path.unlink()
            self.assertFalse(installation._prepared_repair_not_admitted(store, job))
            client.assert_not_called()

    def test_install_refuses_admitted_lost_and_unavailable_nucleus_jobs(self):
        store, job, _ = self.prepared_repair()
        with mock.patch.object(installation, "NucleusClient") as client:
            for state in ("accepted", "running", "completed", "lost"):
                with self.subTest(state=state):
                    client.return_value.get.return_value = {"summary": {"state": state}}
                    self.assertFalse(installation._prepared_repair_not_admitted(store, job))
            client.return_value.get.side_effect = TransportError("unavailable")
            self.assertFalse(installation._prepared_repair_not_admitted(store, job))

    def test_install_rechecks_absence_and_preserves_active_and_queued_jobs(self):
        previous, paths = self.installed_paths()
        store, job, request_path = self.prepared_repair()
        before, retained = self.journal(store), request_path.read_bytes()
        events = []
        with ExitStack() as stack:
            self.host_fixture(stack, store, paths)
            client = stack.enter_context(mock.patch.object(installation, "NucleusClient"))
            client.return_value.get.side_effect = lambda identity: events.append("get")
            stack.enter_context(mock.patch.object(installation, "_stop_if_loaded",
                                                  side_effect=lambda plist: events.append("stop") or True))
            launch = stack.enter_context(mock.patch.object(installation, "_launchctl"))
            result = installation.install()
        self.assertEqual(events, ["get", "stop", "get"])
        self.assertEqual(installation._selected_release(paths).name, result["release"])
        self.assertNotEqual(result["release"], previous.name)
        self.assertEqual(self.journal(store), before)
        self.assertEqual(store.job("fixture"), job)
        self.assertEqual(request_path.read_bytes(), retained)
        self.assertTrue(store.get("paused"))
        launch.assert_called_once_with("bootstrap", f"gui/{installation.os.getuid()}", str(paths["plist"]))

    def test_install_refuses_before_stop_and_restores_service_after_failed_recheck(self):
        previous, paths = self.installed_paths()
        store, _, request_path = self.prepared_repair()
        before, retained = self.journal(store), request_path.read_bytes()
        with ExitStack() as stack:
            self.host_fixture(stack, store, paths)
            client = stack.enter_context(mock.patch.object(installation, "NucleusClient"))
            stop = stack.enter_context(mock.patch.object(installation, "_stop_if_loaded", return_value=True))
            prepare = stack.enter_context(mock.patch.object(installation, "_prepare_release"))
            launch = stack.enter_context(mock.patch.object(installation, "_launchctl"))
            client.return_value.get.side_effect = TransportError("unavailable")
            with self.assertRaises(ManagerError):
                installation.install()
            stop.assert_not_called()
            client.return_value.get.side_effect = [None, {"summary": {"state": "running"}}]
            with self.assertRaisesRegex(ManagerError, "installation failed"):
                installation.install()
            stop.assert_called_once()
            prepare.assert_not_called()
            launch.assert_called_once_with("bootstrap", f"gui/{installation.os.getuid()}", str(paths["plist"]))
        self.assertEqual(installation._selected_release(paths), previous)
        self.assertEqual(self.journal(store), before)
        self.assertEqual(request_path.read_bytes(), retained)

    def test_service_stop_keeps_strict_idle_requirement(self):
        _, paths = self.installed_paths()
        store, _, _ = self.prepared_repair()
        with ExitStack() as stack:
            self.host_fixture(stack, store, paths)
            client = stack.enter_context(mock.patch.object(installation, "NucleusClient"))
            stop = stack.enter_context(mock.patch.object(installation, "_stop_if_loaded"))
            with self.assertRaises(ManagerError):
                installation.service("stop")
            client.assert_not_called()
            stop.assert_not_called()

    def test_failed_repair_maintenance_install_restores_selection_and_retains_job(self):
        previous, paths = self.installed_paths()
        old_plist = paths["plist"].read_bytes()
        store, _, request_path = self.prepared_repair()
        before, retained = self.journal(store), request_path.read_bytes()
        with ExitStack() as stack:
            self.host_fixture(stack, store, paths)
            client = stack.enter_context(mock.patch.object(installation, "NucleusClient"))
            client.return_value.get.return_value = None
            stack.enter_context(mock.patch.object(installation, "_stop_if_loaded", side_effect=[True, False]))
            launch = stack.enter_context(mock.patch.object(
                installation, "_launchctl", side_effect=[ManagerError("fixture bootstrap failure"), None]))
            worker_lock = stack.enter_context(mock.patch.object(
                installation, "_worker_lock_after_stop", wraps=installation._worker_lock_after_stop))
            with self.assertRaisesRegex(ManagerError, "installation failed"):
                installation.install()
        self.assertEqual(client.return_value.get.call_count, 2)
        self.assertEqual(launch.call_count, 2)
        self.assertEqual(worker_lock.call_count, 2)
        self.assertEqual(installation._selected_release(paths), previous)
        self.assertEqual(paths["plist"].read_bytes(), old_plist)
        self.assertEqual(self.journal(store), before)
        self.assertEqual(request_path.read_bytes(), retained)
        self.assertTrue(store.get("paused"))

    def install_idle_fixture(self, paths, *, launch_error=None, loaded=False):
        state = self.root / "idle-state"
        state.mkdir(mode=0o700, exist_ok=True)
        store = mock.Mock()
        store.get.return_value = True
        store.active.return_value = None
        with ExitStack() as stack:
            stack.enter_context(mock.patch.object(installation.sys, "platform", "darwin"))
            stack.enter_context(mock.patch.object(installation, "__file__", str(self.source / "installation.py")))
            stack.enter_context(mock.patch.object(installation, "_paths", return_value=paths))
            stack.enter_context(mock.patch.object(installation, "state_root", return_value=state))
            stack.enter_context(mock.patch.object(installation, "Store", return_value=store))
            stack.enter_context(mock.patch.object(installation, "_loaded", return_value=loaded))
            stack.enter_context(mock.patch.object(installation, "_launchctl", side_effect=launch_error))
            return installation.install()

    def test_repeated_install_keeps_actual_launcher_and_module_paths(self):
        _, paths = self.installed_paths()
        first = self.install_idle_fixture(paths)
        runtime = paths["runtime"]
        launcher = runtime / "bin/cell-ci"
        module = runtime / "ci_manager/client.py"
        first_launcher = launcher.read_bytes()
        first_module = module.read_bytes()
        self.source.joinpath("client.py").write_text("# updated manager\n")
        second = self.install_idle_fixture(paths)
        self.assertNotEqual(first["release"], second["release"])
        self.assertEqual(paths["wrapper"].resolve(), launcher.resolve())
        self.assertFalse(launcher.is_symlink())
        self.assertFalse(module.is_symlink())
        self.assertEqual(launcher.read_bytes(), first_launcher)
        self.assertNotEqual(module.read_bytes(), first_module)
        plist = installation.plistlib.loads(paths["plist"].read_bytes())
        self.assertEqual(plist["ProgramArguments"], [str(launcher), "worker"])
        self.assertEqual(plist["WorkingDirectory"], str(runtime))
        self.assertIn(str(runtime / "ci_manager/client.py").encode(), first_launcher)
        self.assertNotIn(b"/releases/", first_launcher)
        self.assertEqual(paths["provider"].resolve(),
                         (self.programs / "releases" / second["release"] / "ci_manager/chancery").resolve())
        self.assertEqual((self.programs / "releases" / first["release"] / "ci_manager/client.py").read_bytes(),
                         first_module)

    def test_failed_update_restores_runtime_bytes_and_fixed_selectors(self):
        _, paths = self.installed_paths()
        first = self.install_idle_fixture(paths)
        release = installation._selected_release(paths)
        before = installation._runtime_snapshot(paths["runtime"], release)
        old_plist = paths["plist"].read_bytes()
        old_wrapper = installation.os.readlink(paths["wrapper"])
        old_provider = installation.os.readlink(paths["provider"])
        self.source.joinpath("client.py").write_text("# rejected update\n")
        with self.assertRaisesRegex(ManagerError, "installation failed"):
            self.install_idle_fixture(paths, launch_error=ManagerError("bootstrap failed"))
        self.assertEqual(installation._selected_release(paths).name, first["release"])
        self.assertEqual(installation._runtime_snapshot(paths["runtime"], release), before)
        self.assertEqual(installation.os.readlink(paths["wrapper"]), old_wrapper)
        self.assertEqual(installation.os.readlink(paths["provider"]), old_provider)
        self.assertEqual(paths["plist"].read_bytes(), old_plist)

    def test_partial_runtime_publication_restores_files_before_selector_switch(self):
        _, paths = self.installed_paths()
        first = self.install_idle_fixture(paths)
        release = installation._selected_release(paths)
        before = installation._runtime_snapshot(paths["runtime"], release)
        original = installation._write_file
        copies = 0

        def fail_once(path, contents, mode=0o600):
            nonlocal copies
            if path.is_relative_to(paths["runtime"]):
                copies += 1
                if copies == 2:
                    raise ManagerError("runtime file write failed")
            original(path, contents, mode)

        self.source.joinpath("client.py").write_text("# partial update\n")
        with mock.patch.object(installation, "_write_file", side_effect=fail_once):
            with self.assertRaisesRegex(ManagerError, "installation failed"):
                self.install_idle_fixture(paths)
        self.assertGreater(copies, 2)
        self.assertEqual(installation._selected_release(paths).name, first["release"])
        self.assertEqual(installation._runtime_snapshot(paths["runtime"], release), before)

    def test_service_checks_selected_runtime_and_refuses_partial_files(self):
        _, paths = self.installed_paths()
        result = self.install_idle_fixture(paths)
        with ExitStack() as stack:
            stack.enter_context(mock.patch.object(installation.sys, "platform", "darwin"))
            stack.enter_context(mock.patch.object(installation, "_paths", return_value=paths))
            stack.enter_context(mock.patch.object(installation, "_loaded", return_value=False))
            status = installation.service("status")
            self.assertEqual(status["release"], result["release"])
            self.assertTrue(status["installed"])
            paths["runtime"].joinpath("ci_manager/client.py").unlink()
            with self.assertRaisesRegex(ManagerError, "runtime selection"):
                installation.service("start")

    def test_install_does_not_restart_an_incoherent_runtime(self):
        _, paths = self.installed_paths()
        result = self.install_idle_fixture(paths)
        paths["runtime"].joinpath("ci_manager/client.py").unlink()
        operations = []

        def launch(*arguments, **_options):
            operations.append(arguments[0])

        with self.assertRaisesRegex(ManagerError, "paused installation needs recovery"):
            self.install_idle_fixture(paths, loaded=True, launch_error=launch)
        self.assertEqual(operations, ["bootout"])
        self.assertEqual(installation._selected_release(paths).name, result["release"])
        self.assertFalse(paths["runtime"].joinpath("ci_manager/client.py").exists())

    def test_version_two_archive_remains_readable(self):
        release = self.release()
        manifest = json.loads((release / "manifest.json").read_bytes())
        manifest["recipe"]["schema_version"] = 2
        manifest["recipe"].pop("runtime_root")
        self.replace_manifest(release, manifest)
        wrapper = release / "bin/cell-ci"
        wrapper.chmod(0o755)
        wrapper.write_bytes(installation._launcher(str(self.python), release))
        wrapper.chmod(0o555)
        self.assertEqual(installation._verify_release(release)["recipe"]["schema_version"], 2)

    def test_new_releases_have_independent_opaque_ids_and_mode_inventory(self):
        first, second = self.release(), self.release()
        self.assertNotEqual(first.name, second.name)
        self.assertRegex(first.name, r"^[0-9a-f]{32}$")
        manifest = installation._verify_release(first)
        self.assertEqual(set(manifest), {"recipe"})
        self.assertEqual(manifest["recipe"]["schema_version"], 3)
        self.assertTrue(all(set(record) == {"mode"}
                            for record in manifest["recipe"]["files"].values()))
        self.assertEqual((first / "ci_manager/client.py").read_bytes(),
                         (self.source / "client.py").read_bytes())

    def test_current_selection_accepts_new_and_retained_legacy_ids(self):
        paths = {"programs": self.programs, "current": self.programs / "current"}
        for release in (self.release(), self.legacy_release()):
            installation._link(paths["current"], f"releases/{release.name}")
            self.assertEqual(installation._selected_release(paths), release)
            installation._verify_release(release)
        installation._link(paths["current"], "releases/../foreign")
        with self.assertRaisesRegex(ManagerError, "foreign.*current selector"):
            installation._selected_release(paths)

    def test_regular_payloads_are_checked_without_rehashing(self):
        for release in (self.release(), self.legacy_release()):
            payload = release / "ci_manager/client.py"
            payload.chmod(0o644)
            payload.write_text("# changed fixture bytes\n")
            payload.chmod(0o444)
            installation._verify_release(release)

    def test_release_inventory_still_rejects_missing_extra_and_wrong_mode_files(self):
        for problem in ("missing", "extra", "mode"):
            with self.subTest(problem=problem):
                release = self.release()
                payload = release / "ci_manager/client.py"
                if problem == "missing":
                    payload.parent.chmod(0o700)
                    payload.unlink()
                elif problem == "extra":
                    release.chmod(0o700)
                    (release / "extra.py").write_text("# unlisted\n")
                else:
                    payload.chmod(0o644)
                with self.assertRaisesRegex(ManagerError, "integrity failed"):
                    installation._verify_release(release)

    def test_release_rejects_symlinks_and_inventory_path_escape(self):
        release = self.release()
        payload = release / "ci_manager/client.py"
        payload.parent.chmod(0o700)
        payload.unlink()
        payload.symlink_to(self.source / "client.py")
        with self.assertRaisesRegex(ManagerError, "integrity failed"):
            installation._verify_release(release)
        release = self.release()
        manifest = json.loads((release / "manifest.json").read_bytes())
        manifest["recipe"]["files"]["../foreign"] = {"mode": 0o444}
        self.replace_manifest(release, manifest)
        with self.assertRaisesRegex(ManagerError, "integrity failed"):
            installation._verify_release(release)

    def test_launcher_must_use_the_recorded_interpreter_and_owned_release(self):
        release = self.release()
        wrapper = release / "bin/cell-ci"
        wrapper.chmod(0o755)
        wrapper.write_text("#!/bin/sh\nexec /foreign/manager\n")
        wrapper.chmod(0o555)
        with self.assertRaisesRegex(ManagerError, "integrity failed"):
            installation._verify_release(release)

    def test_failed_install_restores_legacy_selection_and_keeps_pause(self):
        previous = self.legacy_release()
        paths = {"programs": self.programs, "current": self.programs / "current", "runtime": self.programs / "runtime",
                 "wrapper": self.root / "bin/cell-ci", "provider": self.root / "providers/ci-manager",
                 "plist": self.root / "agents/manager.plist"}
        installation._link(paths["current"], f"releases/{previous.name}")
        installation._link(paths["wrapper"], str(paths["current"] / "bin/cell-ci"))
        installation._link(paths["provider"], str(paths["current"] / "ci_manager/chancery"))
        old_plist = installation._plist(previous)
        installation._write_file(paths["plist"], old_plist)
        state = self.root / "state"
        state.mkdir(mode=0o700)
        store = mock.Mock()
        store.get.return_value = True
        store.active.return_value = None
        with ExitStack() as stack:
            stack.enter_context(mock.patch.object(installation.sys, "platform", "darwin"))
            stack.enter_context(mock.patch.object(installation, "__file__", str(self.source / "installation.py")))
            stack.enter_context(mock.patch.object(installation, "_paths", return_value=paths))
            stack.enter_context(mock.patch.object(installation, "state_root", return_value=state))
            stack.enter_context(mock.patch.object(installation, "Store", return_value=store))
            stack.enter_context(mock.patch.object(installation, "_loaded", return_value=False))
            stack.enter_context(mock.patch.object(installation, "_launchctl", side_effect=ManagerError("fixture bootstrap failure")))
            worker_lock = stack.enter_context(mock.patch.object(
                installation, "_worker_lock_after_stop", wraps=installation._worker_lock_after_stop))
            with self.assertRaisesRegex(ManagerError, "installation failed"):
                installation.install()
        self.assertEqual(installation._selected_release(paths), previous)
        self.assertEqual(paths["plist"].read_bytes(), old_plist)
        self.assertEqual(worker_lock.call_count, 2)
        store.set.assert_not_called()
        self.assertEqual(store.get.call_args_list, [mock.call("paused"), mock.call("paused")])
        installation._verify_release(previous)


if __name__ == "__main__":
    unittest.main()
