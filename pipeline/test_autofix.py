"""Check private Rust repair preparation with in-memory source and diagnostics."""

from contextlib import nullcontext, redirect_stderr
import io
import json
from pathlib import Path
import stat
import subprocess
from types import SimpleNamespace
import unittest
from unittest import mock

import autofix


ROOT = Path("/candidate")


def span(path, start, end, replacement, applicability="MachineApplicable"):
    return {"file_name": path, "byte_start": start, "byte_end": end,
            "suggested_replacement": replacement, "suggestion_applicability": applicability}


class SuggestionTests(unittest.TestCase):
    def test_multipart_suggestion_uses_original_unicode_byte_offsets(self):
        source = 'let text = "café";\nlet n = 1;\n'.encode()
        start = source.index(b"caf")
        number = source.index(b"1")
        originals = {"src/lib.rs": source}
        groups = [[span("src/lib.rs", start, start + len("café".encode()), "piñata"),
                   span("/candidate/src/lib.rs", number, number + 1, "200")]]
        changes = autofix.apply_edits(originals, autofix.select_edits(groups, ROOT, originals))
        self.assertEqual(changes, {"src/lib.rs": 'let text = "piñata";\nlet n = 200;\n'.encode()})
        self.assertEqual(originals["src/lib.rs"], source)

    def test_incomplete_or_non_machine_group_is_omitted_as_a_whole(self):
        valid = span("src/lib.rs", 0, 1, "B")
        uncertain = span("src/lib.rs", 1, 2, "C", "MaybeIncorrect")
        message = {"spans": [], "children": [{"spans": [valid, uncertain]}]}
        self.assertEqual(autofix.suggestion_groups(message), [])
        groups = [[valid, span("src/missing.rs", 0, 1, "C")],
                  [valid, span("src/lib.rs", 2, 4, "D")]]
        self.assertEqual(autofix.select_edits(groups, ROOT, {"src/lib.rs": b"ab"}), [])

    def test_duplicate_targets_are_deduplicated_and_conflicting_group_is_atomic(self):
        originals = {"src/a.rs": b"abc", "src/b.rs": b"def"}
        first = span("src/a.rs", 0, 1, "A")
        groups = [[first], [span("/candidate/src/a.rs", 0, 1, "A")],
                  [span("src/a.rs", 0, 2, "Z"), span("src/b.rs", 0, 1, "D")],
                  [span("src/a.rs", 2, 3, "C")]]
        edits = autofix.select_edits(groups, ROOT, originals)
        self.assertEqual(len(edits), 2)
        self.assertEqual(autofix.apply_edits(originals, edits), {"src/a.rs": b"AbC"})

    def test_invalid_unicode_boundaries_and_untracked_or_external_paths_are_omitted(self):
        originals = {"src/lib.rs": "éx".encode(), "notes.txt": b"x"}
        groups = [[span("src/lib.rs", 1, 2, "a")],
                  [span("../src/lib.rs", 0, 2, "a")],
                  [span("/dependency/src/lib.rs", 0, 2, "a")],
                  [span("src/link.rs", 0, 2, "a")],
                  [span("notes.txt", 0, 1, "a")]]
        self.assertEqual(autofix.select_edits(groups, ROOT, originals), [])

    def test_duplicate_insertions_are_not_applied_twice(self):
        groups = [[span("src/lib.rs", 1, 1, ";")], [span("src/lib.rs", 1, 1, ";")],
                  [span("src/lib.rs", 1, 1, ",")]]
        originals = {"src/lib.rs": b"x\n"}
        self.assertEqual(autofix.apply_edits(originals, autofix.select_edits(groups, ROOT, originals)),
                         {"src/lib.rs": b"x;\n"})

    def test_clippy_json_stream_relays_diagnostics_and_keeps_exit_status(self):
        message = {"rendered": "error: lint failed\n", "spans": [],
                   "children": [{"spans": [span("src/lib.rs", 1, 1, ";")]}]}
        stream = (json.dumps({"reason": "compiler-message", "message": message}) + "\n"
                  + json.dumps({"reason": "build-finished", "success": False}) + "\n").encode()
        child = SimpleNamespace(stdout=io.BytesIO(stream), wait=lambda: 101)
        diagnostics = io.StringIO()
        with mock.patch.object(autofix.subprocess, "Popen", return_value=child), redirect_stderr(diagnostics):
            code, groups = autofix.run_clippy(ROOT, ["cargo", "clippy", "--message-format=json"])
        self.assertEqual(code, 101)
        self.assertEqual(groups, [message["children"][0]["spans"]])
        self.assertEqual(diagnostics.getvalue(), "error: lint failed\n")


class WorkflowTests(unittest.TestCase):
    def setUp(self):
        self.command = ["/tool/cargo", "clippy", "--manifest-path", "/candidate/Cargo.toml",
                        "--package", "alpha", "--package", "beta", "--all-targets",
                        "--message-format=json", "--", "-D", "clippy::all"]

    def test_formatter_uses_snapshot_manifest_and_same_package_scope(self):
        self.assertEqual(autofix.format_command(ROOT, Path("/scratch/b"), self.command),
                         ["/tool/cargo", "fmt", "--manifest-path", "/scratch/b/Cargo.toml",
                          "--package", "alpha", "--package", "beta"])

    def test_repairs_and_formatting_only_write_snapshot_and_keep_candidate_unchanged(self):
        source = b"let n = 1\n"
        files = {"/candidate/src/lib.rs": source, "/scratch/b/src/lib.rs": source}
        groups = [[span("src/lib.rs", source.index(b"1"), source.index(b"1") + 1, "2")]]
        writes = []

        def write(path, value):
            writes.append(str(path))
            files[str(path)] = value
            return len(value)

        def format_source(command, **kwargs):
            self.assertEqual(kwargs["cwd"], Path("/scratch/b"))
            self.assertEqual(files["/scratch/b/src/lib.rs"], b"let n = 2\n")
            files["/scratch/b/src/lib.rs"] = b"let n = 2;\n"
            return subprocess.CompletedProcess(command, 0)

        raw_patch = b"diff --git a/src/lib.rs b/src/lib.rs\n"
        with mock.patch.object(autofix, "run_clippy", return_value=(101, groups)), mock.patch.object(
                autofix.tempfile, "TemporaryDirectory", return_value=nullcontext("/scratch")), mock.patch.object(
                autofix, "snapshot", return_value=({"src/lib.rs": source}, {"src/lib.rs": 0o644})), mock.patch.object(
                Path, "write_bytes", write), mock.patch.object(autofix.subprocess, "run", side_effect=format_source), mock.patch.object(
                autofix, "make_patch", return_value=raw_patch), mock.patch.object(autofix, "atomic_patch") as retain:
            self.assertEqual(autofix.run(ROOT, Path("/evidence/repair.patch"), self.command), 0)
        self.assertEqual(writes, ["/scratch/b/src/lib.rs"])
        self.assertEqual(files["/candidate/src/lib.rs"], source)
        self.assertEqual(retain.call_args_list, [mock.call(Path("/evidence/repair.patch"), b""),
                                                mock.call(Path("/evidence/repair.patch"), raw_patch)])

    def test_no_patch_preserves_nonfixable_clippy_failure(self):
        with mock.patch.object(autofix, "run_clippy", return_value=(101, [])), mock.patch.object(
                autofix.tempfile, "TemporaryDirectory", return_value=nullcontext("/scratch")), mock.patch.object(
                autofix, "snapshot", return_value=({}, {})), mock.patch.object(
                autofix.subprocess, "run", return_value=SimpleNamespace(returncode=0)), mock.patch.object(
                autofix, "make_patch", return_value=b""), mock.patch.object(autofix, "atomic_patch") as retain:
            self.assertEqual(autofix.run(ROOT, Path("/evidence/repair.patch"), self.command), 101)
        self.assertEqual(retain.call_args.args[1], b"")

    def test_signaled_clippy_cannot_publish_partial_suggestions_or_formatting(self):
        groups = [[span("src/lib.rs", 0, 1, "a")]]
        with mock.patch.object(autofix, "run_clippy", return_value=(-15, groups)), mock.patch.object(
                autofix, "snapshot") as snapshot, mock.patch.object(autofix.subprocess, "run") as format_source, mock.patch.object(
                autofix, "atomic_patch") as retain:
            self.assertEqual(autofix.run(ROOT, Path("/evidence/repair.patch"), self.command), 143)
        snapshot.assert_not_called()
        format_source.assert_not_called()
        retain.assert_called_once_with(Path("/evidence/repair.patch"), b"")

    def test_cli_rejects_a_patch_inside_the_checked_source(self):
        command = ["autofix.py", "--root", "/candidate", "--patch", "/candidate/repair.patch",
                   "--", "cargo", "clippy"]
        diagnostics = io.StringIO()
        with mock.patch.object(autofix.sys, "argv", command), mock.patch.object(
                autofix, "run") as run, redirect_stderr(diagnostics), self.assertRaises(SystemExit) as error:
            autofix.main()
        self.assertEqual(error.exception.code, 2)
        self.assertIn("outside the checked source", diagnostics.getvalue())
        run.assert_not_called()

    def test_format_failure_cannot_publish_partial_repair(self):
        with mock.patch.object(autofix, "run_clippy", return_value=(101, [])), mock.patch.object(
                autofix.tempfile, "TemporaryDirectory", return_value=nullcontext("/scratch")), mock.patch.object(
                autofix, "snapshot", return_value=({}, {})), mock.patch.object(
                autofix.subprocess, "run", return_value=SimpleNamespace(returncode=1)), mock.patch.object(
                autofix, "make_patch") as diff, mock.patch.object(autofix, "atomic_patch") as retain:
            with self.assertRaisesRegex(autofix.AutofixError, "rustfmt failed"):
                autofix.run(ROOT, Path("/evidence/repair.patch"), self.command)
        diff.assert_not_called()
        retain.assert_called_once_with(Path("/evidence/repair.patch"), b"")

    def test_raw_git_diff_uses_actual_paths_and_preserves_source_bytes_and_modes(self):
        before = 'fn café() {}\n'.encode()
        after = 'fn café() { (); }\n'.encode()
        files = {"/scratch/b/src/café.rs": after}
        output = b"diff --git a/src/caf\303\251.rs b/src/caf\303\251.rs\n"

        def write(path, value):
            files[str(path)] = value
            return len(value)

        with mock.patch.object(Path, "is_symlink", return_value=False), mock.patch.object(
                Path, "is_file", return_value=True), mock.patch.object(
                Path, "read_bytes", lambda path: files[str(path)]), mock.patch.object(
                Path, "mkdir"), mock.patch.object(Path, "write_bytes", write), mock.patch.object(
                Path, "chmod") as chmod, mock.patch.object(
                autofix.subprocess, "run", return_value=SimpleNamespace(returncode=1, stdout=output)) as diff:
            patch = autofix.make_patch(Path("/scratch"), {"src/café.rs": before}, {"src/café.rs": 0o755})
        self.assertEqual(patch, output)
        self.assertEqual(files["/scratch/a/src/café.rs"], before)
        self.assertEqual(diff.call_args.args[0][-3:], ["--", "a/src/café.rs", "b/src/café.rs"])
        self.assertIn("--binary", diff.call_args.args[0])
        chmod.assert_called_once_with(0o755)

    def test_snapshot_preserves_symlinks_but_rejects_an_escape_before_formatting(self):
        listed = SimpleNamespace(stdout=b"src/link.rs\0")
        with mock.patch.object(autofix.subprocess, "run", return_value=listed), mock.patch.object(
                Path, "is_symlink", return_value=False), mock.patch.object(
                Path, "lstat", return_value=SimpleNamespace(st_mode=stat.S_IFLNK | 0o777)), mock.patch.object(
                Path, "mkdir"), mock.patch.object(autofix.shutil, "copy2") as copy, mock.patch.object(
                Path, "resolve", return_value=Path("/candidate/src/lib.rs")):
            with self.assertRaisesRegex(autofix.AutofixError, "snapshot symlink escapes"):
                autofix.snapshot(ROOT, Path("/scratch/b"))
        copy.assert_called_once_with(ROOT / "src/link.rs", Path("/scratch/b/src/link.rs"), follow_symlinks=False)


if __name__ == "__main__":
    unittest.main()
