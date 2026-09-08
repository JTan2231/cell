#!/usr/bin/env python3
"""Create Mentor's bundled corpus from an explicitly supplied authored source root."""

import argparse
import hashlib
import json
from pathlib import Path
import re


VERSION = re.compile(r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?")


def read_markdown(path):
    return path.read_bytes().decode("utf-8").replace("\r\n", "\n").replace("\r", "\n")


def export(source_root):
    package = json.loads((source_root / "package.json").read_text(encoding="utf-8"))
    source_version = package["version"]
    if not isinstance(source_version, str) or not VERSION.fullmatch(source_version):
        raise ValueError("source package version must be a semantic version")

    paths = sorted(
        path
        for path in (source_root / "problems").iterdir()
        if path.is_file() and path.suffix == ".md" and path.name != "README.md"
    )
    if not 1 <= len(paths) <= 1000:
        raise ValueError("the source must contain between 1 and 1000 authored problems")
    if len({path.name.lower() for path in paths}) != len(paths):
        raise ValueError("problem filenames must be unique without case distinctions")

    problems = []
    for path in paths:
        markdown = read_markdown(path)
        titles = re.findall(r"^#\s+(.+?)\s*$", markdown, flags=re.MULTILINE)
        if len(titles) != 1 or not titles[0].strip():
            raise ValueError(f"{path.name} must have exactly one nonempty level-one title")
        if not re.sub(r"^#\s+.+?\s*$", "", markdown, count=1, flags=re.MULTILINE).strip():
            raise ValueError(f"{path.name} must have a problem body")
        problems.append({"id": path.stem, "title": titles[0].strip(), "markdown": markdown})

    rubric_markdown = read_markdown(source_root / "rubric" / "system-design.md")
    if not re.search(
        r"^#\s+Mentor system-design evaluation contract\s*$", rubric_markdown, flags=re.MULTILINE
    ):
        raise ValueError("rubric must have its canonical level-one title")
    rubric_versions = re.findall(
        r"^-\s+\*\*Contract version:\*\*\s+(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)\s*$",
        rubric_markdown,
        flags=re.MULTILINE,
    )
    if len(rubric_versions) != 1:
        raise ValueError("rubric must contain exactly one semantic contract version")
    dimensions = re.findall(
        r"^##\s+Dimension\s+(\d+):\s+(.+?)\s*$", rubric_markdown, flags=re.MULTILINE
    )
    if len(dimensions) != 8 or [int(number) for number, _ in dimensions] != list(range(1, 9)):
        raise ValueError("rubric must contain eight dimensions numbered in order")
    if len({name.strip() for _, name in dimensions}) != 8:
        raise ValueError("rubric dimension names must be unique")

    return {
        "schema_version": 1,
        "source_product": "mentor",
        "source_version": source_version,
        "rubric": {"version": rubric_versions[0], "markdown": rubric_markdown},
        "problems": problems,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-root", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    corpus = export(args.source_root)
    canonical = json.dumps(corpus, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(corpus, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"Wrote {len(corpus['problems'])} problems to {args.output}")
    print(f"Canonical corpus SHA-256: {hashlib.sha256(canonical).hexdigest()}")


if __name__ == "__main__":
    main()
