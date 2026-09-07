#!/bin/sh

set -eu

PIPELINE_ROOT=$(CDPATH='' cd "$(dirname "$0")/.." && pwd)
export PIPELINE_ROOT
. "$PIPELINE_ROOT/pipeline/lib.sh"
cd "$PIPELINE_ROOT"

for script_path in \
    ci.sh \
    pipeline/lib.sh \
    pipeline/ci.sh \
    pipeline/release.sh \
    pipeline/generate.sh \
    pipeline/test.sh \
    pipeline/check.sh \
    pipeline/integrated.sh \
    pipeline/recognition.sh \
    pipeline/platform.sh \
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
printf '%s\n' 'pipeline/check.sh: structure checks passed'
