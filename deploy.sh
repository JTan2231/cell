#!/bin/sh
# Product selection starts an independently running, journaled deployment.
set -eu
CELL_ROOT=$(CDPATH='' cd "$(dirname "$0")" && pwd)
exec python3 "$CELL_ROOT/deployment/cli.py" "$@"
