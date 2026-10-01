"""Retain a foreground child's exit evidence independently of the manager."""

from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import sys
import time

sys.dont_write_bytecode = True
if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from ci_manager.storage import atomic_json


def validation_exited(directory: Path, job: dict) -> bool:
    """Prove the current validator exited, including an already recorded result."""
    validations = job.get("validations", [])
    ordinal = len(validations)
    name = f"validation-{ordinal}"
    if not any((directory / f"{name}.{suffix}").exists()
               for suffix in ("request.json", "result.json", "started.json", "stdout", "stderr",
                              "json", "log")):
        if not validations:
            return False
        ordinal -= 1
        name = f"validation-{ordinal}"
        if (validations[-1].get("candidate") != job["candidate_commit"]
                or validations[-1].get("receipt") != str(directory / f"{name}.json")):
            return False
    worktree = directory / "worktree"
    request_path = directory / f"{name}.request.json"
    try:
        request = json.loads(request_path.read_text())
        result = json.loads((directory / f"{name}.result.json").read_text())
        if not isinstance(request, dict) or not isinstance(result, dict):
            return False
        command = request["command"]
        expected = [str(worktree / "pipeline/select_changes.py"), "run",
                    "--base", job["base_commit"],
                    "--candidate", job["candidate_commit"], "--json"]
        if job.get("skip_tests", False):
            expected.append("--skip-tests")
        # Retain exact command correlation for both validator generations.
        if isinstance(command, list) and command[-2:] == [
                "--autofix-patch", str(directory / f"{name}.autofix.patch")]:
            expected.extend(command[-2:])
        return (isinstance(command, list) and len(command) == len(expected) + 1
                and isinstance(command[0], str) and Path(command[0]).is_absolute()
                and command[1:] == expected
                and request["cwd"] == str(worktree)
                and all(request[key] == str(directory / f"{name}.{suffix}")
                        for key, suffix in (("stdout", "stdout"), ("stderr", "stderr"),
                                            ("started", "started.json"), ("result", "result.json")))
                and result["request"] == str(request_path)
                and type(result.get("exit_code")) is int)
    except (OSError, ValueError, KeyError, TypeError):
        return False


def main() -> int:
    request_path, descriptor = Path(sys.argv[1]), int(sys.argv[2])
    request = json.loads(request_path.read_text())
    output = Path(request["stdout"])
    error = Path(request["stderr"])
    environment = dict(os.environ, PYTHONDONTWRITEBYTECODE="1", CHANCERY_USAGE_INTERNAL="1")
    result = {"request": str(request_path), "started": time.time(), "supervisor_pid": os.getpid()}
    try:
        with output.open("xb") as stdout, error.open("xb") as stderr:
            os.chmod(output, 0o600)
            os.chmod(error, 0o600)
            child = subprocess.Popen(request["command"], cwd=request["cwd"], env=environment,
                                     stdout=stdout, stderr=stderr, pass_fds=(descriptor,),
                                     start_new_session=True)
            result["pid"] = child.pid
            atomic_json(Path(request["started"]), result)
            result["exit_code"] = child.wait()
            stdout.flush()
            stderr.flush()
            os.fsync(stdout.fileno())
            os.fsync(stderr.fileno())
    except Exception as exception:
        result["error"] = str(exception)
    result["finished"] = time.time()
    atomic_json(Path(request["result"]), result)
    return 0 if "exit_code" in result else 1


if __name__ == "__main__":
    raise SystemExit(main())
