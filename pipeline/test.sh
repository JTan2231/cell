#!/bin/sh

set -eu

PIPELINE_ROOT=$(CDPATH='' cd "$(dirname "$0")/.." && pwd)
export PIPELINE_ROOT
. "$PIPELINE_ROOT/pipeline/lib.sh"
cd "$PIPELINE_ROOT"

for script_path in \
    pipeline/lib.sh \
    pipeline/ci.sh \
    pipeline/release.sh \
    pipeline/generate.sh \
    pipeline/test.sh \
    pipeline/integrated.sh \
    pipeline/extras/deployment-generated.sh \
    pipeline/extras/decisions-catalog.sh \
    pipeline/extras/semantics-catalog.sh \
    pipeline/extras/todo-catalog.sh
do
    sh -n "$PIPELINE_ROOT/$script_path"
done

product_count=0
provider_entry_count=0
while IFS= read -r product_id; do
    [ -n "$product_id" ] || continue
    pipeline_load_descriptor "$product_id"
    pipeline_validate_descriptor
    sh -n "$PIPELINE_ROOT/pipeline/products/$product_id.sh"
    sh -n "$PIPELINE_ROOT/$PRODUCT_DIR/ci.sh" \
        "$PIPELINE_ROOT/$PRODUCT_DIR/release.sh"
    set +e
    "$PIPELINE_ROOT/$PRODUCT_DIR/release.sh" >/dev/null 2>&1
    release_status=$?
    set -e
    [ "$release_status" -eq 2 ] \
        || pipeline_fail "$product_id release usage should exit 2; found $release_status"
    product_count=$((product_count + 1))

    while IFS='|' read -r unit provider_id provider_dir expected_entries; do
        [ -n "$unit" ] || continue
        set -- "$PIPELINE_ROOT/$provider_dir"/entries/*.json
        [ -f "$1" ] \
            || pipeline_fail "$provider_id has no entry manifests"
        [ "$#" -eq "$expected_entries" ] \
            || pipeline_fail "$provider_id expected $expected_entries entries; found $#"
        provider_entry_count=$((provider_entry_count + expected_entries))
    done <<EOF
$PROVIDERS
EOF
done <<EOF
$(pipeline_products)
EOF

[ "$product_count" -eq "$PIPELINE_EXPECTED_PRODUCT_COUNT" ] \
    || pipeline_fail "expected $PIPELINE_EXPECTED_PRODUCT_COUNT migrated products; found $product_count"
[ "$provider_entry_count" -eq "$PIPELINE_EXPECTED_PROVIDER_ENTRIES" ] \
    || pipeline_fail "expected $PIPELINE_EXPECTED_PROVIDER_ENTRIES provider entries; found $provider_entry_count"

"$PIPELINE_ROOT/pipeline/generate.sh" --check
"$PIPELINE_ROOT/pipeline/generate.sh" --check \
    --product nucleus --product crm
python3 "$PIPELINE_ROOT/deployment/generate.py" --check
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest -q ci_broker.test_broker
printf '%s\n' 'pipeline/test.sh: green'
