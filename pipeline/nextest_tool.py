"""Provision and identify Cell's fixed test runner on external work storage."""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from ci_manager import workspace
from ci_manager.storage import lock


VERSION = "0.9.146"
URL = (f"https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-{VERSION}/"
       f"cargo-nextest-{VERSION}-universal-apple-darwin.tar.gz")


def runner() -> Path:
    if sys.platform != "darwin":
        raise ValueError("Cell's pinned nextest runner requires macOS")
    path = workspace.root() / "tools" / "nextest" / VERSION / "cargo-nextest"
    if path.is_symlink() or not path.is_file() or not os.access(path, os.X_OK):
        raise ValueError("pinned nextest is unavailable; run python3 pipeline/nextest_tool.py install")
    return path


def identity(path: Path) -> dict[str, str]:
    selected = runner()
    if path != selected:
        raise ValueError("the test command does not select Cell's pinned nextest runner")
    return {"path": str(selected), "version": VERSION}


def install() -> Path:
    if sys.platform != "darwin":
        raise ValueError("Cell's pinned nextest runner requires macOS")
    directory = workspace.directory(f"tools/nextest/{VERSION}")
    with lock(directory / "install.lock"):
        path = directory / "cargo-nextest"
        if path.exists() or path.is_symlink():
            return runner()
        with tempfile.TemporaryDirectory(prefix="install-", dir=directory) as temporary:
            archive = Path(temporary) / "nextest.tar.gz"
            subprocess.run(["curl", "--fail", "--silent", "--show-error", "--location",
                            "--max-time", "120", URL, "--output", str(archive)], check=True)
            candidate = Path(temporary) / "cargo-nextest"
            with tarfile.open(archive) as package:
                member = package.getmember("cargo-nextest")
                if not member.isfile():
                    raise ValueError("nextest archive has no regular executable")
                with package.extractfile(member) as source, candidate.open("xb") as destination:
                    shutil.copyfileobj(source, destination)
            candidate.chmod(0o700)
            subprocess.run([str(candidate), "--version"], check=True,
                           capture_output=True, text=True, timeout=30)
            os.replace(candidate, path)
        return runner()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("install", "path"))
    options = parser.parse_args()
    try:
        print(install() if options.command == "install" else runner())
        return 0
    except (OSError, ValueError, subprocess.SubprocessError, tarfile.TarError) as error:
        print(f"nextest: {error}", file=sys.stderr)
        return 78


if __name__ == "__main__":
    raise SystemExit(main())
