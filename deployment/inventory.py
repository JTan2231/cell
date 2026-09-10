"""Literal product inventory shared by deployment, release builds, and cleanup."""
from __future__ import annotations

import json
from pathlib import Path
import re
import shlex


def descriptor(text: str) -> dict[str, str]:
    values: dict[str, str] = {}
    for token in shlex.split(text, comments=True):
        name, separator, value = token.partition("=")
        if not separator or not re.fullmatch(r"[A-Z][A-Z0-9_]*", name) or name in values:
            raise ValueError("invalid literal product descriptor")
        values[name] = value
    return values


def applications(source: Path) -> dict[str, str]:
    result = {}
    for path in sorted((source / "pipeline/products").glob("*.sh")):
        values = descriptor(path.read_text())
        identity, directory = values["PRODUCT_ID"], values["PRODUCT_DIR"]
        if any(not re.fullmatch(r"[a-z][a-z0-9-]*", value) for value in (identity, directory)):
            raise ValueError("invalid product identity in inventory")
        metadata = json.loads((source / directory / "deployment/adapter.json").read_text())
        if metadata.get("schema") != 1 or metadata.get("product") != ("krisis" if identity == "decisions" else identity):
            raise ValueError("deployment metadata does not match product inventory")
        application = metadata["application"]
        if not re.fullmatch(r"[A-Za-z][A-Za-z0-9]*", application) or identity in result or application in result.values():
            raise ValueError("invalid or repeated installation application")
        result[identity] = application
    if not result:
        raise ValueError("product inventory is empty")
    return result
