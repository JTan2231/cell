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
        self.verifiers = {}
        for product in ("annals", "decisions", "clockwork"):
            verifier = self.home / "sealed-candidates" / product / "bin" / (product + "-install")
            verifier.parent.mkdir(parents=True)
            verifier.write_text("admitted candidate fixture")
            verifier.chmod(0o555)
            self.verifiers[product] = verifier

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
        if argv[0] in self.verifiers.values():
            self.assertEqual(argv[1], "verify-release")
            return json.dumps({"ok": True, "data": {"release_id": argv[2].name}})
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

    def run_cleanup(self, usher_installer=None, inspect=None, installers=None):
        with mock.patch.object(cleanup, "require_deployment_lock"), \
                mock.patch.object(cleanup, "inspect_command", side_effect=inspect or self.inspect):
            return cleanup.clean_installed_release_history(self.home, usher_installer,
                installers=self.verifiers if installers is None else installers)

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

    def test_semantics_completed_receipt_uses_its_product_fields(self):
        current = self.release("Semantics", self.current)
        old = self.release("Semantics", self.old)
        install = current.parent.parent
        (install / "current").symlink_to("releases/" + self.current)
        (install / "previous").symlink_to("releases/" + self.old)
        verifier = self.home / "sealed-candidates/semantics/bin/semantics-install"
        verifier.parent.mkdir(parents=True)
        verifier.write_text("admitted candidate fixture")
        verifier.chmod(0o555)
        self.verifiers["semantics"] = verifier
        receipt = {"version": 1, "release_id": self.current,
                   "previous": {"current": None, "previous": None, "entries": {}},
                   "clockwork_definition": "1" * 64, "maintenance_retained": False,
                   "rollback_snapshot": str(self.base / "Semantics/backups/deployments/pre-current-fixture")}
        history = install / "last-update.json"
        for changed in ({"maintenance_retained": True}, {"release_id": self.old},
                        {"version": 2}, {"clockwork_definition": "invalid"}):
            history.write_text(json.dumps(receipt | changed))
            with self.subTest(changed=changed), self.assertRaisesRegex(cleanup.CleanupError, "not completed"):
                self.run_cleanup()
            self.assertTrue(old.is_dir())
            self.assertTrue(self.history.is_file())
        history.write_text(json.dumps(receipt))
        pending = install / ".transaction.fixture"
        pending.mkdir()
        with self.assertRaisesRegex(cleanup.CleanupError, "transaction"):
            self.run_cleanup()
        pending.rmdir()
        self.history.write_text(json.dumps(receipt))
        with self.assertRaisesRegex(cleanup.CleanupError, "not completed"):
            self.run_cleanup()
        self.history.write_text(json.dumps({"release_id": self.current, "completed_at": "completed"}))
        result = self.run_cleanup()
        self.assertEqual(result["removed_history_receipts"], 2)
        self.assertTrue(current.is_dir())
        self.assertFalse(old.exists())
        self.assertFalse(history.exists())

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
            "removed_history_receipts": 1, "retained_releases": 7,
            "product_history": {product: "verified" for product in self.verifiers}})
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
            self.assertEqual(cleanup.clean_installed_release_history(
                self.home, installers=self.verifiers)["removed_releases"], 3)
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
                        cleanup.clean_installed_release_history(self.home, installers=self.verifiers)
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

    def test_all_products_retain_history_without_an_admitted_verifier(self):
        result = self.run_cleanup(installers={})
        self.assertEqual(result["removed_releases"], 0)
        self.assertEqual(result["removed_previous_links"], 0)
        self.assertEqual(result["removed_history_receipts"], 0)
        self.assertEqual(result["product_history"], {
            product: "retained_without_verified_installer" for product in self.verifiers})
        for application in ("Annals", "Decisions", "Clockwork"):
            install = self.base / application / "install"
            self.assertTrue((install / "previous").is_symlink())
            self.assertTrue((install / "releases" / self.old).is_dir())
        self.assertTrue(self.history.exists())

    def test_verifier_map_prunes_only_covered_products(self):
        result = self.run_cleanup(installers={"annals": self.verifiers["annals"]})
        self.assertEqual(result["removed_releases"], 1)
        self.assertEqual(result["removed_previous_links"], 1)
        self.assertFalse(self.history.exists())
        self.assertTrue((self.base / "Decisions/install/previous").is_symlink())
        self.assertTrue((self.base / "Clockwork/install/releases" / self.old).is_dir())

    def test_legacy_and_v2_cast_history_use_supplied_product_verifier(self):
        current = self.release("Cast", self.current)
        old = self.release("Cast", self.old)
        current.chmod(0o755)
        (current / "manifest.txt").unlink()
        (current / "manifest.json").write_text(json.dumps({
            "format": "cell-install-v2", "product": "cast", "versions": {"cast": "1.0.0"},
            "providers": {"cast": {"path": "share/chancery/cast", "version": "1.0.0"}},
            "files": {}, "public": [], "release_id": current.name}))
        current.chmod(0o555)
        install = current.parent.parent
        (install / "current").symlink_to("releases/" + self.current)
        (install / "previous").symlink_to("releases/" + self.old)
        verifier = self.home / "sealed-cast-install"
        verifier.write_text("admitted candidate fixture")
        verifier.chmod(0o555)
        seen = []

        def inspect(argv):
            if argv[0] == verifier:
                self.assertEqual(argv[1], "verify-release")
                seen.append(argv[2])
                return json.dumps({"ok": True, "data": {"release_id": argv[2].name}})
            return self.inspect(argv)

        result = self.run_cleanup(inspect=inspect, installers={**self.verifiers, "cast": verifier})
        self.assertEqual(set(seen), {current, old})
        self.assertEqual(result["product_history"]["cast"], "verified")
        self.assertTrue(current.is_dir())
        self.assertFalse(old.exists())

    def test_retained_installer_cannot_authorize_deletion(self):
        retained = self.base / "Annals/install/releases" / self.current / "program"
        retained.chmod(0o555)
        with self.assertRaisesRegex(cleanup.CleanupError, "retained installers"):
            self.run_cleanup(installers={"annals": retained})
        self.assertTrue(self.history.exists())
        self.assertTrue((self.base / "Annals/install/previous").is_symlink())

    def test_platter_sealed_verifier_prunes_history_under_its_file_lock(self):
        current = self.release("Platter", self.current)
        old = self.release("Platter", self.old)
        install = current.parent.parent
        (install / "current").symlink_to("releases/" + self.current)
        (install / "previous").symlink_to("releases/" + self.old)
        private_state = self.home / ".local/share/job-packets/packets.sqlite3"
        private_state.parent.mkdir(parents=True)
        private_state.write_bytes(b"existing packet and delivery state")
        verifier = self.home / "sealed-candidates/platter/bin/platter-install"
        verifier.parent.mkdir(parents=True)
        verifier.write_text("admitted candidate fixture")
        verifier.chmod(0o555)
        lock = install / ".update-lock"
        verified = []
        locked = []

        def inspect(argv):
            if argv[0] == verifier:
                self.assertEqual(argv[1], "verify-release")
                self.assertTrue(lock.is_file())
                self.assertEqual(lock.read_text().strip(), str(os.getpid()))
                verified.append(argv[2])
                return json.dumps({"ok": True, "data": {"release_id": argv[2].name}})
            if argv[0] == "/usr/bin/shlock" and argv[-1] == lock:
                locked.append(lock)
            return self.inspect(argv)

        installers = cleanup.parse_installers(["platter=" + str(verifier)])
        result = self.run_cleanup(inspect=inspect, installers={**self.verifiers, **installers})
        self.assertEqual(locked, [lock])
        self.assertEqual(set(verified), {current, old})
        self.assertEqual(result["product_history"]["platter"], "verified")
        self.assertEqual((install / "current").resolve(), current)
        self.assertEqual((current / "program").read_text(), "fixture program")
        self.assertFalse(old.exists())
        self.assertFalse((install / "previous").is_symlink())
        self.assertFalse(lock.exists())
        self.assertEqual(private_state.read_bytes(), b"existing packet and delivery state")

    def test_one_product_verification_failure_prevents_every_deletion(self):
        def inspect(argv):
            if argv[0] == self.verifiers["clockwork"] and argv[2].name == self.old:
                return json.dumps({"ok": False, "data": {"release_id": argv[2].name}})
            return self.inspect(argv)

        with self.assertRaisesRegex(cleanup.CleanupError, "did not prove its identity"):
            self.run_cleanup(inspect=inspect)
        self.assertTrue(self.history.exists())
        for application in ("Annals", "Decisions", "Clockwork"):
            self.assertTrue((self.base / application / "install/previous").is_symlink())
            self.assertTrue((self.base / application / "install/releases" / self.old).is_dir())

    def test_stateful_maintenance_and_transaction_barriers_preserve_history(self):
        for relative in ("Annals/spool/.maintenance", "Annals/decisions/spool/.maintenance",
                         "Decisions/.clockwork-maintenance", "Annals/install/transaction.primary.run",
                         "Semantics/.clockwork-maintenance"):
            with self.subTest(relative=relative):
                marker = self.base / relative
                marker.parent.mkdir(parents=True, exist_ok=True)
                marker.write_text("retained recovery evidence")
                with self.assertRaisesRegex(cleanup.CleanupError, "maintenance|transaction"):
                    self.run_cleanup()
                self.assertTrue(marker.exists())
                self.assertTrue(self.history.exists())
                self.assertTrue((self.base / "Annals/install/previous").is_symlink())
                marker.unlink()

    def test_product_verifier_arguments_are_explicit_and_unambiguous(self):
        self.assertEqual(cleanup.parse_installers(["krisis=/sealed/krisis-install"]),
                         {"decisions": Path("/sealed/krisis-install")})
        for arguments in (["unknown=/sealed/tool"], ["annals"],
                          ["krisis=/one", "decisions=/two"]):
            with self.subTest(arguments=arguments), self.assertRaises(cleanup.CleanupError):
                cleanup.parse_installers(arguments)
        with self.assertRaisesRegex(cleanup.CleanupError, "exact sealed candidate"):
            self.run_cleanup(installers={"annals": Path("relative")})


if __name__ == "__main__":
    unittest.main()
