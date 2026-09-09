#!/usr/bin/python3
"""Isolated command-boundary fixture; never starts a scheduled program."""
import hashlib
import json
import os
from pathlib import Path
import sys

home = Path(os.environ["HOME"])
args = sys.argv[1:]
assert args.pop(0) == "--json"
binding_file = home / "clockwork-binding.json"
definitions = home / "clockwork-definitions"
definitions.mkdir(exist_ok=True)
binding = json.loads(binding_file.read_text()) if binding_file.exists() else None


def reply(data):
    print(json.dumps({"ok": True, "data": data}))


if args[:2] == ["binding", "list"]:
    reply({"output_version": 2, "items": [binding] if binding else [], "has_more": False})
elif args[:2] == ["definition", "register"]:
    source = Path(args[2]).read_text()
    # The fixture accepts the generated scalar/table subset only; production
    # TOML validation belongs to Clockwork's own tests.
    manifest = {}
    table = manifest
    for line in source.splitlines():
        line = line.strip()
        if not line:
            continue
        if line.startswith("["):
            table = manifest.setdefault(line[1:-1], {})
        else:
            key, value = line.split("=", 1)
            table[key.strip()] = json.loads(value.strip())
    assert manifest["schema_version"] == 2
    assert manifest.get("failure", {}).get("on_abend", "halt-until-approved") == "halt-until-approved"
    digest = hashlib.sha256(source.encode()).hexdigest()
    record = {"digest": digest, "key": manifest["key"], "registered_at": 1, "manifest": manifest}
    (definitions / (digest + ".json")).write_text(json.dumps(record))
    reply(record)
elif args[:2] == ["definition", "show"]:
    reply(json.loads((definitions / (args[2] + ".json")).read_text()))
elif args[0] == "binding" and args[1] in ["switch", "disable"]:
    if (home / "fail-switch").exists() and args[1] == "switch":
        (home / "fail-switch").unlink()
        sys.exit(1)
    binding = binding or {"key": args[2], "definition_digest": None, "enabled": False,
                          "updated_at": 1, "halted_incident": None, "failure_policy_active": False}
    if args[1] == "switch":
        binding["definition_digest"] = args[3]
        binding["enabled"] = True
        assert not (home / "loaded").exists(), "dual activation"
        (home / "clockwork-loaded").touch()
    else:
        binding["enabled"] = False
        if "--select" in args:
            binding["definition_digest"] = args[args.index("--select") + 1]
        (home / "clockwork-loaded").unlink(missing_ok=True)
    binding["failure_policy_active"] = binding["definition_digest"] is not None
    binding_file.write_text(json.dumps(binding))
    reply(binding)
else:
    sys.exit(64)
