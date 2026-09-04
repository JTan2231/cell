#!/usr/bin/env python3
"""Generate the self-contained selector-only macOS deployers."""

from __future__ import annotations

import argparse
import json
import re
import shlex
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
DESCRIPTORS = Path(__file__).with_name("products.json")
TEMPLATE = Path(__file__).with_name("selector-only-deploy-user.sh.in")
TOKEN = re.compile(r"@[A-Z_]+@")


def load_products() -> list[dict[str, object]]:
    document = json.loads(DESCRIPTORS.read_text())
    if document.get("schema") != 1 or not isinstance(document.get("products"), list):
        raise ValueError("products.json must contain schema 1 and a products array")
    products = document["products"]
    required = {
        "product_id",
        "display_name",
        "application_support_name",
        "binary_name",
        "provider_id",
        "usage_description",
        "manifest_product_line",
        "print_state_path",
        "completion_note",
    }
    seen: set[str] = set()
    for product in products:
        if not isinstance(product, dict) or set(product) != required:
            raise ValueError(f"invalid selector-only product descriptor: {product!r}")
        product_id = product["product_id"]
        if not isinstance(product_id, str) or not re.fullmatch(r"[a-z][a-z0-9-]*", product_id):
            raise ValueError(f"invalid product_id: {product_id!r}")
        if product_id in seen:
            raise ValueError(f"duplicate product_id: {product_id}")
        seen.add(product_id)
        for key in required - {
            "usage_description",
            "manifest_product_line",
            "print_state_path",
            "completion_note",
        }:
            if not isinstance(product[key], str) or not product[key]:
                raise ValueError(f"{product_id}: {key} must be a nonempty string")
        usage_description = product["usage_description"]
        if (
            not isinstance(usage_description, list)
            or not usage_description
            or any(not isinstance(line, str) or not line for line in usage_description)
        ):
            raise ValueError(f"{product_id}: usage_description must be nonempty lines")
        if not isinstance(product["completion_note"], str):
            raise ValueError(f"{product_id}: completion_note must be a string")
        for key in ("manifest_product_line", "print_state_path"):
            if not isinstance(product[key], bool):
                raise ValueError(f"{product_id}: {key} must be boolean")
    return products


def render(template: str, product: dict[str, object]) -> str:
    replacements = {
        "@PRODUCT_ID@": shlex.quote(str(product["product_id"])),
        "@DISPLAY_NAME@": shlex.quote(str(product["display_name"])),
        "@APPLICATION_SUPPORT_NAME@": shlex.quote(str(product["application_support_name"])),
        "@BINARY_NAME@": shlex.quote(str(product["binary_name"])),
        "@PROVIDER_ID@": shlex.quote(str(product["provider_id"])),
        "@USAGE_DESCRIPTION@": "\n".join(product["usage_description"]),
        "@MANIFEST_PRODUCT_LINE@": "1" if product["manifest_product_line"] else "0",
        "@PRINT_STATE_PATH@": "1" if product["print_state_path"] else "0",
        "@COMPLETION_NOTE@": shlex.quote(str(product["completion_note"])),
    }
    rendered = template
    for token, value in replacements.items():
        rendered = rendered.replace(token, value)
    leftovers = sorted(set(TOKEN.findall(rendered)))
    if leftovers:
        raise ValueError(f"unreplaced template tokens: {', '.join(leftovers)}")
    return rendered


def output_path(product: dict[str, object]) -> Path:
    return ROOT / str(product["product_id"]) / "packaging" / "macos" / "deploy-user.sh"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true", help="fail if generated files differ")
    parser.add_argument("--product", action="append", default=[], help="limit generation to one product ID")
    args = parser.parse_args()

    products = load_products()
    requested = set(args.product)
    known = {str(product["product_id"]) for product in products}
    unknown = requested - known
    if unknown:
        parser.error(f"unknown product: {', '.join(sorted(unknown))}")
    if requested:
        products = [product for product in products if product["product_id"] in requested]

    template = TEMPLATE.read_text()
    changed: list[Path] = []
    for product in products:
        path = output_path(product)
        generated = render(template, product)
        current = path.read_text() if path.exists() else None
        executable = path.exists() and path.stat().st_mode & 0o777 == 0o755
        if current == generated and executable:
            continue
        changed.append(path)
        if not args.check:
            if current != generated:
                path.write_text(generated)
            path.chmod(0o755)

    if args.check and changed:
        for path in changed:
            print(f"generated deployer is stale: {path.relative_to(ROOT)}", file=sys.stderr)
        return 1
    if not args.check:
        for path in changed:
            print(f"generated {path.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"generate.py: {error}", file=sys.stderr)
        raise SystemExit(1)
