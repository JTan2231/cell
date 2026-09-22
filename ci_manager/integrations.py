"""Public Nucleus, Bazaar, and Email boundaries for the serial CI manager.

The manager owns persistence, retries, patch validation, and application success.
This module never applies a patch or starts a retry on its own.
"""

from __future__ import annotations

import http.client
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
from typing import Any
from urllib.parse import quote


MAX_REQUEST_BYTES = 16 * 1024 * 1024
MAX_RESPONSE_BYTES = 96 * 1024 * 1024
MAX_RECEIPT_BYTES = 4096
PROMPT_SELECTION_ID = "cell.prompts.ci-manager"
PROMPT_INSTRUCTIONS_ID = "ci-manager.repair.instructions"
PROMPT_TEMPLATE_ID = "ci-manager.repair.prompt"
MODEL_TIERS = {"gpt-5.6-luna": "low", "gpt-5.6-terra": "medium"}
JOB_STATES = {"accepted", "running", "waiting_on_requester", "completed", "failed", "cancelled"}
TERMINAL_STATES = {"completed", "failed", "cancelled"}
TERMINAL_ATTEMPT_STATES = {"completed", "failed", "cancelled", "timed_out", "lost"}


class IntegrationError(Exception):
    def __init__(self, message: str, *, code: str = "integration_error",
                 status: int | None = None, details: Any = None,
                 uncertain: bool = False) -> None:
        super().__init__(message)
        self.code = code
        self.status = status
        self.details = details
        self.uncertain = uncertain


class TransportError(IntegrationError):
    """No authoritative response was obtained; preserve request identity."""


class DeferredError(IntegrationError):
    """Admission is paused; this does not establish a failed repair."""


class RejectedError(IntegrationError):
    """The provider rejected the request, or local input is invalid."""


class EmailError(IntegrationError):
    """Email did not return a proved acceptance receipt."""


def _absolute(path: str | os.PathLike[str], label: str) -> Path:
    value = Path(path)
    if not value.is_absolute():
        raise RejectedError(f"{label} must be an absolute path", code="invalid_path")
    return value


def freeze_request(request: dict[str, Any]) -> bytes:
    """Return the exact bytes the manager must retain before submission."""
    try:
        result = json.dumps(request, ensure_ascii=False, sort_keys=True,
                            separators=(",", ":"), allow_nan=False).encode("utf-8")
    except (TypeError, ValueError, UnicodeError) as error:
        raise RejectedError("Nucleus request is not valid JSON", code="invalid_request") from error
    if len(result) > MAX_REQUEST_BYTES:
        raise RejectedError("Nucleus request exceeds the request limit", code="request_too_large")
    return result


class _UnixConnection(http.client.HTTPConnection):
    def __init__(self, path: Path, timeout: float) -> None:
        super().__init__("localhost", timeout=timeout)
        self.path = path

    def connect(self) -> None:
        connection = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        try:
            connection.settimeout(self.timeout)
            connection.connect(str(self.path))
        except OSError:
            connection.close()
            raise
        self.sock = connection


class NucleusClient:
    """One-request HTTP connections pinned to Nucleus's public Unix socket."""

    def __init__(self, socket_path: str | os.PathLike[str] | None = None,
                 timeout: float = 30.0) -> None:
        selected = socket_path if socket_path is not None else os.environ.get("NUCLEUS_SOCKET")
        if selected is None:
            home = os.environ.get("HOME")
            if not home:
                raise RejectedError("HOME or an explicit Nucleus socket is required", code="invalid_path")
            selected = _absolute(home, "HOME") / "Library/Application Support/Nucleus/nucleus.sock"
        self.socket_path = _absolute(selected, "Nucleus socket")
        if timeout <= 0:
            raise RejectedError("Nucleus HTTP timeout must be positive", code="invalid_timeout")
        self.timeout = timeout

    def _request(self, method: str, path: str, body: bytes | None = None,
                 *, missing_ok: bool = False) -> dict[str, Any] | None:
        connection = _UnixConnection(self.socket_path, self.timeout)
        uncertain = method == "POST"
        try:
            headers = {"Accept": "application/json"}
            if body is not None:
                headers["Content-Type"] = "application/json"
            connection.request(method, path, body=body, headers=headers)
            response = connection.getresponse()
            status = response.status
            raw = response.read(MAX_RESPONSE_BYTES + 1)
        except (OSError, http.client.HTTPException) as error:
            raise TransportError("Nucleus did not return a complete response",
                                 code="transport_error", uncertain=uncertain) from error
        finally:
            connection.close()
        if len(raw) > MAX_RESPONSE_BYTES:
            raise TransportError("Nucleus response exceeds the client limit",
                                 code="response_too_large", status=status, uncertain=uncertain)
        if status == 404 and missing_ok:
            return None
        try:
            value = json.loads(raw)
        except (ValueError, UnicodeError) as error:
            raise TransportError("Nucleus returned an invalid JSON response",
                                 code="invalid_response", status=status, uncertain=uncertain) from error
        if not isinstance(value, dict):
            raise TransportError("Nucleus response is not an object", code="invalid_response",
                                 status=status, uncertain=uncertain)
        if 200 <= status < 300:
            if value.get("version") != 1:
                raise TransportError("Nucleus response has an unsupported protocol version",
                                     code="unsupported_protocol", status=status, uncertain=uncertain)
            return value
        code = value.get("code") if isinstance(value.get("code"), str) else "http_error"
        message = value.get("message")
        message = message[:4096] if isinstance(message, str) else f"Nucleus returned HTTP {status}"
        fields = {"code": code, "status": status, "details": value.get("details")}
        if code in {"quota_deferred", "deployment_maintenance"}:
            raise DeferredError(message, **fields)
        if status >= 500:
            raise TransportError(message, uncertain=uncertain, **fields)
        raise RejectedError(message, **fields)

    def get(self, job_id: str) -> dict[str, Any] | None:
        return self._request("GET", f"/v1/jobs/{quote(job_id, safe='')}", missing_ok=True)

    def submit(self, request: dict[str, Any] | bytes) -> dict[str, Any]:
        body = request if isinstance(request, bytes) else freeze_request(request)
        if len(body) > MAX_REQUEST_BYTES:
            raise RejectedError("Nucleus request exceeds the request limit", code="request_too_large")
        result = self._request("POST", "/v1/jobs", body)
        assert result is not None
        return result

    def cancel(self, job_id: str) -> dict[str, Any]:
        result = self._request("POST", f"/v1/jobs/{quote(job_id, safe='')}/cancel")
        assert result is not None
        return result

    def health(self) -> dict[str, Any]:
        result = self._request("GET", "/v1/health")
        assert result is not None
        return result

    def quota(self) -> dict[str, Any]:
        result = self._request("GET", "/v1/quota")
        assert result is not None
        return result


def terminal_result(view: dict[str, Any]) -> dict[str, Any] | None:
    """Select only the current attempt's completed finalMessage, never log text."""
    summary = view.get("summary")
    attempts = view.get("attempts", [])
    if view.get("version") != 1 or not isinstance(summary, dict) or not isinstance(attempts, list):
        raise TransportError("Invalid Nucleus job view", code="invalid_response")
    state = summary.get("state")
    if state not in JOB_STATES:
        raise TransportError("Unknown Nucleus job state", code="invalid_response")
    if state not in TERMINAL_STATES:
        return None
    current_id = summary.get("currentAttemptId")
    current = [item for item in attempts if isinstance(item, dict) and item.get("id") == current_id]
    if current_id is None and not attempts and state != "completed":
        return {
            "state": state, "attempt_id": None, "attempt_state": None,
            "reason": None, "message": "Job ended without an attempt record",
            "started_at": None, "final_message": None,
        }
    if current_id is None or len(current) != 1:
        raise TransportError("Terminal Nucleus job has no unique current attempt", code="invalid_response")
    attempt = current[0]
    attempt_state = attempt.get("state")
    if attempt_state not in TERMINAL_ATTEMPT_STATES:
        raise TransportError("Terminal Nucleus job has a nonterminal attempt", code="invalid_response")
    if (state == "completed") != (attempt_state == "completed"):
        raise TransportError("Nucleus job and attempt completion disagree", code="invalid_response")
    final_message = None
    output = attempt.get("output")
    if state == "completed" and attempt_state == "completed" and isinstance(output, dict):
        text = output.get("finalMessage")
        if isinstance(text, str):
            final_message = text
    return {
        "state": state,
        "attempt_id": current_id,
        "attempt_state": attempt_state,
        "reason": attempt.get("terminalReason"),
        "message": attempt.get("terminalMessage"),
        "started_at": attempt.get("startedAt"),
        "final_message": final_message,
    }


def _bazaar_get(identifier: str, version: int | None, *, executable: Path,
                database: Path | None) -> dict[str, Any]:
    command = [str(executable)]
    if database is not None:
        command.extend(["--database", str(database)])
    command.extend(["get", identifier])
    if version is not None:
        command.extend(["--version", str(version)])
    try:
        result = subprocess.run(command, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                stderr=subprocess.DEVNULL, timeout=30, check=False,
                                env={**os.environ, "CHANCERY_USAGE_INTERNAL": "1"})
    except (OSError, subprocess.TimeoutExpired) as error:
        raise TransportError("Could not read the selected Bazaar prompt", code="prompt_unavailable") from error
    try:
        envelope = json.loads(result.stdout)
    except (ValueError, UnicodeError) as error:
        raise RejectedError("Bazaar returned an invalid prompt record", code="invalid_prompt") from error
    if (result.returncode != 0 or not isinstance(envelope, dict)
            or envelope.get("schema_version") != 1 or envelope.get("ok") is not True):
        raise RejectedError("The selected Bazaar prompt could not be read", code="prompt_unavailable")
    record = envelope.get("data")
    if (not isinstance(record, dict) or record.get("id") != identifier
            or type(record.get("version")) is not int or record["version"] < 1
            or not isinstance(record.get("content"), str)
            or (version is not None and record["version"] != version)):
        raise RejectedError("Bazaar returned a mismatched prompt record", code="invalid_prompt")
    return record


def load_prompt_selection(*, executable: str | os.PathLike[str] | None = None,
                          database: str | os.PathLike[str] | None = None) -> dict[str, Any]:
    """Resolve one selection and its immutable components without a seed fallback."""
    program = _absolute(Path.home() / ".local/bin/bazaar" if executable is None else executable,
                        "Bazaar executable")
    selected_database = database if database is not None else os.environ.get("CELL_BAZAAR_DATABASE")
    db = _absolute(selected_database, "Bazaar database") if selected_database is not None else None
    selection = _bazaar_get(PROMPT_SELECTION_ID, None, executable=program, database=db)
    try:
        contents = json.loads(selection["content"])
    except ValueError as error:
        raise RejectedError("Bazaar prompt selection is not JSON", code="invalid_prompt") from error
    entries = contents.get("entries") if isinstance(contents, dict) else None
    required = {PROMPT_INSTRUCTIONS_ID, PROMPT_TEMPLATE_ID}
    if (not isinstance(contents, dict) or contents.get("schema_version") != 1
            or not isinstance(entries, dict) or not required.issubset(entries)
            or any(type(entries[key]) is not int or entries[key] < 1 for key in required)):
        raise RejectedError("Bazaar prompt selection is missing required components", code="invalid_prompt")
    instructions = _bazaar_get(PROMPT_INSTRUCTIONS_ID, entries[PROMPT_INSTRUCTIONS_ID],
                               executable=program, database=db)
    template = _bazaar_get(PROMPT_TEMPLATE_ID, entries[PROMPT_TEMPLATE_ID],
                           executable=program, database=db)
    return {
        "selection_id": PROMPT_SELECTION_ID,
        "selection_version": selection["version"],
        "component_versions": {key: entries[key] for key in sorted(required)},
        "instructions": instructions["content"],
        "prompt_template": template["content"],
    }


def make_request(job_id: str, domain_job: str, cwd: str | os.PathLike[str], base: str,
                 candidate: str, diagnostic_path: str | os.PathLike[str], history: Any,
                 model: str, reasoning: str, *, prompts: dict[str, Any] | None = None,
                 timeout_seconds: int = 600) -> dict[str, Any]:
    """Build a detached request snapshot; persist freeze_request() before submit.

    The manager can retain one prompt selection per domain job and pass it for
    every attempt. Diagnostic and history values stay in the user input field.
    """
    if MODEL_TIERS.get(model) != reasoning:
        raise RejectedError("Unsupported CI remediation model tier", code="invalid_model_tier")
    if type(timeout_seconds) is not int or timeout_seconds <= 0:
        raise RejectedError("Model timeout must be a positive integer", code="invalid_timeout")
    root = _absolute(cwd, "Candidate directory")
    diagnostics = _absolute(diagnostic_path, "Diagnostic path")
    selected = load_prompt_selection() if prompts is None else prompts
    try:
        context = json.dumps({
            "domain_job": domain_job,
            "accepted_base": base,
            "candidate_commit": candidate,
            "candidate_directory": str(root),
            "diagnostic_path": str(diagnostics),
            "prior_attempts": history,
            "prompt_selection": {
                "id": selected["selection_id"],
                "version": selected["selection_version"],
                "components": selected["component_versions"],
            },
        }, ensure_ascii=False, sort_keys=True, indent=2, allow_nan=False)
        prompt = selected["prompt_template"].format_map({"context": context})
        instructions = selected["instructions"]
    except (KeyError, TypeError, ValueError, AttributeError) as error:
        raise RejectedError("Cannot render the selected CI prompt", code="invalid_prompt") from error
    if not isinstance(instructions, str) or not instructions.strip() or not prompt.strip():
        raise RejectedError("Selected CI prompt is empty", code="invalid_prompt")
    request = {
        "version": 1,
        "id": job_id,
        "label": f"Repair CI job {domain_job}",
        "requester": {"program": "ci-manager", "id": domain_job},
        "instructions": instructions,
        "prompt": prompt,
        "invocation": {
            "version": 1,
            "harness": "codex",
            "model": model,
            "reasoningEffort": reasoning,
            "cwd": str(root),
            "workspaceAccess": "read-only",
            "builtinTools": {"localExecution": True, "webSearch": False},
            "timeoutSeconds": timeout_seconds,
        },
    }
    return json.loads(freeze_request(request))


def send_email(subject: str, body: str, key: str, *,
               executable: str | os.PathLike[str] | None = None,
               timeout: float = 120.0) -> dict[str, Any]:
    """Send one authorized frozen notification; leave every retry to the manager."""
    program = _absolute(Path.home() / ".local/bin/email" if executable is None else executable,
                        "Email executable")
    if (not isinstance(key, str) or not 1 <= len(key) <= 256
            or any(ord(character) < 33 or ord(character) > 126 for character in key)):
        raise EmailError("Email idempotency key is invalid", code="invalid_email")
    if (not isinstance(subject, str) or not isinstance(body, str) or not subject.strip()
            or any(character in subject for character in "\r\n\x00")):
        raise EmailError("Email subject or body is invalid", code="invalid_email")
    if timeout <= 0:
        raise EmailError("Email timeout must be positive", code="invalid_timeout")
    try:
        payload = body.encode("utf-8")
        process = subprocess.Popen(
            [str(program), "--idempotency-key", key, "--", subject, "-"],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
            start_new_session=True,
            env={**os.environ, "CHANCERY_USAGE_INTERNAL": "1"},
        )
    except (OSError, UnicodeError) as error:
        raise EmailError("Could not start the Email command", code="email_unavailable") from error
    try:
        output, _ = process.communicate(payload, timeout=timeout)
    except subprocess.TimeoutExpired as error:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        process.communicate()
        raise EmailError("Email acceptance is unknown after a timeout", code="email_timeout",
                         uncertain=True) from error
    except OSError as error:
        raise EmailError("Email acceptance is unknown after a process error",
                         code="email_transport", uncertain=True) from error
    if process.returncode != 0:
        raise EmailError(f"Email exited with status {process.returncode}; acceptance is unproved",
                         code="email_command_failed", uncertain=True)
    if len(output) > MAX_RECEIPT_BYTES:
        raise EmailError("Email receipt exceeds the client limit", code="invalid_email_receipt",
                         uncertain=True)
    try:
        receipt = output.decode("utf-8").strip()
    except UnicodeError as error:
        raise EmailError("Email receipt is not UTF-8", code="invalid_email_receipt",
                         uncertain=True) from error
    fields = receipt.split()
    if len(fields) != 2 or fields[0] != "Accepted":
        raise EmailError("Email did not return a recognized acceptance receipt",
                         code="invalid_email_receipt", uncertain=True)
    return {"accepted": True, "message_id": fields[1], "idempotency_key": key}
