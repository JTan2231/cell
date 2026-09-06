#!/usr/bin/env python3
"""Annals owns both supported installed inboxes and the shared binary release."""
import os
from pathlib import Path
import pwd
import sys
import tomllib

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from deployment.adapter_support import command, digest, main, Stopped
from deployment.stateful_adapter import StatefulAdapter
from annals.deployment.canary import verify as verify_canary

SPEC = {
    "product": "annals", "application": "Annals", "format": "4", "allow_absent": True,
    "manifest": "manifest.json", "manifest_metadata": ["source_revision", "source_dirty"],
    "database": "annals.db", "deployer": "annals/packaging/launchd/deploy-user.sh",
    "dependencies": ["nucleus", "clockwork"], "maintenance_products": ["nucleus"],
    "proofs": {
        "binary_sha256": ["libexec/annals"], "usage_binary_sha256": ["libexec/annals-usage"],
        "frontend_sha256": ["bin/annals", "package/annals-user"],
        "runner_sha256": ["bin/annals-inbox", "package/annals-inbox"],
        "clockwork_template_sha256": ["package/annals-inbox.clockwork.toml.in"],
        "decisions_config_template_sha256": ["package/annals-decisions.toml.in"],
        "decisions_clockwork_template_sha256": ["package/annals-decisions-inbox.clockwork.toml.in"],
        "decisions_provisioner_sha256": ["package/provision-decisions-user.sh"],
        "legacy_agent_plist_sha256": ["package/org.annals.inbox.agent.plist"],
        "updater_sha256": ["package/deploy-user.sh"],
    },
    "bundles": {"chancery_annals_sha256": "annals", "chancery_usage_sha256": "annals-usage"},
}
SPEC["candidate_proofs"] = {"binary_sha256": "annals", "usage_binary_sha256": "annals-usage"}
SPEC["source_proofs"] = {key: "annals/packaging/launchd/" + next(path.split("/", 1)[1] for path in paths if path.startswith("package/"))
                         for key, paths in SPEC["proofs"].items() if key not in SPEC["candidate_proofs"]}
SPEC["bundle_sources"] = {"share/chancery/" + name: "annals/chancery/" + name for name in SPEC["bundles"].values()}


class Adapter(StatefulAdapter):
    def libraries(self):
        return [self.install.parent, self.install.parent / "decisions"]

    def targets(self):
        primary, decisions = self.libraries()
        result = [["--config", str(primary / "config.toml")]] if (primary / "config.toml").exists() else [["--library", str(primary / "annals.db")]]
        if (decisions / "config.toml").exists():
            result.append(["--config", str(decisions / "config.toml")])
        elif decisions.exists():
            raise Stopped("dedicated decisions state is incomplete; use product recovery")
        # A new dedicated library is fenced by the provisioner before its first
        # binding is enabled; creating its final directory here is not safe.
        return result

    def installed(self):
        result = super().installed()
        if result["current"] != "absent":
            usage = self.home / ".local/bin/annals-usage"
            if not usage.is_symlink() or os.readlink(usage) != str(self.install / "current/libexec/annals-usage"):
                raise Stopped("Annals Usage selector is not owned by the Annals release")
        return result

    def control_specs(self):
        if self.installed()["current"] == "absent":
            return []
        release = self.install / self.installed()["current"]
        common = self.common_substitutions("annals-inbox")
        common.update(ANNALS_HOME=self.home, ANNALS_USER=pwd.getpwuid(os.getuid()).pw_name)
        primary, decisions = self.libraries()
        result = [("annals/inbox", release / "package/annals-inbox.clockwork.toml.in",
                   {**common, "ANNALS_STATE": primary, "ANNALS_LOGS": primary / "log"})]
        if (decisions / "config.toml").exists():
            result.append(("annals/decisions-inbox", release / "package/annals-decisions-inbox.clockwork.toml.in",
                           {**common, "ANNALS_DECISIONS_STATE": decisions, "ANNALS_DECISIONS_LOGS": decisions / "log"}))
        return result

    def installer_holds(self):
        return [*super().installer_holds(), *(root / "spool/.maintenance" for root in self.libraries())]

    def pause_state(self):
        return {str(root): (root / "spool/.paused").exists() for root in self.libraries() if (root / "config.toml").exists()}

    def runtime_inspect(self):
        result = super().runtime_inspect()
        result["operator_pauses"] = self.pause_state()
        # Annals changes the binary and decisions-feed provider underneath both
        # consumers. Fence installed consumers without selecting their upgrade.
        affected = ["nucleus"]
        for name in ("krisis", "semantics"):
            if (self.home / ".local/bin" / name).exists():
                affected.append(name)
        result["maintenance_products"] = affected
        return result

    def inspect(self):
        result = super().inspect()
        # ProductAdapter's static defaults must not overwrite the discovered
        # installed feed consumers.
        result["data"]["maintenance_products"] = self.runtime_inspect()["maintenance_products"]
        return result

    def deploy_arguments(self):
        return [*super().deploy_arguments(), "--usage-binary", self.candidate("annals-usage"),
                "--nucleus", self.home / ".local/bin/nucleus", "--nucleus-socket", self.socket(),
                "--clockwork", self.home / ".local/bin/clockwork"]

    def socket(self):
        primary = self.install.parent / "config.toml"
        if primary.exists():
            configured = tomllib.loads(primary.read_text()).get("liaison", {}).get("nucleus_socket")
            if configured:
                return Path(configured)
        return self.home / "Library/Application Support/Nucleus/nucleus.sock"

    def after_install(self):
        release = self.install / self.installed()["current"]
        command([release / "package/provision-decisions-user.sh", "--release-root", release,
                 "--nucleus-socket", self.socket(), "--clockwork", self.home / ".local/bin/clockwork"],
                env=self.environment(), timeout=1200)
        for target in self.targets():
            status = self.maintenance("status", target)
            if status["holds"] != [self.run_id] or not status["drained"]:
                raise Stopped("Annals publication did not retain the run-owned library holds")

    def runtime_readiness(self):
        pauses = self.pause_state()
        if any(paused and not pauses.get(path) for path, paused in self.prior.get("operator_pauses", {}).items()):
            raise Stopped("a previously active Annals operator pause was cleared during deployment")
        for target in self.targets():
            command([self.payload(), *target, "--json", "stats"], env=self.environment(), json_output=True)
            command([self.payload(), *target, "--json", "inbox", "status"], env=self.environment(), json_output=True)
        command([self.payload(), "--config", self.libraries()[1] / "config.toml", "--json", "decision-feed", "watermark"], json_output=True)
        usage = self.payload().with_name("annals-usage")
        if self.candidate_dir and digest(usage) != digest(self.candidate("annals-usage")):
            raise Stopped("Annals Usage differs from the tested candidate")
        command([usage, "doctor", "--config", self.install.parent / "usage.toml"], env=self.environment())
        return {"operator_pauses": pauses}

    def runtime_verify(self):
        readiness = self.runtime_readiness()
        return {**readiness, **verify_canary(self)}


if __name__ == "__main__":
    sys.exit(main(Adapter, SPEC))
