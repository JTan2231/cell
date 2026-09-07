#!/bin/sh

set -eu

ROOT=$(CDPATH='' cd "$(dirname "$0")" && pwd)
PIPELINE_ROOT=$ROOT
export PIPELINE_ROOT
. "$ROOT/pipeline/lib.sh"

for option in "$@"; do
    case "$option" in
        -h|--help) exec python3 "$ROOT/pipeline/select_changes.py" plan --help ;;
    esac
done

plan=$(python3 "$ROOT/pipeline/select_changes.py" plan "$@")
# The helper emits only fixed modes, hashes, a count, and validated product IDs.
# Disable pathname expansion while splitting those tokens; never evaluate them.
set -f
set -- $plan
mode=$1
verbose=$2
source_key=$3
status_key=$4
product_count=$5
shift 5
case "$verbose" in
    verbose) verbose=--verbose ;;
    quiet) verbose= ;;
esac
scope=${*:-root-only}
skipped=$((product_count - $#))
CELL_CI_EXPECTED_SOURCE_KEY=$source_key
export CELL_CI_EXPECTED_SOURCE_KEY

"$ROOT/pipeline/test.sh" --quiet-result $verbose
python3 "$ROOT/ci_broker/client.py" run --quiet-result $verbose \
    --repo-root "$ROOT" --gate cell.recognition --lane heavy -- \
    "$ROOT/pipeline/recognition.sh"

for project in "$@"; do
    pipeline_load_descriptor "$project"
    "$ROOT/$PRODUCT_DIR/ci.sh" --quiet-result $verbose
done

if [ "$mode" = all ]; then
    scope=all
    python3 "$ROOT/ci_broker/client.py" run --quiet-result $verbose \
        --repo-root "$ROOT" --gate cell.integrated --lane heavy -- \
        "$ROOT/pipeline/integrated.sh"
fi

python3 "$ROOT/pipeline/select_changes.py" check "$source_key" "$status_key"
unset CELL_CI_EXPECTED_SOURCE_KEY

printf 'ci: passed; mode=%s; scope=%s; skipped=%s\n' "$mode" "$scope" "$skipped"
