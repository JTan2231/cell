"""One retained, isolated Annals examination and its Nucleus/Usage proofs."""
import json
import os
from pathlib import Path

from deployment.adapter_support import Stopped, command, digest
from deployment.cli import durable_json, private_directory, read_json


LABEL = "Cell deployment verification"
SOURCE = ("A deployment prevents new application work while replacing its programs.\n"
          "Previously admitted work must finish before replacement begins.\n"
          "Dispatch resumes only after execution and application results are verified.\n")


def immutable_text(path, content):
    """Keep the same synthetic input and configuration through recovery."""
    if path.exists() or path.is_symlink():
        digest(path)
        if path.read_text() != content:
            raise Stopped("Annals canary input or configuration changed")
        return
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, "w") as stream:
        stream.write(content)
        stream.flush()
        os.fsync(stream.fileno())
    directory = os.open(path.parent, os.O_RDONLY)
    try:
        os.fsync(directory)
    finally:
        os.close(directory)


def verify(adapter):
    root = adapter.run_dir / "annals-canary"
    private_directory(root)
    root = root.resolve()
    library = root / "annals.db"
    if library.is_symlink():
        raise Stopped("Annals canary library must not be symbolic")
    socket = str(adapter.socket())
    nucleus = adapter.home / ".local/bin/nucleus"
    config, usage_config, source = root / "config.toml", root / "usage.toml", root / "work.txt"
    immutable_text(source, SOURCE)
    immutable_text(config, 'library = "annals.db"\n[inbox]\nroot = "spool"\n'
                   '[liaison]\nquality = "medium"\nnucleus_socket = ' + json.dumps(socket) + "\n")
    immutable_text(usage_config, 'library = "annals.db"\nspool = "spool"\nnucleus = '
                   + json.dumps(str(nucleus)) + "\nnucleus_socket = " + json.dumps(socket) + "\n")
    private_directory(root / "spool")
    args = [adapter.payload(), "--config", config, "--json"]
    environment = {**adapter.environment(), "ANNALS_LIBRARY": "", "ANNALS_CONFIG": ""}
    marker = root / "examination-started.json"
    identity = {"schema": 1, "run_id": adapter.run_id, "source_sha256": digest(source)}
    if marker.exists():
        if read_json(marker) != identity:
            raise Stopped("Annals canary examination identity changed")
    else:
        if not library.exists():
            command([*args, "init"], env=environment, json_output=True)
        initial = command([*args, "stats"], env=environment, json_output=True)["data"]
        if any(initial.get(key) != 0 for key in ("revision", "work_count", "model_run_count")):
            raise Stopped("Annals canary library has unowned prior work")
        # Persist intent before starting the model. An interrupted verifier may
        # inspect this examination, but cannot start a replacement examination.
        durable_json(marker, identity)
        result = command([*args, "integrate", source, "--name", LABEL, "--apply"],
                         env=environment, timeout=1800, json_output=True)
        durable_json(root / "integration-result.json", result)

    stats = command([*args, "stats"], env=environment, json_output=True)["data"]
    change = command([*args, "change", "show", "--work", LABEL],
                     env=environment, json_output=True)["data"]
    required = {"revision": 1, "work_count": 1, "model_run_count": 1,
                "commit_count": 1, "pending_reconciliation_count": 0}
    if (any(stats.get(key) != value for key, value in required.items())
            or stats.get("concept_count", 0) < 1 or stats.get("evidence_count", 0) < 1
            or change.get("status") != "applied" or change.get("applied_revision") != 1
            or change.get("work") != LABEL or change.get("base_revision") != 0):
        raise Stopped("Annals canary did not retain one applied evidence-grounded reconciliation")

    usage = command([adapter.payload().with_name("annals-usage"), "report", "--json",
                     "--config", usage_config, "--limit", "1000"],
                    env=environment, timeout=180, json_output=True)
    runs = [run for run in usage.get("unattributedRuns", [])
            if run.get("annalsModelRunId") is not None]
    for delivery in usage.get("deliveries", []):
        runs.extend(run for run in delivery.get("attempts", [])
                    if run.get("annalsModelRunId") is not None)
    if len(runs) != 1:
        raise Stopped("Annals canary must correlate exactly one retained model run")
    run = runs[0]
    if (run.get("workLabel") != LABEL or run.get("baseRevision") != 0
            or run.get("status") != "completed" or run.get("coverage") not in {"exact", "cumulative"}
            or not run.get("threadId") or not run.get("turnId")
            or not isinstance(run.get("usage"), dict) or run["usage"].get("totalTokens", 0) <= 0):
        raise Stopped("Annals canary usage lacks completed, correlated token coverage")
    job = command([nucleus, "--socket", socket, "jobs", "show", run["jobId"], "--compact"],
                  env=environment, json_output=True)
    attempts = job.get("attempts", [])
    output = attempts[0].get("output") if len(attempts) == 1 else None
    output = output if isinstance(output, dict) else {}
    requester = job.get("summary", {}).get("requester", {})
    if (job.get("summary", {}).get("state") != "completed" or len(attempts) != 1
            or attempts[0].get("state") != "completed" or requester.get("program") != "annals"
            or requester.get("id") != run["modelRunToken"]
            or output.get("threadId") != run["threadId"] or output.get("turnId") != run["turnId"]
            or not isinstance(output.get("finalMessage"), str) or not output["finalMessage"].strip()):
        raise Stopped("Annals canary Nucleus job lacks its correlated structured final output")
    proof = {"verified": True, "directory": str(root), "job_id": run["jobId"],
             "model_run_token": run["modelRunToken"], "revision": 1,
             "usage_coverage": run["coverage"], "usage": run["usage"]}
    durable_json(root / "verification.json", proof)
    return {"canary": proof}
