"""One serial delivery state machine, with intent persisted before external effects."""

from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import sys
import time
import uuid

from ci_manager import VERSION
from ci_manager import git_ops as git
from ci_manager.integrations import (
    DeferredError, IntegrationError, NucleusClient, TransportError,
    freeze_request, load_prompt_selection, make_request, send_email, terminal_result,
)
from ci_manager.storage import (
    ManagerError, Store, TERMINAL, atomic_bytes, atomic_json, lock, private_directory,
)


class Worker:
    def __init__(self, store: Store, descriptor: int):
        self.store = store
        self.root = store.root
        self.config = store.get("config")
        if not self.config:
            raise ManagerError("CI manager is not initialized")
        self.repository = Path(self.config["repository"])
        if str(git.common(self.repository)) != self.config["common_git_dir"]:
            raise ManagerError("configured repository identity changed")
        self.descriptor = descriptor
        self.nucleus = NucleusClient()

    def directory(self, job: dict) -> Path:
        path = self.root / "jobs" / job["id"]
        private_directory(path)
        return path

    def worktree(self, job: dict) -> Path:
        return self.directory(job) / "worktree"

    def save(self, job: dict, phase: str | None = None) -> None:
        self.store.save(job, phase)

    def claim(self) -> dict | None:
        with self.store.transaction():
            active = self.store.active()
            if active:
                return active
            if self.store.get("paused", True) or self.store.owners():
                return None
            row = self.store.db.execute("SELECT * FROM jobs WHERE phase='queued' ORDER BY sequence LIMIT 1").fetchone()
            if not row:
                return None
            job = self.store.decode(row)
            job.update(base_commit=git.commit(self.repository, git.ACCEPTED),
                       worker_version=VERSION, worker_path=str(Path(__file__).resolve()),
                       attempts=[], validations=[], candidate_commit=None, model_unresolved=False)
            self.save(job, "integrating")
            return job

    def finish(self, job: dict, outcome: str, message: str, *, unresolved: bool = False) -> None:
        job["stopped_phase"] = job["phase"]
        job.update(outcome=outcome, outcome_message=message, unresolved=unresolved)
        if outcome not in {"succeeded", "already_included"}:
            self.store.set("paused", True)
        if "notification" not in job:
            deployment = job.get("deployment_result", {})
            title = "deployed" if job.get("installation_verified") else outcome.replace("_", " ")
            body = "\n".join([
                f"Cell CI job {job['id']}: {message}",
                f"Submitted commit: {job['input_commit']}",
                f"Accepted base: {job.get('base_commit', 'not started')}",
                f"Final candidate: {job.get('candidate_commit') or 'none'}",
                f"Accepted: {bool(job.get('accepted'))}",
                f"Repair invocations: {len(job.get('attempts', []))}",
                "Models: " + ", ".join(item["model"] for item in job.get("attempts", [])),
                f"Deployment state: {deployment.get('state', 'not started')}",
                f"Artifacts: {self.directory(job)}",
                f"Inspect: cell-ci status {job['id']}",
            ])
            job["notification"] = {"subject": f"Cell CI {job['id']}: {title}", "body": body,
                                   "key": f"cell-ci/{job['id']}/outcome/{job.get('outcome_generation', 1)}", "attempts": [],
                                   "created": time.time(), "state": "pending"}
        self.save(job, "notifying")

    def step(self, job: dict) -> None:
        # Cancellation drains a running child. It does not erase its effect evidence.
        if job["cancel_requested"] and job["phase"] in {"integrating", "repair_prepare", "applying", "accepting"}:
            self.finish(job, "cancelled", "Cancelled before the next external operation.")
            return
        operation = getattr(self, job["phase"], None)
        if operation is None:
            raise ManagerError(f"unknown active phase: {job['phase']}")
        operation(job)

    def integrating(self, job: dict) -> None:
        base, submitted = job["base_commit"], job["input_commit"]
        if git.git(self.repository, "merge-base", "--is-ancestor", submitted, base, check=False).returncode == 0:
            job["candidate_commit"] = base
            self.finish(job, "already_included", "The submitted commit is already in accepted history; no fresh deployment was run.")
            return
        worktree = self.worktree(job)
        git.ensure_worktree(self.repository, worktree, base)
        merge = git.git(worktree, "-c", "merge.autoStash=false", "merge", "--no-commit", "--no-ff", submitted, check=False)
        atomic_bytes(self.directory(job) / "merge.log", merge.stdout + merge.stderr)
        if merge.returncode:
            atomic_bytes(self.directory(job) / "merge-conflicts.diff", git.git(worktree, "diff", check=False).stdout)
            self.finish(job, "failed", "Integration failed. Merge conflicts are outside automated remediation.")
            return
        tree = git.value(worktree, "write-tree")
        candidate = git.commit_tree(self.repository, tree, [base, submitted], job["id"],
                                    int(job["created"]), f"Integrate CI submission {job['id']}")
        git.git(self.repository, "update-ref", git.private_ref(job["id"], "candidate"), candidate)
        job["candidate_commit"] = candidate
        self.save(job, "checking")
        git.ensure_worktree(self.repository, worktree, candidate)

    def process(self, job: dict, name: str, command: list[str], cwd: Path) -> tuple[dict, bytes, bytes] | None:
        directory = self.directory(job)
        request_path = directory / f"{name}.request.json"
        result_path = directory / f"{name}.result.json"
        stdout_path, stderr_path = directory / f"{name}.stdout", directory / f"{name}.stderr"
        if result_path.exists():
            result = json.loads(result_path.read_text())
            if "exit_code" not in result:
                raise ManagerError(f"child execution is unresolved: {result.get('error')}")
            return result, stdout_path.read_bytes(), stderr_path.read_bytes()
        if request_path.exists():
            # A surviving child inherits the worker lock. Once that lock can be
            # reacquired, missing terminal evidence still cannot be called success.
            raise ManagerError(f"interrupted {name} has no terminal child receipt; inspect {request_path}")
        request = {"command": command, "cwd": str(cwd), "stdout": str(stdout_path),
                   "stderr": str(stderr_path), "started": str(directory / f"{name}.started.json"),
                   "result": str(result_path)}
        atomic_json(request_path, request)
        subprocess.run([sys.executable, str(Path(__file__).with_name("process.py")),
                        str(request_path), str(self.descriptor)], pass_fds=(self.descriptor,), check=False)
        if not result_path.exists():
            raise ManagerError(f"{name} supervisor ended without a terminal receipt")
        return self.process(job, name, command, cwd)

    def checking(self, job: dict) -> None:
        candidate = job["candidate_commit"]
        worktree = self.worktree(job)
        # Recovery materializes a previously committed candidate before checking.
        git.ensure_worktree(self.repository, worktree, candidate)
        ordinal = len(job["validations"])
        command = [sys.executable, str(worktree / "pipeline/select_changes.py"), "run",
                   "--base", job["base_commit"],
                   "--candidate", candidate, "--json"]
        result, output, diagnostic = self.process(job, f"validation-{ordinal}", command, worktree)
        try:
            receipt = json.loads(output)
        except (ValueError, UnicodeError) as exception:
            raise ManagerError("validator returned no aggregate JSON receipt") from exception
        if (receipt.get("schema_version") != 1 or receipt.get("base_commit") != job["base_commit"]
                or receipt.get("candidate_commit") != candidate):
            raise ManagerError("validator receipt does not identify the requested candidate")
        git.clean_candidate(worktree, candidate)
        retained = self.directory(job) / f"validation-{ordinal}.json"
        atomic_json(retained, receipt)
        # Broker retention is independent. Copy the failed gate transcript now.
        diagnostics = bytearray(diagnostic[-1024 * 1024:])
        for gate in receipt.get("gates", []):
            path = gate.get("diagnostic_path")
            if path and Path(path).is_file():
                with Path(path).open("rb") as stream:
                    diagnostics.extend(stream.read(8 * 1024 * 1024))
        diagnostic_path = self.directory(job) / f"validation-{ordinal}.log"
        atomic_bytes(diagnostic_path, bytes(diagnostics))
        job["validations"].append({"candidate": candidate, "receipt": str(retained),
                                    "diagnostics": str(diagnostic_path), "state": receipt.get("state")})
        job["last_receipt"] = receipt
        if self.store.job(job["id"])["cancel_requested"]:
            self.finish(job, "cancelled", "Cancelled after the current validation drained.")
        elif receipt.get("state") == "passed" and result["exit_code"] == 0:
            self.save(job, "accepting")
        elif receipt.get("state") == "failed":
            self.save(job, "repair_prepare")
        else:
            self.finish(job, "failed", f"Validation execution requires recovery: {receipt.get('state')}",
                        unresolved=receipt.get("state") in {"lost", "stale"})

    def repair_prepare(self, job: dict) -> None:
        policy = job["policy"]
        used = len(job["attempts"])
        if used >= policy["luna_attempts"] + policy["terra_attempts"]:
            self.finish(job, "failed", "The repair invocation budget is exhausted.")
            return
        if self.store.owners():
            job["waiting_reason"] = "requester_maintenance"
            self.save(job)
            return
        # Retain the existing count keys for stored policy compatibility.
        primary = used < policy["luna_attempts"]
        model, reasoning = ("gpt-5.6-terra", "medium") if primary else ("gpt-5.6-sol", "high")
        identity = f"ci-{job['id']}-repair-{used + 1}"
        if "prompts" not in job:
            job["prompts"] = load_prompt_selection()
            self.save(job)
        request = make_request(job_id=identity, domain_job=job["id"], cwd=str(self.worktree(job)),
                               base=job["base_commit"], candidate=job["candidate_commit"],
                               diagnostic_path=job["validations"][-1]["diagnostics"],
                               history=job["attempts"], model=model, reasoning=reasoning,
                               prompts=job["prompts"],
                               timeout_seconds=policy["model_timeout_seconds"])
        request_path = self.directory(job) / f"repair-{used + 1}.request.json"
        atomic_bytes(request_path, freeze_request(request))
        # The admission lock closes the race between maintenance hold and submit.
        with lock(self.root / "admission.lock"):
            if self.store.owners():
                return
            job["attempts"].append({"number": used + 1, "nucleus_job_id": identity,
                                     "model": model, "reasoning": reasoning, "request": str(request_path),
                                     "parent": job["candidate_commit"], "stamp": int(time.time())})
            job["model_unresolved"] = True
            job.pop("waiting_reason", None)
            self.save(job, "repair_wait")

    def repair_wait(self, job: dict) -> None:
        attempt = job["attempts"][-1]
        identity = attempt["nucleus_job_id"]
        try:
            view = self.nucleus.get(identity)
            if view is None:
                with lock(self.root / "admission.lock"):
                    if self.store.owners() or job["cancel_requested"]:
                        # A confirmed 404 proves this prepared request was not admitted.
                        job["model_unresolved"] = False
                        attempt["state"] = "not_admitted"
                        if job["cancel_requested"]:
                            self.finish(job, "cancelled", "Cancelled before model admission.")
                        else:
                            job["waiting_reason"] = "requester_maintenance"
                            self.save(job)
                        return
                    job["model_unresolved"] = True
                    self.save(job)
                    self.nucleus.submit(Path(attempt["request"]).read_bytes())
                    attempt["transport_failures"] = 0
                    self.save(job)
                return
            attempt["transport_failures"] = 0
            job["model_unresolved"] = True
            if job["cancel_requested"]:
                self.nucleus.cancel(identity)
            terminal = terminal_result(view)
            if terminal is None:
                job["waiting_reason"] = "nucleus"
                self.save(job)
                return
            attempt["terminal"] = {key: value for key, value in terminal.items() if key != "final_message"}
            attempt["state"] = terminal["state"]
            atomic_json(self.directory(job) / f"repair-{attempt['number']}.result.json", view)
            if terminal.get("attempt_state") == "lost":
                self.finish(job, "failed", "Nucleus lost the repair execution; absence of an orphan must be established before continuation.", unresolved=True)
                return
            job["model_unresolved"] = False
            if job["cancel_requested"]:
                self.finish(job, "cancelled", "The model invocation was cancelled and drained.")
            elif terminal["state"] == "completed" and isinstance(terminal.get("final_message"), str):
                path = self.directory(job) / f"repair-{attempt['number']}.patch"
                atomic_bytes(path, terminal["final_message"].encode())
                attempt["patch"] = str(path)
                self.save(job, "applying")
            elif terminal["state"] == "completed":
                attempt["rejection"] = "Completed invocation did not return a final patch response."
                self.save(job, "repair_prepare")
            else:
                self.finish(job, "failed", f"Model execution failed: {terminal.get('reason') or terminal['state']}")
        except DeferredError as exception:
            # A quota rejection admits no job. Preserve request identity and budget.
            job["model_unresolved"] = False
            job["waiting_reason"] = exception.code
            self.save(job)
        except TransportError as exception:
            job["waiting_reason"] = "nucleus_transport"
            job["last_transport_error"] = str(exception)
            attempt["transport_failures"] = attempt.get("transport_failures", 0) + 1
            if attempt["transport_failures"] >= 5:
                self.finish(job, "failed", "Nucleus observation remains unavailable; the existing request identity is retained.", unresolved=True)
                return
            self.save(job)

    def applying(self, job: dict) -> None:
        attempt = job["attempts"][-1]
        parent = attempt["parent"]
        try:
            raw = Path(attempt["patch"]).read_bytes()
            tree = git.patch_tree(self.repository, parent, raw, self.directory(job) / "patch.index")
        except ManagerError as exception:
            attempt["rejection"] = str(exception)
            self.save(job, "repair_prepare")
            return
        candidate = git.commit_tree(self.repository, tree, [parent], job["id"], attempt["stamp"],
                                    f"Apply CI repair {attempt['number']} ({attempt['model']})")
        attempt["candidate"] = candidate
        # Persist the intended commit before publishing the private ref.
        self.save(job)
        git.advance(self.repository, git.private_ref(job["id"], "candidate"), parent, candidate)
        job["candidate_commit"] = candidate
        git.ensure_worktree(self.repository, self.worktree(job), candidate)
        self.save(job, "checking")

    def accepting(self, job: dict) -> None:
        receipt = job["last_receipt"]
        candidate, base = job["candidate_commit"], job["base_commit"]
        if receipt["state"] != "passed" or receipt["candidate_commit"] != candidate:
            raise ManagerError("acceptance has no matching successful validation")
        git.clean_candidate(self.worktree(job), candidate)
        job["acceptance_intent"] = {"old": base, "new": candidate}
        self.save(job)
        git.advance(self.repository, git.ACCEPTED, base, candidate)
        job["accepted"] = True
        products = job["deploy_products"]
        if products is None:
            selection = receipt["selection"]
            products = sorted(set(selection["product_tests"] + selection["platform_products"]))
        job["deployment_request"] = {"request_id": f"ci:{job['id']}:deployment:1",
                                      "source_commit": candidate, "products": products}
        self.save(job, "deploying")

    def deployment_command(self, job: dict, command: str) -> list[str]:
        request = job["deployment_request"]
        args = [sys.executable, str(self.worktree(job) / "deployment/cli.py"), command,
                "--request-id", request["request_id"]]
        if command == "start":
            args += ["--source-commit", request["source_commit"], *request["products"]]
        return args

    def deployment_read(self, job: dict, action: str) -> dict:
        result = subprocess.run(self.deployment_command(job, action), cwd=self.worktree(job),
                                capture_output=True, timeout=120,
                                env=dict(os.environ, PYTHONDONTWRITEBYTECODE="1"))
        try:
            return json.loads(result.stdout)
        except (ValueError, UnicodeError) as exception:
            raise ManagerError(f"deployment {action} has no machine receipt: {result.stderr.decode('utf-8', 'replace')[-1024:]}") from exception

    def deploying(self, job: dict) -> None:
        request = job["deployment_request"]
        if not request["products"]:
            job["deployment_result"] = {"state": "not_required", "source_commit": job["candidate_commit"]}
            self.finish(job, "succeeded", "CI accepted the candidate; no products were selected for deployment.")
            return
        # Always inspect the same operation first, including after lost stdout.
        observed = self.deployment_read(job, "status")
        state = observed.get("operation_state")
        if job["cancel_requested"] and state == "not_found":
            self.finish(job, "cancelled", "Cancelled after acceptance and before deployment admission.")
            return
        if state in {"active", "blocked"}:
            job["waiting_reason"] = "deployment"
            self.save(job)
            return
        if state == "terminal":
            result = observed
        else:
            action = "reconcile" if state == "needs_reconciliation" else "start" if state == "not_found" else None
            if action is None:
                raise ManagerError("unknown deployment operation state")
            # A retained deployment identity makes a repeated start safe. Child
            # invocation files are distinct observations of that same operation.
            ordinal = job.get("deployment_observations", 0)
            name = f"deployment-{ordinal}"
            request_path = self.directory(job) / f"{name}.request.json"
            if request_path.exists() and not (self.directory(job) / f"{name}.result.json").exists():
                ordinal += 1
                name = f"deployment-{ordinal}"
            job["deployment_observations"] = ordinal
            self.save(job)
            _, output, _ = self.process(job, name, self.deployment_command(job, action), self.worktree(job))
            try:
                result = json.loads(output)
            except (ValueError, UnicodeError) as exception:
                # Consult retained deployment authority on the next iteration.
                job["deployment_observations"] = ordinal + 1
                self.save(job)
                return
            if result.get("operation_state") != "terminal":
                job["deployment_observations"] = ordinal + 1
                self.save(job)
                return
        if result.get("source_commit") != request["source_commit"] or result.get("request_id") != request["request_id"]:
            raise ManagerError("deployment receipt does not match the admitted operation")
        atomic_json(self.directory(job) / "deployment.json", result)
        job["deployment_result"] = result
        installed = result.get("state") in {"succeeded", "installed", "cleanup_failed"}
        released = result.get("maintenance", {}).get("state") == "released"
        job["installation_verified"] = installed and released
        if job["installation_verified"]:
            self.finish(job, "succeeded", "Deployment verified the accepted candidate." +
                        (" Cleanup requires attention." if result.get("state") == "cleanup_failed" else ""))
        else:
            self.finish(job, "failed", "Deployment failed or recovery did not establish successful installation.",
                        unresolved=result.get("maintenance", {}).get("state") not in {"released", "not_started"})

    def notifying(self, job: dict) -> None:
        notice = job["notification"]
        attempts = notice["attempts"]
        if notice["state"] == "accepted":
            self.save(job, "blocked" if job.get("unresolved") else job["outcome"])
            return
        now = time.time()
        if attempts and now - attempts[-1]["started"] < 300:
            return
        if len(attempts) >= 2 or now - notice["created"] >= 23 * 3600:
            notice["state"] = "uncertain"
            job["notification_blocked"] = True
            self.store.set("paused", True)
            self.save(job, "blocked")
            return
        attempts.append({"started": now})
        # Persist before calling Email; a crash consumes this transport invocation.
        self.save(job)
        try:
            receipt = send_email(notice["subject"], notice["body"], notice["key"])
            attempts[-1]["receipt"] = receipt
            if receipt.get("accepted") is True:
                notice["state"] = "accepted"
            else:
                attempts[-1]["error"] = "Email did not report provider acceptance"
        except IntegrationError as exception:
            attempts[-1]["error"] = str(exception)
            attempts[-1]["uncertain"] = getattr(exception, "uncertain", True)
        self.save(job)

    def blocked(self, job: dict) -> None:
        if self.store.get("recovery_request") != job["id"]:
            return
        self.store.set("recovery_request", None)
        if job.get("notification_blocked") and not job.get("unresolved"):
            job["last_error"] = "Email acceptance is still uncertain; recovery cannot create a replacement message."
            self.save(job)
            return
        phase = job.get("stopped_phase")
        next_phase = None
        if job.get("model_unresolved") and job.get("attempts"):
            view = self.nucleus.get(job["attempts"][-1]["nucleus_job_id"])
            terminal = terminal_result(view) if view is not None else None
            if terminal and terminal.get("attempt_state") == "lost":
                job["last_error"] = "Nucleus still reports lost execution; orphan containment is not established."
            else:
                next_phase = "repair_wait"
        elif phase == "deploying" and job.get("deployment_request"):
            observed = self.deployment_read(job, "status")
            if observed.get("operation_state") in {"terminal", "active", "needs_reconciliation", "not_found"}:
                next_phase = "deploying"
        elif phase == "checking":
            ordinal = len(job.get("validations", []))
            if (self.directory(job) / f"validation-{ordinal}.result.json").is_file():
                next_phase = "checking"
        if next_phase:
            job["outcome_generation"] = job.get("outcome_generation", 1) + 1
            for key in ("notification", "notification_blocked", "outcome", "outcome_message", "unresolved"):
                job.pop(key, None)
            self.save(job, next_phase)
        else:
            job.setdefault("last_error", "Recovery has no authoritative new outcome; the job remains blocked.")
            self.save(job)

    def run(self) -> None:
        while True:
            job = self.claim()
            if job is not None:
                try:
                    self.step(job)
                except (ManagerError, IntegrationError, OSError, ValueError, subprocess.SubprocessError) as exception:
                    if job["phase"] in {"notifying", "blocked"}:
                        job["last_error"] = str(exception)
                        self.store.set("paused", True)
                        self.save(job, "blocked")
                    else:
                        self.finish(job, "failed", str(exception),
                                    unresolved=bool(job.get("model_unresolved")) or job["phase"] in {"checking", "deploying"})
            time.sleep(2)
