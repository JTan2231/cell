"""Literal product inventory shared by deployment and release builds."""
from __future__ import annotations

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
