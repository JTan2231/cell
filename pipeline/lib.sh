#!/bin/sh

# Shared helpers for the checked-in Cell pipeline descriptors. This file is
# sourced by the CI, release, generator, and self-test entry points.

PIPELINE_EXPECTED_PRODUCT_COUNT=14
PIPELINE_EXPECTED_PROVIDER_ENTRIES=55

pipeline_products() {
    for descriptor in "$PIPELINE_ROOT"/pipeline/products/*.sh; do
        descriptor_name=${descriptor##*/}
        printf '%s\n' "${descriptor_name%.sh}"
    done
}

pipeline_fail() {
    printf 'pipeline: %s\n' "$1" >&2
    exit 1
}

pipeline_clear_descriptor() {
    unset PIPELINE_SCHEMA PRODUCT_ID PRODUCT_NAME PRODUCT_DIR PRODUCT_ALIASES
    unset CI_GATE_ID
    unset CI_RESOURCE_CLASS RELEASE_BRANCH DEPLOY_PROFILE DEPLOY_CONFLICT_KEYS
    unset CARGO_MANIFEST CARGO_PACKAGES CARGO_OFFLINE CARGO_PATH_PREFIX
    unset CLIPPY_KEEP_GOING TEST_NO_FAIL_FAST
    unset CI_SHELL_CHECKS CI_RUN_CHECKS CI_PLIST_CHECKS
    unset CI_PROVIDER_VALIDATION_PHASE CI_EXTRA_BEFORE_RUST
    unset CI_EXTRA_AFTER_BUILD CI_BINARY_CHECKS
    unset RELEASE_UNITS RELEASE_ALLOW_EXPLICIT_UNIT RELEASE_USAGE
    unset RELEASE_METADATA_NO_DEPS RELEASE_BINARY_CHECKS PROVIDERS
}

pipeline_load_descriptor() {
    requested_product=$1
    case "$requested_product" in
        *[!a-z0-9-]*|'') pipeline_fail "invalid product ID: $requested_product" ;;
    esac

    descriptor_path="$PIPELINE_ROOT/pipeline/products/$requested_product.sh"
    [ -f "$descriptor_path" ] \
        || pipeline_fail "product descriptor not found: $requested_product"

    pipeline_clear_descriptor
    # Descriptors are trusted, checked-in shell data. Keeping them sourceable
    # avoids adding a parser, binary, or dependency to bootstrap CI.
    . "$descriptor_path"

    [ "${PIPELINE_SCHEMA:-}" = 1 ] \
        || pipeline_fail "unsupported descriptor schema: $requested_product"
    [ "${PRODUCT_ID:-}" = "$requested_product" ] \
        || pipeline_fail "descriptor identity mismatch: $requested_product"

    PRODUCT_ALIASES=${PRODUCT_ALIASES:-}
    CI_GATE_ID=${CI_GATE_ID:-$PRODUCT_ID}
    CARGO_MANIFEST=${CARGO_MANIFEST:-Cargo.toml}
    CARGO_OFFLINE=${CARGO_OFFLINE:-0}
    CARGO_PATH_PREFIX=${CARGO_PATH_PREFIX:-}
    CLIPPY_KEEP_GOING=${CLIPPY_KEEP_GOING:-1}
    TEST_NO_FAIL_FAST=${TEST_NO_FAIL_FAST:-1}
    CI_SHELL_CHECKS=${CI_SHELL_CHECKS:-}
    CI_RUN_CHECKS=${CI_RUN_CHECKS:-}
    CI_PLIST_CHECKS=${CI_PLIST_CHECKS:-}
    CI_PROVIDER_VALIDATION_PHASE=${CI_PROVIDER_VALIDATION_PHASE:-before-rust}
    CI_EXTRA_BEFORE_RUST=${CI_EXTRA_BEFORE_RUST:-}
    CI_EXTRA_AFTER_BUILD=${CI_EXTRA_AFTER_BUILD:-}
    CI_BINARY_CHECKS=${CI_BINARY_CHECKS:-}
    RELEASE_ALLOW_EXPLICIT_UNIT=${RELEASE_ALLOW_EXPLICIT_UNIT:-0}
    RELEASE_USAGE=${RELEASE_USAGE:-Usage: ./release.sh --patch|--minor|--major}
    RELEASE_METADATA_NO_DEPS=${RELEASE_METADATA_NO_DEPS:-1}
    RELEASE_BINARY_CHECKS=${RELEASE_BINARY_CHECKS:-}
    PROVIDERS=${PROVIDERS:-}
}

pipeline_unit_field() {
    unit_name=$1
    field_number=$2
    printf '%s\n' "$RELEASE_UNITS" | awk -F '|' \
        -v unit="$unit_name" -v field="$field_number" '
            $1 == unit { print $field; found = 1; exit }
            END { if (!found) exit 1 }
        '
}

pipeline_default_unit() {
    printf '%s\n' "$RELEASE_UNITS" | awk -F '|' '
        $6 == "1" { print $1; found = 1; exit }
        END { if (!found) exit 1 }
    '
}

pipeline_read_version() {
    version_kind=$1
    version_manifest=$2
    case "$version_kind" in
        package)
            version_heading='[package]'
            ;;
        workspace-package)
            version_heading='[workspace.package]'
            ;;
        *)
            pipeline_fail "unsupported version source: $version_kind"
            ;;
    esac

    awk -v heading="$version_heading" '
        $0 == heading { in_section = 1; next }
        in_section && /^\[/ { exit }
        in_section && /^[[:space:]]*version[[:space:]]*=/ {
            value = $0
            sub(/^[^=]*=[[:space:]]*"/, "", value)
            sub(/"[[:space:]]*$/, "", value)
            print value
            exit
        }
    ' "$version_manifest"
}

pipeline_provider_release() {
    awk -F '"' '/"release"[[:space:]]*:/ { print $4; exit }' "$1"
}

pipeline_bootstrap_cargo() {
    if [ -n "$CARGO_PATH_PREFIX" ]; then
        PATH="$CARGO_PATH_PREFIX:$PATH"
        export PATH
    fi
    if ! command -v cargo >/dev/null 2>&1; then
        cargo_home=${CARGO_HOME:-}
        if [ -z "$cargo_home" ] && [ -n "${HOME:-}" ]; then
            cargo_home="$HOME/.cargo"
        fi
        if [ -n "$cargo_home" ] && [ -x "$cargo_home/bin/cargo" ]; then
            PATH="$cargo_home/bin:$PATH"
            export PATH
        fi
    fi
}

pipeline_target_file() {
    target_relative_path=$1
    if [ -n "${CARGO_TARGET_DIR:-}" ]; then
        printf '%s/%s\n' "$CARGO_TARGET_DIR" "$target_relative_path"
        return
    fi

    git_common_dir=$(git -C "$PIPELINE_ROOT" rev-parse --git-common-dir 2>/dev/null) \
        || pipeline_fail 'unable to resolve the Git common directory'
    case "$git_common_dir" in
        /*) ;;
        *) git_common_dir="$PIPELINE_ROOT/$git_common_dir" ;;
    esac
    git_common_dir=$(CDPATH='' cd "$git_common_dir" && pwd)
    primary_checkout=$(dirname "$git_common_dir")
    printf '%s/target/%s\n' "$primary_checkout" "$target_relative_path"
}

pipeline_should_run() {
    run_condition=$1
    case "$run_condition" in
        always) return 0 ;;
        darwin) [ "$(uname -s)" = Darwin ] ;;
        darwin-if-tool:*)
            required_tool=${run_condition#darwin-if-tool:}
            [ "$(uname -s)" = Darwin ] \
                && command -v "$required_tool" >/dev/null 2>&1
            ;;
        *) pipeline_fail "unsupported run condition: $run_condition" ;;
    esac
}

pipeline_validate_descriptor() {
    [ -n "${PRODUCT_NAME:-}" ] || pipeline_fail "$PRODUCT_ID has no display name"
    [ -n "${PRODUCT_DIR:-}" ] || pipeline_fail "$PRODUCT_ID has no directory"
    case "${CI_RESOURCE_CLASS:-}" in heavy|light) ;; \
        *) pipeline_fail "$PRODUCT_ID has invalid CI resource class" ;; esac
    [ "${RELEASE_BRANCH:-}" = main ] \
        || pipeline_fail "$PRODUCT_ID has unsupported release branch: ${RELEASE_BRANCH:-}"
    case "${DEPLOY_PROFILE:-}" in selector-only-v1|custom) ;; \
        *) pipeline_fail "$PRODUCT_ID has invalid deployment profile" ;; esac
    [ -n "${DEPLOY_CONFLICT_KEYS:-}" ] \
        || pipeline_fail "$PRODUCT_ID declares no deployment conflict keys"
    [ -d "$PIPELINE_ROOT/$PRODUCT_DIR" ] \
        || pipeline_fail "$PRODUCT_ID directory is missing: $PRODUCT_DIR"
    [ -f "$PIPELINE_ROOT/$CARGO_MANIFEST" ] \
        || pipeline_fail "$PRODUCT_ID Cargo manifest is missing: $CARGO_MANIFEST"
    [ -n "${CARGO_PACKAGES:-}" ] \
        || pipeline_fail "$PRODUCT_ID declares no Cargo packages"
    [ -n "${RELEASE_UNITS:-}" ] \
        || pipeline_fail "$PRODUCT_ID declares no release units"
    pipeline_default_unit >/dev/null \
        || pipeline_fail "$PRODUCT_ID has no default release unit"

    while IFS='|' read -r unit display kind manifest tag_prefix is_default; do
        [ -n "$unit" ] || continue
        [ -n "$display" ] && [ -n "$tag_prefix" ] \
            || pipeline_fail "$PRODUCT_ID has an incomplete release unit: $unit"
        case "$kind" in package|workspace-package) ;; \
            *) pipeline_fail "$PRODUCT_ID has an invalid version source: $kind" ;; esac
        [ -f "$PIPELINE_ROOT/$manifest" ] \
            || pipeline_fail "$PRODUCT_ID version manifest is missing: $manifest"
        case "$is_default" in 0|1) ;; \
            *) pipeline_fail "$PRODUCT_ID release unit has invalid default flag: $unit" ;; esac
    done <<EOF
$RELEASE_UNITS
EOF

    while IFS='|' read -r unit provider_id provider_dir expected_entries; do
        [ -n "$unit" ] || continue
        pipeline_unit_field "$unit" 1 >/dev/null \
            || pipeline_fail "$PRODUCT_ID provider references unknown unit: $unit"
        [ -n "$provider_id" ] \
            || pipeline_fail "$PRODUCT_ID provider has no ID"
        [ -f "$PIPELINE_ROOT/$provider_dir/provider.json" ] \
            || pipeline_fail "$PRODUCT_ID provider is missing: $provider_dir"
        case "$expected_entries" in
            ''|*[!0-9]*) pipeline_fail "$PRODUCT_ID provider has invalid entry count: $provider_id" ;;
        esac
    done <<EOF
$PROVIDERS
EOF

    while IFS='|' read -r shell_name script_path; do
        [ -n "$shell_name" ] || continue
        case "$shell_name" in sh|zsh) ;; \
            *) pipeline_fail "$PRODUCT_ID has unsupported shell: $shell_name" ;; esac
        [ -f "$PIPELINE_ROOT/$script_path" ] \
            || pipeline_fail "$PRODUCT_ID shell input is missing: $script_path"
    done <<EOF
$CI_SHELL_CHECKS
EOF

    while IFS='|' read -r run_condition script_path; do
        [ -n "$run_condition" ] || continue
        [ -x "$PIPELINE_ROOT/$script_path" ] \
            || pipeline_fail "$PRODUCT_ID check is not executable: $script_path"
    done <<EOF
$CI_RUN_CHECKS
EOF

    for extra_path in "$CI_EXTRA_BEFORE_RUST" "$CI_EXTRA_AFTER_BUILD"; do
        [ -z "$extra_path" ] || [ -x "$PIPELINE_ROOT/$extra_path" ] \
            || pipeline_fail "$PRODUCT_ID CI extension is not executable: $extra_path"
    done
}
