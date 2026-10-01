"""Test-runner selection requires its configured executable path."""

from pathlib import Path
import tempfile
import unittest
from unittest import mock

import nextest_tool


class NextestSelectionTests(unittest.TestCase):
    def test_missing_and_foreign_runner_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            path = root / "tools" / "nextest" / nextest_tool.VERSION / "cargo-nextest"
            with mock.patch.object(nextest_tool.workspace, "root", return_value=root), \
                    mock.patch.object(nextest_tool.sys, "platform", "darwin"):
                with self.assertRaisesRegex(ValueError, "unavailable"):
                    nextest_tool.runner()
                path.parent.mkdir(parents=True)
                path.write_bytes(b"fixture")
                path.chmod(0o600)
                with self.assertRaisesRegex(ValueError, "unavailable"):
                    nextest_tool.runner()
                path.chmod(0o700)
                self.assertEqual(nextest_tool.runner(), path)
                self.assertEqual(nextest_tool.identity(path),
                                 {"path": str(path), "version": nextest_tool.VERSION})
                with self.assertRaisesRegex(ValueError, "does not select"):
                    nextest_tool.identity(root / "other")
                saved = path.with_name("saved")
                path.rename(saved)
                path.symlink_to(saved)
                with self.assertRaisesRegex(ValueError, "unavailable"):
                    nextest_tool.runner()


if __name__ == "__main__":
    unittest.main()
