#!/usr/bin/env python3
"""Run a complete Cargo test group, inside an admitted product gate."""

import argparse
import json
import subprocess

from platform_inputs import PLATFORM_PACKAGES, platform_target


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest-path", required=True)
    parser.add_argument("--group", choices=("product", "platform"), required=True)
    parser.add_argument("--offline", action="store_true")
    parser.add_argument("--no-fail-fast", action="store_true")
    parser.add_argument("packages", nargs="+")
    args = parser.parse_args()
    common = ["--manifest-path", args.manifest_path, "--locked"]
    if args.offline:
        common.append("--offline")
    metadata = subprocess.run(
        ["cargo", "metadata", *common, "--no-deps", "--format-version", "1"],
        capture_output=True, text=True, check=True,
    )
    packages = {package["name"]: package for package in json.loads(metadata.stdout)["packages"]}
    failure = 0
    for name in args.packages:
        # Shared library suites have their own gate; never repeat them as an
        # incidental part of a consumer's tests.
        if name in PLATFORM_PACKAGES:
            continue
        package = packages[name]
        targets = []
        docs = False
        for target in package["targets"]:
            if platform_target(name, target) != (args.group == "platform"):
                continue
            kinds = target["kind"]
            if any(kind in kinds for kind in ("lib", "rlib", "dylib", "cdylib", "staticlib", "proc-macro")):
                if target.get("test", True):
                    targets.append("--lib")
                docs |= target.get("doctest", False)
            elif target.get("test", True):
                for kind in ("bin", "test", "example", "bench"):
                    if kind in kinds:
                        targets.extend(["--" + kind, target["name"]])
                        break
        command = ["cargo", "test", *common, "--package", name]
        if args.no_fail_fast:
            command.append("--no-fail-fast")
        # Never issue an unqualified cargo test: it would re-enable every
        # integration target, including installation tests.
        for selection in ([targets] if targets else []) + ([["--doc"]] if docs else []):
            result = subprocess.run([*command, *selection], check=False)
            if result.returncode:
                failure = result.returncode
                if not args.no_fail_fast:
                    return failure
    return failure


if __name__ == "__main__":
    raise SystemExit(main())
