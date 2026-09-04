#!/bin/sh

set -eu

: "${PIPELINE_ROOT:?}"
: "${CELL_PIPELINE_PRODUCT:?}"

python3 "$PIPELINE_ROOT/deployment/generate.py" \
    --check --product "$CELL_PIPELINE_PRODUCT"
