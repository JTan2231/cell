#!/bin/sh

set -eu

PIPELINE_ROOT=$(CDPATH='' cd "$(dirname "$0")/.." && pwd)
export PIPELINE_ROOT
. "$PIPELINE_ROOT/pipeline/lib.sh"

integrated_fail() {
    printf 'ci.sh: %s\n' "$1" >&2
    exit 1
}

catalog_workspace=$(mktemp -d "${TMPDIR:-/tmp}/cell-catalog.XXXXXX")
catalog_workspace=$(CDPATH='' cd "$catalog_workspace" && pwd)
catalog_registry="$catalog_workspace/providers"
mkdir "$catalog_registry"
cleanup_catalog_registry() {
    rm -rf "$catalog_workspace"
}
trap cleanup_catalog_registry 0
trap 'exit 1' 1 2 15

catalog_expected_entries=0
while IFS= read -r product_id; do
    [ -n "$product_id" ] || continue
    pipeline_load_descriptor "$product_id"
    pipeline_validate_descriptor
    while IFS='|' read -r unit provider_id provider_dir provider_entries; do
        [ -n "$unit" ] || continue
        provider_selector="$catalog_registry/$provider_id"
        [ ! -e "$provider_selector" ] && [ ! -L "$provider_selector" ] \
            || integrated_fail "duplicate provider ID in source catalog: $provider_id"
        ln -s "$PIPELINE_ROOT/$provider_dir" "$provider_selector"
        catalog_expected_entries=$((catalog_expected_entries + provider_entries))
    done <<EOF
$PROVIDERS
EOF
done <<EOF
$(pipeline_products)
EOF

[ "$catalog_expected_entries" -eq "$PIPELINE_EXPECTED_PROVIDER_ENTRIES" ] \
    || integrated_fail "expected descriptor inventory of $PIPELINE_EXPECTED_PROVIDER_ENTRIES entries; found $catalog_expected_entries"

pipeline_bootstrap_cargo
command -v cargo >/dev/null 2>&1 \
    || integrated_fail 'required tool not found: cargo'
printf '%s\n' '==> exact-candidate Chancery release build'
CARGO_BUILD_WARNINGS=deny cargo build \
    --manifest-path "$PIPELINE_ROOT/Cargo.toml" \
    --package chancery --release --locked

chancery_candidate=$(pipeline_target_file release/chancery)
[ -f "$chancery_candidate" ] && [ -x "$chancery_candidate" ] \
    || integrated_fail "Chancery release candidate is unavailable: $chancery_candidate"
"$chancery_candidate" --registry "$catalog_registry" doctor
"$chancery_candidate" --registry "$catalog_registry" --json list >/dev/null

normalized_entries=0
for provider_path in "$catalog_registry"/*; do
    provider_id=${provider_path##*/}
    grep -Eq '"schema_version"[[:space:]]*:[[:space:]]*3' \
        "$provider_path/provider.json" \
        || integrated_fail "provider is not schema 3: $provider_id"
    grep -F '"promise_scope"' "$provider_path/provider.json" >/dev/null \
        || integrated_fail "provider has no promise scope: $provider_id"
    for entry_path in "$provider_path"/entries/*.json; do
        grep -F '"promise"' "$entry_path" >/dev/null \
            || integrated_fail "entry has no normalized promise: $entry_path"
        entry_id=$(awk -F '"' '
            /^[[:space:]]*"id"[[:space:]]*:/ { print $4; exit }
        ' "$entry_path")
        [ -n "$entry_id" ] \
            || integrated_fail "entry has no readable ID: $entry_path"
        resolution="$catalog_workspace/resolution-$normalized_entries.json"
        set +e
        "$chancery_candidate" --registry "$catalog_registry" \
            --json resolve "$entry_id" >"$resolution"
        resolution_status=$?
        set -e
        [ "$resolution_status" -le 1 ] \
            || integrated_fail "entry resolution failed structurally: $entry_id"
        if grep -Eq '"code":"(provider_scope_undeclared|provider_inventory_partial|facet_undeclared)"' \
            "$resolution"
        then
            integrated_fail "entry has undeclared promise coverage: $entry_id"
        fi
        grep -F '"dependency_closure_status":"complete"' "$resolution" >/dev/null \
            || integrated_fail "entry dependency closure is incomplete: $entry_id"
        grep -F '"issues":[]' "$resolution" >/dev/null \
            || integrated_fail "entry resolution has catalog issues: $entry_id"
        normalized_entries=$((normalized_entries + 1))
    done
done

[ "$normalized_entries" -eq "$catalog_expected_entries" ] \
    || integrated_fail "expected $catalog_expected_entries normalized entries; found $normalized_entries"

"$chancery_candidate" --registry "$catalog_registry" --json resolve \
    decisions.lifecycle.consume --require completeness_and_freshness >/dev/null

annals_resolution="$catalog_workspace/annals-usage-resolution.json"
set +e
"$chancery_candidate" --registry "$catalog_registry" --json resolve \
    annals-usage.consumption.inspect >"$annals_resolution"
annals_resolution_status=$?
set -e
[ "$annals_resolution_status" -eq 1 ] \
    || integrated_fail "Annals Usage resolution should report incomplete declaration; exit $annals_resolution_status"
grep -F '"status":"incomplete_declaration"' "$annals_resolution" >/dev/null
grep -F '"code":"uncontracted_reliance"' "$annals_resolution" >/dev/null

printf '%s\n' 'pipeline/integrated.sh: green'
