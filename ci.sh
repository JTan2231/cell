#!/bin/sh

set -eu

ROOT=$(CDPATH='' cd "$(dirname "$0")" && pwd)
export PYTHONDONTWRITEBYTECODE=1
case "${1:-}" in
    init|install)
        exec python3 "$ROOT/ci_manager/client.py" "$@"
        ;;
    submit|status|wait|pause|resume|cancel|recover|maintenance|service)
        exec "$HOME/.local/bin/cell-ci" "$@"
        ;;
esac
exec python3 "$ROOT/pipeline/select_changes.py" run "$@"
