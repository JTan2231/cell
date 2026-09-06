#!/usr/bin/env python3
"""Product-owned deployment boundary; invoked by the Cell coordinator."""
import json
from pathlib import Path
import sys

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from deployment.adapter_support import ProductAdapter, MaintainedAdapter, Stopped, command, digest, main

class ClockworkAdapter(ProductAdapter):
    def deploy_arguments(self):
        reader = (self.home / ".local/bin/chancery").resolve(strict=True)
        digest(reader)
        return super().deploy_arguments() + ["--chancery", reader]


if __name__ == "__main__":
    raise SystemExit(main(ClockworkAdapter, json.loads(Path(__file__).with_name("adapter.json").read_text())))
