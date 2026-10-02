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
        python3 "$PIPELINE_ROOT/pipeline/test_generate.py" -q
        python3 "$PIPELINE_ROOT/pipeline/test_parallel_tests.py" -q
        python3 "$PIPELINE_ROOT/pipeline/test_autofix.py" -q
        python3 "$PIPELINE_ROOT/pipeline/test_autofix_dispatch.py" -q
        python3 "$PIPELINE_ROOT/pipeline/test_release_build.py" -q
        python3 "$PIPELINE_ROOT/pipeline/test_ci_notification.py" -q
        python3 -m unittest -q deployment.test_inventory deployment.test_signing deployment.test_candidate deployment.test_build deployment.test_cli ci_manager.test_signing ci_manager.test_installation ci_manager.test_integrations ci_manager.test_autofix ci_manager.test_worktree_cleanup ci_manager.test_recovery
        ;;
    install|maintenance|prompts)
        CARGO_PATH_PREFIX=
        pipeline_bootstrap_cargo
        package=cell-$suite
        if [ -z "${CELL_CI_AUTOFIX_PATCH:-}" ]; then
            cargo fmt --manifest-path "$PIPELINE_ROOT/Cargo.toml" --package "$package" -- --check
        fi
        if [ "$checks_only" = 0 ]; then
            sh "$PIPELINE_ROOT/pipeline/clippy.sh" --shared-suite "$suite"
            cargo test --manifest-path "$PIPELINE_ROOT/Cargo.toml" --package "$package" --locked --all-targets
        fi
        ;;
    catalog) "$PIPELINE_ROOT/pipeline/integrated.sh" ;;
    *) pipeline_fail "unknown platform suite: $suite" ;;
esac
printf 'ci: platform %s passed\n' "$suite"
