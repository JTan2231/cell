#!/bin/sh
# Public lightweight preflight. Its checks never invoke Cargo.
set -eu
PIPELINE_ROOT=$(CDPATH='' cd "$(dirname "$0")/.." && pwd)
flags=
for option in "$@"; do
    case "$option" in
        --verbose|--quiet-result) flags="$flags $option" ;;
        *) printf '%s\n' 'Usage: pipeline/test.sh [--verbose] [--quiet-result]' >&2; exit 2 ;;
    esac
done
# flags contains only the fixed presentation options above.
exec python3 "$PIPELINE_ROOT/ci_broker/client.py" run $flags \
    --repo-root "$PIPELINE_ROOT" --gate cell.preflight --lane light -- \
    "$PIPELINE_ROOT/pipeline/check.sh"
