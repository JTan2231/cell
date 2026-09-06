"""Release mechanics shared by the Annals, Krisis and Semantics adapters.

Each product supplies its exact artifact layout, control definitions, admission
targets, installer and domain verification. No database is read by this module.
"""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import re
import time
import tomllib

from deployment.adapter_support import ProductAdapter, Stopped, command, digest, maintenance_data, response, tree_digest


class StatefulAdapter(ProductAdapter):
    def installed(self):
        link = self.install / "current"
        if not link.is_symlink():
            if link.exists() or self.cli.exists() or self.cli.is_symlink():
                raise Stopped("foreign or incomplete installation selectors")
            return {"current": "absent", "release_id": None}
        selected = os.readlink(link)
        if not re.fullmatch(r"releases/[0-9a-f]{64}", selected):
            raise Stopped("unsupported installed release selection")
        release = self.install / selected
        for path in (self.install, self.install / "releases", release):
            if path.is_symlink() or not path.is_dir():
                raise Stopped("invalid release directory")
        if not self.cli.is_symlink() or os.readlink(self.cli) != str(link / "bin" / self.cli.name):
            raise Stopped("public command is not owned by this release")
        manifest_path = release / self.spec.get("manifest", "manifest.txt")
        manifest_hash = digest(manifest_path)
        if manifest_path.suffix == ".json":
            manifest = json.loads(manifest_path.read_text())
        else:
            pairs = [line.split("=", 1) for line in manifest_path.read_text().splitlines()]
            if any(len(pair) != 2 for pair in pairs) or len(dict(pairs)) != len(pairs):
                raise Stopped("invalid release manifest")
            manifest = dict(pairs)
        if str(manifest.get("format")) != self.spec["format"]:
            raise Stopped("legacy installation requires its documented migration")
        if manifest.get("release_id") != selected.split("/")[1]:
            raise Stopped("release manifest and selector disagree")
        allowed = {"format", "release_id", *self.spec.get("manifest_metadata", ["version"]),
                   *self.spec["proofs"], *self.spec["bundles"]}
        if set(manifest) != allowed:
            raise Stopped("release manifest fields differ from the supported format")
        hashes, files = [], {manifest_path.name}
        for key, paths in self.spec["proofs"].items():
            for relative in paths:
                if digest(release / relative) != manifest[key]:
                    raise Stopped("installed artifact does not match its release manifest")
                files.add(relative)
            hashes.append(manifest[key])
        for key, name in self.spec["bundles"].items():
            bundle = release / "share/chancery" / name
            if tree_digest(bundle, path_lines=self.spec.get("bundle_path_lines", True)) != manifest[key]:
                raise Stopped("installed contract bytes do not match their release")
            provider = self.home / "Library/Application Support/Chancery/providers" / name
            if not provider.is_symlink() or os.readlink(provider) != str(link / "share/chancery" / name):
                raise Stopped("contract selector is not owned by this installation")
            hashes.append(manifest[key])
            files.update(path.relative_to(release).as_posix() for path in bundle.rglob("*") if path.is_file())
        expected = hashlib.sha256("".join(value + "\n" for value in hashes).encode()).hexdigest()
        if expected != manifest["release_id"]:
            raise Stopped("installed release content identity is invalid")
        for path in release.rglob("*"):
            if path.is_symlink() or (not path.is_dir() and path.relative_to(release).as_posix() not in files):
                raise Stopped("installed release contains unmanifested entries")
        return {"current": selected, "release_id": expected, "manifest_sha256": manifest_hash}

    def payload(self):
        current = self.installed()["current"]
        if current == "absent":
            return self.candidate()
        return self.install / current / "libexec" / self.cli.name

    def targets(self):
        return [["--database", str(self.install.parent / self.spec["database"])]]

    def maintenance(self, operation, target, *, owner=True):
        args = [self.payload(), *target, "--json", "maintenance", operation]
        if owner and operation != "status":
            args.append(self.run_id)
        value = maintenance_data(command(args, json_output=True))
        if value.get("protocol_version") != 1 or value.get("contract_version") != 1:
            raise Stopped("installed admission needs the one-time compatibility deployment")
        return value

    def control_specs(self):
        """Return product-owned (key, release template, substitutions) tuples."""
        return []

    def controls(self):
        result = {}
        for key, template, substitutions in self.control_specs():
            binding = command([self.home / ".local/bin/clockwork", "--json", "binding", "show", key], json_output=True)["data"]
            if binding.get("key") != key or not isinstance(binding.get("enabled"), bool):
                raise Stopped("invalid owned binding result")
            definition = command([self.home / ".local/bin/clockwork", "--json", "definition", "show", binding["definition_digest"]], json_output=True)["data"]
            text = template.read_text()
            for marker, value in substitutions.items():
                # Template values are TOML basic-string contents, not shell text.
                text = text.replace("__" + marker + "__", json.dumps(str(value))[1:-1])
            if re.search(r"__[A-Z_]+__", text):
                raise Stopped("incomplete owned definition template")
            expected = tomllib.loads(text)
            if definition.get("key") != key or definition.get("digest") != binding["definition_digest"] or definition.get("manifest") != expected:
                raise Stopped("Clockwork definition differs from the complete product-owned definition")
            result[key] = {"enabled": binding["enabled"], "definition_digest": binding["definition_digest"]}
        return result

    def runtime_inspect(self):
        for target in self.targets():
            status = self.maintenance("status", target)
            if status["holds"] and status["holds"] != [self.run_id]:
                raise Stopped("another operation owns admission maintenance")
        self.assert_no_installer_hold()
        return {"controls": self.controls()}

    def assert_no_installer_hold(self):
        for path in self.installer_holds():
            if path.exists() or path.is_symlink():
                raise Stopped("product installer has retained maintenance or an unfinished transaction")

    def installer_holds(self):
        return [self.install / ".update-lock"]

    def hold(self):
        values = [self.maintenance("hold", target) for target in self.targets()]
        return response("held", "run-owned product admission holds persisted", {"targets": values})

    def drain(self):
        deadline = time.monotonic() + 45
        while True:
            values = [self.maintenance("status", target) for target in self.targets()]
            if any(value["holds"] != [self.run_id] for value in values):
                raise Stopped("drain requires the sole recorded product hold")
            if all(value["drained"] for value in values):
                break
            if time.monotonic() >= deadline:
                raise Stopped("admitted product commands have not drained; holds remain")
            time.sleep(0.2)
        # A process can exit while its Nucleus job survives. Do not infer domain
        # quiescence solely from the process lock. Never cancel someone else's job.
        for requester in self.spec.get("requesters", [self.product]):
            for state in ("accepted", "running", "waiting-on-requester"):
                value = command([self.home / ".local/bin/nucleus", "--compact", "jobs", "list",
                                 "--requester", requester, "--state", state, "--limit", "1"], json_output=True)
                if value.get("version") != 1 or not isinstance(value.get("jobs"), list) or value["jobs"]:
                    raise Stopped("durable requester work is not drained; holds remain for product recovery")
        return response("drained", "product admission and durable execution are drained", {"drained": True})

    def restore_controls(self):
        observed = self.controls()
        for key, prior in self.prior.get("controls", {}).items():
            if key not in observed:
                raise Stopped("owned schedule disappeared during deployment")
            if observed[key]["enabled"] != prior["enabled"]:
                if prior["enabled"]:
                    # The installer should have enabled it. Recovery never starts
                    # an unexpectedly disabled schedule to conceal a failed cutover.
                    raise Stopped("previously enabled schedule is unexpectedly disabled")
                command([self.home / ".local/bin/clockwork", "--json", "binding", "disable", key], json_output=True)
        result = self.controls()
        if any(result[key]["enabled"] != prior["enabled"] for key, prior in self.prior.get("controls", {}).items()):
            raise Stopped("operator schedule state was not restored")
        return result

    def apply(self):
        self.check_prior()
        self.drain()
        command(self.deploy_arguments(), env=self.environment(), timeout=1200)
        self.after_install()
        self.restore_controls()
        return response("applied", "product installation and operator controls restored", self.installed())

    def after_install(self):
        pass

    def verify(self):
        self.assert_no_installer_hold()
        installed = self.installed()
        if installed["current"] == "absent":
            raise Stopped("installed program is absent")
        self.prove_candidate_release(installed)
        if self.candidate_dir and digest(self.payload()) != digest(self.candidate()):
            raise Stopped("installed payload differs from the tested candidate")
        command([self.payload(), "--version"])
        command([self.payload(), "--help"])
        installed["controls"] = self.restore_controls()
        installed.update(self.runtime_verify())
        return response("verified", "installed contract, controls and readiness verified", installed)

    def release(self):
        self.assert_no_installer_hold()
        self.restore_controls()
        values = [self.maintenance("release", target) for target in self.targets()]
        return response("released", "only this run's admission holds released", {"targets": values})

    def recover(self):
        self.assert_no_installer_hold()
        observed = self.installed()
        prior = observed["current"] == self.prior.get("current")
        if prior:
            self.check_prior()
        else:
            self.prove_candidate_release(observed)
        context = self.request.get("recovery") or {}
        self.restore_controls()
        if prior and context.get("any_apply_started") is False:
            return response("recovered", "exact prior programs preserved before any installation began",
                            {"safe_to_release": True, "installed": "prior"})
        statuses = [self.maintenance("status", target) for target in self.targets()]
        released_after_proof = context.get("verified") is True and all(
            not value["holds"] or value["holds"] == [self.run_id] for value in statuses)
        if not released_after_proof and any(value["holds"] != [self.run_id] or not value["drained"] for value in statuses):
            raise Stopped("recovery requires drained owned holds or a captured completed verification")
        if prior:
            candidate_dir = self.candidate_dir
            try:
                self.candidate_dir = None
                if released_after_proof:
                    self.runtime_readiness()
                else:
                    self.runtime_verify()
            finally:
                self.candidate_dir = candidate_dir
        elif released_after_proof:
            self.runtime_readiness()
        else:
            self.verify()
        return response("recovered", "coherent product generation verified; no database rollback inferred",
                        {"safe_to_release": True, "installed": "prior" if prior else "candidate"})

    def runtime_readiness(self):
        return {}

    def common_substitutions(self, runner):
        release = self.install / self.installed()["current"]
        return {"RELEASE_ID": release.name, "RELEASE_ROOT": release,
                "INTERPRETER_SHA256": hashlib.sha256(Path("/bin/sh").read_bytes()).hexdigest(),
                "RUNNER_SHA256": digest(release / "bin" / runner)}
