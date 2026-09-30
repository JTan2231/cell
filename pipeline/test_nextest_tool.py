"""Pinned test-runner selection fails before CI admission on changed bytes."""

import hashlib
from pathlib import Path
import tempfile
import unittest
from unittest import mock

import nextest_tool


class NextestSelectionTests(unittest.TestCase):
    def test_missing_changed_and_foreign_runner_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            path = root / "tools" / "nextest" / nextest_tool.VERSION / "cargo-nextest"
            with mock.patch.object(nextest_tool.workspace, "root", return_value=root), \
                    mock.patch.object(nextest_tool.sys, "platform", "darwin"):
                with self.assertRaisesRegex(ValueError, "absent or changed"):
                    nextest_tool.runner()
                path.parent.mkdir(parents=True)
                path.write_bytes(b"verified fixture")
                path.chmod(0o700)
                with self.assertRaisesRegex(ValueError, "absent or changed"):
                    nextest_tool.runner()
                expected = hashlib.sha256(path.read_bytes()).hexdigest()
                with mock.patch.object(nextest_tool, "BINARY_SHA256", expected):
                    self.assertEqual(nextest_tool.runner(), path)
                    self.assertEqual(nextest_tool.identity(path)["sha256"], expected)
                    with self.assertRaisesRegex(ValueError, "does not select"):
                        nextest_tool.identity(root / "other")
                    saved = path.with_name("saved")
                    path.rename(saved)
                    path.symlink_to(saved)
                    with self.assertRaisesRegex(ValueError, "absent or changed"):
                        nextest_tool.runner()


if __name__ == "__main__":
    unittest.main()
