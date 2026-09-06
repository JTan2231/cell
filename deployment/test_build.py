"""Release preparation with disposable Git and fake compilers only."""

from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

sys.dont_write_bytecode = True

from deployment import build, candidate


FAKE_CARGO = r'''#!PYTHON
import json, pathlib, sys
base = pathlib.Path(BASE)
args = sys.argv[1:]
if args == ["--version"]:
    print("cargo fixture 1.0")
elif args[0] == "metadata":
    print(pathlib.Path("metadata.json").read_text())
elif args[0] == "build":
    with (base / "calls.jsonl").open("a") as stream:
        stream.write(json.dumps(args) + "\n")
    fault = json.loads((base / "fault.json").read_text()) if (base / "fault.json").exists() else {}
    if fault.get("source_change"):
        pathlib.Path("source.txt").write_text("changed during build\n")
    target = pathlib.Path(args[args.index("--target-dir") + 1]) / args[args.index("--target") + 1] / "release"
    target.mkdir(parents=True, exist_ok=True)
    for index, value in enumerate(args):
        if value == "--bin":
            name = args[index + 1]
            binary = target / name
            version = "9.9.9" if fault.get("wrong_version") else "1.0.0"
            binary.write_text("#!/bin/sh\nprintf '%s\\n' '" + name + " " + version + "'\n")
            binary.chmod(0o755)
else:
    raise SystemExit("unexpected Cargo command: " + repr(args))
'''


class BuildTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.base = Path(self.temporary.name)
        self.source = self.base / "source"
        self.source.mkdir()
        self.git("init", "-q", "-b", "main")
        self.git("config", "user.name", "Build fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        self.write("Cargo.toml", "[workspace]\nmembers = []\n")
        self.write("Cargo.lock", "version = 4\n")
        self.write("source.txt", "source\n")
        self.write("deployment/crates/cell-install/src/lib.rs", "// shared adapter\n")
        packages = []
        for name in ("alpha", "beta"):
            binaries = [name, name + "-helper"] if name == "alpha" else [name]
            binary_rows = "\n".join(f"{binary}|target/release/{binary}|{binary}" for binary in binaries)
            self.write(f"pipeline/products/{name}.sh", f"PRODUCT_ID={name}\nPRODUCT_DIR={name}\n"
                       f"CARGO_PACKAGES={name}-package\nRELEASE_UNITS='" + "\n".join(
                           f"{binary}|Fixture|package|Cargo.toml|fixture-|1" for binary in binaries)
                       + f"'\nRELEASE_BINARY_CHECKS='{binary_rows}'\n")
            self.write(f"{name}/packaging/manifest.txt", "owned packaging\n")
            self.write(f"{name}/chancery/provider.json", '{"release":"1.0.0"}\n')
            self.write(f"{name}/deployment/adapter.py", "# owned adapter\n")
            packages.append({"name": name + "-package", "version": "1.0.0",
                             "targets": [{"name": binary, "kind": ["bin"]} for binary in binaries]})
        self.write("metadata.json", json.dumps({"packages": packages}))
        self.commit()
        tools = self.base / "tools"
        tools.mkdir()
        (tools / "cargo").write_text(FAKE_CARGO.replace("PYTHON", sys.executable).replace("BASE", repr(str(self.base))))
        (tools / "rustc").write_text("#!/bin/sh\nprintf '%s\\n' 'rustc fixture 1.0' 'host: fixture-host'\n")
        for tool in tools.iterdir():
            tool.chmod(0o755)
        self.cache = self.base / "cache"
        self.environment = mock.patch.dict(os.environ, {
            "PATH": str(tools) + os.pathsep + os.environ.get("PATH", ""),
            "CELL_RELEASE_CACHE_DIR": str(self.cache), "CELL_RELEASE_BUILD_JOBS": "3",
        })
        self.environment.start()

    def tearDown(self) -> None:
        self.environment.stop()
        for path in self.base.rglob("*"):
            if path.is_dir() and not path.is_symlink():
                path.chmod(0o700)
        self.temporary.cleanup()

    def write(self, relative: str, content: str) -> None:
        path = self.source / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)

    def git(self, *arguments: str) -> str:
        result = subprocess.run(["git", "-C", str(self.source), *arguments], check=True,
                                capture_output=True, text=True)
        return result.stdout.strip()

    def commit(self) -> None:
        self.git("add", ".")
        self.git("commit", "-qm", "fixture")

    def calls(self) -> list[list[str]]:
        path = self.base / "calls.jsonl"
        return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []

    def prepare(self, name: str, products: list[str] | None = None, unit: str | None = None) -> dict:
        return build.prepare(self.source, products or ["alpha"], self.base / name, unit)

    def test_one_selected_batch_and_complete_cached_materials(self) -> None:
        result = self.prepare("output", ["beta", "alpha"])
        self.assertEqual(len(self.calls()), 1)
        command = self.calls()[0]
        self.assertEqual(command.count("--package"), 2)
        self.assertEqual(command.count("--bin"), 3)
        self.assertIn("--release", command)
        self.assertIn("--locked", command)
        self.assertNotIn("--workspace", command)
        self.assertEqual(command[command.index("--jobs") + 1], "3")
        self.assertFalse(result["cache_hit"])
        for product in ("alpha", "beta"):
            manifest = candidate.verify(self.base / "output" / "candidates" / product, product=product)
            self.assertEqual(manifest["source_key"], result["source_key"])
            for relative, digest in manifest["source_inputs"].items():
                self.assertEqual(candidate.digest(Path(result["cache_entry"]) / "materials" / relative), digest)
        self.assertEqual(json.loads((self.base / "output/result.json").read_text()), result)

    def test_dirty_version_source_reuses_after_commit_and_new_worktree(self) -> None:
        self.write("source.txt", "release edits before commit\n")
        first = self.prepare("before")
        self.commit()
        worktree = self.base / "worktree"
        self.git("worktree", "add", "--detach", str(worktree), "HEAD")
        second = build.prepare(worktree, ["alpha"], self.base / "after")
        self.assertTrue(second["cache_hit"])
        self.assertEqual(first["source_key"], second["source_key"])
        self.assertEqual(len(self.calls()), 1)
        manifest = candidate.verify(self.base / "after/candidates/alpha", commit=self.git("rev-parse", "HEAD"))
        self.assertNotEqual(first["candidates"]["alpha"]["candidate_id"], manifest["candidate_id"])

    def test_unit_build_excludes_other_product_binaries(self) -> None:
        self.prepare("one-unit", unit="alpha-helper")
        command = self.calls()[0]
        self.assertEqual(command.count("--bin"), 1)
        self.assertEqual(command[command.index("--bin") + 1], "alpha-helper")
        self.assertEqual(set(candidate.verify(self.base / "one-unit/candidates/alpha")["binaries"]), {"alpha-helper"})

    def test_cache_detects_changed_binary_and_materials(self) -> None:
        first = self.prepare("first")
        material = Path(first["cache_entry"]) / "materials/alpha/packaging/manifest.txt"
        material.chmod(0o600)
        material.write_text("changed cache content")
        with self.assertRaisesRegex(build.BuildError, "cached release materials changed"):
            self.prepare("second")
        self.assertEqual(len(self.calls()), 1)

    def test_source_change_during_build_never_publishes_candidate(self) -> None:
        (self.base / "fault.json").write_text('{"source_change":true}')
        with self.assertRaisesRegex(build.BuildError, "source changed during release build"):
            self.prepare("changed")
        self.assertFalse((self.base / "changed").exists())
        self.assertEqual(list((self.cache / "entries").iterdir()), [])

    def test_wrong_binary_version_is_not_cached(self) -> None:
        (self.base / "fault.json").write_text('{"wrong_version":true}')
        with self.assertRaisesRegex(build.BuildError, "release version differs"):
            self.prepare("wrong-version")
        self.assertEqual(list((self.cache / "entries").iterdir()), [])

    def test_configuration_change_causes_new_build(self) -> None:
        self.prepare("first")
        with mock.patch.dict(os.environ, {"RUSTFLAGS": "-C opt-level=2"}):
            result = self.prepare("second")
        self.assertFalse(result["cache_hit"])
        self.assertEqual(len(self.calls()), 2)
        manifest = json.loads((Path(result["cache_entry"]) / "manifest.json").read_text())
        environment = manifest["identity"]["configuration"]["environment"]
        self.assertEqual(environment["RUSTFLAGS"], build.hashlib.sha256(b"-C opt-level=2").hexdigest())
        self.assertNotIn("-C opt-level=2", json.dumps(manifest))

    def test_source_content_and_unit_selection_are_in_identity(self) -> None:
        self.prepare("all")
        self.prepare("one", unit="alpha")
        self.write("untracked.txt", "untracked source also matters\n")
        self.prepare("new-source", unit="alpha")
        self.assertEqual(len(self.calls()), 3)


if __name__ == "__main__":
    unittest.main()
