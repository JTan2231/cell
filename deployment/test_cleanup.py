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
        bundle = path / "share/chancery/provider"
        bundle.mkdir(parents=True)
        (bundle / "entry.json").write_text("{}")
        for directory in (path, path / "share", path / "share/chancery", bundle):
            directory.chmod(0o555)
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

    def run_cleanup(self, usher_installer=None, inspect=None):
        with mock.patch.object(cleanup, "require_deployment_lock"), \
                mock.patch.object(cleanup, "inspect_command", side_effect=inspect or self.inspect):
            return cleanup.clean_installed_release_history(self.home, usher_installer)

    def usher_releases(self):
        current = self.release("Usher", self.current)
        old = self.release("Usher", self.old)
        current.chmod(0o755)
        (current / "manifest.txt").unlink()
        (current / "manifest.json").write_text(json.dumps({
            "format": "cell-install-v1", "product": "usher", "provider": "usher",
            "versions": {"usher": "usher 1.0.0"}, "files": {}, "release_id": current.name}))
        current.chmod(0o555)
        install = current.parent.parent
        (install / "current").symlink_to("releases/" + self.current)
        (install / "previous").symlink_to("releases/" + self.old)
        return current, old

    def test_usher_history_is_retained_without_a_trusted_candidate_installer(self):
        current, old = self.usher_releases()
        result = self.run_cleanup()
        self.assertEqual(result["usher_history"], "retained_without_verified_installer")
        self.assertTrue(current.is_dir())
        self.assertTrue(old.is_dir())
        self.assertTrue((current.parent.parent / "previous").is_symlink())
        self.assertEqual(result["removed_releases"], 3)

    def test_usher_legacy_and_rust_history_use_only_the_supplied_sealed_verifier(self):
        current, old = self.usher_releases()
        verifier = self.home / "sealed-candidate/bin/usher-install"
        verifier.parent.mkdir(parents=True)
        verifier.write_text("sealed admitted fixture")
        verifier.chmod(0o555)
        verified = []

        def inspect(argv):
            if argv[0] == verifier:
                self.assertEqual(argv[1], "verify-release")
                verified.append(argv[2])
                return json.dumps({"ok": True, "data": {"release_id": argv[2].name}})
            return self.inspect(argv)

        result = self.run_cleanup(verifier, inspect)
        self.assertEqual(set(verified), {current, old})
        self.assertEqual(result["usher_history"], "verified")
        self.assertTrue(current.is_dir())
        self.assertFalse(old.exists())
        self.assertFalse((current.parent.parent / "previous").is_symlink())

    def test_usher_verification_failure_prevents_all_history_deletion(self):
        current, old = self.usher_releases()
        verifier = self.home / "sealed-usher-install"
        verifier.write_text("sealed admitted fixture")
        verifier.chmod(0o555)

        def inspect(argv):
            if argv[0] == verifier:
                return json.dumps({"ok": True, "data": {"release_id": "wrong-release"}})
            return self.inspect(argv)

        with self.assertRaisesRegex(cleanup.CleanupError, "did not prove its identity"):
            self.run_cleanup(verifier, inspect)
        self.assertTrue(old.is_dir())
        self.assertTrue((current.parent.parent / "previous").is_symlink())
        self.assertTrue((self.base / "Annals/install/releases" / self.old).is_dir())
        self.assertTrue(self.history.exists())

    def test_prunes_only_unreferenced_history_and_releases_its_locks(self):
        self.assertEqual(self.run_cleanup(), {"removed_releases": 3, "removed_previous_links": 3,
                                             "removed_history_receipts": 1, "retained_releases": 7})
        for application in ("Annals", "Decisions", "Clockwork"):
            install = self.base / application / "install"
            self.assertTrue((install / "current").exists())
            self.assertEqual((install / "current").stat().st_mode & 0o777, 0o555)
            self.assertFalse((install / "previous").is_symlink())
            self.assertFalse((install / "releases" / self.old).exists())
            self.assertFalse((install / ".update-lock").exists())
        for path in (self.annals_pin, self.disabled_pin, self.running_pin, self.plist_pin):
            self.assertTrue(path.is_dir())
            self.assertEqual(path.stat().st_mode & 0o777, 0o555)
        self.assertFalse((self.base / "Clockwork/.update-lock").exists())
        self.assertFalse(self.history.exists())
        self.assertEqual(self.domain.read_text(), "retained domain state")

    def test_paged_inventory_expands_before_pruning_disabled_binding_pins(self):
        calls = []

        def inspect(argv):
            args = [str(value) for value in argv]
            if args[2:4] == ["binding", "list"]:
                limit = int(args[-1]) if "--limit" in args else 20
                calls.append(limit)
                items = [{"key": f"fixture/{index}", "definition_digest": None}
                         for index in range(20)]
                items.append({"key": "decisions/observer", "enabled": False,
                              "definition_digest": "1" * 64})
                return json.dumps({"ok": True, "data": {"output_version": 2,
                    "items": items[:limit], "has_more": len(items) > limit}})
            return self.inspect(argv)

        with mock.patch.object(cleanup, "require_deployment_lock"), \
                mock.patch.object(cleanup, "inspect_command", side_effect=inspect):
            self.assertEqual(cleanup.clean_installed_release_history(self.home)["removed_releases"], 3)
        self.assertEqual(calls, [20, 40])
        self.assertTrue(self.disabled_pin.is_dir())

    def test_unknown_or_incomplete_inventory_refuses_cleanup_before_deletion(self):
        for data in ({"output_version": 99, "items": [], "has_more": False},
                     {"output_version": 2, "items": [], "has_more": True},
                     {"output_version": 2, "items": [], "has_more": "false"}):
            with self.subTest(data=data):
                def inspect(argv):
                    if [str(value) for value in argv][2:4] == ["binding", "list"]:
                        return json.dumps({"ok": True, "data": data})
                    return self.inspect(argv)
                with mock.patch.object(cleanup, "require_deployment_lock"), \
                        mock.patch.object(cleanup, "inspect_command", side_effect=inspect):
                    with self.assertRaises(cleanup.CleanupError):
                        cleanup.clean_installed_release_history(self.home)
                self.assertTrue(self.disabled_pin.is_dir())
                self.assertTrue(self.history.exists())
                self.assertTrue((self.base / "Annals/install/previous").is_symlink())

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
