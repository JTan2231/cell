"""Discard completed Cell installation history after coordinated success only."""
from __future__ import annotations

import argparse
import contextlib
import json
import os
from pathlib import Path
import plistlib
import pwd
import re
import shutil
import stat
import subprocess
import sys
import tomllib


PRODUCTS = {
    "annals": "Annals", "decisions": "Decisions", "semantics": "Semantics",
    "crm": "CRM", "todo": "Todo", "weaver": "Weaver", "nucleus": "Nucleus",
    "chancery": "Chancery", "clockwork": "Clockwork", "conversations": "Conversations",
    "email": "Email", "usher": "Usher", "cast": "Cast",
    "platter": "Platter", "paperboy": "Paperboy",
}
HEX = re.compile(r"[0-9a-f]{64}")


class CleanupError(RuntimeError):
    """Deployment succeeded, but retained release history could not be pruned."""


def require(condition, message):
    if not condition:
        raise CleanupError(message)


def regular(path):
    info = path.lstat()
    require(stat.S_ISREG(info.st_mode) and info.st_uid == os.getuid()
            and info.st_nlink == 1 and not info.st_mode & 0o022,
            "history contains an unowned or unsafe file")
    return info


def directory(path):
    info = path.lstat()
    require(stat.S_ISDIR(info.st_mode) and info.st_uid == os.getuid()
            and not info.st_mode & 0o022, "installation directory is unsafe")
    return info


def read_record(path):
    regular(path)
    text = path.read_text()
    if path.suffix == ".json":
        result = json.loads(text)
    elif path.suffix == ".toml":
        result = tomllib.loads(text)
    else:
        pairs = [line.split("=", 1) for line in text.splitlines()]
        require(all(len(pair) == 2 for pair in pairs), "invalid installation receipt")
        result = dict(pairs)
        require(len(result) == len(pairs), "duplicate installation receipt fields")
    require(isinstance(result, dict), "invalid installation record")
    return result


def require_deployment_lock(home):
    descriptor = int(os.environ.get("CELL_DEPLOYMENT_LOCK_FD", "-1"))
    require(descriptor >= 3, "cleanup requires the coordinator's deployment lock")
    actual = os.fstat(descriptor)
    expected = regular(home / "Library/Application Support/Cell/deployments/deployment.lock")
    require((actual.st_dev, actual.st_ino) == (expected.st_dev, expected.st_ino),
            "cleanup inherited a different deployment lock")


def inspect_command(argv):
    result = subprocess.run([str(value) for value in argv], stdin=subprocess.DEVNULL,
                            capture_output=True, text=True, timeout=30,
                            env={"HOME": pwd.getpwuid(os.getuid()).pw_dir,
                                 "PATH": "/usr/bin:/bin:/usr/sbin:/sbin"})
    require(result.returncode == 0, "a live release reference could not be inspected")
    return result.stdout


def strings(value):
    if isinstance(value, str):
        yield value
    elif isinstance(value, dict):
        for item in value.values():
            yield from strings(item)
    elif isinstance(value, list):
        for item in value:
            yield from strings(item)


def clockwork_bindings(clockwork):
    """Read a complete inventory from legacy arrays or bounded version-two pages."""
    limit = 20
    while True:
        argv = [clockwork, "--json", "binding", "list"]
        if limit != 20:
            argv.extend(["--limit", str(limit)])
        reply = json.loads(inspect_command(argv))
        require(isinstance(reply, dict) and reply.get("ok") is True,
                "Clockwork binding inventory is unavailable")
        data = reply.get("data")
        if isinstance(data, list):
            require(limit == 20, "Clockwork binding inventory changed format")
            return data
        require(isinstance(data, dict) and data.get("output_version") == 2
                and isinstance(data.get("items"), list)
                and type(data.get("has_more")) is bool,
                "Clockwork binding inventory has an unsupported page format")
        items = data["items"]
        require(len(items) <= limit, "Clockwork binding page exceeds its requested limit")
        if not data["has_more"]:
            return items
        require(len(items) == limit, "Clockwork binding inventory is incomplete")
        limit *= 2


def live_pins(home, installs, currents):
    protected = set(currents.values())
    expression = re.compile(re.escape(str(home / "Library/Application Support"))
                            + r"/(" + "|".join(PRODUCTS.values())
                            + r")/install/(releases/[0-9a-f]{64}|current|previous)(?=/|[\s\"']|$)")

    def retain(value):
        for text in strings(value):
            for match in expression.finditer(text):
                require(match.group(2) != "previous", "a live dependency still uses a previous selector")
                path = Path(match.group(0))
                resolved = path.resolve(strict=True)
                require(resolved.parent == installs[match.group(1)] / "releases"
                        and HEX.fullmatch(resolved.name), "configured release pin is unsafe")
                directory(resolved)
                protected.add(resolved)

    clockwork = home / ".local/bin/clockwork"
    if "Clockwork" in currents:
        for binding in clockwork_bindings(clockwork):
            require(isinstance(binding, dict), "Clockwork binding inventory is invalid")
            selected = binding.get("definition_digest")
            if selected is None:
                continue
            require(isinstance(selected, str) and HEX.fullmatch(selected),
                    "Clockwork selection is invalid")
            definition = json.loads(inspect_command([clockwork, "--json", "definition", "show", selected]))
            require(definition.get("ok") is True and definition.get("data", {}).get("digest") == selected
                    and definition["data"].get("key") == binding.get("key"),
                    "selected Clockwork definition is unavailable")
            # Disabled bindings still select a configured release. Unselected
            # immutable definitions and old activation history are not pins.
            retain(definition["data"]["manifest"])

    agents = home / "Library/LaunchAgents"
    if agents.exists():
        for path in agents.glob("*.plist"):
            require(path.is_file() and not path.is_symlink(), "a LaunchAgent pin is unreadable")
            retain(plistlib.loads(path.read_bytes()))
    for root in (home / ".local/bin", home / ".local/libexec",
                 home / "Library/Application Support/Chancery/providers"):
        if root.exists():
            for path in root.iterdir():
                if path.is_symlink():
                    retain(str(path.resolve(strict=False)))
    base = home / "Library/Application Support"
    for relative in ("Annals/config.toml", "Annals/usage.toml", "Annals/decisions/config.toml",
                     "Todo/config.toml", "Weaver/current.json"):
        path = base / relative
        if path.exists() or path.is_symlink():
            retain(read_record(path))
    if "Decisions" in currents:
        receipt = read_record(installs["Decisions"] / "krisis-observer-binding.txt")
        require(receipt.get("release_id") == currents["Decisions"].name,
                "Krisis dependency receipt is not current")
        retain(receipt)
    # Protect a directly running old binary, mapped executable, or script even
    # when its public selector has since moved to the new release.
    retain(inspect_command(["/usr/sbin/lsof", "-n", "-a", "-u", str(os.getuid()), "-d", "txt", "-Fn"]))
    retain(inspect_command(["/bin/ps", "-axo", "command="]))
    return protected


@contextlib.contextmanager
def installer_locks(home, installs):
    # These are the existing product-deployer locks, not another lock scheme.
    held = []
    file_locks = {"Clockwork", "Conversations", "CRM", "Usher", "Platter"}
    try:
        for application, install in installs.items():
            if not install.exists() and not install.is_symlink():
                continue
            directory(install)
            require(install.resolve(strict=True) == install, "installation ancestry is symbolic")
            lock = (install.parent / ".deploy-lock" if application == "Nucleus" else
                    install.parent / ".update-lock" if application == "Clockwork" else
                    install / ".update-lock")
            require(not lock.exists() and not lock.is_symlink(), "a product installer lock remains")
            if application in file_locks:
                inspect_command(["/usr/bin/shlock", "-p", str(os.getpid()), "-f", lock])
            else:
                lock.mkdir(mode=0o700)
            info = lock.lstat()
            held.append((lock, info.st_dev, info.st_ino, application in file_locks))
        catalog = home / "Library/Application Support/Chancery/.catalog-update-lock"
        if catalog.parent.exists():
            directory(catalog.parent)
            require(catalog.parent.resolve(strict=True) == catalog.parent, "catalog ancestry is symbolic")
            require(not catalog.exists() and not catalog.is_symlink(), "a catalog writer lock remains")
            inspect_command(["/usr/bin/shlock", "-p", str(os.getpid()), "-f", catalog])
            info = catalog.lstat()
            held.append((catalog, info.st_dev, info.st_ino, True))
        yield
    finally:
        for path, device, inode, is_file in reversed(held):
            info = path.lstat()
            require((info.st_dev, info.st_ino) == (device, inode), "cleanup lock ownership changed")
            if is_file:
                require(path.read_text().strip() == str(os.getpid()), "cleanup file lock owner changed")
                path.unlink()
            else:
                path.rmdir()


def trusted_installers(home, installers=None, usher_installer=None):
    """The coordinator supplies admitted candidates; retained releases supply no trust."""
    result = {}
    for supplied, path in (installers or {}).items():
        product = "decisions" if supplied == "krisis" else supplied
        require(product in PRODUCTS and product not in result,
                "history verifier has an unknown or duplicate product")
        result[product] = Path(path)
    if usher_installer is not None:
        require("usher" not in result, "Usher history verifier was supplied twice")
        result["usher"] = Path(usher_installer)
    for path in result.values():
        require(path.is_absolute() and path.resolve(strict=True) == path,
                "history verifier must be an exact sealed candidate path")
        require(not any(path.is_relative_to(home / "Library/Application Support" / application / "install")
                        for application in PRODUCTS.values()),
                "retained installers cannot establish trust for history deletion")
        info = regular(path)
        require(info.st_mode & stat.S_IXUSR, "history verifier is not executable")
    return result


def parse_installers(values):
    result = {}
    for value in values:
        product, separator, path = value.partition("=")
        require(separator and product and path, "use --installer PRODUCT=ABSOLUTE_PATH")
        product = "decisions" if product == "krisis" else product
        require(product in PRODUCTS and product not in result,
                "history verifier has an unknown or duplicate product")
        result[product] = Path(path)
    return result


def clean_installed_release_history(home: Path, usher_installer: Path | None = None, *,
                                    installers: dict[str, Path] | None = None) -> dict:
    """Prune known Cell installation trees using only supplied trusted candidates."""
    require(home.is_absolute() and home.resolve(strict=True) == home,
            "cleanup requires the canonical operator home")
    require_deployment_lock(home)
    verifiers = trusted_installers(home, installers, usher_installer)
    source = Path(__file__).resolve().parents[1]
    base = home / "Library/Application Support"
    installs = {}
    for product, application in PRODUCTS.items():
        metadata = json.loads((source / product / "deployment/adapter.json").read_text())
        require(metadata.get("application") == application, "Cell installation metadata differs")
        installs[application] = base / application / "install"
    with installer_locks(home, installs):
        return prune(home, installs, verifiers)


def lifecycle_barriers(base, application, install):
    if install.exists() or install.is_symlink():
        directory(install)
        require(not any((path.name.startswith(".") and path.name != ".update-lock")
                        or path.name.startswith("transaction.")
                        for path in install.iterdir()),
                "an installer transaction or lock remains")
    markers = {
        "Annals": ["spool/.maintenance", "decisions/spool/.maintenance"],
        "Decisions": [".clockwork-maintenance"],
        "Semantics": [".clockwork-maintenance"],
        "Weaver": [".maintenance"],
    }
    for relative in markers.get(application, []):
        path = base / application / relative
        require(not path.exists() and not path.is_symlink(),
                "product maintenance or migration recovery remains")


def prune(home, installs, verifiers=None):
    base = home / "Library/Application Support"
    currents, selectors, previous, releases, receipts = {}, {}, [], [], []
    verifiers = verifiers or {}
    retained_unverified = set()
    product_history = {}
    for product, application in PRODUCTS.items():
        install = base / application / "install"
        lifecycle_barriers(base, application, install)
        if not install.exists() and not install.is_symlink():
            continue
        for path in (base, base / application, install, install / "releases"):
            directory(path)
        verifier = verifiers.get(product)
        product_history[product] = ("verified" if verifier is not None
                                    else "retained_without_verified_installer")
        for name in ("current", "previous"):
            link = install / name
            if not link.exists() and not link.is_symlink():
                continue
            require(link.is_symlink(), "release selector is not owned")
            target = os.readlink(link)
            require(re.fullmatch(r"releases/[0-9a-f]{64}", target), "release selector is invalid")
            directory(install / target)
            selectors[link] = target
            if name == "current":
                currents[application] = install / target
            elif verifier is not None:
                previous.append(link)
        # An uninstalled retained tree has no current ownership proof.
        require(application in currents, "retained installation has no current release")
        for release in (install / "releases").iterdir():
            directory(release)
            require(HEX.fullmatch(release.name), "unrecognized release directory")
            manifests = [path for path in (release / "manifest.json", release / "manifest.txt") if path.exists()]
            require(len(manifests) == 1, "release has no unique manifest")
            manifest = read_record(manifests[0])
            require(manifest.get("release_id") == release.name
                    and manifest.get("product", product) in {product, "krisis" if product == "decisions" else product},
                    "release manifest ownership is unproved")
            if verifier is None:
                retained_unverified.add(release)
            else:
                reply = json.loads(inspect_command([verifier, "verify-release", release]))
                require(isinstance(reply, dict) and reply.get("ok") is True
                        and isinstance(reply.get("data"), dict)
                        and reply["data"].get("release_id") == release.name,
                        "release history verification did not prove its identity")
            for path in release.rglob("*"):
                if path.is_dir() and not path.is_symlink():
                    directory(path)
                else:
                    regular(path)
            releases.append(release)
        for name in ("last-update.json", "last-update.txt"):
            path = install / name
            if path.exists() or path.is_symlink():
                receipt = read_record(path)
                selected = receipt.get("release_id", receipt.get("release", "").removeprefix("releases/"))
                completed = receipt.get("completed_at")
                if product == "semantics" and name == "last-update.json" and not completed:
                    # Semantics records completion through its released maintenance
                    # state; lifecycle_barriers already excludes pending transactions.
                    snapshot = receipt.get("rollback_snapshot")
                    completed = (receipt.get("version") == 1
                                 and receipt.get("maintenance_retained") is False
                                 and isinstance(receipt.get("clockwork_definition"), str)
                                 and HEX.fullmatch(receipt["clockwork_definition"])
                                 and isinstance(snapshot, str)
                                 and Path(snapshot).parent == base / application / "backups/deployments")
                require(completed and selected == currents[application].name,
                        "installation history receipt is not completed current state")
                if verifier is not None:
                    receipts.append(path)
    protected = live_pins(home, installs, currents) | retained_unverified
    require(all(link.is_symlink() and os.readlink(link) == target for link, target in selectors.items()),
            "release selectors changed during cleanup inspection")
    require(shutil.rmtree.avoids_symlink_attacks, "safe directory removal is unavailable")
    removed = [path for path in releases if path not in protected]
    for link in previous:
        link.unlink()
    for path in removed:
        # Installed bundles can be sealed read-only. Only obsolete releases
        # already validated above may have owner write restored for removal.
        for root, _, _ in os.walk(path, followlinks=False):
            entry = Path(root)
            info = directory(entry)
            entry.chmod(stat.S_IMODE(info.st_mode) | stat.S_IWUSR, follow_symlinks=False)
        shutil.rmtree(path)
    for path in receipts:
        path.unlink()
    result = {"removed_releases": len(removed), "removed_previous_links": len(previous),
              "removed_history_receipts": len(receipts), "retained_releases": len(protected),
              "product_history": product_history}
    if "usher" in product_history:
        result["usher_history"] = product_history["usher"]
    return result


if __name__ == "__main__":
    try:
        require(sys.platform == "darwin", "cleanup is a coordinated macOS operation")
        parser = argparse.ArgumentParser(description=__doc__)
        parser.add_argument("--usher-installer", type=Path)
        parser.add_argument("--installer", action="append", default=[], metavar="PRODUCT=ABSOLUTE_PATH")
        arguments = parser.parse_args()
        os.umask(0o077)
        print(json.dumps(clean_installed_release_history(Path(pwd.getpwuid(os.getuid()).pw_dir),
                                                       arguments.usher_installer,
                                                       installers=parse_installers(arguments.installer)),
                         separators=(",", ":"), sort_keys=True))
    except (CleanupError, OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        print(f"cell-deploy: installed release cleanup failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
