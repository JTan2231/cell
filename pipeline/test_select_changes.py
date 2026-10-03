"""Check prompt consumer coverage with in-memory repository inputs."""

from contextlib import redirect_stderr
from io import StringIO
from pathlib import Path
import unittest
from unittest import mock

import select_changes as selection


class PromptSelectionTests(unittest.TestCase):
    def setUp(self):
        self.root = Path("/evidence/worktree")
        self.products = {
            "bazaar": ("infrastructure/bazaar", ["bazaar"]),
            "annals": ("products/annals", ["annals"]),
            "decisions": ("products/decisions", ["decisions", "krisis"]),
            "semantics": ("infrastructure/semantics", ["semantics"]),
            "platter": ("products/platter", ["platter"]),
            "weaver": ("products/weaver-narrative", ["weaver"]),
            "emt": ("infrastructure/emt", ["emt"]),
            "conatus": ("products/conatus", ["conatus"]),
            "unrelated": ("products/unrelated", ["unrelated"]),
        }

    def select(self, path):
        with mock.patch.object(selection, "output", return_value=b"b" * 40), \
                mock.patch.object(selection, "git_status", return_value=b"fixture"), \
                mock.patch.object(selection, "changed_paths", return_value={path}), \
                mock.patch.object(selection, "inventory", return_value=self.products), \
                mock.patch.object(selection, "at_revision", return_value=b"present"), \
                mock.patch.object(selection, "platform_change", return_value=False), \
                mock.patch.object(selection, "check"), redirect_stderr(StringIO()):
            return selection.make_plan(self.root, [])

    def test_prompt_sources_and_seed_select_bazaar_and_all_current_consumers(self):
        for path in ("infrastructure/bazaar/src/prompts.rs",
                     "infrastructure/bazaar/src/prompt_import.rs",
                     "infrastructure/bazaar/seed.json"):
            with self.subTest(path=path):
                plan = self.select(path)
                self.assertEqual(set(plan.selected), {
                    "bazaar", "annals", "decisions", "semantics", "platter",
                    "weaver", "emt", "conatus",
                })
                self.assertFalse(any(plan.shared.values()))

    def test_other_bazaar_changes_do_not_expand_prompt_consumer_coverage(self):
        for path in ("infrastructure/bazaar/src/api.rs",
                     "infrastructure/bazaar/README.md"):
            with self.subTest(path=path):
                self.assertEqual(self.select(path).selected, ["bazaar"])

    def test_prompt_paths_do_not_select_absent_consumers(self):
        self.products = {"bazaar": self.products["bazaar"]}
        self.assertEqual(self.select("infrastructure/bazaar/seed.json").selected, ["bazaar"])


if __name__ == "__main__":
    unittest.main()
