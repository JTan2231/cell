#!/bin/sh

set -eu

PIPELINE_ROOT=$(CDPATH='' cd "$(dirname "$0")/.." && pwd)
export PIPELINE_ROOT
. "$PIPELINE_ROOT/pipeline/lib.sh"

ci_fail() {
    printf 'ci.sh: %s\n' "$1" >&2
    exit 1
}

ci_packages() {
    printf '%s\n' "$CARGO_PACKAGES"
}

ci_check_tools() {
    pipeline_bootstrap_cargo
    for tool in cargo rustc; do
        command -v "$tool" >/dev/null 2>&1 \
            || ci_fail "required tool not found: $tool"
    done
    case "$(rustc --version)" in
        "rustc 1.97.1 "*) ;;
        *) ci_fail "Rust 1.97.1 is required; found $(rustc --version)" ;;
    esac
    case "$(cargo --version)" in
        "cargo 1.97.1 "*) ;;
        *) ci_fail "Cargo 1.97.1 is required; found $(cargo --version)" ;;
    esac
}

ci_shell_and_packaging() {
    printf '%s\n' '==> shell and packaging'
    while IFS='|' read -r shell_name script_path; do
        [ -n "$shell_name" ] || continue
        absolute_path="$PIPELINE_ROOT/$script_path"
        [ -f "$absolute_path" ] || ci_fail "missing $script_path"
        case "$shell_name" in
            sh) sh -n "$absolute_path" ;;
            zsh) /bin/zsh -n "$absolute_path" ;;
            *) ci_fail "unsupported shell for $script_path: $shell_name" ;;
        esac
    done <<EOF
$CI_SHELL_CHECKS
EOF

    while IFS='|' read -r run_condition script_path; do
        [ -n "$run_condition" ] || continue
        if pipeline_should_run "$run_condition"; then
            "$PIPELINE_ROOT/$script_path"
        fi
    done <<EOF
$CI_RUN_CHECKS
EOF

    while IFS='|' read -r run_condition plist_mode plist_path; do
        [ -n "$run_condition" ] || continue
        if pipeline_should_run "$run_condition"; then
            case "$plist_mode" in
                lint)
                    plutil -lint "$PIPELINE_ROOT/$plist_path" >/dev/null
                    ;;
                convert)
                    plutil -convert binary1 -o /dev/null -- \
                        "$PIPELINE_ROOT/$plist_path"
                    ;;
                *) ci_fail "unsupported plist check: $plist_mode" ;;
            esac
        fi
    done <<EOF
$CI_PLIST_CHECKS
EOF
}

ci_check_provider_versions() {
    while IFS='|' read -r unit provider_id provider_dir expected_entries; do
        [ -n "$unit" ] || continue
        version_kind=$(pipeline_unit_field "$unit" 3)
        version_manifest=$(pipeline_unit_field "$unit" 4)
        package_version=$(pipeline_read_version "$version_kind" \
            "$PIPELINE_ROOT/$version_manifest")
        provider_version=$(pipeline_provider_release \
            "$PIPELINE_ROOT/$provider_dir/provider.json")
        [ -n "$package_version" ] && [ "$provider_version" = "$package_version" ] \
            || ci_fail "$provider_id provider release $provider_version does not match package version $package_version"
    done <<EOF
$PROVIDERS
EOF
}

ci_validate_providers() {
    printf '%s\n' '==> Chancery provider bundles'
    while IFS='|' read -r unit provider_id provider_dir expected_entries; do
        [ -n "$unit" ] || continue
        set -- cargo run --manifest-path "$PIPELINE_ROOT/Cargo.toml" \
            --package chancery --locked
        if [ "$CARGO_OFFLINE" = 1 ]; then
            set -- "$@" --offline
        fi
        set -- "$@" --quiet -- validate "$PIPELINE_ROOT/$provider_dir"
        "$@"
    done <<EOF
$PROVIDERS
EOF
}

ci_run_extra() {
    extra_path=$1
    [ -z "$extra_path" ] || "$PIPELINE_ROOT/$extra_path"
}

ci_fmt() {
    printf '%s\n' '==> rustfmt'
    set -- cargo fmt --manifest-path "$PIPELINE_ROOT/$CARGO_MANIFEST"
    while IFS= read -r cargo_package; do
        [ -n "$cargo_package" ] || continue
        set -- "$@" --package "$cargo_package"
    done <<EOF
$(ci_packages)
EOF
    set -- "$@" -- --check
    "$@"
}

ci_clippy() {
    printf '%s\n' '==> clippy'
    set -- cargo clippy --manifest-path "$PIPELINE_ROOT/$CARGO_MANIFEST"
    while IFS= read -r cargo_package; do
        [ -n "$cargo_package" ] || continue
        set -- "$@" --package "$cargo_package"
    done <<EOF
$(ci_packages)
EOF
    set -- "$@" --all-targets --locked
    if [ "$CLIPPY_KEEP_GOING" = 1 ]; then
        set -- "$@" --keep-going
    fi
    if [ "$CARGO_OFFLINE" = 1 ]; then
        set -- "$@" --offline
    fi
    set -- "$@" -- \
        -D warnings \
        -F unsafe_code \
        -D clippy::all \
        -D clippy::pedantic \
        -D clippy::dbg_macro \
        -D clippy::todo \
        -D clippy::unimplemented \
        -D clippy::unwrap_used \
        -D clippy::expect_used
    "$@"
}

ci_test() {
    printf '%s\n' '==> tests'
    set -- cargo test --manifest-path "$PIPELINE_ROOT/$CARGO_MANIFEST"
    while IFS= read -r cargo_package; do
        [ -n "$cargo_package" ] || continue
        set -- "$@" --package "$cargo_package"
    done <<EOF
$(ci_packages)
EOF
    set -- "$@" --locked
    if [ "$TEST_NO_FAIL_FAST" = 1 ]; then
        set -- "$@" --no-fail-fast
    fi
    if [ "$CARGO_OFFLINE" = 1 ]; then
        set -- "$@" --offline
    fi
    "$@"
}

ci_doc() {
    printf '%s\n' '==> rustdoc'
    set -- cargo doc --manifest-path "$PIPELINE_ROOT/$CARGO_MANIFEST"
    while IFS= read -r cargo_package; do
        [ -n "$cargo_package" ] || continue
        set -- "$@" --package "$cargo_package"
    done <<EOF
$(ci_packages)
EOF
    set -- "$@" --no-deps --locked
    if [ "$CARGO_OFFLINE" = 1 ]; then
        set -- "$@" --offline
    fi
    RUSTDOCFLAGS='-D warnings' "$@"
}

ci_build() {
    printf '%s\n' '==> release build'
    set -- cargo build --manifest-path "$PIPELINE_ROOT/$CARGO_MANIFEST"
    while IFS= read -r cargo_package; do
        [ -n "$cargo_package" ] || continue
        set -- "$@" --package "$cargo_package"
    done <<EOF
$(ci_packages)
EOF
    set -- "$@" --release --locked
    if [ "$CARGO_OFFLINE" = 1 ]; then
        set -- "$@" --offline
    fi
    "$@"
}

ci_check_binaries() {
    while IFS='|' read -r unit binary_path command_name; do
        [ -n "$unit" ] || continue
        version_kind=$(pipeline_unit_field "$unit" 3)
        version_manifest=$(pipeline_unit_field "$unit" 4)
        expected_version=$(pipeline_read_version "$version_kind" \
            "$PIPELINE_ROOT/$version_manifest")
        case "$binary_path" in
            target/*)
                absolute_binary=$(pipeline_target_file "${binary_path#target/}")
                ;;
            *) absolute_binary="$PIPELINE_ROOT/$binary_path" ;;
        esac
        reported_version=$("$absolute_binary" --version) \
            || ci_fail "unable to read $command_name release binary version"
        [ "$reported_version" = "$command_name $expected_version" ] \
            || ci_fail "$command_name reported an unexpected version: $reported_version"
    done <<EOF
$CI_BINARY_CHECKS
EOF
}

[ "$#" -ge 1 ] || pipeline_fail 'usage: pipeline/ci.sh PRODUCT'
product_id=$1
pipeline_load_descriptor "$product_id"
pipeline_validate_descriptor

ci_check_tools
[ -f "$PIPELINE_ROOT/Cargo.toml" ] \
    || ci_fail 'root workspace Cargo.toml is required'
[ -f "$PIPELINE_ROOT/Cargo.lock" ] \
    || ci_fail 'root Cargo.lock is required for reproducible builds'

export CARGO_BUILD_WARNINGS=deny
if [ "$CARGO_OFFLINE" = 1 ]; then
    export CARGO_NET_OFFLINE=true
fi
export CELL_PIPELINE_PRODUCT="$PRODUCT_ID"
cd "$PIPELINE_ROOT/$PRODUCT_DIR"

ci_shell_and_packaging
ci_check_provider_versions
if [ "$CI_PROVIDER_VALIDATION_PHASE" = before-rust ]; then
    ci_validate_providers
fi
ci_run_extra "$CI_EXTRA_BEFORE_RUST"
ci_fmt
ci_clippy
ci_test
if [ "$CI_PROVIDER_VALIDATION_PHASE" = after-tests ]; then
    ci_validate_providers
fi
ci_doc
ci_build
ci_run_extra "$CI_EXTRA_AFTER_BUILD"
ci_check_binaries

printf '%s\n' 'ci.sh: green'
