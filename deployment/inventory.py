"""Literal product inventory shared by deployment and release builds."""
from __future__ import annotations

import re
import shlex
from pathlib import Path, PurePosixPath


def descriptor(text: str) -> dict[str, str]:
    values: dict[str, str] = {}
    for token in shlex.split(text, comments=True):
        name, separator, value = token.partition("=")
        if not separator or not re.fullmatch(r"[A-Z][A-Z0-9_]*", name) or name in values:
            raise ValueError("invalid literal product descriptor")
        values[name] = value
    return values


def product_directory(value: str) -> str:
    """Accept one canonical product path relative to the Cell source root."""
    path = PurePosixPath(value)
    if (not value or path.is_absolute() or ".." in path.parts
            or str(path) != value or value == "."):
        raise ValueError(f"invalid product directory: {value!r}")
    return value


def product_root(source: Path, directory: str) -> Path:
    """Locate a declared source directory without following symbolic components."""
    root = source.resolve() / product_directory(directory)
    if not root.is_dir() or root.resolve() != root:
        raise ValueError(f"product source directory is unavailable: {directory!r}")
    return root
