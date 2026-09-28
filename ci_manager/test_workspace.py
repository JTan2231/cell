"""Storage admission and child writes, using isolated volumes and no live state."""

import json
import ctypes
import os
from pathlib import Path
import plistlib
import subprocess
import sys
import tempfile
import unittest
import uuid
from unittest import mock

from ci_manager import workspace


class WorkspaceTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="cell-storage-")
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        self.volume = self.base / "volume"
        self.volume.mkdir()
        self.work = self.volume / "cell"
        self.work.mkdir(mode=0o700)
        self.config = self.base / "host/workspace.json"
        self.config.parent.mkdir()
        self.value = {"schema_version": 1, "volume": str(self.volume),
                      "volume_uuid": "expected", "directory": "cell"}
        self.config.write_text(json.dumps(self.value))
        self.info = {"MountPoint": str(self.volume), "Internal": False,
                     "FilesystemType": "apfs", "WritableVolume": True,
                     "GlobalPermissionsEnabled": True, "VolumeUUID": "expected"}
        for patch in (mock.patch.object(workspace, "config_path", return_value=self.config),
                      mock.patch("ci_manager.installation._loaded", return_value=False),
                      mock.patch.object(Path, "is_mount", return_value=True),
                      mock.patch.object(workspace.subprocess, "run", return_value=
                                        subprocess.CompletedProcess([], 0, plistlib.dumps(self.info)))):
            patch.start()
            self.addCleanup(patch.stop)

    def test_missing_mount_fails_without_creating_fallback(self):
        with mock.patch.object(Path, "is_mount", return_value=False):
            with self.assertRaisesRegex(workspace.WorkspaceError, "not mounted"):
                workspace.directory("ci-manager")
        self.assertEqual(list(self.work.iterdir()), [])

    def test_wrong_volume_readonly_and_disabled_ownership_are_rejected(self):
        for key, value in (("VolumeUUID", "replacement"), ("WritableVolume", False),
                           ("GlobalPermissionsEnabled", False), ("Internal", True),
                           ("FilesystemType", "exfat")):
            with self.subTest(key=key):
                info = {**self.info, key: value}
                with mock.patch.object(workspace.subprocess, "run", return_value=
                                       subprocess.CompletedProcess([], 0, plistlib.dumps(info))):
                    with self.assertRaises(workspace.WorkspaceError):
                        workspace.directory("ci-manager")
        self.assertEqual(list(self.work.iterdir()), [])

    def test_missing_configuration_never_selects_local_defaults(self):
        self.config.unlink()
        with self.assertRaisesRegex(workspace.WorkspaceError, "configure"):
            workspace.directory("ci-manager")

    def test_configured_root_cannot_be_replaced_by_a_link(self):
        self.work.rmdir()
        self.work.symlink_to(self.config.parent, target_is_directory=True)
        with self.assertRaises(workspace.WorkspaceError):
            workspace.root()

    def test_output_escape_is_rejected(self):
        (self.work / "escape").symlink_to(self.config.parent, target_is_directory=True)
        for path in (self.config.parent / "output", self.work / "escape/output"):
            with self.assertRaises(workspace.WorkspaceError):
                workspace.require_path(path)

    def test_unmounted_state_roots_fail_before_writing(self):
        from ci_manager.storage import state_root as manager_root
        from ci_broker.client import canonical_state_dir
        from deployment.cli import state_root as deployment_root
        with mock.patch.object(Path, "is_mount", return_value=False):
            for resolve in (manager_root, canonical_state_dir, deployment_root):
                with self.assertRaises(workspace.WorkspaceError):
                    resolve()
        self.assertEqual(list(self.work.iterdir()), [])

    def test_child_temporary_and_cache_writes_stay_external(self):
        environment = {**os.environ, **workspace.environment()}
        # Release the diskutil mock only for this real, bounded child process.
        with mock.patch.object(subprocess, "run", wraps=REAL_RUN):
            result = subprocess.run([sys.executable, "-B", "-c", '''
import json, os, pathlib, tempfile
paths = []
with tempfile.TemporaryDirectory() as temporary:
    paths.append(temporary)
    for key in ("CARGO_HOME", "XDG_CACHE_HOME", "CLANG_MODULE_CACHE_PATH", "SWIFT_MODULECACHE_PATH"):
        path = pathlib.Path(os.environ[key]) / "fixture-output"
        path.write_text("generated")
        paths.append(str(path))
    print(json.dumps(paths))
'''], env=environment, check=True, capture_output=True, text=True)
        self.assertTrue(all(Path(path).is_relative_to(self.work) for path in json.loads(result.stdout)))
        self.assertEqual(list(self.config.parent.iterdir()), [self.config])

    def test_existing_destination_cannot_be_changed(self):
        self.config.write_text(json.dumps({**self.value, "volume_uuid": "old"}))
        with self.assertRaisesRegex(workspace.WorkspaceError, "second queue"):
            workspace.configure(self.volume)

    def test_selection_requires_the_legacy_queue_to_be_paused_empty_and_stopped(self):
        from ci_manager.storage import Store, lock
        self.config.unlink()
        manager = self.config.parent / "ci-manager"
        store = Store(manager, create=True)
        self.addCleanup(store.db.close)
        store.set("paused", False)
        with self.assertRaisesRegex(workspace.WorkspaceError, "pause, drain and stop"):
            workspace.configure(self.volume)
        store.set("paused", True)
        store.db.execute("INSERT INTO holds VALUES ('fixture-owner', 1)")
        with self.assertRaises(workspace.WorkspaceError):
            workspace.configure(self.volume)
        store.db.execute("DELETE FROM holds")
        store.db.execute("INSERT INTO jobs (id,submission_key,phase,created,updated,data) VALUES ('fixture','fixture','queued',1,1,'{}')")
        with self.assertRaises(workspace.WorkspaceError):
            workspace.configure(self.volume)
        store.db.execute("DELETE FROM jobs")
        with mock.patch("ci_manager.installation._loaded", return_value=True):
            with self.assertRaisesRegex(workspace.WorkspaceError, "stop the existing"):
                workspace.configure(self.volume)
        with lock(manager / "worker.lock", blocking=False):
            with self.assertRaises(BlockingIOError):
                workspace.configure(self.volume)
        self.assertFalse(self.config.exists())
        self.assertEqual(workspace.configure(self.volume), self.value)
        self.assertEqual(json.loads(self.config.read_text()), self.value)

    def test_selection_does_not_bypass_an_unresolved_deployment(self):
        self.config.unlink()
        active = self.config.parent / "deployments/active"
        active.mkdir(parents=True)
        active.parent.chmod(0o700)
        with self.assertRaisesRegex(workspace.WorkspaceError, "resolve the existing deployment"):
            workspace.configure(self.volume)
        self.assertFalse(self.config.exists())

    def test_sandbox_denies_output_outside_workspace_and_temp_fallback(self):
        forbidden = Path("/private/tmp") / ("cell-storage-denied-" + uuid.uuid4().hex)
        self.addCleanup(lambda: forbidden.unlink(missing_ok=True))
        command = [sys.executable, "-B", "-c", '''
import pathlib, sys, tempfile
try:
    pathlib.Path(sys.argv[1]).write_text("must be denied")
except PermissionError:
    pass
else:
    raise SystemExit("write escaped the workspace")
try:
    tempfile.TemporaryFile()
except FileNotFoundError:
    pass
else:
    raise SystemExit("temporary files fell back outside the workspace")
''', str(forbidden)]
        # macOS cannot tighten path extensions inside an inherited sandbox.
        # In CI, exercise the enclosing production profile; standalone runs
        # install the profile here. Both must deny host writes and fallback.
        check = ctypes.CDLL("/usr/lib/libsandbox.dylib").sandbox_check
        if check(os.getpid(), None, 0) == 0:
            command = workspace.confined_command(command)
        with mock.patch.object(subprocess, "run", wraps=REAL_RUN):
            result = subprocess.run(command, cwd="/",
                                    env={**os.environ, "TMPDIR": "/absent-cell-volume",
                                         "TMP": "/absent-cell-volume", "TEMP": "/absent-cell-volume"},
                                    capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(forbidden.exists())

    def test_capacity_blocks_new_work(self):
        with mock.patch.object(workspace.shutil, "disk_usage", return_value=mock.Mock(free=1)):
            with self.assertRaisesRegex(workspace.WorkspaceError, "2 GiB"):
                workspace.require_capacity()


REAL_RUN = subprocess.run


if __name__ == "__main__":
    unittest.main()
