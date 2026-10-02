"""Product source roots are canonical, relative and confined to the checkout."""

from pathlib import Path
import tempfile
import unittest

from deployment.inventory import product_directory, product_root


class InventoryTests(unittest.TestCase):
    def test_flat_and_nested_directories_keep_their_declared_paths(self):
        for directory in ("email", "infrastructure/email", "products/weaver-narrative"):
            with self.subTest(directory=directory):
                self.assertEqual(product_directory(directory), directory)

    def test_absolute_parent_and_noncanonical_directories_are_rejected(self):
        for directory in ("", ".", "/email", "../email", "infrastructure/../email",
                          "./email", "infrastructure/./email", "infrastructure//email", "email/"):
            with self.subTest(directory=directory):
                with self.assertRaisesRegex(ValueError, "invalid product directory"):
                    product_directory(directory)

    def test_source_root_requires_an_existing_directory_without_symbolic_components(self):
        with tempfile.TemporaryDirectory() as name:
            base = Path(name).resolve()
            source = base / "source"
            owned = source / "products/clew"
            owned.mkdir(parents=True)
            external = base / "external"
            external.mkdir()
            (source / "inside").symlink_to(source / "products", target_is_directory=True)
            (source / "outside").symlink_to(external, target_is_directory=True)
            self.assertEqual(product_root(source, "products/clew"), owned)
            for directory in ("missing", "inside/clew", "outside"):
                with self.subTest(directory=directory):
                    with self.assertRaisesRegex(ValueError, "product source directory is unavailable"):
                        product_root(source, directory)


if __name__ == "__main__":
    unittest.main()
