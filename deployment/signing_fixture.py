#!/usr/bin/env python3
"""Sign copied native test fixtures using the existing current-user policy."""

from __future__ import annotations

import argparse
from pathlib import Path
import sys

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from deployment import signing


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("product")
    parser.add_argument("artifact")
    parser.add_argument("path", type=Path)
    args = parser.parse_args()
    try:
        if sys.platform == "darwin" and signing.is_native(args.path):
            policy = signing.load_policy()
            signing.sign(args.path, policy, args.product, args.artifact)
            signing.verify(args.path, policy, args.product, args.artifact)
        return 0
    except (OSError, ValueError, signing.SigningError) as error:
        print(f"cell-fixture-signing: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
