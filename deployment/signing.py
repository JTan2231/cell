#!/usr/bin/env python3
"""Persistent current-user macOS signing policy for Cell production artifacts."""

from __future__ import annotations

import argparse
from contextlib import contextmanager, ExitStack
import fcntl
import json
import os
from pathlib import Path
import pwd
import re
import stat
import subprocess
import sys
import tempfile
from typing import Any, Iterator, Sequence

sys.dont_write_bytecode = True
if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

NAME = re.compile(r"[a-z][a-z0-9-]*")
NAMESPACE = re.compile(r"[a-z][a-z0-9-]*(?:\.[a-z][a-z0-9-]*)+")
FINGERPRINT = re.compile(r"[0-9a-fA-F]{40}")
NATIVE_MAGIC = {bytes.fromhex(value) for value in (
    "feedface", "cefaedfe", "feedfacf", "cffaedfe", "cafebabe", "bebafeca", "cafebabf", "bfbafeca")}


class SigningError(RuntimeError):
    """Signing configuration or its exact key cannot be used."""


def home() -> Path:
    return Path(pwd.getpwuid(os.getuid()).pw_dir)


def config_path() -> Path:
    return home() / "Library/Application Support/Cell/signing.json"


def json_bytes(value: Any) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode()


def _object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result = {}
    for name, value in pairs:
        if name in result:
            raise SigningError("duplicate signing configuration field")
        result[name] = value
    return result


def validate_policy(value: Any) -> dict[str, Any]:
    if not isinstance(value, dict) or set(value) != {"schema", "macos"} or type(value["schema"]) is not int or value["schema"] != 1:
        raise SigningError("unsupported Cell signing policy; expected schema 1")
    settings = value["macos"]
    if not isinstance(settings, dict) or set(settings) != {"profile", "certificate_sha1", "keychain", "identifier_namespace"}:
        raise SigningError("invalid macOS signing configuration")
    if settings["profile"] != "local":
        raise SigningError("unsupported signing profile; this release supports local signing")
    fingerprint = settings["certificate_sha1"]
    namespace = settings["identifier_namespace"]
    keychain = settings["keychain"]
    if not isinstance(fingerprint, str) or not FINGERPRINT.fullmatch(fingerprint):
        raise SigningError("certificate_sha1 must contain exactly 40 hexadecimal digits")
    if not isinstance(namespace, str) or not NAMESPACE.fullmatch(namespace):
        raise SigningError("identifier_namespace must be a permanent dotted lower-case identifier")
    if not isinstance(keychain, str) or not Path(keychain).is_absolute() or any(c in keychain for c in "\x00\n\r"):
        raise SigningError("keychain must be an absolute path")
    return {"schema": 1, "macos": {**settings, "certificate_sha1": fingerprint.lower()}}


def _private_file(path: Path) -> None:
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1 or info.st_uid != os.getuid() or stat.S_IMODE(info.st_mode) != 0o600:
        raise SigningError(f"signing configuration must be an owned non-symbolic file with mode 0600: {path}")


def load_policy() -> dict[str, Any] | None:
    if sys.platform != "darwin":
        return None
    path = config_path()
    try:
        _private_file(path)
        if path.stat().st_size > 65536:
            raise SigningError("Cell signing configuration exceeds its size limit")
        return validate_policy(json.loads(path.read_text(), object_pairs_hook=_object))
    except FileNotFoundError as error:
        raise SigningError("Cell signing is not configured; run cell-ci signing create-local or configure") from error
    except (OSError, ValueError) as error:
        raise SigningError("cannot read Cell signing configuration") from error


def assert_current(policy: dict[str, Any] | None) -> None:
    normalized = validate_policy(policy) if policy is not None else None
    if load_policy() != normalized:
        raise SigningError("Cell signing policy changed after admission; publication stopped")


def _run(command: list[str], *, timeout: int = 30) -> subprocess.CompletedProcess[str]:
    try:
        result = subprocess.run(command, capture_output=True, text=True, timeout=timeout, check=False)
    except (OSError, subprocess.TimeoutExpired) as error:
        raise SigningError("signing tool unavailable or key access timed out; unlock and authorize the configured Keychain") from error
    if result.returncode:
        # Commands here never include credential bytes in diagnostics. Do not
        # expose subprocess arguments, particularly during private-key import.
        detail = (result.stderr or result.stdout).strip()[-1500:]
        raise SigningError(f"{Path(command[0]).name} failed: {detail}")
    return result


def preflight(policy: dict[str, Any] | None) -> None:
    if policy is None:
        if sys.platform == "darwin":
            raise SigningError("macOS production preparation requires a signing policy")
        return
    settings = validate_policy(policy)["macos"]
    path = Path(settings["keychain"])
    try:
        info = path.lstat()
    except OSError as error:
        raise SigningError("configured Keychain is unavailable; identity was not changed") from error
    if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid():
        raise SigningError("configured Keychain is not an owned regular file")
    output = _run(["/usr/bin/security", "find-identity", "-v", "-p", "codesigning", str(path)]).stdout.lower()
    if not re.search(r"\b" + settings["certificate_sha1"] + r"\b", output):
        raise SigningError("configured certificate/private key is missing, expired, or unavailable; identity was not changed")
    _run(["/usr/bin/codesign", "--dryrun", "--detached", "/dev/null", "--force", "--sign", settings["certificate_sha1"],
          "--keychain", str(path), "--identifier", settings["identifier_namespace"] + ".preflight",
          "--timestamp=none", "/usr/bin/true"])


def identifier(policy: dict[str, Any], product: str, artifact_key: str) -> str:
    if not NAME.fullmatch(product) or not NAME.fullmatch(artifact_key):
        raise SigningError("invalid permanent product or artifact signing key")
    return f"{validate_policy(policy)['macos']['identifier_namespace']}.{product}.{artifact_key}"


def is_native(path: Path) -> bool:
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode):
        raise SigningError("signed artifact must be a regular non-symbolic file")
    with path.open("rb") as stream:
        return stream.read(4) in NATIVE_MAGIC


def requirement(policy: dict[str, Any], product: str, artifact_key: str) -> str:
    return f'identifier "{identifier(policy, product, artifact_key)}" and certificate leaf = H"{policy["macos"]["certificate_sha1"]}"'


def verify(path: Path, policy: dict[str, Any] | None, product: str, artifact_key: str) -> None:
    if policy is None:
        if sys.platform == "darwin":
            raise SigningError("cannot verify macOS production code without its signing policy")
        return
    if not is_native(path):
        raise SigningError("production executable is not native macOS code")
    _run(["/usr/bin/codesign", "--verify", "--strict", "--all-architectures",
          "--test-requirement", "=" + requirement(policy, product, artifact_key), str(path)])


def sign(path: Path, policy: dict[str, Any] | None, product: str, artifact_key: str) -> None:
    if policy is None:
        if sys.platform == "darwin":
            raise SigningError("cannot sign macOS production code without its signing policy")
        return
    assert_current(policy)
    if not is_native(path):
        raise SigningError("production executable is not native macOS code")
    settings = policy["macos"]
    _run(["/usr/bin/codesign", "--force", "--sign", settings["certificate_sha1"],
          "--keychain", settings["keychain"], "--identifier", identifier(policy, product, artifact_key),
          "--requirements", "=designated => " + requirement(policy, product, artifact_key),
          "--timestamp=none", str(path)])
    verify(path, policy, product, artifact_key)


def _directory(path: Path) -> None:
    path.mkdir(parents=True, exist_ok=True, mode=0o700)
    info = path.lstat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid() or stat.S_IMODE(info.st_mode) != 0o700:
        raise SigningError("signing state directory must be owned and mode 0700")


@contextmanager
def configuration_lock() -> Iterator[None]:
    """Serialize explicit configuration writers with queue and deployment admission."""
    from ci_manager import workspace
    from ci_manager.storage import Store, lock
    parent = config_path().parent
    _directory(parent)
    with ExitStack() as stack:
        state = workspace.directory("ci-manager")
        if (state / "queue.sqlite3").exists():
            stack.enter_context(lock(state / "admission.lock", blocking=False))
            store = Store(state)
            stack.callback(store.db.close)
            queued = store.db.execute("SELECT 1 FROM jobs WHERE phase='queued' LIMIT 1").fetchone()
            if not store.get("paused") or store.active() or queued:
                raise SigningError("pause and settle all active and queued CI jobs before changing signing configuration")
            common = Path(store.get("config", {})["common_git_dir"])
            if any((common / name).exists() for name in ("cell-release-publication.lock", "cell-release-publication.lock.d")):
                raise SigningError("settle release publication before changing signing configuration")
        deployment = workspace.directory("deployments")
        _directory(deployment)
        descriptor = os.open(deployment / "deployment.lock", os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
        stack.callback(os.close, descriptor)
        try:
            fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            raise SigningError("settle the active deployment before changing signing configuration") from error
        if (deployment / "active").exists():
            raise SigningError("settle retained deployment recovery before changing signing configuration")
        yield


def _write_policy(policy: dict[str, Any]) -> None:
    path = config_path()
    if path.exists() or path.is_symlink():
        _private_file(path)
    with tempfile.NamedTemporaryFile(dir=path.parent, prefix=".signing-", delete=False) as stream:
        temporary = Path(stream.name)
        try:
            os.fchmod(stream.fileno(), 0o600)
            stream.write(json_bytes(policy))
            stream.flush()
            os.fsync(stream.fileno())
            os.replace(temporary, path)
        finally:
            temporary.unlink(missing_ok=True)
    descriptor = os.open(path.parent, os.O_RDONLY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def configure(fingerprint: str, keychain: Path, namespace: str = "local.cell") -> dict[str, Any]:
    policy = validate_policy({"schema": 1, "macos": {"profile": "local", "certificate_sha1": fingerprint,
                             "keychain": str(keychain), "identifier_namespace": namespace}})
    with configuration_lock():
        preflight(policy)
        _write_policy(policy)
    return policy


def create_local() -> dict[str, Any]:
    """Create one explicitly requested identity; never replace an existing one."""
    keychain = home() / "Library/Keychains/login.keychain-db"
    with configuration_lock():
        if config_path().exists() or config_path().is_symlink():
            raise SigningError("Cell signing is already configured; use configure for an explicit identity change")
        try:
            existing = subprocess.run(["/usr/bin/security", "find-certificate", "-c", "Cell Local Signing", str(keychain)],
                                      capture_output=True, text=True, timeout=30, check=False)
        except (OSError, subprocess.TimeoutExpired) as error:
            raise SigningError("cannot establish whether a Cell signing certificate already exists") from error
        if existing.returncode == 0:
            raise SigningError("Cell Local Signing already exists in Keychain; configure that identity")
        # errSecItemNotFound is security's exit status 44. Other failures do not
        # prove absence and must not create a second identity.
        if existing.returncode != 44:
            raise SigningError("cannot establish whether a Cell signing certificate already exists")
        # Generation material exists only in private temporary staging. The
        # persistent certificate and private key live in the user's Keychain.
        with tempfile.TemporaryDirectory(prefix=".signing-setup-", dir=config_path().parent) as directory:
            staging = Path(directory)
            key = staging / "key.pem"
            certificate = staging / "certificate.pem"
            openssl = "/usr/bin/openssl"
            _run([openssl, "req", "-new", "-newkey", "rsa:3072", "-nodes", "-x509", "-sha256",
                  "-days", "3650", "-subj", "/CN=Cell Local Signing", "-keyout", str(key),
                  "-out", str(certificate), "-addext", "basicConstraints=critical,CA:FALSE",
                  "-addext", "keyUsage=critical,digitalSignature", "-addext", "extendedKeyUsage=critical,codeSigning"])
            certificate.chmod(0o600)
            # Import PEM certificate and private key separately, avoiding a
            # password argument or broad '-A' access on the key.
            _run(["/usr/bin/security", "import", str(certificate), "-k", str(keychain)])
            _run(["/usr/bin/security", "import", str(key), "-k", str(keychain), "-T", "/usr/bin/codesign"])
            _run(["/usr/bin/security", "add-trusted-cert", "-r", "trustRoot", "-p", "codeSign",
                  "-k", str(keychain), str(certificate)], timeout=60)
            output = _run([openssl, "x509", "-in", str(certificate), "-noout", "-fingerprint", "-sha1"]).stdout
            fingerprint = output.strip().split("=", 1)[-1].replace(":", "").lower()
            policy = validate_policy({"schema": 1, "macos": {"profile": "local", "certificate_sha1": fingerprint,
                                     "keychain": str(keychain), "identifier_namespace": "local.cell"}})
            preflight(policy)
            _write_policy(policy)
        return {"policy": policy}


def run_cli(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    status = commands.add_parser("status")
    status.add_argument("--json", action="store_true", help="return structured JSON")
    create = commands.add_parser("create-local")
    create.add_argument("--json", action="store_true", help="return structured JSON")
    command = commands.add_parser("configure")
    command.add_argument("--json", action="store_true", help="return structured JSON")
    command.add_argument("--certificate-sha1", required=True)
    command.add_argument("--keychain", type=Path, required=True)
    command.add_argument("--identifier-namespace", default="local.cell")
    args = parser.parse_args(argv)
    try:
        if sys.platform != "darwin":
            raise SigningError("Cell signing setup requires the current macOS user")
        if args.command == "status":
            policy = load_policy()
            preflight(policy)
            result = {"policy": policy, "ready": True}
        elif args.command == "create-local":
            result = create_local()
        else:
            policy = configure(args.certificate_sha1, args.keychain, args.identifier_namespace)
            result = {"policy": policy}
        if args.json:
            print(json.dumps(result, sort_keys=True))
        else:
            settings = result["policy"]["macos"]
            state = "ready" if args.command == "status" else "configured"
            print(f"Cell macOS signing {state}: {settings['certificate_sha1']}")
            print(f"Identifier namespace: {settings['identifier_namespace']}")
            print(f"Keychain: {settings['keychain']}")
        return 0
    except (SigningError, OSError, ValueError, RuntimeError) as error:
        print(f"cell-signing: {error}", file=sys.stderr)
        return 78


if __name__ == "__main__":
    raise SystemExit(run_cli())
