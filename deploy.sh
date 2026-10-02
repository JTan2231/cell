#!/bin/sh
# Execute selected products' declared deployment instructions in the foreground.
set -eu
CELL_ROOT=$(CDPATH='' cd "$(dirname "$0")" && pwd)
exec python3 "$CELL_ROOT/deployment/cli.py" "$@"
