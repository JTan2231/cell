"""Mechanical helpers for product-owned deployment adapters.

Products supply their own release layout, installer, runtime maintenance and
verification. This module neither interprets domain databases nor owns schedules.
"""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
import time


class Stopped(RuntimeError):
    """The adapter cannot prove a supported safe transition."""


def digest(path: Path) -> str:
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
        raise Stopped(f"not a regular single-link artifact: {path}")
    with path.open("rb") as stream:
        result = hashlib.sha256()
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            result.update(chunk)
        return result.hexdigest()


def tree_digest(root: Path, *, path_lines: bool = True) -> str:
    result = hashlib.sha256()
    for path in sorted(root.rglob("*"), key=lambda p: p.relative_to(root).as_posix()):
        if path.is_symlink():
            raise Stopped(f"symbolic artifact: {path}")
        if path.is_dir():
            continue
        relative = "./" + path.relative_to(root).as_posix()
        if "\n" in relative or "\r" in relative:
            raise Stopped("invalid artifact pathname")
        if path_lines:
            result.update(f"path={relative}\n".encode())
        result.update(f"{digest(path)}  {relative}\n".encode())
    return result.hexdigest()


def command(argv, *, env=None, timeout=180, json_output=False):
    effective = os.environ.copy()
    effective["PYTHONDONTWRITEBYTECODE"] = "1"
    if env:
        effective.update(env)
    inherited = ()
    lock_fd = os.environ.get("CELL_DEPLOYMENT_LOCK_FD")
    if lock_fd is not None:
        try:
            descriptor = int(lock_fd)
            info = os.fstat(descriptor)
        except (ValueError, OSError) as error:
            raise Stopped("deployment lock descriptor is unavailable") from error
        if descriptor < 3 or not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid():
            raise Stopped("deployment lock descriptor is invalid")
        inherited = (descriptor,)
    try:
        result = subprocess.run([str(arg) for arg in argv], stdin=subprocess.DEVNULL,
                                capture_output=True, text=True, env=effective, timeout=timeout,
                                pass_fds=inherited)
    except (OSError, subprocess.TimeoutExpired) as error:
        raise Stopped(f"{Path(str(argv[0])).name}: {error}") from error
    if result.returncode:
        # Do not relay unbounded diagnostics that may contain private domain data.
        raise Stopped(f"{Path(str(argv[0])).name} {argv[1] if len(argv)>1 else ''} failed (exit {result.returncode})")
    if json_output:
        try:
            value = json.loads(result.stdout)
        except ValueError as error:
            raise Stopped("invalid product JSON response") from error
        if not isinstance(value, dict) or value.get("ok") is False:
            raise Stopped("product did not report a successful result")
        return value
    return result.stdout.strip()


def response(status, detail, data=None):
    return {"schema": 1, "status": status, "detail": detail, "data": data or {}}


def maintenance_data(value):
    current = value
    for _ in range(4):
        if isinstance(current, dict) and "holds" in current and "drained" in current:
            if not isinstance(current["holds"], list) or not isinstance(current["drained"], bool):
                break
            return current
        if isinstance(current, dict):
            current = current.get("maintenance", current.get("data"))
    raise Stopped("product does not expose a supported maintenance result")


class ProductAdapter:
    """Common release mechanics; specs and runtime methods belong to products."""

    def __init__(self, request, spec):
        if request.get("schema") != 1 or request.get("product") != spec["product"]:
            raise Stopped("unsupported adapter request")
        self.request, self.spec = request, spec
        self.product = spec["product"]
        self.home = Path.home()
        self.source = Path(request["source_root"])
        self.run_dir = Path(request["run_dir"])
        self.run_id = request["run_id"]
        if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]{0,127}", self.run_id):
            raise Stopped("invalid run identity")
        self.install = self.home / "Library/Application Support" / spec["application"] / "install"
        self.cli = self.home / ".local/bin" / spec.get("binary", self.product)
        self.candidate_dir = Path(request["candidate_dir"]) if request.get("candidate_dir") else None
        self.prior = request.get("prior") or {}

    def candidate(self, name=None):
        if self.candidate_dir is None:
            raise Stopped("selected product has no sealed candidate")
        path = self.candidate_dir / "bin" / (name or self.spec.get("binary", self.product))
        digest(path)
        return path

    def environment(self):
        return {"CELL_DEPLOYMENT_RUN_ID": self.run_id}

    def installed(self):
        link = self.install / "current"
        if not link.is_symlink():
            if link.exists() or self.cli.exists() or self.cli.is_symlink():
                raise Stopped("foreign or incoherent installed selection")
            return {"current": "absent", "release_id": None}
        selected = os.readlink(link)
        if not re.fullmatch(r"releases/[0-9a-f]{64}", selected):
            raise Stopped("invalid installed release selection")
        release = self.install / selected
        for directory in (self.install, self.install / "releases", release):
            if directory.is_symlink() or not directory.is_dir():
                raise Stopped("symbolic or absent installation directory")
        if self.spec.get("copied_commands"):
            for public, relative in self.spec["copied_commands"].items():
                if digest(self.home / public) != digest(release / relative):
                    raise Stopped("installed command copy differs from the selected release")
        elif not self.cli.is_symlink() or os.readlink(self.cli) != str(link / "bin" / self.cli.name):
            raise Stopped("public command is not owned by the selected installation")
        provider = self.home / "Library/Application Support/Chancery/providers" / self.product
        bundle_relative = self.spec.get("bundle_path", "share/chancery/" + self.product)
        target = link / bundle_relative
        if not provider.is_symlink() or os.readlink(provider) != str(target):
            raise Stopped("provider selector is not owned by the selected installation")
        manifest_path = release / "manifest.txt"
        digest(manifest_path)
        pairs = [line.split("=", 1) for line in manifest_path.read_text().splitlines()]
        if any(len(pair) != 2 for pair in pairs) or len(dict(pairs)) != len(pairs):
            raise Stopped("invalid release manifest")
        manifest = dict(pairs)
        if manifest.get("release_id") != selected.split("/")[1]:
            raise Stopped("release identity does not match selector")
        if manifest.get("format") not in self.spec.get("formats", ["1"]):
            raise Stopped("unsupported installed release format")
        if manifest.get("product", self.product) != self.product:
            raise Stopped("foreign release manifest")
        proofs = self.spec["proofs"]
        bundle_key = self.spec.get("bundle_key", "chancery_sha256")
        known = {"format", "product", "release_id", "version", bundle_key, *proofs}
        if set(manifest) - known:
            raise Stopped("unrecognized release manifest fields")
        values = []
        expected_files = {"manifest.txt"}
        for key, paths in proofs.items():
            if isinstance(paths, str):
                paths = [paths]
            for relative in paths:
                expected_files.add(relative)
                if digest(release / relative) != manifest.get(key):
                    raise Stopped(f"installed {key} does not match its manifest")
            values.append(manifest[key])
        bundle = release / bundle_relative
        bundle_hash = tree_digest(bundle, path_lines=self.spec.get("bundle_path_lines", True))
        if bundle_hash != manifest.get(bundle_key):
            raise Stopped("installed provider bytes do not match the release")
        values.append(bundle_hash)
        expected_id = hashlib.sha256("".join(value + "\n" for value in values).encode()).hexdigest()
        if expected_id != manifest["release_id"]:
            raise Stopped("installed content identity is invalid")
        provider_data = json.loads((bundle / "provider.json").read_text())["provider"]
        if manifest.get("version") and provider_data.get("release") != manifest["version"]:
            raise Stopped("provider and program version disagree")
        for path in release.rglob("*"):
            if path.is_symlink():
                raise Stopped("installed release contains a symbolic entry")
            if path.is_dir():
                continue
            relative = path.relative_to(release).as_posix()
            if relative not in expected_files and not relative.startswith(bundle_relative + "/"):
                raise Stopped("installed release contains unmanifested files")
            digest(path)
        return {"current": selected, "release_id": manifest["release_id"],
                "manifest_sha256": digest(manifest_path), "version": provider_data.get("release")}

    def runtime_inspect(self):
        return {}

    def inspect(self):
        prior = self.installed()
        if prior["current"] == "absent" and not self.spec.get("allow_absent", False):
            raise Stopped("existing configured installation required")
        if self.candidate_dir:
            command([self.candidate(), "--version"])
            command([self.candidate(), "--help"])
        prior.update(self.runtime_inspect())
        prior["maintenance_products"] = self.spec.get("maintenance_products", [])
        prior["after"] = self.spec.get("dependencies", [])
        return response("ready", "owned installation inspected", prior)

    def check_prior(self):
        observed = self.installed()
        if observed != {key: self.prior[key] for key in observed if key in self.prior}:
            raise Stopped("installed state changed since the recorded plan")

    def deploy_arguments(self):
        args = [self.source / self.spec["deployer"], "--binary", self.candidate()]
        if self.spec.get("expected_current"):
            args += ["--expected-current", self.prior["current"]]
        return args

    def apply(self):
        self.check_prior()
        command(self.deploy_arguments(), env=self.environment(), timeout=1200)
        return response("applied", "product installer completed", self.installed())

    def runtime_verify(self):
        return {}

    def prove_candidate_release(self, installed):
        """Bind every installed payload to the sealed candidate and source.

        A self-consistent release containing the right executable but different
        documentation or installer bytes is not the selected candidate.
        """
        if self.candidate_dir is None:
            self.check_prior()
            return
        manifest = self.request.get("candidate")
        if not isinstance(manifest, dict) or manifest.get("schema") != 1 or manifest.get("product") != self.product:
            raise Stopped("selected product lacks an exact candidate manifest")
        binary_proofs = self.spec.get("candidate_proofs", {})
        source_proofs = self.spec.get("source_proofs", {})
        if set(binary_proofs) & set(source_proofs) or set(self.spec["proofs"]) != set(binary_proofs) | set(source_proofs):
            raise Stopped("product has incomplete candidate artifact proof declarations")
        release = self.install / installed["current"]
        for key, value in {**binary_proofs, **source_proofs}.items():
            paths = self.spec["proofs"][key]
            if isinstance(paths, str):
                paths = [paths]
            if key in binary_proofs:
                expected = manifest.get("binaries", {}).get(value, {}).get("sha256")
                selected = self.candidate(value)
            else:
                expected = manifest.get("source_inputs", {}).get(value)
                selected = self.source / value
            if not isinstance(expected, str) or digest(selected) != expected:
                raise Stopped("candidate artifact differs from its sealed evidence")
            if any(digest(release / path) != expected for path in paths):
                raise Stopped(f"installed {key} differs from the selected candidate")
        # Stateful products may own several bundles. Their specs provide an
        # explicit provider-to-source mapping; the usual single-provider profile
        # declares its one bundle_source.
        bundle_sources = self.spec.get("bundle_sources")
        if bundle_sources is None:
            source = self.spec.get("bundle_source")
            if not isinstance(source, str):
                raise Stopped("product has no exact candidate provider source")
            bundle_sources = {self.spec.get("bundle_path", "share/chancery/" + self.product): source}
        for relative, source in bundle_sources.items():
            expected = {path: value for path, value in manifest.get("source_inputs", {}).items()
                        if path.startswith(source + "/")}
            actual = {path.relative_to(self.source).as_posix(): digest(path)
                      for path in (self.source / source).rglob("*") if not path.is_dir()}
            if not expected or actual != expected:
                raise Stopped("candidate provider source differs from its sealed evidence")
            options = {"path_lines": self.spec.get("bundle_path_lines", True)}
            if tree_digest(release / relative, **options) != tree_digest(self.source / source, **options):
                raise Stopped("installed provider differs from the selected candidate")

    def verify(self):
        installed = self.installed()
        if installed["current"] == "absent":
            raise Stopped("installed release is absent")
        self.prove_candidate_release(installed)
        release = self.install / installed["current"]
        if self.candidate_dir:
            payload = release / self.spec.get("payload", "bin/" + self.cli.name)
            if digest(payload) != digest(self.candidate()):
                raise Stopped("installed binary differs from the tested candidate")
        command([self.cli, "--version"], env=self.environment())
        command([self.cli, "--help"], env=self.environment())
        installed.update(self.runtime_verify())
        return response("verified", "installed product checks passed", installed)

    def hold(self):
        return response("held", "product has no runtime admission", {"drained": True})

    def drain(self):
        return response("drained", "product has no runtime admission", {"drained": True})

    def release(self):
        return response("released", "product has no runtime admission")

    def recover(self):
        observed = self.installed()
        if observed["current"] == self.prior.get("current"):
            if observed != {key: self.prior[key] for key in observed if key in self.prior}:
                raise Stopped("prior release changed after inspection")
            self.runtime_recover()
            return response("recovered", "prior program selection preserved",
                            {"safe_to_release": True, "installed": "prior"})
        verified = self.verify()
        return response("recovered", "candidate installation verified after interruption",
                        {**verified["data"], "safe_to_release": True, "installed": "candidate"})

    def runtime_recover(self):
        return self.runtime_verify()


class MaintainedAdapter(ProductAdapter):
    """A product's documented durable admission and isolated canary commands."""

    def maintenance(self, operation, *args):
        status = maintenance_data(command([self.cli, *self.spec.get("cli_flags", []),
                                          "maintenance", operation, *args],
                                         env=self.environment(), json_output=True))
        if status.get("protocol_version") != 1:
            raise Stopped("product maintenance protocol is unsupported")
        return status

    def runtime_inspect(self):
        status = self.maintenance("status")
        if status["holds"]:
            raise Stopped("another maintenance owner already holds the product")
        return {"runtime": status}

    def hold(self):
        status = self.maintenance("hold", self.run_id)
        if self.run_id not in status["holds"]:
            raise Stopped("product did not retain the requested hold")
        return response("held", "new product admission is held", status)

    def drain(self):
        deadline = time.monotonic() + 600
        while True:
            status = self.maintenance("drain" if self.spec.get("drain_command") else "status")
            if status["holds"] != [self.run_id]:
                raise Stopped("installation requires the sole matching maintenance owner")
            if status["drained"]:
                return response("drained", "previously admitted product work has settled", status)
            if time.monotonic() >= deadline:
                raise Stopped("product did not drain within ten minutes; hold retained")
            time.sleep(1)

    def release(self):
        status = self.maintenance("release", self.run_id)
        if self.run_id in status["holds"]:
            raise Stopped("product did not release this run's hold")
        return response("released", "this run's product hold was released", status)

    def runtime_verify(self):
        status = (self.maintenance("ready", self.run_id) if self.spec.get("ready_command")
                  else self.maintenance("status"))
        if status["holds"] != [self.run_id] or not status["drained"]:
            raise Stopped("verification requires this run's drained product hold")
        # Independently added operator pauses remain in force. This adapter only
        # owns its run's hold and never restores a stale captured pause value.
        self.runtime_readiness()
        value = command([self.cli, *self.spec.get("cli_flags", []), "maintenance", "canary",
                         "--directory", self.run_dir / (self.product + "-canary")],
                        env=self.environment(), timeout=1800, json_output=True)
        canary = value.get("data", value).get("canary", value.get("data", value))
        if canary.get("verified") is not True:
            raise Stopped("product canary did not verify its domain result")
        return {"canary": canary}

    def runtime_readiness(self):
        if self.spec.get("doctor_command"):
            return command([self.cli, *self.spec.get("cli_flags", []), "doctor"],
                           env=self.environment(), json_output=True)
        return {}

    def recover(self):
        observed = self.installed()
        prior = observed["current"] == self.prior.get("current")
        if prior:
            self.check_prior()
        else:
            self.prove_candidate_release(observed)
        context = self.request.get("recovery") or {}
        status = self.maintenance("status")
        if prior and context.get("any_apply_started") is False:
            # Maintenance never reached any software/schema cutover. Existing
            # work can resume with exactly the same programs; a product whose
            # hold never began needs no artificial hold or new domain canary.
            return response("recovered", "unchanged program before any cutover",
                            {"safe_to_release": True, "installed": "prior"})
        if self.run_id not in status["holds"] and context.get("verified") is True:
            # Release may have happened before its reply was recorded. Preserve
            # the proof already captured and independently added operator holds.
            self.runtime_readiness()
        elif status["holds"] == [self.run_id]:
            self.runtime_verify()
        else:
            raise Stopped("recovery has neither this run's hold nor a captured verification")
        return response("recovered", "coherent maintained product verified",
                        {"safe_to_release": True, "installed": "prior" if prior else "candidate"})


def main(adapter_type, spec):
    try:
        if len(sys.argv) != 2 or sys.argv[1] not in {"inspect", "hold", "drain", "apply", "verify", "release", "recover"}:
            raise Stopped("unsupported adapter operation")
        request = json.load(sys.stdin)
        adapter = adapter_type(request, spec)
        result = getattr(adapter, sys.argv[1])()
    except (Stopped, OSError, ValueError, KeyError, TypeError) as error:
        result = response("stopped", str(error))
    print(json.dumps(result, sort_keys=True))
    return 0 if result["status"] != "stopped" else 1
