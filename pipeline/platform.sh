#!/bin/sh
# Private platform body. The dispatcher selects a suite and obtains admission.
set -eu
PIPELINE_ROOT=$(CDPATH='' cd "$(dirname "$0")/.." && pwd)
export PIPELINE_ROOT PYTHONDONTWRITEBYTECODE=1
. "$PIPELINE_ROOT/pipeline/lib.sh"
cd "$PIPELINE_ROOT"
[ "$#" -ge 1 ] && [ "$#" -le 2 ] \
    || pipeline_fail 'usage: pipeline/platform.sh SUITE [--checks-only]'
suite=$1
checks_only=0
if [ "$#" -eq 2 ]; then
    [ "$2" = --checks-only ] \
        || pipeline_fail 'usage: pipeline/platform.sh SUITE [--checks-only]'
    case "$suite" in install|maintenance|prompts) ;; \
        *) pipeline_fail 'checks-only requires a shared Rust suite' ;; esac
    checks_only=1
fi
case "$suite" in
    pipeline)
        "$PIPELINE_ROOT/pipeline/check.sh"
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
        python3 "$PIPELINE_ROOT/pipeline/test_parallel_tests.py" -q
        python3 "$PIPELINE_ROOT/pipeline/test_nextest_tool.py" -q
        python3 "$PIPELINE_ROOT/pipeline/test_ci_entry.py" -q
        python3 "$PIPELINE_ROOT/pipeline/test_ci_budget.py" -q
        python3 "$PIPELINE_ROOT/pipeline/test_ci_notification.py" -q
        python3 -m unittest -q ci_manager.test_integrations
        ;;
    broker) python3 -m unittest -q ci_broker.test_broker ;;
    deployment) python3 -m unittest -q deployment.test_coordinator ;;
    build) python3 -m unittest -q deployment.test_build ;;
    cleanup) python3 -m unittest -q deployment.test_cleanup ;;
    install|maintenance|prompts)
        CARGO_PATH_PREFIX=
        pipeline_bootstrap_cargo
        package=cell-$suite
        cargo fmt --manifest-path "$PIPELINE_ROOT/Cargo.toml" --package "$package" -- --check
        if [ "$checks_only" = 0 ]; then
            sh "$PIPELINE_ROOT/pipeline/clippy.sh" --shared-suite "$suite"
            cargo test --manifest-path "$PIPELINE_ROOT/Cargo.toml" --package "$package" --locked
        fi
        ;;
    catalog) "$PIPELINE_ROOT/pipeline/integrated.sh" ;;
    *) pipeline_fail "unknown platform suite: $suite" ;;
esac
printf 'ci: platform %s passed\n' "$suite"
