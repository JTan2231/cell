#!/bin/sh
# Private platform body. The dispatcher selects a suite and obtains admission.
set -eu
PIPELINE_ROOT=$(CDPATH='' cd "$(dirname "$0")/.." && pwd)
export PIPELINE_ROOT PYTHONDONTWRITEBYTECODE=1
. "$PIPELINE_ROOT/pipeline/lib.sh"
cd "$PIPELINE_ROOT"
[ "$#" -eq 1 ] || pipeline_fail 'usage: pipeline/platform.sh SUITE'
suite=$1
case "$suite" in
    pipeline)
        "$PIPELINE_ROOT/pipeline/check.sh"
        "$PIPELINE_ROOT/pipeline/generate.sh" --check --product nucleus --product crm
        for product_id in $(pipeline_products); do
            pipeline_load_descriptor "$product_id"
            set +e
            "$PIPELINE_ROOT/$PRODUCT_DIR/release.sh" >/dev/null 2>&1
            release_status=$?
            set -e
            [ "$release_status" -eq 2 ] \
                || pipeline_fail "$product_id release usage should exit 2; found $release_status"
        done
        python3 "$PIPELINE_ROOT/pipeline/test_release.py" -q
        python3 "$PIPELINE_ROOT/pipeline/test_select_changes.py" -q
        python3 "$PIPELINE_ROOT/pipeline/test_todo_catalog.py" -q
        ;;
    broker) python3 -m unittest -q ci_broker.test_broker ;;
    deployment) python3 -m unittest -q deployment.test_coordinator ;;
    build) python3 -m unittest -q deployment.test_build ;;
    cleanup) python3 -m unittest -q deployment.test_cleanup ;;
    install|maintenance)
        CARGO_PATH_PREFIX=
        pipeline_bootstrap_cargo
        package=cell-$suite
        cargo fmt --manifest-path "$PIPELINE_ROOT/Cargo.toml" --package "$package" -- --check
        cargo clippy --manifest-path "$PIPELINE_ROOT/Cargo.toml" \
            --package "$package" --all-targets --locked -- \
            -D warnings -F unsafe_code -D clippy::all -D clippy::pedantic \
            -D clippy::dbg_macro -D clippy::todo -D clippy::unimplemented \
            -D clippy::unwrap_used -D clippy::expect_used
        cargo test --manifest-path "$PIPELINE_ROOT/Cargo.toml" --package "$package" --locked
        ;;
    catalog) "$PIPELINE_ROOT/pipeline/integrated.sh" ;;
    *) pipeline_fail "unknown platform suite: $suite" ;;
esac
printf 'ci: platform %s passed\n' "$suite"
