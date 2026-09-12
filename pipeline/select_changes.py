#!/usr/bin/env python3
"""Select root CI gates from one stable Git checkout."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from fnmatch import fnmatchcase
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys

from platform_inputs import (
    INSTALL_FIXTURE_CONSUMERS, MAINTENANCE_CONSUMERS, PRODUCT_INPUTS,
    PRODUCT_RUNTIME_INPUTS, SHARED_INPUTS,
)


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


@dataclass
class Plan:
    mode: str
    verbose: bool
    quiet: bool
    source: str
    status: str
    head: str
    products: dict[str, tuple[str, list[str]]]
    selected: list[str]
    platform: dict[str, list[str]]
    shared: dict[str, list[str]]
    stage_candidate: str | None


def at_head(root: Path, path: str) -> bytes | None:
    result = subprocess.run(
        ["git", "-C", str(root), "show", f"HEAD:{path}"], capture_output=True, check=False,
    )
    return result.stdout if result.returncode == 0 else None


def platform_change(root: Path, path: str) -> bool:
    """Ignore prose and release-version-only edits, not operational metadata."""
    if Path(path).suffix.lower() in (".md", ".txt"):
        return False
    previous = at_head(root, path)
    current_path = root / path
    if previous is None or not current_path.is_file() or current_path.is_symlink():
        return True
    current = current_path.read_bytes()
    if path == "Cargo.toml" or path.endswith("/Cargo.toml"):
        def without_release_version(value: bytes) -> list[bytes]:
            section = b""
            lines = []
            for line in value.splitlines():
                line = line.strip()
                if not line or line.startswith(b"#"):
                    continue
                if line.startswith(b"["):
                    section = line
                if section in (b"[package]", b"[workspace.package]") and re.match(rb"version\s*=", line):
                    continue
                lines.append(line)
            return lines
        return without_release_version(previous) != without_release_version(current)
    if path.endswith("/provider.json"):
        try:
            before, after = json.loads(previous), json.loads(current)
            before.pop("release", None)
            after.pop("release", None)
            return before != after
        except (ValueError, AttributeError):
            return True
    if path.startswith("pipeline/products/"):
        def assignments(value: bytes) -> list[bytes]:
            return [line.strip() for line in value.splitlines()
                    if line.strip() and not line.lstrip().startswith(b"#")]
        return assignments(previous) != assignments(current)
    return True


def make_plan(root: Path, arguments: list[str], direct: str | None = None) -> Plan:
    parser = argparse.ArgumentParser(
        prog=f"{direct}/ci.sh" if direct else "./ci.sh",
        description="Run product tests; add platform tests for platform inputs or on request.",
    )
    parser.add_argument("--all", action="store_true", help="run every product and platform suite")
    parser.add_argument("--platform", action="store_true",
                        help="add platform tests for named products; without names, run everything")
    parser.add_argument("--verbose", action="store_true", help="stream gate output")
    parser.add_argument("--quiet-result", action="store_true", help=argparse.SUPPRESS)
    if direct:
        parser.add_argument("--stage-candidate", metavar="ABSOLUTE_DIRECTORY")
    parser.add_argument("products", nargs="*", metavar="PRODUCT")
    args = parser.parse_intermixed_args(arguments)
    if direct:
        if args.products or args.all:
            parser.error("use --platform to request this product's full gate")
        args.products = [direct]
    stage_candidate = getattr(args, "stage_candidate", None)
    if stage_candidate and not Path(stage_candidate).is_absolute():
        parser.error("candidate staging directory must be absolute")
    if args.all and args.products:
        parser.error("--all cannot be combined with product arguments")

    head = output(["git", "-C", str(root), "rev-parse", "HEAD"]).decode().strip()
    before = git_status(root)
    expected_source = source_key(root)
    products = inventory(root)
    changes = changed_paths(before)
    reasons: dict[str, list[str]] = {product: [] for product in products}
    shared = []
    retired_descriptors = []
    for path in sorted(changes):
        owners = [product for product, (directory, _) in products.items()
                  if path == directory or path.startswith(directory + "/")
                  or path == f"pipeline/products/{product}.sh"]
        if owners:
            for owner in owners:
                reasons[owner].append(path)
        else:
            if path.startswith("pipeline/products/") and path.endswith(".sh"):
                current = root / path
                if current.exists() or current.is_symlink() or at_head(root, path) is None:
                    raise SelectionError(f"changed descriptor has no product owner: {path!r}")
                retired_descriptors.append(path)
            shared.append(path)

    everything = args.all or (args.platform and not args.products)
    mode = "all" if everything else "explicit" if args.products else "changed"
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
        selected = [product for product in products if everything or reasons[product]]

    platform: dict[str, list[str]] = {product: [] for product in products}
    suites: dict[str, list[str]] = {suite: [] for suite in SHARED_INPUTS}
    suites["catalog"].extend(retired_descriptors)
    for path in sorted(changes):
        if not platform_change(root, path):
            continue
        for suite, patterns in SHARED_INPUTS.items():
            if any(fnmatchcase(path, pattern) for pattern in patterns):
                suites[suite].append(path)
        for product, (directory, _) in products.items():
            owned = path.startswith(directory + "/")
            descriptor = path == f"pipeline/products/{product}.sh"
            extra = path == f"pipeline/extras/{product}-catalog.sh"
            patterns = (*PRODUCT_INPUTS, *PRODUCT_RUNTIME_INPUTS.get(product, ()))
            catalog = ("/chancery/" in path or "/chancery-" in path
                       or path.startswith("chancery/provider/")) and path.endswith(".json")
            if descriptor or extra or (owned and (catalog or any(fnmatchcase(path, p) for p in patterns))):
                platform[product].append(path)

    for suite in ("install", "maintenance"):
        if suites[suite]:
            fixture_only = suite == "install" and all(
                path == "deployment/tests/simple_fixture.rs" for path in suites[suite]
            )
            for product in products:
                affected = (product in INSTALL_FIXTURE_CONSUMERS if fixture_only
                            else suite == "install" or product in MAINTENANCE_CONSUMERS)
                if affected:
                    platform[product].append(f"shared {suite}: {describe(suites[suite], args.verbose)}")

    for product in products:
        if at_head(root, f"pipeline/products/{product}.sh") is None:
            platform[product].append("new product descriptor relative to HEAD")
            suites["pipeline"].append(f"new product: {product}")
            suites["catalog"].append(f"new product: {product}")

    # Pipeline/runtime changes affect the checks themselves, not the meaning of
    # every product installer. Do not turn these into blanket installer runs.
    if mode == "changed":
        selected = [product for product in products if reasons[product] or platform[product]]
    if everything:
        for suite in suites:
            suites[suite].append("explicit full platform request")
    for product in selected:
        if everything or args.platform or stage_candidate:
            platform[product].append("candidate staging" if stage_candidate else "explicit platform request")
        if platform[product]:
            suites["install"].append(f"platform coverage for {product}")
            if product in MAINTENANCE_CONSUMERS:
                suites["maintenance"].append(f"platform coverage for {product}")

    # The broker source key omits index state. Check exact status as well so
    # staging-only status changes cannot invalidate selection without detection.
    expected_status = status_key(before)
    check(root, expected_source, expected_status)
    if output(["git", "-C", str(root), "rev-parse", "HEAD"]).decode().strip() != head:
        raise StaleSelection("HEAD changed during CI selection")
    print(f"ci: baseline=HEAD {head[:12]}; staged, unstaged, and nonignored untracked changes; "
          "committed branch changes excluded", file=sys.stderr)
    print(f"ci: mode={mode}; selected={','.join(selected) or 'none'}; "
          f"skipped={len(products) - len(selected)}", file=sys.stderr)
    if mode == "changed":
        for product in selected:
            print(f"ci: product {product}: {describe(reasons[product], args.verbose) or 'platform input changed'}", file=sys.stderr)
    for product in selected:
        why = platform[product]
        print(f"ci: platform {product}: " +
              (f"run; {describe(why, args.verbose)}" if why else "skip; no platform input changed"),
              file=sys.stderr)
    for suite, why in suites.items():
        print(f"ci: platform shared/{suite}: " +
              (f"run; {describe(why, args.verbose)}" if why else "skip; no platform input changed"),
              file=sys.stderr)
    if shared:
        print(f"ci: shared or unowned changes ({len(shared)} paths): "
              f"{describe(shared, args.verbose)}; platform inputs mapped explicitly; "
              "ordinary dependency coverage is not expanded", file=sys.stderr)
    outside = [product for product in products if platform[product] and product not in selected]
    if outside:
        print(f"ci: affected platform products outside requested scope: {','.join(outside)}", file=sys.stderr)
    return Plan(mode, args.verbose, args.quiet_result, expected_source, expected_status,
                head, products, selected, platform, suites, stage_candidate)


def plan(root: Path, arguments: list[str]) -> None:
    selection = make_plan(root, arguments)
    # Every token is a fixed mode, hash, count, or validated product ID.
    # The shell caller can split this output without evaluating shell code.
    print(selection.mode, "verbose" if selection.verbose else "quiet", selection.source,
          selection.status, len(selection.products), *selection.selected)


def run_command(command: list[str], environment: dict[str, str]) -> None:
    result = subprocess.run(command, env=environment, check=False)
    if result.returncode:
        raise SystemExit(result.returncode if result.returncode > 0 else 128 - result.returncode)


def broker(root: Path, gate: str, lane: str, body: list[str], *, verbose: bool,
           environment: dict[str, str], receipt: bool = False) -> None:
    command = [sys.executable, str(root / "ci_broker/client.py"), "run", "--quiet-result",
               "--env", "CHANCERY_USAGE_DISABLED=1"]
    if verbose:
        command.append("--verbose")
    if receipt:
        command.append("--verbose-receipt")
    run_command([*command, "--repo-root", str(root), "--gate", gate, "--lane", lane,
                 "--", *body], environment)


def shared_gate(root: Path, suite: str, verbose: bool, environment: dict[str, str]) -> None:
    lane = "heavy" if suite in ("install", "maintenance", "catalog") else "light"
    broker(root, f"cell.platform.{suite}", lane,
           ["sh", str(root / "pipeline/platform.sh"), suite],
           verbose=verbose, environment=environment)


def run(root: Path, arguments: list[str], direct: str | None = None) -> None:
    selection = make_plan(root, arguments, direct)
    environment = {**os.environ, "CELL_CI_EXPECTED_SOURCE_KEY": selection.source,
                   "PYTHONDONTWRITEBYTECODE": "1"}
    if not direct:
        broker(root, "cell.structure", "light", [str(root / "pipeline/check.sh")],
               verbose=selection.verbose, environment=environment)
        broker(root, "cell.recognition", "heavy", [str(root / "pipeline/recognition.sh")],
               verbose=selection.verbose, environment=environment)
    # Each suite and product obtains admission separately. There is no outer
    # lease, and selection is part of each product's brokered command identity.
    for suite, why in selection.shared.items():
        if why and suite != "catalog":
            shared_gate(root, suite, selection.verbose, environment)
    for product in selection.selected:
        gate, lane = output(["sh", "-eu", "-c", '''
PIPELINE_ROOT=$1
export PIPELINE_ROOT
. "$PIPELINE_ROOT/pipeline/lib.sh"
pipeline_load_descriptor "$2"
printf '%s\\n%s\\n' "$CI_GATE_ID" "$CI_RESOURCE_CLASS"
''', "ci-product", str(root), product]).decode().splitlines()
        group = "all" if selection.platform[product] else "product"
        body = [str(root / "pipeline/ci.sh"), product, "--tests", group]
        if selection.stage_candidate:
            body.extend(["--stage-candidate", selection.stage_candidate])
        broker(root, gate, lane, body, verbose=selection.verbose, environment=environment,
               receipt=bool(selection.stage_candidate))
    if selection.shared["catalog"]:
        shared_gate(root, "catalog", selection.verbose, environment)
    check(root, selection.source, selection.status)
    if output(["git", "-C", str(root), "rev-parse", "HEAD"]).decode().strip() != selection.head:
        raise StaleSelection("HEAD changed during CI; results are stale")
    if not selection.quiet and not selection.stage_candidate:
        platform = [product for product in selection.selected if selection.platform[product]]
        shared = [suite for suite, why in selection.shared.items() if why]
        print(f"ci: passed; mode={selection.mode}; product-tests={','.join(selection.selected) or 'none'}; "
              f"platform-products={','.join(platform) or 'none'}; "
              f"platform-shared={','.join(shared) or 'none'}; "
              f"products-skipped={len(selection.products) - len(selection.selected)}; "
              f"platform-products-skipped={len(selection.products) - len(platform)}")


def run_shared(root: Path, arguments: list[str]) -> None:
    parser = argparse.ArgumentParser(prog="pipeline/test.sh", description="Explicit shared platform tests")
    parser.add_argument("--verbose", action="store_true")
    parser.add_argument("--quiet-result", action="store_true")
    parser.add_argument("suites", nargs="*", metavar="SUITE", help=", ".join(SHARED_INPUTS))
    args = parser.parse_intermixed_args(arguments)
    if any(suite not in SHARED_INPUTS for suite in args.suites):
        parser.error("unknown shared platform suite")
    suites = list(dict.fromkeys(args.suites or SHARED_INPUTS))
    before = git_status(root)
    expected_source = source_key(root)
    expected_status = status_key(before)
    environment = {**os.environ, "CELL_CI_EXPECTED_SOURCE_KEY": expected_source,
                   "PYTHONDONTWRITEBYTECODE": "1"}
    check(root, expected_source, expected_status)
    for suite in suites:
        shared_gate(root, suite, args.verbose, environment)
    check(root, expected_source, expected_status)
    if not args.quiet_result:
        print(f"ci: passed; platform-shared={','.join(suites)}; explicit request")


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    try:
        if len(sys.argv) >= 2 and sys.argv[1] == "plan":
            plan(root, sys.argv[2:])
        elif len(sys.argv) >= 2 and sys.argv[1] == "run":
            run(root, sys.argv[2:])
        elif len(sys.argv) >= 3 and sys.argv[1] == "product":
            run(root, sys.argv[3:], sys.argv[2])
        elif len(sys.argv) >= 2 and sys.argv[1] == "shared":
            run_shared(root, sys.argv[2:])
        elif len(sys.argv) == 4 and sys.argv[1] == "check":
            check(root, sys.argv[2], sys.argv[3])
        else:
            raise SelectionError("usage: select_changes.py plan [OPTIONS] [PRODUCT...] | check SOURCE STATUS")
    except StaleSelection as error:
        print(f"ci.sh: {error}", file=sys.stderr)
        return 75
    except (SelectionError, UnicodeError, OSError) as error:
        print(f"ci.sh: {error}", file=sys.stderr)
        return 78
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
