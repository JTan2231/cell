#!/usr/bin/env python3
"""Krisis ordinary updates preserve the Annals library's persistent identity."""
from pathlib import Path
import re
import sys
import tempfile
import tomllib
import os
import stat

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from deployment.adapter_support import command, digest, main, Stopped
from deployment.stateful_adapter import StatefulAdapter

SPEC = {
    "product": "krisis", "application": "Decisions", "format": "4", "allow_absent": True,
    "database": "decisions.db", "deployer": "decisions/packaging/macos/deploy-user.sh",
    "dependencies": ["annals", "nucleus", "clockwork", "conversations"],
    "maintenance_products": ["nucleus"], "requesters": ["krisis", "decisions"],
    "proofs": {
        "binary_sha256": ["libexec/krisis"],
        "frontend_sha256": ["bin/krisis", "package/krisis"],
        "observer_runner_sha256": ["bin/krisis-observer", "package/krisis-observer"],
        "observer_clockwork_definition_sha256": ["package/krisis-observer.clockwork.toml.in"],
        "hooks_sha256": ["package/hooks.json"], "deployer_sha256": ["package/deploy-user.sh"],
        "uninstaller_sha256": ["package/uninstall-user.sh"],
    },
    "bundles": {"krisis_chancery_sha256": "krisis", "decisions_chancery_sha256": "decisions"},
}
SPEC["candidate_proofs"] = {"binary_sha256": "krisis"}
SPEC["source_proofs"] = {key: "decisions/packaging/macos/" + next(path.split("/", 1)[1] for path in paths if path.startswith("package/"))
                         for key, paths in SPEC["proofs"].items() if key not in SPEC["candidate_proofs"]}
SPEC["bundle_sources"] = {"share/chancery/krisis": "decisions/chancery", "share/chancery/decisions": "decisions/chancery-legacy"}


class Adapter(StatefulAdapter):
    def pins(self):
        config = self.home / "Library/Application Support/Annals/decisions/config.toml"
        digest(config)
        library_id = tomllib.loads(config.read_text())["decision_feed"]["expected_library_id"]
        if not re.fullmatch("[0-9a-f]{32}", library_id):
            raise Stopped("invalid Annals decisions-library identity")
        annals = (self.home / "Library/Application Support/Annals/install/current/libexec/annals").resolve(strict=True)
        return annals, config, library_id

    def installed_pins(self):
        receipt = self.install / "krisis-observer-binding.txt"
        digest(receipt)
        info = receipt.stat()
        if info.st_uid != os.getuid() or stat.S_IMODE(info.st_mode) != 0o600:
            raise Stopped("Annals pin receipt is not private current-user state")
        pairs = [line.split("=", 1) for line in receipt.read_text().splitlines()]
        if any(len(pair) != 2 for pair in pairs) or len(dict(pairs)) != 6:
            raise Stopped("invalid installed Annals pin receipt")
        values = dict(pairs)
        if set(values) != {"format", "release_id", "definition_digest", "annals_binary", "annals_config", "annals_library_id"} or values["format"] != "1":
            raise Stopped("unsupported Annals pin receipt")
        if values["release_id"] != self.installed()["release_id"]:
            raise Stopped("Annals pin receipt belongs to another installation")
        return values

    def codex(self):
        if self.installed()["current"] != "absent":
            receipt = self.installed_pins()
            definition = command([self.home / ".local/bin/clockwork", "--json", "definition", "show",
                                  receipt["definition_digest"]], json_output=True)["data"]
            path = Path(definition["manifest"]["environment"]["CONVERSATIONS_CODEX"])
            if not path.is_absolute() or not path.is_file():
                raise Stopped("installed Conversations executable pin is unavailable")
            return path
        for path in (Path("/Applications/ChatGPT.app/Contents/Resources/codex"),
                     Path("/opt/homebrew/bin/codex"), self.home / ".local/bin/codex"):
            if path.is_file():
                return path
        raise Stopped("Conversations requires its configured Codex executable")

    def environment(self):
        return {**super().environment(), "CONVERSATIONS_CODEX": str(self.codex())}

    def control_specs(self):
        if self.installed()["current"] == "absent":
            return []
        pins = self.installed_pins()
        substitutions = self.common_substitutions("krisis-observer")
        substitutions.update(KRISIS_STATE=self.install.parent, KRISIS_HOME=self.home,
                             KRISIS_LOGS=self.home / "Library/Logs/Decisions", CONVERSATIONS_CODEX=self.codex(),
                             KRISIS_ANNALS_BINARY=pins["annals_binary"], KRISIS_ANNALS_CONFIG=pins["annals_config"],
                             KRISIS_ANNALS_LIBRARY_ID=pins["annals_library_id"])
        release = self.install / self.installed()["current"]
        return [("krisis/observer", release / "package/krisis-observer.clockwork.toml.in", substitutions)]

    def controls(self):
        result = super().controls()
        if result and result["krisis/observer"]["definition_digest"] != self.installed_pins()["definition_digest"]:
            raise Stopped("observer selection differs from its exact dependency receipt")
        return result

    def runtime_inspect(self):
        result = super().runtime_inspect()
        self.pins()
        if self.installed()["current"] != "absent":
            result["annals_library_id"] = self.installed_pins()["annals_library_id"]
        return result

    def installer_holds(self):
        return [*super().installer_holds(), self.install.parent / ".clockwork-maintenance"]

    def deploy_arguments(self):
        annals, config, library_id = self.pins()
        if self.prior.get("annals_library_id", library_id) != library_id:
            raise Stopped("ordinary deployment cannot replace the Annals decisions library")
        return [*super().deploy_arguments(), "--clockwork", self.home / ".local/bin/clockwork",
                "--codex", self.codex(), "--annals", annals, "--annals-config", config,
                "--annals-library-id", library_id]

    def after_install(self):
        # Preparation deliberately leaves the deployer's authenticated hold.
        # Final cutover remains inside the durable outer admission hold.
        command([*self.deploy_arguments(), "--final-cutover"], env=self.environment(), timeout=1200)

    def runtime_readiness(self):
        annals, config, library_id = self.pins()
        pins = self.installed_pins()
        if self.candidate_dir and (pins["annals_binary"], pins["annals_config"], pins["annals_library_id"]) != (str(annals), str(config), library_id):
            raise Stopped("candidate observer did not adopt the validated Annals executable pin")
        if not self.candidate_dir:
            annals, config, library_id = pins["annals_binary"], pins["annals_config"], pins["annals_library_id"]
        command([self.payload(), *self.targets()[0], "--annals-binary", annals, "--annals-config", config,
                 "--annals-library-id", library_id, "--json", "doctor"], env=self.environment(), json_output=True)
        return {}

    def runtime_verify(self):
        self.runtime_readiness()
        root = Path(tempfile.mkdtemp(prefix="krisis-canary-", dir=self.run_dir))
        args = [self.payload(), "--database", root / "krisis.db", "--json"]
        first = command([*args, "observe", "activate"], env=self.environment(), json_output=True)
        second = command([*args, "observe", "activate"], env=self.environment(), json_output=True)
        status = command([*args, "observe", "status"], env=self.environment(), json_output=True)
        if first.get("created") is not True or second.get("created") is not False or status.get("queued") != 0:
            raise Stopped("isolated observer activation was not durable and idempotent")
        return {"canary": "isolated durable observer baseline and replay", "canary_directory": str(root)}


if __name__ == "__main__":
    sys.exit(main(Adapter, SPEC))
