#!/usr/bin/env python3
"""Product-owned deployment boundary; invoked by the Cell coordinator."""
import json
from pathlib import Path
import sys

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from deployment.adapter_support import ProductAdapter, MaintainedAdapter, Stopped, command, digest, main

class CrmAdapter(MaintainedAdapter):
    def apply(self):
        self.check_prior()
        command([self.candidate(), "--json", "migrate", "--backup",
                 self.install.parent / f"crm-pre-migration-{self.run_id}.sqlite"], env=self.environment(), json_output=True)
        return super().apply()

if __name__ == "__main__":
    raise SystemExit(main(CrmAdapter, json.loads(Path(__file__).with_name("adapter.json").read_text())))
