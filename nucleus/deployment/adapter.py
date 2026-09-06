#!/usr/bin/env python3
"""Product-owned deployment boundary; invoked by the Cell coordinator."""
import json
from pathlib import Path
import sys

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from deployment.adapter_support import ProductAdapter, MaintainedAdapter, Stopped, command, digest, main, response

class NucleusAdapter(MaintainedAdapter):
    def runtime_inspect(self):
        data = super().runtime_inspect()
        health = command([self.cli, "--compact", "health"], json_output=True)
        executable = health.get("harnessExecutable")
        if not isinstance(executable, str) or not Path(executable).is_absolute():
            raise Stopped("Nucleus did not report its configured absolute harness")
        digest(Path(executable))
        data.update(harness_executable=executable)
        return data

    def deploy_arguments(self):
        return super().deploy_arguments() + ["--daemon", self.candidate("nucleusd"),
                                             "--codex", self.prior["harness_executable"]]

    def runtime_verify(self):
        status = self.maintenance("status")
        if status["holds"] != [self.run_id] or not status["drained"]:
            raise Stopped("Nucleus verification requires this run's drained hold")
        health = command([self.cli, "--compact", "maintenance", "health", self.run_id], json_output=True)
        if health.get("harnessExecutable") != self.prior["harness_executable"]:
            raise Stopped("configured Nucleus harness changed")
        return {"service_ready": True}

    def runtime_recover(self):
        status = self.maintenance("status")
        if self.run_id in status["holds"]:
            return self.runtime_verify()
        if status["holds"]:
            raise Stopped("another owner holds Nucleus during recovery")
        health = command([self.cli, "--compact", "health"], json_output=True)
        if health.get("harnessExecutable") != self.prior["harness_executable"]:
            raise Stopped("configured Nucleus harness changed during recovery")
        return {"service_ready": True}

    def recover(self):
        recovery = self.request.get("recovery") or {}
        if recovery.get("apply_started") is True and recovery.get("applied") is not True:
            # Candidate files can be selected while the old daemon is resident,
            # even when both declare the same version. Health does not
            # establish which bytes that process loaded. Never release this
            # hold on an uncertain service cutover, including unchanged files.
            raise Stopped("Nucleus service cutover is uncertain; hold retained. Use the supported Nucleus service recovery procedure before resolving this deployment.")
        observed = self.installed()
        prior = observed["current"] == self.prior.get("current")
        if prior:
            self.check_prior()
        else:
            self.prove_candidate_release(observed)
            status = self.maintenance("status")
            if not status["holds"] and recovery.get("verified") is not True:
                raise Stopped("candidate Nucleus has no hold or captured successful verification")
        # A verified Nucleus is deliberately reopened before requester dispatch resumes.
        # Its subsequent recovery must validate ordinary service health rather
        # than demand that already-released hold or rerun an uncertain cutover.
        self.runtime_recover()
        return response("recovered", "coherent Nucleus program and service verified",
                        {"safe_to_release": True, "installed": "prior" if prior else "candidate"})


if __name__ == "__main__":
    raise SystemExit(main(NucleusAdapter, json.loads(Path(__file__).with_name("adapter.json").read_text())))
