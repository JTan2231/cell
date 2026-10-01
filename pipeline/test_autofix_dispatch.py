"""Check that mechanical proposals stop validation before later stages."""

from contextlib import ExitStack, redirect_stdout
from io import StringIO
import json
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest import mock

import select_changes as selection


class AutofixDispatchTests(unittest.TestCase):
    def setUp(self):
        self.root = Path("/evidence/worktree")
        self.patch = Path("/evidence/job/validation-0.autofix.patch")
        self.arguments = ["--base", "a" * 40, "--candidate", "b" * 40, "--json",
                          "--autofix-patch", str(self.patch)]
        self.plan = SimpleNamespace(
            base="a" * 40, head="b" * 40, source="b" * 40,
            committed=True, mode="changed", tests_skipped=False, release_builds_deferred=False,
            selected=["alpha"], platform={"alpha": []}, shared={}, verbose=False,
        )
        self.gates = [(name, "heavy", ["body"]) for name in
                      ("alpha.pre", "cell.clippy", "cell.tests.rust", "alpha.post")]
        stack = ExitStack()
        self.addCleanup(stack.close)
        stack.enter_context(mock.patch.object(selection, "make_plan", return_value=self.plan))
        stack.enter_context(mock.patch.object(selection, "gate_plan", return_value=self.gates))
        stack.enter_context(mock.patch.object(selection, "output", return_value=b"b" * 40))
        self.check = stack.enter_context(mock.patch.object(selection, "check_plan"))
        self.broker = stack.enter_context(mock.patch.object(selection, "broker"))
        stack.enter_context(mock.patch.object(Path, "resolve", autospec=True, side_effect=lambda path: path))
        stack.enter_context(mock.patch.object(Path, "unlink"))
        stack.enter_context(mock.patch.object(Path, "is_file", return_value=True))
        self.size = stack.enter_context(mock.patch.object(Path, "stat", return_value=SimpleNamespace(st_size=42)))

    def run_validation(self, lint_state="failed"):
        self.broker.side_effect = lambda _root, gate, *_args, **_kwargs: {
            "gate": gate, "state": lint_state if gate == "cell.clippy" else "passed",
            "exit_code": 1 if gate == "cell.clippy" and lint_state == "failed" else 0,
            "detail": "lint diagnostics", "execution_id": gate,
        }
        stream = StringIO()
        with redirect_stdout(stream):
            code = selection.run_json(self.root, self.arguments)
        return code, json.loads(stream.getvalue())

    def test_one_patch_collects_fixes_and_stops_before_tests(self):
        code, receipt = self.run_validation()
        self.assertEqual(code, 0)
        self.assertEqual(receipt["state"], "autofix")
        self.assertEqual(receipt["autofix_patch"], str(self.patch))
        self.assertEqual(receipt["gates"][-1]["state"], "failed")
        self.assertEqual([call.args[1] for call in self.broker.call_args_list],
                         ["alpha.pre", "cell.clippy"])
        self.check.assert_called_with(self.root, self.plan)
        self.assertEqual(self.broker.call_args.kwargs["environment"]["CELL_CI_AUTOFIX_PATCH"],
                         str(self.patch))

    def test_format_only_patch_is_not_successful_validation(self):
        code, receipt = self.run_validation("passed")
        self.assertEqual((code, receipt["state"]), (0, "autofix"))
        self.assertEqual(self.broker.call_count, 2)

    def test_no_fix_preserves_lint_failure(self):
        self.size.return_value.st_size = 0
        code, receipt = self.run_validation()
        self.assertEqual((code, receipt["state"]), (1, "failed"))
        self.assertNotIn("autofix_patch", receipt)
        self.assertEqual(self.broker.call_count, 2)

    def test_no_patch_runs_tests_and_post_checks(self):
        self.size.return_value.st_size = 0
        code, receipt = self.run_validation("passed")
        self.assertEqual((code, receipt["state"]), (0, "passed"))
        self.assertEqual(self.broker.call_count, 4)

    def test_unsettled_gate_cannot_supply_a_mechanical_proposal(self):
        code, receipt = self.run_validation("lost")
        self.assertEqual((code, receipt["state"]), (70, "lost"))
        self.assertNotIn("autofix_patch", receipt)

    def test_candidate_mutation_stays_stale_even_with_a_patch(self):
        self.check.side_effect = [None, selection.StaleSelection("source changed")]
        code, receipt = self.run_validation()
        self.assertEqual((code, receipt["state"]), (75, "stale"))

    def test_patch_cannot_be_written_inside_candidate(self):
        self.arguments[-1] = str(self.root / "fix.patch")
        code, receipt = self.run_validation()
        self.assertEqual((code, receipt["state"]), (78, "error"))
        self.broker.assert_not_called()


if __name__ == "__main__":
    unittest.main()
