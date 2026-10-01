"""Regression checks for the CI manager's provider boundaries."""

import json
from pathlib import Path
import subprocess
import unittest
from unittest import mock

from ci_manager import integrations


class PromptSelectionTests(unittest.TestCase):
    def test_prompt_reads_select_json_when_bazaar_defaults_to_text(self):
        selection = integrations.PROMPT_SELECTION_ID
        instructions = integrations.PROMPT_INSTRUCTIONS_ID
        template = integrations.PROMPT_TEMPLATE_ID
        records = {
            selection: {
                "id": selection,
                "version": 3,
                "content": json.dumps({
                    "schema_version": 1,
                    "entries": {instructions: 2, template: 4},
                }),
            },
            instructions: {"id": instructions, "version": 2, "content": "Repair instructions"},
            template: {"id": template, "version": 4, "content": "Repair {context}"},
        }

        def read(command, **_kwargs):
            identifier = command[command.index("get") + 1]
            record = records[identifier]
            if "--version" in command:
                self.assertEqual(int(command[command.index("--version") + 1]), record["version"])
            if "--json" in command:
                output = json.dumps({"schema_version": 1, "ok": True, "data": record})
            else:
                output = f"ID: {identifier}\nContent:\n{record['content']}\n"
            return subprocess.CompletedProcess(command, 0, output.encode(), b"")

        with mock.patch("ci_manager.integrations.subprocess.run", side_effect=read) as calls:
            result = integrations.load_prompt_selection(
                executable=Path("/provider/bazaar"), database=Path("/private/bazaar.sqlite3")
            )

        self.assertEqual(calls.call_count, 3)
        self.assertEqual(result["selection_version"], 3)
        self.assertEqual(result["component_versions"], {instructions: 2, template: 4})
        self.assertEqual(result["instructions"], "Repair instructions")
        self.assertEqual(result["prompt_template"], "Repair {context}")


if __name__ == "__main__":
    unittest.main()
