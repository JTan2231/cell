"""Check manager release inventories and compatible selection without file hashes."""

from contextlib import ExitStack
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

from ci_manager import installation
from ci_manager.storage import ManagerError


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
        for record in manifest["recipe"]["files"].values():
            record["sha256"] = "retained legacy metadata"
        manifest["wrapper_sha256"] = "retained legacy metadata"
        self.replace_manifest(legacy, manifest)
        wrapper = legacy / "bin/cell-ci"
        wrapper.chmod(0o755)
        wrapper.write_bytes(installation._launcher(str(self.python), legacy))
        wrapper.chmod(0o555)
        return legacy

    def test_new_releases_have_independent_opaque_ids_and_mode_inventory(self):
        first, second = self.release(), self.release()
        self.assertNotEqual(first.name, second.name)
        self.assertRegex(first.name, r"^[0-9a-f]{32}$")
        manifest = installation._verify_release(first)
        self.assertEqual(set(manifest), {"recipe"})
        self.assertEqual(manifest["recipe"]["schema_version"], 2)
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
        paths = {"programs": self.programs, "current": self.programs / "current",
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
