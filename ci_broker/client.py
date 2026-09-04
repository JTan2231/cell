#!/usr/bin/env python3
"""Repository-facing client for the Cell CI broker."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import shutil
import stat
import subprocess
import sys
from pathlib import Path
from typing import Sequence

try:
    import pwd
except ImportError:  # pragma: no cover - the CI body gate is POSIX-only
    pwd = None  # type: ignore[assignment]

# Source identity includes untracked files.  The repository-facing client must
# therefore never create __pycache__ as a side effect of measuring that identity.
sys.dont_write_bytecode = True

if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from ci_broker import broker


SNAPSHOT_VERSION = 1
DEFAULT_CARGO_JOBS = 2


def canonical_state_dir() -> Path:
    """Return the one production journal location for the current user."""

    home = (
        Path(pwd.getpwuid(os.getuid()).pw_dir)
        if pwd is not None
        else Path.home()
    )
    if sys.platform == "darwin":
        return home / "Library" / "Application Support" / "Cell" / "ci-broker"
    return home / ".local" / "state" / "cell" / "ci-broker"


def bootstrap_cargo_path() -> None:
    """Match the product gates' existing rustup PATH bootstrap."""

    if shutil.which("cargo") is not None and shutil.which("rustc") is not None:
        return
    cargo_home = os.environ.get("CARGO_HOME")
    if cargo_home is None:
        home = os.environ.get("HOME")
        if home:
            cargo_home = str(Path(home) / ".cargo")
    if not cargo_home:
        return
    cargo_bin = Path(cargo_home).expanduser() / "bin"
    if (cargo_bin / "cargo").is_file():
        existing = os.environ.get("PATH")
        os.environ["PATH"] = os.pathsep.join(
            part for part in (str(cargo_bin), existing) if part
        )


def git(repository: Path, *arguments: str) -> bytes:
    try:
        result = subprocess.run(
            ("git", "-C", str(repository), *arguments),
            check=False,
            capture_output=True,
            timeout=30,
        )
    except (OSError, subprocess.SubprocessError) as error:
        raise broker.BrokerError(f"cannot inspect Git repository: {error}") from error
    if result.returncode != 0:
        message = result.stderr.decode("utf-8", "replace").strip()
        raise broker.BrokerError(message or "Git repository inspection failed")
    return result.stdout


def repository_root(path: Path) -> Path:
    raw = git(path, "rev-parse", "--show-toplevel").decode("utf-8", "strict").strip()
    root = Path(raw).resolve()
    if not root.is_dir():
        raise broker.BrokerError(f"Git worktree root does not exist: {root}")
    return root


def common_git_directory(root: Path) -> Path:
    try:
        raw = git(
            root, "rev-parse", "--path-format=absolute", "--git-common-dir"
        ).decode("utf-8", "strict").strip()
        common = Path(raw)
    except broker.BrokerError:
        raw = git(root, "rev-parse", "--git-common-dir").decode(
            "utf-8", "strict"
        ).strip()
        common = Path(raw)
        if not common.is_absolute():
            common = root / common
    common = common.resolve()
    if not common.is_dir():
        raise broker.BrokerError(f"Git common directory does not exist: {common}")
    return common


def logical_repository_id(common: Path) -> str:
    identity = hashlib.sha256(os.fsencode(str(common.resolve()))).hexdigest()
    return f"git-common-sha256:{identity}"


def update_field(hasher: "hashlib._Hash", value: bytes) -> None:
    hasher.update(len(value).to_bytes(8, "big"))
    hasher.update(value)


def source_snapshot(root: Path) -> tuple[str, bool]:
    """Hash every tracked/untracked non-ignored source path without temp files."""

    status_arguments = (
        "status",
        "--porcelain=v1",
        "-z",
        "--untracked-files=all",
        "--ignore-submodules=none",
    )
    status_before = git(root, *status_arguments)
    head = git(root, "rev-parse", "--verify", "HEAD").strip()
    paths = git(
        root,
        "ls-files",
        "-z",
        "--cached",
        "--others",
        "--exclude-standard",
    ).split(b"\x00")
    paths = sorted(path for path in paths if path)

    hasher = hashlib.sha256()
    update_field(hasher, f"cell-source-snapshot-v{SNAPSHOT_VERSION}".encode())
    update_field(hasher, head)
    for raw_path in paths:
        relative_text = os.fsdecode(raw_path)
        relative = Path(relative_text)
        if relative.is_absolute() or ".." in relative.parts:
            raise broker.BrokerError("Git returned a path outside the worktree")
        path = root / relative
        update_field(hasher, raw_path)
        try:
            metadata = path.lstat()
        except FileNotFoundError:
            update_field(hasher, b"missing")
            continue
        if stat.S_ISREG(metadata.st_mode):
            update_field(hasher, b"file")
            update_field(hasher, oct(stat.S_IMODE(metadata.st_mode)).encode())
            try:
                with path.open("rb") as source:
                    while True:
                        chunk = source.read(1024 * 1024)
                        if not chunk:
                            break
                        hasher.update(chunk)
            except OSError as error:
                raise broker.BrokerError(
                    f"cannot hash source path {relative_text!r}: {error}"
                ) from error
        elif stat.S_ISLNK(metadata.st_mode):
            update_field(hasher, b"symlink")
            update_field(hasher, os.fsencode(os.readlink(path)))
        elif stat.S_ISDIR(metadata.st_mode):
            # A path returned by ls-files as a directory is normally a Git
            # submodule.  Partial hashing would be unsafe for CI identity.
            raise broker.BrokerError(
                f"cannot establish an exact source identity for directory {relative_text!r}"
            )
        else:
            raise broker.BrokerError(
                f"unsupported source path type for {relative_text!r}"
            )

    status_after = git(root, *status_arguments)
    key = f"sha256:{hasher.hexdigest()}"
    return key, status_before == b"" and status_after == b""


def toolchain_key() -> str:
    facts: dict[str, str] = {
        "platform": platform.platform(),
        "python": sys.version,
    }
    for command, arguments in (("git", ("--version",)), ("cargo", ("-Vv",)), ("rustc", ("-vV",))):
        executable = shutil.which(command)
        if executable is None:
            raise broker.BrokerError(f"required toolchain command is unavailable: {command}")
        try:
            result = subprocess.run(
                (executable, *arguments),
                check=False,
                capture_output=True,
                text=True,
                timeout=30,
            )
        except (OSError, subprocess.SubprocessError) as error:
            raise broker.BrokerError(f"cannot identify {command}: {error}") from error
        if result.returncode != 0:
            raise broker.BrokerError(f"cannot identify {command} toolchain")
        facts[f"{command}_path"] = str(Path(executable).resolve())
        facts[f"{command}_version"] = result.stdout.strip()
    return f"sha256:{broker.digest_json(facts)}"


def cargo_jobs() -> int:
    raw = os.environ.get("CELL_CI_CARGO_JOBS", str(DEFAULT_CARGO_JOBS))
    try:
        jobs = int(raw)
    except ValueError as error:
        raise broker.BrokerError("CELL_CI_CARGO_JOBS must be a positive integer") from error
    if jobs < 1:
        raise broker.BrokerError("CELL_CI_CARGO_JOBS must be a positive integer")
    return jobs


def client_parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(description=__doc__)
    commands = root.add_subparsers(dest="subcommand", required=True)

    run = commands.add_parser("run", help="run one repository CI body through the broker")
    run.add_argument("--gate", required=True)
    run.add_argument("--lane", choices=("heavy", "light"), default="heavy")
    run.add_argument("--gate-version")
    run.add_argument(
        "--expected-source-key",
        help="bind this gate to a source key captured by an enclosing root plan",
    )
    run.add_argument("--repo-root", default=os.getcwd())
    run.add_argument("--cwd")
    run.add_argument("--full-environment", action="store_true")
    run.add_argument("--inherit-env", action="append", default=[])
    run.add_argument("--env", action="append", default=[])
    run.add_argument("--unset-env", action="append", default=[])
    run.add_argument("--attribution-json")
    run.add_argument(
        "--verbose-receipt",
        action="store_true",
        help="print the JSON receipt on success as well as failure",
    )
    run.add_argument("command", nargs=argparse.REMAINDER)

    source = commands.add_parser("source-key", help="print the exact current source key")
    source.add_argument("--repo-root", default=os.getcwd())
    source.add_argument("--show-clean", action="store_true")

    status = commands.add_parser("status", help="read one durable execution receipt")
    add_client_scope_arguments(status)
    status.add_argument("--events", action="store_true")
    status.add_argument("execution_id")

    recover = commands.add_parser("recover", help="recover abandoned broker work")
    add_client_scope_arguments(recover)
    return root


def add_client_scope_arguments(command: argparse.ArgumentParser) -> None:
    command.add_argument("--repo-root", default=os.getcwd())


def broker_scope_arguments(arguments: argparse.Namespace) -> list[str]:
    root = repository_root(Path(arguments.repo_root).expanduser().resolve())
    common = common_git_directory(root)
    return [
        "--repo-id",
        logical_repository_id(common),
        "--host-id",
        broker.default_host_id(),
        "--state-dir",
        str(canonical_state_dir()),
        "--light-slots",
        str(broker.DEFAULT_LIGHT_SLOTS),
        "--cargo-jobs",
        str(cargo_jobs()),
        "--heartbeat-timeout",
        str(broker.DEFAULT_HEARTBEAT_TIMEOUT),
        "--poll-interval",
        str(broker.DEFAULT_POLL_INTERVAL),
    ]


def run(arguments: argparse.Namespace) -> int:
    bootstrap_cargo_path()
    root = repository_root(Path(arguments.repo_root).expanduser().resolve())
    common = common_git_directory(root)
    cwd = (
        Path(arguments.cwd).expanduser().resolve()
        if arguments.cwd is not None
        else root
    )
    try:
        cwd.relative_to(root)
    except ValueError as error:
        raise broker.BrokerError("CI body working directory must be inside the worktree") from error
    if not cwd.is_dir():
        raise broker.BrokerError(f"CI body working directory does not exist: {cwd}")

    command = broker.strip_separator(arguments.command)
    current_source_key, clean = source_snapshot(root)
    expected_source_key = (
        arguments.expected_source_key
        if arguments.expected_source_key is not None
        else os.environ.get("CELL_CI_EXPECTED_SOURCE_KEY")
    )
    source_key = (
        broker.require_text("--expected-source-key", expected_source_key)
        if expected_source_key is not None
        else current_source_key
    )
    if expected_source_key is not None and current_source_key != source_key:
        print(
            json.dumps(
                {
                    "protocol_version": broker.PROTOCOL_VERSION,
                    "execution_id": None,
                    "state": "stale",
                    "execution_state": "stale",
                    "gate": arguments.gate,
                    "source_key": source_key,
                    "observed_source_key": current_source_key,
                    "detail": "expected source identity does not match",
                },
                sort_keys=True,
            )
        )
        return 75
    jobs = cargo_jobs()
    source_check = [
        sys.executable,
        str(Path(__file__).resolve()),
        "source-key",
        "--repo-root",
        str(root),
    ]
    gate_version = arguments.gate_version
    if gate_version is None:
        relative_cwd = cwd.relative_to(root).as_posix() or "."
        gate_version = "sha256:" + broker.digest_json(
            {
                "gate": arguments.gate,
                "source_key": source_key,
                "command": broker.normalize_argv(command, root),
                "cwd": relative_cwd,
                "protocol_version": broker.PROTOCOL_VERSION,
            }
        )
    primary_root = common.parent
    target = primary_root / "target"

    broker_arguments = [
        "run",
        "--repo-id",
        logical_repository_id(common),
        "--host-id",
        broker.default_host_id(),
        "--state-dir",
        str(canonical_state_dir()),
        "--light-slots",
        str(broker.DEFAULT_LIGHT_SLOTS),
        "--cargo-jobs",
        str(jobs),
        "--heartbeat-timeout",
        str(broker.DEFAULT_HEARTBEAT_TIMEOUT),
        "--poll-interval",
        str(broker.DEFAULT_POLL_INTERVAL),
        "--source-key",
        source_key,
        "--gate",
        arguments.gate,
        "--gate-version",
        gate_version,
        "--toolchain-key",
        toolchain_key(),
        "--lane",
        arguments.lane,
        "--cwd",
        str(cwd),
        "--identity-root",
        str(root),
        "--source-check-json",
        json.dumps(source_check),
        "--environment-mode",
        "full" if arguments.full_environment else "minimal",
        "--env",
        f"CARGO_TARGET_DIR={target}",
        "--env",
        "CARGO_INCREMENTAL=0",
        "--unset-env",
        "CELL_CI_CARGO_JOBS",
    ]
    for name in arguments.inherit_env:
        broker_arguments.extend(("--inherit-env", name))
    for assignment in arguments.env:
        broker_arguments.extend(("--env", assignment))
    for name in arguments.unset_env:
        broker_arguments.extend(("--unset-env", name))
    if arguments.attribution_json is not None:
        broker_arguments.extend(("--attribution-json", arguments.attribution_json))
    if clean and current_source_key == source_key:
        broker_arguments.append("--share-clean-candidate")
    if not arguments.verbose_receipt:
        broker_arguments.append("--quiet-success")
    broker_arguments.extend(("--", *command))
    return broker.main(broker_arguments)


def source_key_command(arguments: argparse.Namespace) -> int:
    root = repository_root(Path(arguments.repo_root).expanduser().resolve())
    key, clean = source_snapshot(root)
    if arguments.show_clean:
        print(json.dumps({"clean": clean, "source_key": key}, sort_keys=True))
    else:
        print(key)
    return 0


def status_command(arguments: argparse.Namespace) -> int:
    command = ["status", *broker_scope_arguments(arguments)]
    if arguments.events:
        command.append("--events")
    command.append(arguments.execution_id)
    return broker.main(command)


def recover_command(arguments: argparse.Namespace) -> int:
    return broker.main(["recover", *broker_scope_arguments(arguments)])


def main(argv: Sequence[str] | None = None) -> int:
    arguments = client_parser().parse_args(argv)
    try:
        if arguments.subcommand == "run":
            return run(arguments)
        if arguments.subcommand == "source-key":
            return source_key_command(arguments)
        if arguments.subcommand == "status":
            return status_command(arguments)
        if arguments.subcommand == "recover":
            return recover_command(arguments)
        raise broker.BrokerError(f"unsupported subcommand: {arguments.subcommand}")
    except broker.BrokerError as error:
        print(f"cell-ci: broker error: {error}", file=sys.stderr)
        return 78


if __name__ == "__main__":
    raise SystemExit(main())
