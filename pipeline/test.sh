#!/bin/sh
# Compatibility wrapper for the single CI manager path.
set -eu
PIPELINE_ROOT=$(CDPATH='' cd "$(dirname "$0")/.." && pwd)
exec "$PIPELINE_ROOT/ci.sh" "$@"
