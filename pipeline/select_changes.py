#!/usr/bin/env python3
"""Select root CI gates from one stable Git checkout."""

from __future__ import annotations

import argparse
import hashlib
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys


class SelectionError(Exception):
    pass


class StaleSelection(SelectionError):
    pass


def output(command: list[str]) -> bytes:
    try:
        result = subprocess.run(command, capture_output=True, check=False, timeout=120)
    except (OSError, subprocess.SubprocessError) as error:
        raise SelectionError(str(error)) from error
    if result.returncode:
        message = result.stderr.decode("utf-8", "replace").strip()
        raise SelectionError(message or f"command failed: {command[0]}")
    return result.stdout


def git_status(root: Path) -> bytes:
    return output([
        "git", "-C", str(root), "status", "--porcelain=v1", "-z",
        "--untracked-files=all", "--ignore-submodules=none",
    ])


def status_key(status: bytes) -> str:
    return "sha256:" + hashlib.sha256(status).hexdigest()


def source_key(root: Path) -> str:
    value = output([
        sys.executable, str(root / "ci_broker/client.py"), "source-key",
        "--repo-root", str(root),
    ]).decode("ascii").strip()
    if not re.fullmatch(r"sha256:[0-9a-f]{64}", value):
        raise SelectionError("invalid source key from CI broker client")
    return value


def inventory(root: Path) -> dict[str, tuple[str, list[str]]]:
    # Use the existing trusted descriptor loader, without a second parser.
    raw = output(["sh", "-eu", "-c", '''
PIPELINE_ROOT=$1
export PIPELINE_ROOT
. "$PIPELINE_ROOT/pipeline/lib.sh"
for product in $(pipeline_products); do
    pipeline_load_descriptor "$product"
    printf '%s\\0%s\\0%s\\0' "$PRODUCT_ID" "$PRODUCT_DIR" "$PRODUCT_ALIASES"
done
''', "ci-selection", str(root)])
    fields = raw.split(b"\0")
    if fields[-1] != b"" or (len(fields) - 1) % 3:
        raise SelectionError("invalid product inventory output")
    products = {}
    claims = {}
    directories = []
    for index in range(0, len(fields) - 1, 3):
        product, directory, aliases = [
            os.fsdecode(value) for value in fields[index:index + 3]
        ]
        names = set([product, *aliases.split()])
        if any(not re.fullmatch(r"[a-z][a-z0-9-]*", name) for name in names):
            raise SelectionError("invalid product ID or alias")
        if product in products or any(name in claims for name in names):
            raise SelectionError(f"ambiguous product identity: {product}")
        path = PurePosixPath(directory)
        if (not directory or path.is_absolute() or ".." in path.parts
                or str(path) != directory or directory == "."):
            raise SelectionError(f"invalid product root: {directory!r}")
        owned_root = root / directory
        if (not owned_root.is_dir() or owned_root.resolve() != owned_root
                or not (owned_root / "ci.sh").is_file()):
            raise SelectionError(f"product root or CI entry point is unavailable: {directory!r}")
        if any(path == other or path in other.parents or other in path.parents
               for other in directories):
            raise SelectionError(f"ambiguous product root: {directory!r}")
        directories.append(path)
        products[product] = (directory, sorted(names))
        claims.update(dict.fromkeys(names, product))
    if not products:
        raise SelectionError("product inventory is empty")
    return products


def changed_paths(status: bytes) -> set[str]:
    if not status:
        return set()
    records = status.split(b"\0")
    if records.pop() != b"":
        raise SelectionError("incomplete Git status output")
    paths = set()
    index = 0
    while index < len(records):
        record = records[index]
        index += 1
        if len(record) < 4 or record[2:3] != b" ":
            raise SelectionError("invalid Git status record")
        state = record[:2]
        if b"U" in state or state in (b"AA", b"DD"):
            raise SelectionError("resolve Git conflicts before running root CI")
        if state != b"??" and any(value not in b" MTADRC" for value in state):
            raise SelectionError("unsupported Git status record")
        paths.add(os.fsdecode(record[3:]))
        if b"R" in state or b"C" in state:
            if index == len(records) or not records[index]:
                raise SelectionError("incomplete Git rename record")
            paths.add(os.fsdecode(records[index]))
            index += 1
    return paths


def check(root: Path, expected_source: str, expected_status: str) -> None:
    before = git_status(root)
    observed_source = source_key(root)
    after = git_status(root)
    if (before != after or status_key(after) != expected_status
            or observed_source != expected_source):
        raise StaleSelection("source or Git status changed during root CI; results are stale")


def describe(paths: list[str], verbose: bool) -> str:
    shown = paths if verbose else paths[:3]
    text = ", ".join(repr(path) for path in shown)
    if len(shown) < len(paths):
        text += f", and {len(paths) - len(shown)} more paths"
    return text


def plan(root: Path, arguments: list[str]) -> None:
    parser = argparse.ArgumentParser(
        prog="./ci.sh", description="Run changed product gates, explicit products, or full CI.",
    )
    parser.add_argument("--all", action="store_true", help="run all products and integrated checks")
    parser.add_argument("--verbose", action="store_true", help="stream gate output")
    parser.add_argument("products", nargs="*", metavar="PRODUCT")
    args = parser.parse_intermixed_args(arguments)
    if args.all and args.products:
        parser.error("--all cannot be combined with product arguments")

    before = git_status(root)
    expected_source = source_key(root)
    products = inventory(root)
    changes = changed_paths(before)
    reasons: dict[str, list[str]] = {product: [] for product in products}
    shared = []
    for path in sorted(changes):
        owners = [product for product, (directory, _) in products.items()
                  if path == directory or path.startswith(directory + "/")
                  or path == f"pipeline/products/{product}.sh"]
        if owners:
            for owner in owners:
                reasons[owner].append(path)
        else:
            if path.startswith("pipeline/products/") and path.endswith(".sh"):
                raise SelectionError(f"changed descriptor has no product owner: {path!r}")
            shared.append(path)

    mode = "all" if args.all else "explicit" if args.products else "changed"
    if mode == "explicit":
        aliases = {alias: product for product, (_, names) in products.items() for alias in names}
        selected = []
        for requested in args.products:
            if requested not in aliases:
                parser.error(f"unknown product: {requested}")
            product = aliases[requested]
            if product not in selected:
                selected.append(product)
    else:
        selected = [product for product in products if args.all or reasons[product]]

    # The broker source key omits index state. Check exact status as well so
    # staging-only status changes cannot invalidate selection without detection.
    expected_status = status_key(before)
    check(root, expected_source, expected_status)
    print(f"ci: mode={mode}; selected={','.join(selected) or 'none'}; "
          f"skipped={len(products) - len(selected)}", file=sys.stderr)
    if mode == "changed":
        for product in selected:
            print(f"ci: {product}: {describe(reasons[product], args.verbose)}", file=sys.stderr)
    if shared:
        print(f"ci: shared or unowned changes ({len(shared)} paths): "
              f"{describe(shared, args.verbose)}; use --all for full validation", file=sys.stderr)
    # Every token is a fixed mode, hash, count, or validated product ID.
    # The shell caller can split this output without evaluating shell code.
    print(mode, "verbose" if args.verbose else "quiet", expected_source,
          expected_status, len(products), *selected)


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    try:
        if len(sys.argv) >= 2 and sys.argv[1] == "plan":
            plan(root, sys.argv[2:])
        elif len(sys.argv) == 4 and sys.argv[1] == "check":
            check(root, sys.argv[2], sys.argv[3])
        else:
            raise SelectionError("usage: select_changes.py plan [OPTIONS] [PRODUCT...] | check SOURCE STATUS")
    except StaleSelection as error:
        print(f"ci.sh: {error}", file=sys.stderr)
        return 75
    except (SelectionError, UnicodeError) as error:
        print(f"ci.sh: {error}", file=sys.stderr)
        return 78
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
