"""Check generated entry points from flat and nested product source roots."""

from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


class GeneratorTests(unittest.TestCase):
    def test_wrappers_reach_cell_root_and_preserve_arguments(self):
        for product_directory in ("alpha", "products/alpha", "infrastructure/alpha",
                                  "nested/group/alpha"):
            with self.subTest(product_directory=product_directory), \
                    tempfile.TemporaryDirectory(prefix="cell-generator-") as temporary:
                root = Path(temporary) / "Cell workspace"
                pipeline = root / "pipeline"
                descriptors = pipeline / "products"
                descriptors.mkdir(parents=True)
                source = Path(__file__).resolve().parent
                for name in ("generate.sh", "lib.sh"):
                    shutil.copyfile(source / name, pipeline / name)
                (descriptors / "alpha.sh").write_text(
                    "PIPELINE_SCHEMA=1\nPRODUCT_ID=alpha\n"
                    f"PRODUCT_DIR='{product_directory}'\n")
                product = root / product_directory
                product.mkdir(parents=True)
                for target in (root / "ci.sh", pipeline / "release.sh"):
                    target.write_text('#!/bin/sh\nprintf "%s\\n" "$0" "$@"\n')
                    target.chmod(0o755)
                generator = ["sh", str(pipeline / "generate.sh")]
                subprocess.run([*generator, "--write", "--product", "alpha"],
                               check=True, capture_output=True, text=True)
                subprocess.run([*generator, "--check", "--product", "alpha"],
                               check=True, capture_output=True, text=True)

                arguments = ["argument with spaces", "--flag"]
                for name, target, forwarded in (
                    ("ci.sh", root / "ci.sh", arguments),
                    ("release.sh", pipeline / "release.sh", ["alpha", *arguments]),
                ):
                    result = subprocess.run([str(product / name), *arguments], cwd=temporary,
                                            check=True, capture_output=True, text=True)
                    self.assertEqual(result.stdout.splitlines(), [str(target), *forwarded])


if __name__ == "__main__":
    unittest.main()
