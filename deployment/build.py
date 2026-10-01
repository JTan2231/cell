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
import shutil
import subprocess
import sys
import tempfile
import time
from typing import Any

sys.dont_write_bytecode = True
if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from ci_broker.client import bootstrap_cargo_path, git
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


class CompilationError(BuildError):
    """Cargo rejected the selected source during release compilation."""


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


def build_configuration(source: Path, cache: Path, *, release_check: bool = False,
                        offline: bool = False) -> tuple[dict[str, str], dict[str, Any]]:
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
    if release_check:
        environment["CARGO_BUILD_WARNINGS"] = "deny"
    if offline:
        environment["CARGO_NET_OFFLINE"] = "true"
    cargo_version = tool_output(source, environment, "cargo", "--version")
    rustc_version = tool_output(source, environment, "rustc", "--version", "--verbose")
    if release_check and (cargo_version.split()[:2] != ["cargo", "1.97.1"]
                          or rustc_version.split()[:2] != ["rustc", "1.97.1"]):
        raise BuildError("release checks require cargo and rustc 1.97.1")
    host = next((line.removeprefix("host: ") for line in rustc_version.splitlines()
                 if line.startswith("host: ")), None)
    if host is None:
        raise BuildError("rustc did not report its host target")
    return environment, {"target": host, "jobs": jobs}


def prepare(source: Path, products: list[str], output: Path, unit: str | None = None, *,
            signing_policy: dict[str, Any] | None = None, prepared_build: Path | None = None,
            prepared_build_snapshot: dict[str, Any] | None = None,
            release_check: bool = False) -> dict[str, Any]:
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
    source_key = candidate.source_identity(source)
    if prepared_build is not None:
        return prepare_supplied(source, products, output, unit, policy, source_key, prepared_build,
                                prepared_build_snapshot=prepared_build_snapshot, release_check=release_check)
    if prepared_build_snapshot is not None:
        raise BuildError("prepared build snapshot requires a prepared build")
    cache = Path(os.environ.get("CELL_RELEASE_CACHE_DIR", str(workspace.directory("releases")))).expanduser()
    workspace.require_path(cache)
    if not cache.is_absolute() or cache.is_symlink():
        raise BuildError("release cache must be an absolute non-symbolic directory")
    cache.mkdir(mode=0o700, parents=True, exist_ok=True)
    offline = release_check and any(read_descriptor(source, product).get("CARGO_OFFLINE") == "1"
                                   for product in products)
    environment, configuration = build_configuration(source, cache, release_check=release_check,
                                                      offline=offline)
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
            if release_check:
                packages = sorted({package for product in products
                                   for package in read_descriptor(source, product).get("CARGO_PACKAGES", "").split()})
            else:
                packages = sorted({record["package"] for item in selected.values()
                                   for record in item["binaries"].values()})
            command = ["cargo", "build", "--release", "--locked", "--manifest-path", str(source / "Cargo.toml"),
                       "--target", configuration["target"], "--target-dir", str(cache / "target"),
                       "--jobs", str(configuration["jobs"])]
            if offline:
                command.append("--offline")
            for package in packages:
                command.extend(("--package", package))
            if not release_check:
                for name in names:
                    command.extend(("--bin", name))
            build_started = time.monotonic()
            try:
                subprocess.run(workspace.confined_command(command), cwd=source, env=environment,
                               check=True, stdout=sys.stderr)
            except subprocess.CalledProcessError as error:
                raise CompilationError(f"release compilation failed (exit {error.returncode})") from error
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
                      "elapsed_seconds": time.monotonic() - started, "release_check": release_check,
                      "signing_policy": policy}
            (staging / "result.json").write_bytes(candidate.json_bytes(result))
            staging.rename(output)
            return result
    finally:
        if staging.exists():
            candidate.remove_tree(staging)


def prepare_supplied(source: Path, products: list[str], output: Path, unit: str | None,
                     policy: dict[str, Any] | None, source_key: str, prepared_build: Path, *,
                     prepared_build_snapshot: dict[str, Any] | None, release_check: bool) -> dict[str, Any]:
    """Copy verified supplied bundles and compile only the missing product scope."""
    started = time.monotonic()
    if not prepared_build.is_absolute() or prepared_build.is_symlink():
        raise BuildError("prepared build must be an absolute non-symbolic result file")
    workspace.require_path(prepared_build)
    candidate.regular(prepared_build)
    supplied = json.loads(prepared_build.read_text())
    if prepared_build_snapshot is not None and supplied != prepared_build_snapshot:
        raise BuildError("prepared build changed after deployment admission")
    if (not isinstance(supplied, dict) or supplied.get("schema") != 1
            or supplied.get("state") != "built" or supplied.get("source_key") != source_key):
        raise BuildError("prepared build does not match the selected source")
    if supplied.get("signing_policy") != policy:
        raise BuildError("prepared build does not match the selected signing policy")
    if release_check and supplied.get("release_check") is not True:
        raise BuildError("prepared build has no release check evidence")
    available = supplied.get("candidates")
    if not isinstance(available, dict):
        raise BuildError("prepared build candidate inventory is invalid")
    selected = sorted({"krisis" if product == "decisions" else product for product in products})
    if unit is not None and len(selected) != 1:
        raise BuildError("--unit requires exactly one selected product")
    commit = git(source, "rev-parse", "HEAD").decode().strip()
    reused: dict[str, tuple[Path, dict[str, Any]]] = {}
    for product in selected:
        if product not in available:
            continue
        record = available[product]
        if not isinstance(record, dict) or not isinstance(record.get("candidate_dir"), str):
            raise BuildError(f"prepared candidate record is invalid: {product}")
        directory = Path(record["candidate_dir"])
        if not directory.is_absolute():
            raise BuildError(f"prepared candidate path must be absolute: {product}")
        workspace.require_path(directory)
        manifest = candidate.verify(directory, signing_policy=policy)
        if (manifest.get("product") != product or manifest.get("source_commit") != commit
                or manifest.get("source_key") != source_key
                or manifest.get("candidate_id") != record.get("candidate_id")
                or record.get("source_key") != source_key):
            raise BuildError(f"prepared candidate does not match the selected source and identity: {product}")
        values = read_descriptor(source, product)
        rows = [row.split("|") for row in values.get("RELEASE_BINARY_CHECKS", "").splitlines()]
        if any(len(row) != 3 for row in rows):
            raise BuildError(f"invalid binary declaration: {product}")
        expected = {row[2] for row in rows if unit is None or row[0] == unit}
        if not expected or set(manifest["binaries"]) != expected:
            raise BuildError(f"prepared candidate executable scope does not match: {product}")
        reused[product] = directory, manifest
    output.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    staging = Path(tempfile.mkdtemp(prefix=".preparation-", dir=output.parent))
    try:
        missing = [product for product in selected if product not in reused]
        built = prepare(source, missing, staging / "missing", unit, signing_policy=policy,
                        release_check=release_check) if missing else {}
        candidates = {}
        for product in selected:
            if product in reused:
                directory, expected_manifest = reused[product]
            else:
                directory = Path(built["candidates"][product]["candidate_dir"])
                expected_manifest = candidate.verify(directory, signing_policy=policy)
            destination = staging / "candidates" / product
            shutil.copytree(directory, destination)
            manifest = candidate.verify(destination, signing_policy=policy)
            if manifest != expected_manifest:
                raise BuildError(f"prepared candidate changed while copying: {product}")
            candidate.seal_tree(destination)
            candidates[product] = {"candidate_id": manifest["candidate_id"], "source_key": source_key,
                                   "candidate_dir": str(output / "candidates" / product)}
        if missing:
            candidate.remove_tree(staging / "missing")
        result = {"schema": 1, "state": "built", "source_key": source_key,
                  "candidates": candidates, "prepared_build": str(prepared_build),
                  "reused_products": sorted(reused), "built_products": missing,
                  "build_seconds": built.get("build_seconds", 0.0),
                  "queue_seconds": built.get("queue_seconds", 0.0),
                  "elapsed_seconds": time.monotonic() - started,
                  "release_check": release_check,
                  "signing_policy": policy}
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
    parser.add_argument("--prepared-build", type=Path, help="reuse verified candidates from this build result")
    parser.add_argument("--prepared-build-snapshot-file", type=Path, help=argparse.SUPPRESS)
    parser.add_argument("--release-check", action="store_true", help=argparse.SUPPRESS)
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
        snapshot = None
        if arguments.prepared_build_snapshot_file is not None:
            candidate.regular(arguments.prepared_build_snapshot_file)
            snapshot = json.loads(arguments.prepared_build_snapshot_file.read_text())
            if not isinstance(snapshot, dict):
                raise BuildError("prepared build snapshot must be an object")
        result = prepare(arguments.source_root, arguments.product, arguments.output,
                         arguments.unit, prepared_build=arguments.prepared_build,
                         prepared_build_snapshot=snapshot, release_check=arguments.release_check, **options)
        print(json.dumps(result, sort_keys=True))
        return 0
    except (BuildError, candidate.CandidateError, OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        print(f"cell-build: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
