#!/usr/bin/env python3
"""Exercise catalog diagnostics with local output fixtures, without Cargo."""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parent.parent
ENTRY = {
    "id": "todo.concern.capture-and-route",
    "title": "Save and research a concern for later",
    "summary": "Save a concern and its source. Research a pending proposal to attach it, create or revise a todo, unify duplicates, defer or dismiss it.",
}


class CatalogDiagnosticsTests(unittest.TestCase):
    def run_check(self, entries, shown=None, raw_catalog=None):
        with tempfile.TemporaryDirectory(prefix="cell-todo-catalog-test-") as temporary:
            fixture = Path(temporary)
            (fixture / "list.json").write_text(
                raw_catalog if raw_catalog is not None else json.dumps(
                    {"data": {"entries": entries}}, separators=(",", ":")
                )
            )
            (fixture / "show.json").write_text(json.dumps(
                {"data": {"entry": ENTRY if shown is None else shown,
                          "manual": "UNRELATED_MANUAL" * 65536}},
                separators=(",", ":"),
            ))
            cargo = fixture / "cargo"
            cargo.write_text('''#!/bin/sh
for argument in "$@"; do
    if [ "$argument" = show ]; then
        cat "$CATALOG_FIXTURE_DIR/show.json"
        exit
    fi
done
cat "$CATALOG_FIXTURE_DIR/list.json"
''')
            cargo.chmod(0o755)
            environment = {
                **os.environ,
                "PATH": str(fixture) + os.pathsep + os.environ["PATH"],
                "PIPELINE_ROOT": str(ROOT),
                "CATALOG_FIXTURE_DIR": str(fixture),
            }
            return subprocess.run(
                ["sh", str(ROOT / "pipeline/extras/todo-catalog.sh")],
                env=environment, capture_output=True, check=False,
            )

    def test_success_output_is_unchanged(self):
        result = self.run_check([ENTRY])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, b"Todo catalog regression passed\n")
        self.assertEqual(result.stderr, b"")

    def test_failures_show_only_bounded_relevant_values(self):
        unrelated = {"id": "unrelated", "summary": "UNRELATED_BODY" * 65536}
        cases = [
            ([unrelated], None, b"does not contain concern capture", b"<missing entry>"),
            ([{**ENTRY, "title": "renamed\n" * 65536}, unrelated], None,
             b"omits the concern-capture title", b"truncated"),
            ([{**ENTRY, "summary": "changed"}, unrelated], None,
             b"omits the concern-capture summary", b'observed summary="changed"'),
            ([ENTRY, unrelated], {"id": "other"},
             b"contract cannot be shown", b'observed id="other"'),
        ]
        for entries, shown, expectation, observed in cases:
            with self.subTest(expectation=expectation):
                result = self.run_check(entries, shown)
                self.assertEqual(result.returncode, 1)
                self.assertEqual(result.stdout, b"")
                self.assertIn(expectation, result.stderr)
                self.assertIn(observed, result.stderr)
                self.assertLess(len(result.stderr), 512)
                self.assertEqual(len(result.stderr.splitlines()), 1)
                self.assertNotIn(b"UNRELATED", result.stderr)

    def test_invalid_json_does_not_dump_the_payload(self):
        result = self.run_check([], raw_catalog="UNRELATED_BODY" * 65536)
        self.assertEqual(result.returncode, 1)
        self.assertIn(b"<invalid catalog JSON>", result.stderr)
        self.assertLess(len(result.stderr), 512)
        self.assertNotIn(b"UNRELATED", result.stderr)


if __name__ == "__main__":
    unittest.main()
