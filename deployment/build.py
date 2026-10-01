#!/usr/bin/env python3
"""Build selected release binaries once and prepare immutable product bundles."""

from __future__ import annotations

import argparse
from concurrent.futures import ThreadPoolExecutor
import fcntl
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time
from typing import Any

sys.dont_write_bytecode = True
if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from ci_broker.client import bootstrap_cargo_path
from ci_broker.broker import MINIMAL_ENVIRONMENT
from deployment import candidate, signing
from deployment.inventory import descriptor
from ci_manager import workspace

NAME = re.compile(r"[a-z][a-z0-9-]*")
CONFIG_ENV = ("AR", "CC", "CXX", "CFLAGS", "CXXFLAGS", "LDFLAGS", "SDKROOT",
              "DEVELOPER_DIR", "MACOSX_DEPLOYMENT_TARGET", "PATH", "PKG_CONFIG_PATH",
              "PKG_CONFIG", "OPENSSL_DIR", "OPENSSL_LIB_DIR", "OPENSSL_INCLUDE_DIR")


class BuildError(RuntimeError):
    """Selected release materials could not be built."""


def read_descriptor(source: Path, product: str) -> dict[str, str]:
    product = "decisions" if product == "krisis" else product
    if not NAME.fullmatch(product):
        raise BuildError("invalid product identity")
    path = source / "pipeline" / "products" / f"{product}.sh"
    candidate.regular(path)
    values = descriptor(path.read_text())
    if values.get("PRODUCT_ID") != product:
        raise BuildError(f"product descriptor identity mismatch: {product}")
    return values


def selection(source: Path, products: list[str], unit: str | None,
              metadata: dict[str, Any]) -> dict[str, dict[str, Any]]:
    selected: dict[str, dict[str, Any]] = {}
    if unit is not None and len(products) != 1:
        raise BuildError("--unit requires exactly one selected product")
    for requested in sorted(set(products)):
        product = "krisis" if requested == "decisions" else requested
        if product in selected:
            continue
        values = read_descriptor(source, product)
        units = {row.split("|")[0] for row in values.get("RELEASE_UNITS", "").splitlines()}
        if unit is not None and unit not in units:
            raise BuildError(f"unknown release unit for {product}: {unit}")
        packages = set(values.get("CARGO_PACKAGES", "").split())
        binaries: dict[str, dict[str, str]] = {}
        rows: list[str] = []
        for row in values.get("RELEASE_BINARY_CHECKS", "").splitlines():
            fields = row.split("|")
            if len(fields) != 3:
                raise BuildError(f"invalid binary declaration: {product}")
            release_unit, relative, name = fields
            if unit is not None and release_unit != unit:
                continue
            if not NAME.fullmatch(name) or relative != f"target/release/{name}" or name in binaries:
                raise BuildError(f"invalid or repeated release executable: {name}")
            matches = [package for package in metadata["packages"]
                       if package["name"] in packages and any(
                           target["name"] == name and "bin" in target["kind"]
                           for target in package["targets"])]
            if len(matches) != 1:
                raise BuildError(f"release executable must select exactly one package: {name}")
            package = matches[0]
            binaries[name] = {"package": package["name"], "version": f"{name} {package['version']}"}
            rows.append(row)
        if not binaries:
            raise BuildError(f"product declares no selected release executables: {product}")
        selected[product] = {"binary_spec": "\n".join(rows), "binaries": binaries,
                             "directory": values["PRODUCT_DIR"]}
    all_names = [name for item in selected.values() for name in item["binaries"]]
    if len(all_names) != len(set(all_names)):
        raise BuildError("selected products declare colliding executable names")
    return selected


def tool_output(source: Path, environment: dict[str, str], *command: str) -> str:
    result = subprocess.run(workspace.confined_command(list(command)), cwd=source, env=environment, check=True,
                            capture_output=True, text=True, timeout=60)
    return result.stdout.strip()


def build_configuration(source: Path, cache: Path) -> tuple[dict[str, str], dict[str, Any]]:
    bootstrap_cargo_path()
    environment = {name: value for name, value in os.environ.items()
                   if name in (*MINIMAL_ENVIRONMENT, *CONFIG_ENV)
                   or name.startswith(("CARGO_", "RUST", "CC_", "CXX_", "AR_", "CFLAGS_", "CXXFLAGS_"))}
    try:
        jobs = int(os.environ.get("CELL_RELEASE_BUILD_JOBS", str(min(os.cpu_count() or 1, 8))))
    except ValueError as error:
        raise BuildError("CELL_RELEASE_BUILD_JOBS must be a positive integer") from error
    if jobs < 1:
        raise BuildError("CELL_RELEASE_BUILD_JOBS must be a positive integer")
    target = cache / "target"
    environment.update(workspace.environment())
    environment.update(CARGO_TARGET_DIR=str(target), CARGO_BUILD_BUILD_DIR=str(target),
                       CARGO_BUILD_JOBS=str(jobs), CARGO_INCREMENTAL="0")
    rustc_version = tool_output(source, environment, "rustc", "--version", "--verbose")
    host = next((line.removeprefix("host: ") for line in rustc_version.splitlines()
                 if line.startswith("host: ")), None)
    if host is None:
        raise BuildError("rustc did not report its host target")
    return environment, {"target": host, "jobs": jobs}


def prepare(source: Path, products: list[str], output: Path, unit: str | None = None, *,
            signing_policy: dict[str, Any] | None = None) -> dict[str, Any]:
    source = source.resolve()
    if not output.is_absolute() or output.is_symlink() or output == source or source in output.parents:
        raise BuildError("build output must be an absolute non-symbolic path outside the source worktree")
    if output.exists():
        raise BuildError("build output already exists")
    if not products:
        raise BuildError("at least one product is required")
    policy = signing.load_policy() if signing_policy is None else signing_policy
    if policy is not None:
        signing.assert_current(policy)
        signing.preflight(policy)
    workspace.require_capacity()
    workspace.require_path(output)
    cache = Path(os.environ.get("CELL_RELEASE_CACHE_DIR", str(workspace.directory("releases")))).expanduser()
    workspace.require_path(cache)
    if not cache.is_absolute() or cache.is_symlink():
        raise BuildError("release cache must be an absolute non-symbolic directory")
    cache.mkdir(mode=0o700, parents=True, exist_ok=True)
    source_key = candidate.source_identity(source)
    environment, configuration = build_configuration(source, cache)
    metadata = json.loads(tool_output(source, environment, "cargo", "metadata", "--format-version", "1",
                                      "--no-deps", "--locked", "--manifest-path", str(source / "Cargo.toml")))
    selected = selection(source, products, unit, metadata)
    started = time.monotonic()
    lock_path = cache / "build.lock"
    if lock_path.is_symlink():
        raise BuildError("release cache lock must not be symbolic")
    output.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    staging = Path(tempfile.mkdtemp(prefix=".preparation-", dir=output.parent))
    try:
        # Cargo owns cache freshness. Keep its outputs locked until all selected
        # binaries have been copied into independently signed product packages.
        with lock_path.open("a+b") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            queue_seconds = time.monotonic() - started
            names = sorted(name for item in selected.values() for name in item["binaries"])
            packages = sorted({record["package"] for item in selected.values() for record in item["binaries"].values()})
            command = ["cargo", "build", "--release", "--locked", "--manifest-path", str(source / "Cargo.toml"),
                       "--target", configuration["target"], "--target-dir", str(cache / "target"),
                       "--jobs", str(configuration["jobs"])]
            for package in packages:
                command.extend(("--package", package))
            for name in names:
                command.extend(("--bin", name))
            build_started = time.monotonic()
            subprocess.run(workspace.confined_command(command), cwd=source, env=environment,
                           check=True, stdout=sys.stderr)
            build_seconds = time.monotonic() - build_started

            def assemble(product: str) -> tuple[str, dict[str, Any]]:
                item = selected[product]
                manifest = candidate.stage_build(source, product, staging / "candidates" / product,
                                                 item["binary_spec"],
                                                 target=cache / "target" / configuration["target"],
                                                 source_key=source_key, signing_policy=policy)
                return product, {"candidate_id": manifest["candidate_id"], "source_key": manifest["source_key"],
                                 "candidate_dir": str(output / "candidates" / product)}

            with ThreadPoolExecutor(max_workers=min(len(selected), 8)) as executor:
                candidates = dict(executor.map(assemble, selected))
            result = {"schema": 1, "state": "built", "source_key": source_key,
                      "candidates": candidates, "build_seconds": build_seconds, "queue_seconds": queue_seconds,
                      "elapsed_seconds": time.monotonic() - started, "signing_policy": policy}
            (staging / "result.json").write_bytes(candidate.json_bytes(result))
            staging.rename(output)
            return result
    finally:
        if staging.exists():
            candidate.remove_tree(staging)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-root", type=Path, required=True)
    parser.add_argument("--product", action="append", required=True)
    parser.add_argument("--unit")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--signing-policy-file", type=Path, help=argparse.SUPPRESS)
    arguments = parser.parse_args()
    try:
        options = {}
        if arguments.signing_policy_file is not None:
            candidate.regular(arguments.signing_policy_file)
            value = json.loads(arguments.signing_policy_file.read_text())
            if value is None and sys.platform != "darwin":
                policy = None
            else:
                policy = signing.validate_policy(value)
            signing.assert_current(policy)
            options["signing_policy"] = policy
        result = prepare(arguments.source_root, arguments.product, arguments.output,
                         arguments.unit, **options)
        print(json.dumps(result, sort_keys=True))
        return 0
    except (BuildError, candidate.CandidateError, OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        print(f"cell-build: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
