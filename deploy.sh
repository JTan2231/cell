#!/bin/sh
# Deploy selected products in the foreground and remove temporary run state.
set -eu
CELL_ROOT=$(CDPATH='' cd "$(dirname "$0")" && pwd)
exec python3 "$CELL_ROOT/deployment/cli.py" "$@"
