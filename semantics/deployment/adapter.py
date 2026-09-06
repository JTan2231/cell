#!/usr/bin/env python3
"""Semantics owns its installation, account-feed boundary and worker admission."""
import json
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from deployment.adapter_support import command, main, Stopped
from deployment.stateful_adapter import StatefulAdapter

SPEC = {
    "product": "semantics", "application": "Semantics", "format": "2", "allow_absent": True,
    "database": "semantics.db", "deployer": "semantics/packaging/macos/deploy-user.sh",
    "dependencies": ["annals", "nucleus", "clockwork", "conversations"],
    "maintenance_products": ["nucleus"],
    "proofs": {
        "binary_sha256": ["libexec/semantics"],
        "frontend_sha256": ["bin/semantics", "package/semantics"],
        "runner_sha256": ["bin/semantics-worker", "package/semantics-worker"],
        "clockwork_template_sha256": ["package/semantics-worker.clockwork.toml.in"],
        "deployer_sha256": ["package/deploy-user.sh"],
        "uninstaller_sha256": ["package/uninstall-user.sh"],
    },
    "bundles": {"chancery_sha256": "semantics"},
}
SPEC["candidate_proofs"] = {"binary_sha256": "semantics"}
SPEC["source_proofs"] = {key: "semantics/packaging/macos/" + next(path.split("/", 1)[1] for path in paths if path.startswith("package/"))
                         for key, paths in SPEC["proofs"].items() if key not in SPEC["candidate_proofs"]}
SPEC["bundle_sources"] = {"share/chancery/semantics": "semantics/chancery"}


class Adapter(StatefulAdapter):
    def control_specs(self):
        if self.installed()["current"] == "absent":
            return []
        substitutions = self.common_substitutions("semantics-worker")
        substitutions.update(SEMANTICS_STATE=self.install.parent, SEMANTICS_HOME=self.home,
                             SEMANTICS_LOGS=self.home / "Library/Logs/Semantics")
        release = self.install / self.installed()["current"]
        return [("semantics/worker", release / "package/semantics-worker.clockwork.toml.in", substitutions)]

    def installer_holds(self):
        return [*super().installer_holds(), self.install.parent / ".clockwork-maintenance"]

    def deploy_arguments(self):
        return [*super().deploy_arguments(), "--clockwork", self.home / ".local/bin/clockwork"]

    def runtime_readiness(self):
        command([self.payload(), *self.targets()[0], "--json", "doctor"], env=self.environment(), json_output=True)
        return {}

    def runtime_verify(self):
        self.runtime_readiness()
        root = self.run_dir / "semantics-canary"
        proof = command(["/usr/bin/env", "-u", "SEMANTICS_DATABASE", self.payload(), "deployment-canary",
                         "--directory", root, "--run-id", self.run_id, "--nucleus-socket",
                         self.home / "Library/Application Support/Nucleus/nucleus.sock"],
                        env=self.environment(), json_output=True, timeout=1500)
        if proof.get("verified") is not True:
            raise Stopped("isolated reconciliation did not prove its grounded revision and Nucleus result")
        return {"canary": "isolated real account reconciliation, revision replay and Nucleus result",
                "canary_directory": str(root), "proof": proof}


if __name__ == "__main__":
    sys.exit(main(Adapter, SPEC))
