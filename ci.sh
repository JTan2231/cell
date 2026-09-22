#!/bin/sh

set -eu

ROOT=$(CDPATH='' cd "$(dirname "$0")" && pwd)
export PYTHONDONTWRITEBYTECODE=1

usage() {
    cat <<'EOF'
Usage: ./ci.sh submit COMMIT [--repo PATH] [--request-id KEY] [--deploy PRODUCT]...
       ./ci.sh status [JOB] | wait JOB [--timeout SECONDS]
       ./ci.sh pause | resume | cancel JOB | recover JOB
       ./ci.sh maintenance ACTION [--owner OWNER] | service ACTION
       ./ci.sh init --repo PATH --accepted-baseline COMMIT | install

CI runs through the installed cell-ci manager. Commit changes, then submit the
commit for integration, validation, bounded repair, deployment, and outcome email.
EOF
}

case "${1:-}" in
    -h|--help|help)
        usage
        exit 0
        ;;
    init|install)
        exec python3 "$ROOT/ci_manager/client.py" "$@"
        ;;
    submit|status|wait|pause|resume|cancel|recover|maintenance|service|--version)
        exec "$HOME/.local/bin/cell-ci" "$@"
        ;;
    *)
        printf '%s\n' 'ci.sh: submit a committed change with ./ci.sh submit COMMIT; direct validation is no longer supported.' >&2
        usage >&2
        exit 2
        ;;
esac
