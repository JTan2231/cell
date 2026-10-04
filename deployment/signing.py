#!/usr/bin/env python3
"""Read and apply the selected macOS signing policy to production artifacts."""

from __future__ import annotations

import json
import os
from pathlib import Path
import pwd
import re
import stat
import subprocess
import sys
from typing import Any

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
        raise SigningError("Cell signing is not configured; run telete signing create-local or telete signing configure --host") from error
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
    settings = policy["macos"]
    _run(["/usr/bin/codesign", "--force", "--sign", settings["certificate_sha1"],
          "--keychain", settings["keychain"], "--identifier", identifier(policy, product, artifact_key),
          "--requirements", "=designated => " + requirement(policy, product, artifact_key),
          "--timestamp=none", str(path)])
