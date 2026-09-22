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
