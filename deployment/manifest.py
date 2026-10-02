"""The small ordered instruction format used by the deployment executor."""

from __future__ import annotations

from pathlib import Path
from typing import Any


class ManifestError(ValueError):
    """An instruction cannot be executed as declared."""


def declaration(value: Any, product: str) -> dict[str, Any]:
    if not isinstance(value, dict) or set(value) != {"schema", "product", "order", "steps"}:
        raise ManifestError("deployment declaration requires schema, product, order and steps")
    if value["schema"] != 1 or value["product"] != product or type(value["order"]) is not int:
        raise ManifestError("deployment declaration identity or order is invalid")
    if not isinstance(value["steps"], list) or not value["steps"]:
        raise ManifestError("deployment declaration requires an ordered instruction list")
    ids = set()
    for step in value["steps"]:
        instruction(step)
        if step["id"] in ids:
            raise ManifestError("deployment instruction IDs must be unique within a product")
        ids.add(step["id"])
    return value


def instruction(step: Any) -> None:
    if not isinstance(step, dict) or not isinstance(step.get("id"), str) or not step["id"]:
        raise ManifestError("instruction requires a nonempty ID")
    kind = step.get("kind")
    if kind == "run":
        allowed = {"id", "kind", "argv", "cwd", "env", "stdin", "timeout_seconds"}
        if not isinstance(step.get("argv"), list) or not step["argv"] or any(
                not isinstance(value, str) or "\0" in value for value in step["argv"]):
            raise ManifestError("run instruction requires a literal argument list")
        if "cwd" in step and not isinstance(step["cwd"], str):
            raise ManifestError("run working directory must be text")
        env = step.get("env", {})
        if not isinstance(env, dict) or any(not isinstance(key, str) or not key or "=" in key
                or "\0" in key or not isinstance(value, str) or "\0" in value for key, value in env.items()):
            raise ManifestError("run environment must map names to text")
        if step.get("stdin") is not None and not isinstance(step["stdin"], str):
            raise ManifestError("run input must be literal text or deployment_request")
        if "timeout_seconds" in step and (type(step["timeout_seconds"]) not in (int, float)
                or not 0 < step["timeout_seconds"] < float("inf")):
            raise ManifestError("run timeout must be a positive number of seconds")
    elif kind in ("copy", "link"):
        allowed = {"id", "kind", "source", "destination", "mode"} if kind == "copy" else {
            "id", "kind", "target", "destination"}
        for key in ("source" if kind == "copy" else "target", "destination"):
            if not isinstance(step.get(key), str) or not step[key] or "\0" in step[key]:
                raise ManifestError("file instruction requires source/target and destination")
        if "mode" in step and (type(step["mode"]) is not int or not 0 <= step["mode"] <= 0o777):
            raise ManifestError("copy mode must be an integer from 0 to 0777")
    else:
        raise ManifestError("unsupported instruction kind")
    if set(step) - allowed:
        raise ManifestError("unknown instruction field")


def expand(value: str, context: dict[str, str]) -> str:
    try:
        return value.format_map(context)
    except (KeyError, ValueError) as error:
        raise ManifestError("instruction refers to an unknown path variable") from error


def absolute(value: str, context: dict[str, str]) -> Path:
    path = Path(expand(value, context))
    if not path.is_absolute():
        raise ManifestError("instruction filesystem paths must be absolute")
    return path
