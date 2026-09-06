#!/usr/bin/env python3
"""Product-owned deployment boundary; invoked by the Cell coordinator."""
import json
import os
from pathlib import Path
import plistlib
import re
import subprocess
import sys

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from deployment.adapter_support import ProductAdapter, MaintainedAdapter, Stopped, command, digest, main


class TodoAdapter(MaintainedAdapter):
    """Todo owns its daily-email service; deployment never changes its pause."""

    launchctl = "/bin/launchctl"

    def prove_schedule_definition(self):
        plist = self.home / "Library/LaunchAgents/org.todo.daily-email.plist"
        digest(plist)
        if plist.parent.is_symlink() or plist.stat().st_uid != os.getuid():
            raise Stopped("Todo email schedule plist is not owned by this installation")
        selected = os.readlink(self.install / "current")
        if not re.fullmatch(r"releases/[0-9a-f]{64}", selected):
            raise Stopped("invalid Todo selection for schedule proof")
        template = self.install / selected / "package/org.todo.daily-email.plist"
        digest(template)
        try:
            expected = plistlib.loads(template.read_bytes())
            expected["WorkingDirectory"] = str(self.install.parent)
            expected["EnvironmentVariables"]["HOME"] = str(self.home)
            expected["ProgramArguments"][1] = str(self.install / "current/bin/todo-daily-email")
            logs = self.home / "Library/Logs/Todo"
            expected["StandardOutPath"] = str(logs / "email.stdout.log")
            expected["StandardErrorPath"] = str(logs / "email.stderr.log")
            actual = plistlib.loads(plist.read_bytes())
        except (plistlib.InvalidFileException, IndexError, KeyError, TypeError) as error:
            raise Stopped("invalid Todo email schedule definition") from error
        if actual != expected:
            raise Stopped("Todo email schedule plist differs from the owned release definition")

    def schedule_state(self):
        self.prove_schedule_definition()
        domain = f"gui/{os.getuid()}"
        disabled = command([self.launchctl, "print-disabled", domain])
        if "disabled services = {" not in disabled:
            raise Stopped("unrecognized Todo schedule disabled state")
        matches = re.findall(r'^\s*"org\.todo\.daily-email"\s*=>\s*(\S+)\s*$', disabled, re.MULTILINE)
        if len(matches) > 1 or any(value not in {"true", "false"} for value in matches):
            raise Stopped("unrecognized Todo schedule disabled override")
        # launchctl's missing-service result is 113. A failed query is not proof
        # of an unloaded schedule. These bounded reads never change launchd.
        try:
            service = subprocess.run([self.launchctl, "print", domain + "/org.todo.daily-email"],
                                     stdin=subprocess.DEVNULL, capture_output=True, timeout=30)
        except (OSError, subprocess.TimeoutExpired) as error:
            raise Stopped("unable to inspect Todo email schedule") from error
        if service.returncode not in {0, 113}:
            raise Stopped("unable to prove Todo email schedule load state")
        return {"loaded": service.returncode == 0, "disabled": matches == ["true"]}

    def runtime_inspect(self):
        value = super().runtime_inspect()
        value["schedule"] = self.schedule_state()
        return value

    def runtime_readiness(self):
        expected = self.prior.get("schedule")
        if expected is None or self.schedule_state() != expected:
            # In particular, a killed installer must not release its hold after
            # bootout but before bootstrap. Nor may recovery erase a new pause.
            raise Stopped("Todo email schedule differs from the captured operator state; hold retained")
        return super().runtime_readiness()

    def apply(self):
        self.runtime_readiness()
        return super().apply()

    def recover(self):
        self.runtime_readiness()
        return super().recover()

    def release(self):
        self.runtime_readiness()
        return super().release()


if __name__ == "__main__":
    raise SystemExit(main(TodoAdapter, json.loads(Path(__file__).with_name("adapter.json").read_text())))
