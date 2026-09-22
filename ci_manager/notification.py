"""Concise outcome mail projected from retained CI and deployment evidence."""

from __future__ import annotations

from pathlib import Path
import re


NAMES = {"ci-manager": "Cell CI", "cell-ci": "Cell CI", "decisions": "Krisis",
         "weaver-narrative": "Weaver", "pipeline": "CI pipeline",
         "catalog": "capability catalog", "broker": "CI broker"}


def name(value: str) -> str:
    return NAMES.get(value, value.replace("-", " ").capitalize())


def check_name(gate: str) -> str:
    parts = gate.split(".")
    if len(parts) == 3 and parts[:2] == ["cell", "platform"]:
        return name(parts[2]) + " checks"
    if parts[:1] == ["cell"]:
        parts = parts[1:]
    return " ".join(name(part) for part in parts) + " checks"


def detail(value: str, job: dict) -> str:
    """Remove machine references from a short diagnostic, never append logs."""
    text = str(value).splitlines()[0] if value else ""
    references = [job.get(key) for key in ("id", "input_commit", "base_commit", "candidate_commit")]
    for record in (job.get("deployment_request", {}), job.get("deployment_result", {}),
                   *((job.get("last_receipt") or {}).get("gates") or []),
                   *job.get("attempts", [])):
        references.extend(value for key, value in record.items()
                          if key.endswith(("_id", "_key")) or key == "model")
    for reference in references:
        if isinstance(reference, str) and reference:
            text = text.replace(reference, "[reference]")
    text = re.sub(r"(?:[\w.-]+:)*[0-9a-fA-F]{8}-[0-9a-fA-F-]{27,}(?:[\w:.-]*)", "[reference]", text)
    text = re.sub(r"\b[0-9a-fA-F]{7,64}\b", "[reference]", text)
    text = re.sub(r"(?:~?/|[A-Za-z]:\\)[^\n]*", "[local details omitted]", text)
    text = " ".join(text.split())
    return text[:350] + ("…" if len(text) > 350 else "")


def validation_reason(job: dict, failure: dict) -> str:
    validations = job.get("validations") or []
    path = validations[-1].get("diagnostics") if validations else None
    if path:
        try:
            with Path(path).open("rb") as stream:
                stream.seek(0, 2)
                stream.seek(max(0, stream.tell() - 1024 * 1024))
                transcript = stream.read().decode("utf-8", "replace")
            failed = re.findall(r"^test (.+?) \.\.\. FAILED$|^(?:FAIL|ERROR): (.+)$",
                                transcript, re.MULTILINE)
            names = list(dict.fromkeys(detail(first or second, job) for first, second in failed))
            if names:
                return "Failed tests: " + "; ".join(names[:3]) + ("; and others." if len(names) > 3 else ".")
        except OSError:
            pass
    reason = failure.get("message")
    if not reason or reason == "body exited nonzero":
        return "The check failed; no specific cause was reported."
    return detail(reason, job)


def render(job: dict) -> tuple[str, str]:
    deployment = job.get("deployment_result") or {}
    receipt = job.get("last_receipt") or {}
    selection = receipt.get("selection") or {}
    products = (deployment.get("products") or job.get("deployment_request", {}).get("products")
                or selection.get("product_tests") or [])
    # These receipts do not promise installed versions or individual final states.
    scope = ", ".join(name(product) for product in products)
    outcome = job["outcome"]
    phase = job.get("stopped_phase", job.get("phase"))
    lines = []
    if job.get("installation_verified"):
        title = "deployed"
        lines = [f"{scope or 'Selected products'} deployed successfully.",
                 "Required checks and deployment verification passed."]
        if deployment.get("state") == "cleanup_failed":
            title = "deployed; cleanup failed"
            lines += ["", "Installed releases are verified, but release cleanup failed."]
    elif outcome == "already_included":
        title = "already included"
        lines = ["These changes are already included. No new checks or deployment ran."]
    elif outcome == "succeeded":
        title = "passed"
        lines = ["Required checks passed. No deployment was required."]
    elif outcome == "cancelled":
        title = "cancelled"
        lines = ["CI was cancelled."]
    else:
        failure = receipt.get("failure") or {}
        gate = failure.get("gate")
        failed_checks = [check_name(item["gate"]) for item in receipt.get("gates", [])
                         if item.get("state") != "passed" and item.get("gate")]
        if phase == "deploying":
            failed = "deployment"
            reason = detail(deployment.get("detail"), job) or "The deployment result could not be verified."
        elif phase == "integrating":
            failed = "integration"
            reason = detail(job.get("outcome_message"), job)
        elif phase in {"repair_wait", "applying"}:
            failed = "automatic repair"
            reason = detail(job.get("outcome_message"), job)
        elif gate or failed_checks:
            failed = check_name(gate) if gate else ", ".join(failed_checks)
            reason = validation_reason(job, failure)
        else:
            failed = "validation" if phase == "checking" else "CI processing"
            reason = detail(job.get("outcome_message"), job) or "The cause was not reported."
        title = f"failed — {failed}"
        lines = [f"What failed: {failed}.", reason]
        if phase == "repair_prepare" and job.get("attempts"):
            lines.append("Automatic repair did not resolve the failure.")

    if outcome not in {"succeeded", "already_included"}:
        lines.append("")
        if job.get("accepted"):
            lines.append("Required checks passed; the changes were accepted.")
        if phase == "deploying" or deployment:
            lines.append("Successful deployment was not established.")
            recovery = deployment.get("recovery", {}).get("state")
            if recovery == "succeeded":
                lines.append("Deployment recovery completed.")
            elif recovery == "failed":
                lines.append("Deployment recovery also failed.")
        else:
            lines.append("Nothing was deployed.")
        lines.append("The CI queue is paused." + (" Recovery is required." if job.get("unresolved") else ""))
    suffix = f" — {scope}" if scope else ""
    return f"Cell CI: {title}{suffix}", "\n".join(lines)
