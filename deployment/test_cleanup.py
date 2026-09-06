"""Offline release pruning keeps current, configured, and running references."""
import json
import os
from pathlib import Path
import plistlib
import tempfile
import unittest
from unittest import mock

from deployment import cleanup


class ReleaseCleanupTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.home = Path(self.temporary.name).resolve()
        self.base = self.home / "Library/Application Support"
        self.current = "a" * 64
        self.old = "b" * 64
        for application in ("Annals", "Decisions", "Clockwork"):
            self.release(application, self.current)
            self.release(application, self.old)
            install = self.base / application / "install"
            (install / "current").symlink_to("releases/" + self.current)
            (install / "previous").symlink_to("releases/" + self.old)
        self.annals_pin = self.release("Annals", "c" * 64)
        self.disabled_pin = self.release("Decisions", "d" * 64)
        self.running_pin = self.release("Clockwork", "e" * 64)
        self.plist_pin = self.release("Clockwork", "f" * 64)
        receipt = self.base / "Decisions/install/krisis-observer-binding.txt"
        receipt.write_text(f"release_id={self.current}\nannals_binary={self.annals_pin}/bin/annals\n")
        self.history = self.base / "Annals/install/last-update.json"
        self.history.write_text(json.dumps({"release_id": self.current, "completed_at": "2026-09-06T00:00:00Z"}))
        agents = self.home / "Library/LaunchAgents"
        agents.mkdir()
        (agents / "org.clockwork.fixture.plist").write_bytes(plistlib.dumps({
            "ProgramArguments": [str(self.plist_pin / "bin/clockwork")]}))
        self.domain = self.base / "Annals/annals.db"
        self.domain.write_text("retained domain state")

    def tearDown(self):
        self.temporary.cleanup()

    def release(self, application, identity):
        path = self.base / application / "install/releases" / identity
        path.mkdir(parents=True)
        (path / "manifest.txt").write_text(f"format=1\nrelease_id={identity}\n")
        (path / "program").write_text("fixture program")
        return path

    def inspect(self, argv):
        argv = [str(value) for value in argv]
        if argv[0] == "/usr/bin/shlock":
            Path(argv[-1]).write_text(str(os.getpid()) + "\n")
            return ""
        if argv[0] == "/usr/sbin/lsof":
            return "n" + str(self.running_pin / "bin/clockwork") + "\n"
        if argv[0] == "/bin/ps":
            return ""
        if argv[-2:] == ["binding", "list"]:
            return json.dumps({"ok": True, "data": [{"key": "decisions/observer", "enabled": False,
                                                       "definition_digest": "1" * 64}]})
        if argv[-3:] == ["definition", "show", "1" * 64]:
            return json.dumps({"ok": True, "data": {"key": "decisions/observer", "digest": "1" * 64,
                                                       "manifest": {"release_root": str(self.disabled_pin)}}})
        raise AssertionError("unexpected inspection: " + repr(argv))

    def run_cleanup(self):
        with mock.patch.object(cleanup, "require_deployment_lock"), \
                mock.patch.object(cleanup, "inspect_command", side_effect=self.inspect):
            return cleanup.clean_installed_release_history(self.home)

    def test_prunes_only_unreferenced_history_and_releases_its_locks(self):
        self.assertEqual(self.run_cleanup(), {"removed_releases": 3, "removed_previous_links": 3,
                                             "removed_history_receipts": 1, "retained_releases": 7})
        for application in ("Annals", "Decisions", "Clockwork"):
            install = self.base / application / "install"
            self.assertTrue((install / "current").exists())
            self.assertFalse((install / "previous").is_symlink())
            self.assertFalse((install / "releases" / self.old).exists())
            self.assertFalse((install / ".update-lock").exists())
        for path in (self.annals_pin, self.disabled_pin, self.running_pin, self.plist_pin):
            self.assertTrue(path.is_dir())
        self.assertFalse((self.base / "Clockwork/.update-lock").exists())
        self.assertFalse(self.history.exists())
        self.assertEqual(self.domain.read_text(), "retained domain state")

    def test_live_previous_alias_refuses_cleanup_before_deletion(self):
        config = self.base / "Annals/config.toml"
        config.write_text('executable = ' + json.dumps(str(self.base / "Annals/install/previous/program")) + "\n")
        with self.assertRaisesRegex(cleanup.CleanupError, "previous selector"):
            self.run_cleanup()
        self.assertTrue(self.history.exists())
        self.assertTrue((self.base / "Annals/install/previous").is_symlink())
        self.assertTrue((self.base / "Annals/install/releases" / self.old).exists())

    def test_existing_installer_lock_refuses_pruning_and_is_preserved(self):
        lock = self.base / "Decisions/install/.update-lock"
        lock.mkdir()
        with self.assertRaisesRegex(cleanup.CleanupError, "installer lock"):
            self.run_cleanup()
        self.assertTrue(lock.is_dir())
        self.assertFalse((self.base / "Annals/install/.update-lock").exists())
        self.assertTrue(self.history.exists())


if __name__ == "__main__":
    unittest.main()
