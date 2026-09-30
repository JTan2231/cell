"""Provision and identify Cell's fixed test runner on external work storage."""

from __future__ import annotations

import argparse
import hashlib
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
ARCHIVE_SHA256 = "39785160b3c2f6ed9a765049cf4fa79f3b39aa02eb7598a5a0e2a1a0b9ffb9a8"
BINARY_SHA256 = "7a558b157d164ab4fb6cb1a48cbac5a57b7b8ad99f5d3492eb6eda64faf91df0"
URL = (f"https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-{VERSION}/"
       f"cargo-nextest-{VERSION}-universal-apple-darwin.tar.gz")


def digest(path: Path) -> str:
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()


def runner() -> Path:
    if sys.platform != "darwin":
        raise ValueError("Cell's pinned nextest runner requires macOS")
    path = workspace.root() / "tools" / "nextest" / VERSION / "cargo-nextest"
    if (path.is_symlink() or not path.is_file() or not os.access(path, os.X_OK)
            or digest(path) != BINARY_SHA256):
        raise ValueError("pinned nextest is absent or changed; run python3 pipeline/nextest_tool.py install")
    return path


def identity(path: Path) -> dict[str, str]:
    selected = runner()
    if path != selected:
        raise ValueError("the test command does not select Cell's pinned nextest runner")
    return {"path": str(selected), "version": VERSION, "sha256": BINARY_SHA256}


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
            if digest(archive) != ARCHIVE_SHA256:
                raise ValueError("nextest download does not match the pinned archive")
            candidate = Path(temporary) / "cargo-nextest"
            with tarfile.open(archive) as package:
                member = package.getmember("cargo-nextest")
                if not member.isfile():
                    raise ValueError("nextest archive has no regular executable")
                with package.extractfile(member) as source, candidate.open("xb") as destination:
                    shutil.copyfileobj(source, destination)
            if digest(candidate) != BINARY_SHA256:
                raise ValueError("nextest executable does not match the pinned binary")
            candidate.chmod(0o700)
            result = subprocess.run([str(candidate), "--version"], check=True,
                                    capture_output=True, text=True, timeout=30)
            if result.stdout.split()[:2] != ["cargo-nextest", VERSION]:
                raise ValueError("unexpected nextest version")
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
