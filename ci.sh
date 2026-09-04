#!/bin/sh

set -eu

ROOT=$(CDPATH='' cd "$(dirname "$0")" && pwd)

usage() {
    printf '%s\n' \
        'Usage: ./ci.sh [nucleus|annals|todo|chancery|weaver|email|conversations|krisis|decisions|semantics|geste|pratica|clockwork|crm]...'
}

if [ "$#" -eq 0 ]; then
    set -- nucleus annals todo chancery weaver email conversations krisis \
        semantics geste pratica clockwork crm
fi

nucleus_selected=0
annals_selected=0
todo_selected=0
chancery_selected=0
weaver_selected=0
email_selected=0
conversations_selected=0
krisis_selected=0
semantics_selected=0
geste_selected=0
pratica_selected=0
clockwork_selected=0
crm_selected=0
for project in "$@"; do
    case "$project" in
        nucleus) nucleus_selected=1 ;;
        annals) annals_selected=1 ;;
        todo) todo_selected=1 ;;
        chancery) chancery_selected=1 ;;
        weaver) weaver_selected=1 ;;
        email) email_selected=1 ;;
        conversations) conversations_selected=1 ;;
        krisis|decisions) krisis_selected=1 ;;
        semantics) semantics_selected=1 ;;
        geste) geste_selected=1 ;;
        pratica) pratica_selected=1 ;;
        clockwork) clockwork_selected=1 ;;
        crm) crm_selected=1 ;;
        *) usage >&2; exit 2 ;;
    esac
done

# These checks are read-only and do not consume the shared Cargo lane. Run them
# before binding the exact source candidate used by every selected product.
"$ROOT/pipeline/test.sh"
source_key=$(python3 "$ROOT/ci_broker/client.py" source-key --repo-root "$ROOT")
CELL_CI_EXPECTED_SOURCE_KEY=$source_key
export CELL_CI_EXPECTED_SOURCE_KEY

for project in "$@"; do
    printf '==> %s CI\n' "$project"
    case "$project" in
        krisis) "$ROOT/decisions/ci.sh" ;;
        *) "$ROOT/$project/ci.sh" ;;
    esac
done

if [ "$nucleus_selected$annals_selected$todo_selected$chancery_selected$weaver_selected$email_selected$conversations_selected$krisis_selected$semantics_selected$geste_selected$pratica_selected$clockwork_selected$crm_selected" = \
    1111111111111 ]
then
    printf '%s\n' '==> integrated Chancery source catalog'
    python3 "$ROOT/ci_broker/client.py" run \
        --repo-root "$ROOT" --gate cell.integrated --lane heavy -- \
        "$ROOT/pipeline/integrated.sh"
fi

observed_source_key=$(python3 "$ROOT/ci_broker/client.py" \
    source-key --repo-root "$ROOT")
if [ "$observed_source_key" != "$source_key" ]; then
    printf '%s\n' \
        'ci.sh: source changed while the root plan was running; results are stale' >&2
    exit 75
fi
unset CELL_CI_EXPECTED_SOURCE_KEY

printf '%s\n' 'ci.sh: all selected project gates are green'
