#!/usr/bin/env python3
"""Prepare signed production candidates through the installed manager package."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import subprocess
import sys

sys.dont_write_bytecode = True
if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from deployment import build, candidate, signing
from ci_manager import git_ops


def prepare(source: Path, products: list[str], output: Path, policy: dict | None) -> dict:
    commit = source_commit(source)
    git_ops.clean_candidate(source, commit)
    signing.assert_current(policy)
    signing.preflight(policy)
    # A surviving successful preparation is reusable only after independently
    # checking its signatures and source identity. No candidate code is imported.
    if output.exists():
        result = json.loads((output / "result.json").read_text())
    else:
        result = build.prepare(source, products, output, signing_policy=policy)
    if not isinstance(result, dict) or result.get("schema") != 1 or result.get("state") != "built":
        raise candidate.CandidateError("production preparation has an incompatible receipt")
    if result.get("source_key") != commit:
        raise candidate.CandidateError("production preparation has another source identity")
    canonical_products = sorted({"krisis" if product == "decisions" else product for product in products})
    if (not isinstance(result.get("candidates"), dict)
            or sorted(result["candidates"]) != canonical_products):
        raise candidate.CandidateError("production preparation has another product selection")
    for product, record in result["candidates"].items():
        if (not isinstance(record, dict) or not isinstance(record.get("candidate_dir"), str)
                or not isinstance(record.get("candidate_id"), str)):
            raise candidate.CandidateError("production preparation has an invalid candidate record")
        path = Path(record["candidate_dir"])
        if path != output / "candidates" / product:
            raise candidate.CandidateError("production candidate is outside its preparation")
        manifest = candidate.verify(path, signing_policy=policy)
        if (manifest.get("source_commit") != commit
                or manifest.get("source_key") != result["source_key"]
                or manifest.get("candidate_id") != record.get("candidate_id")
                or manifest.get("product") != product):
            raise candidate.CandidateError("production candidate does not match its preparation")
    git_ops.clean_candidate(source, commit)
    signing.assert_current(policy)
    return {"schema_version": 1, "state": "passed", "source_commit": commit,
            "signing_policy_digest": signing.policy_digest(policy), "products": canonical_products,
            "preparation": result}


def source_commit(source: Path) -> str:
    return git_ops.commit(source, "HEAD")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-root", required=True, type=Path)
    parser.add_argument("--product", action="append", required=True)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--signing-policy-file", required=True, type=Path)
    args = parser.parse_args(argv)
    try:
        policy = json.loads(args.signing_policy_file.read_text())
        result = prepare(args.source_root, args.product, args.output, policy)
        code = 0
    except signing.SigningError as error:
        result = {"schema_version": 1, "state": "error", "failure_kind": "signing_configuration",
                  "message": str(error)}
        code = 78
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        result = {"schema_version": 1, "state": "error", "failure_kind": "production_preparation",
                  "message": str(error)}
        code = 1
    print(json.dumps(result, sort_keys=True), flush=True)
    return code


if __name__ == "__main__":
    raise SystemExit(main())
