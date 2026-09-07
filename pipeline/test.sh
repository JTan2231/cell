#!/bin/sh
# Explicit shared platform suites. Each suite obtains its own broker admission.
set -eu
PIPELINE_ROOT=$(CDPATH='' cd "$(dirname "$0")/.." && pwd)
export PYTHONDONTWRITEBYTECODE=1
exec python3 "$PIPELINE_ROOT/pipeline/select_changes.py" shared "$@"
