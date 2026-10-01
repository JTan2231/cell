#!/usr/bin/env python3
"""Run selected Rust tests in one nextest pool inside an admitted CI gate."""

from __future__ import annotations

import argparse
from dataclasses import dataclass, field
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET

from platform_inputs import platform_target


SHARED_SUITES = {
    "install": "cell-install",
    "maintenance": "cell-maintenance",
    "prompts": "cell-prompts",
}
SHARED_PACKAGES = frozenset(SHARED_SUITES.values())
LIBRARY_KINDS = frozenset(("lib", "rlib", "dylib", "cdylib", "staticlib", "proc-macro"))
PROMPT_CONSUMERS = frozenset((
    "annals", "decisions", "semantics", "paperboy", "platter", "weaver", "emt",
    "conatus", "cell-prompts",
))


@dataclass
class TestPlan:
    # Cargo package, nextest binary kind, and Cargo target name identify a target.
    targets: dict[tuple[str, str, str], set[str]] = field(default_factory=dict)
    doctests: dict[str, bool] = field(default_factory=dict)
    offline: bool = False


def load_products(root: Path, products: list[str]) -> dict:
    if not products:
        return {}
    result = subprocess.run([
        "sh", "-eu", "-c", '''
PIPELINE_ROOT=$1
export PIPELINE_ROOT
shift
. "$PIPELINE_ROOT/pipeline/lib.sh"
for product do
    pipeline_load_descriptor "$product"
    printf '%s\\0%s\\0%s\\0' "$PRODUCT_ID" "$CARGO_PACKAGES" "$CARGO_OFFLINE"
done
''', "parallel-tests", str(root), *sorted(set(products)),
    ], check=True, capture_output=True, text=True)
    fields = result.stdout.split("\0")
    if fields[-1] != "" or (len(fields) - 1) % 3:
        raise ValueError("invalid product descriptor output")
    configs = {}
    for index in range(0, len(fields) - 1, 3):
        product, packages, offline = fields[index:index + 3]
        if not packages.split() or offline not in ("0", "1"):
            raise ValueError(f"invalid Cargo settings for product {product}")
        configs[product] = {"packages": packages.split(), "offline": offline == "1"}
    return configs


def binary_kind(target: dict) -> str | None:
    kinds = target["kind"]
    if "proc-macro" in kinds:
        return "proc-macro"
    if LIBRARY_KINDS.intersection(kinds):
        return "lib"
    return next((kind for kind in ("bin", "test", "example", "bench") if kind in kinds), None)


def make_plan(workspace: dict, configs: dict, products: list[str],
              platform_products: list[str], shared_suites: list[str]) -> TestPlan:
    packages = {package["name"]: package for package in workspace["packages"]}
    plan = TestPlan(offline=any(config["offline"] for config in configs.values()))

    def include(name, owner, group, offline):
        if name not in packages:
            raise ValueError(f"selected Cargo package is absent from metadata: {name}")
        for target in packages[name]["targets"]:
            if group is not None and platform_target(name, target) != group:
                continue
            kind = binary_kind(target)
            if kind is None:
                continue
            if target.get("test", True):
                key = (name, kind, target["name"])
                plan.targets.setdefault(key, set()).add(owner)
            if kind in ("lib", "proc-macro") and target.get("doctest", False):
                plan.doctests[name] = plan.doctests.get(name, False) or offline

    for selected, group, label in ((products, False, "product"),
                                   (platform_products, True, "platform")):
        for product in sorted(set(selected)):
            config = configs[product]
            for name in config["packages"]:
                # Shared suites are required only by explicit shared selection.
                if name not in SHARED_PACKAGES:
                    include(name, f"{label}:{product}", group, config["offline"])
    for suite in sorted(set(shared_suites)):
        include(SHARED_SUITES[suite], f"shared:{suite}", None, False)
    return plan


def exact_match(value: str) -> str:
    # Equality matchers still require nextest's documented DSL escapes.
    for original, replacement in (("\\", "\\\\"), ("\n", "\\n"), ("\r", "\\r"),
                                  ("\t", "\\t"), (")", "\\)"), (",", "\\,")):
        value = value.replace(original, replacement)
    return "=" + value


def filterset(plan: TestPlan) -> str:
    return " | ".join(
        f"(package({exact_match(package)}) & kind({exact_match(kind)}) "
        f"& binary({exact_match(name)}))"
        for package, kind, name in sorted(plan.targets)
    ) or "none()"


def report_failures(path: Path) -> None:
    if not path.is_file():
        return  # Compilation or discovery can fail before nextest writes JUnit.
    for case in ET.parse(path).iter("testcase"):
        if case.find("failure") is not None or case.find("error") is not None:
            name = " ".join(f"{case.get('classname', '')}::{case.get('name', '')}".split())
            # Keep existing manager failure-name extraction compatible with libtest.
            print(f"test {name} ... FAILED", flush=True)


def run_plan(root: Path, plan: TestPlan, nextest_path: Path, test_threads: int) -> int:
    for (package, kind, name), owners in sorted(plan.targets.items()):
        print(f"ci: selected {','.join(sorted(owners))} {package}/{kind}/{name}", flush=True)
    for package in sorted(plan.doctests):
        print(f"ci: selected doctests {package}", flush=True)
    selected_packages = {package for package, _, _ in plan.targets} | plan.doctests.keys()
    if not selected_packages:
        print("ci: selected Rust groups contain no test targets or doctests", flush=True)
        return 0

    common = ["--manifest-path", str(root / "Cargo.toml"), "--locked"]
    environment = {key: value for key, value in os.environ.items()
                   if not key.startswith("NEXTEST_")}
    failure = 0
    with tempfile.TemporaryDirectory(prefix="cell-parallel-tests-") as temporary:
        temporary = Path(temporary)
        if PROMPT_CONSUMERS.intersection(selected_packages):
            database = temporary / "private" / "bazaar.sqlite3"
            subprocess.run([
                "cargo", "run", *common, *(["--offline"] if plan.offline else []),
                "--quiet", "--package", "cell-prompts", "--", str(database),
                str(root / "prompting/seed.json"),
            ], check=True, env=environment, cwd=root)
            environment["CELL_BAZAAR_DATABASE"] = str(database)

        if plan.targets:
            config = temporary / "nextest.toml"
            user_config = temporary / "user.toml"
            report = temporary / "nextest/default/report.xml"
            # This gate's coverage and scheduler do not depend on user/repo overrides.
            config.write_text(
                "[store]\ndir = " + json.dumps(str(temporary / "nextest"))
                + '\n[test-groups]\nnucleus-harness = { max-threads = 1 }\n'
                + '[[profile.default.overrides]]\n'
                + "filter = '(package(=nucleus-codex) & kind(=lib)) | "
                + "(package(=nucleus-codex) & binary(=local_execution)) | "
                + "(package(=nucleus-daemon) & binary(=http_contract))'\n"
                + 'test-group = "nucleus-harness"\n'
                + '[profile.default.junit]\npath = "report.xml"\n'
            )
            user_config.write_text("")
            command = [
                str(nextest_path), "nextest", "run", *common, "--all-targets",
                "--config-file", str(config), "--user-config-file", str(user_config),
                "--profile", "default", "--ignore-default-filter", "--filterset", filterset(plan),
                "--no-fail-fast", "--retries", "0", "--test-threads", str(test_threads),
                "--no-tests", "pass", "--failure-output", "final", "--status-level", "pass",
                "--final-status-level", "fail", "--color", "never", "--no-input-handler",
            ]
            if plan.offline:
                command.append("--offline")
            if environment.get("CARGO_BUILD_JOBS"):
                command.extend(["--build-jobs", environment["CARGO_BUILD_JOBS"]])
            for package in sorted({package for package, _, _ in plan.targets}):
                command.extend(["--package", package])
            print(f"ci: parallel Rust pool uses {test_threads} test workers", flush=True)
            failure = subprocess.run(command, check=False, env=environment, cwd=root).returncode
            try:
                report_failures(report)
            except (OSError, ET.ParseError) as error:
                print(f"ci: cannot read nextest failure summary: {error}", file=sys.stderr)
                failure = failure or 1

        # Rustdoc remains required after the parallel pool drains, including on failure.
        for package, offline in sorted(plan.doctests.items()):
            print(f"==> doctests {package}", flush=True)
            result = subprocess.run([
                "cargo", "test", *common, "--package", package, "--doc", "--no-fail-fast",
                *(["--offline"] if offline else []),
            ], check=False, env=environment, cwd=root)
            failure = failure or result.returncode
    return failure


def positive_integer(value: str) -> int:
    number = int(value)
    if number < 1:
        raise argparse.ArgumentTypeError("test threads must be a positive integer")
    return number


def absolute_path(value: str) -> Path:
    path = Path(value)
    if not path.is_absolute():
        raise argparse.ArgumentTypeError("nextest path must be absolute")
    return path


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--product", action="append", default=[])
    parser.add_argument("--platform-product", action="append", default=[])
    parser.add_argument("--shared-suite", choices=tuple(SHARED_SUITES), action="append", default=[])
    parser.add_argument("--test-threads", type=positive_integer, default=4)
    parser.add_argument("--nextest-path", type=absolute_path, required=True)
    args = parser.parse_args(argv)
    root = Path(__file__).resolve().parent.parent
    try:
        configs = load_products(root, args.product + args.platform_product)
        metadata = subprocess.run([
            "cargo", "metadata", "--manifest-path", str(root / "Cargo.toml"), "--locked",
            *(["--offline"] if any(config["offline"] for config in configs.values()) else []),
            "--no-deps", "--format-version", "1",
        ], check=True, capture_output=True, text=True, cwd=root)
        plan = make_plan(json.loads(metadata.stdout), configs, args.product,
                         args.platform_product, args.shared_suite)
        return run_plan(root, plan, args.nextest_path, args.test_threads)
    except (OSError, ValueError, subprocess.SubprocessError, ET.ParseError) as error:
        message = getattr(error, "stderr", None) or str(error)
        print(f"ci: parallel Rust test stage failed: {message.strip()}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
