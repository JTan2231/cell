#!/usr/bin/env python3
"""Build selected product packages once inside an admitted release CI gate."""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import subprocess
import sys


def load_configuration(root: Path, products: list[str]) -> tuple[dict, str]:
    result = subprocess.run([
        "sh", "-eu", "-c", '''
PIPELINE_ROOT=$1
export PIPELINE_ROOT
shift
. "$PIPELINE_ROOT/pipeline/lib.sh"
for product do
    pipeline_load_descriptor "$product"
    pipeline_bootstrap_cargo
    printf '%s\\0%s\\0%s\\0' "$PRODUCT_ID" "$CARGO_PACKAGES" "$CARGO_OFFLINE"
done
printf '%s\\0' "$PATH"
''', "release-build", str(root), *sorted(set(products)),
    ], check=True, capture_output=True, text=True)
    fields = result.stdout.split("\0")
    if fields.pop() != "" or not fields:
        raise ValueError("invalid product descriptor output")
    cargo_path = fields.pop()
    if len(fields) % 3:
        raise ValueError("invalid product descriptor output")
    configs = {}
    for index in range(0, len(fields), 3):
        product, packages, offline = fields[index:index + 3]
        if not packages.split() or offline not in ("0", "1"):
            raise ValueError(f"invalid Cargo settings for product {product}")
        configs[product] = {"packages": packages.split(), "offline": offline == "1"}
    return configs, cargo_path


def run_build(root: Path, configs: dict, cargo_path: str) -> int:
    packages = sorted({package for config in configs.values() for package in config["packages"]})
    if not packages:
        raise ValueError("release build requires selected product packages")
    environment = {**os.environ, "PATH": cargo_path, "CARGO_BUILD_WARNINGS": "deny"}
    offline = any(config["offline"] for config in configs.values())
    if offline:
        environment["CARGO_NET_OFFLINE"] = "true"
    for tool in ("rustc", "cargo"):
        version = subprocess.run([tool, "--version"], check=True, capture_output=True,
                                 text=True, env=environment, cwd=root).stdout.strip()
        if not version.startswith(f"{tool} 1.97.1 "):
            raise ValueError(f"{tool} 1.97.1 is required; found {version}")
    command = ["cargo", "build", "--manifest-path", str(root / "Cargo.toml")]
    for package in packages:
        command.extend(["--package", package])
    command.extend(["--release", "--locked"])
    if offline:
        command.append("--offline")
    print("==> release build for selected Rust packages", flush=True)
    return subprocess.run(command, check=False, env=environment, cwd=root).returncode


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--product", action="append", required=True)
    args = parser.parse_args(argv)
    root = Path(__file__).resolve().parent.parent
    try:
        configs, cargo_path = load_configuration(root, args.product)
        return run_build(root, configs, cargo_path)
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        message = getattr(error, "stderr", None) or str(error)
        print(f"ci: release build stage failed: {message.strip()}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
