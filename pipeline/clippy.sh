#!/bin/sh
# Private shared Clippy body. The dispatcher supplies the selected Rust scope.
set -eu
set -f
PIPELINE_ROOT=$(CDPATH='' cd "$(dirname "$0")/.." && pwd)
export PIPELINE_ROOT
. "$PIPELINE_ROOT/pipeline/lib.sh"
cd "$PIPELINE_ROOT"

clippy_packages=
clippy_offline=0
clippy_keep_going=0
CARGO_PATH_PREFIX=
while [ "$#" -gt 0 ]; do
    [ "$#" -ge 2 ] \
        || pipeline_fail 'usage: clippy.sh [--product PRODUCT] [--shared-suite SUITE]'
    case "$1" in
        --product)
            pipeline_load_descriptor "$2"
            [ -n "${CARGO_PACKAGES:-}" ] \
                || pipeline_fail "$PRODUCT_ID declares no Cargo packages"
            clippy_packages="$clippy_packages
$CARGO_PACKAGES"
            case "$CARGO_OFFLINE" in
                1) clippy_offline=1 ;;
                0) ;;
                *) pipeline_fail "$PRODUCT_ID has invalid CARGO_OFFLINE" ;;
            esac
            case "$CLIPPY_KEEP_GOING" in
                1) clippy_keep_going=1 ;;
                0) ;;
                *) pipeline_fail "$PRODUCT_ID has invalid CLIPPY_KEEP_GOING" ;;
            esac
            pipeline_bootstrap_cargo
            ;;
        --shared-suite)
            case "$2" in
                install|maintenance|prompts)
                    clippy_packages="$clippy_packages
cell-$2"
                    ;;
                *) pipeline_fail "unknown shared Rust suite: $2" ;;
            esac
            ;;
        *) pipeline_fail "unknown Clippy scope option: $1" ;;
    esac
    shift 2
done
clippy_packages=$(printf '%s\n' $clippy_packages | LC_ALL=C sort -u)
[ -n "$clippy_packages" ] || pipeline_fail 'Clippy requires a selected Rust scope'
pipeline_bootstrap_cargo
case "$(rustc --version)" in
    "rustc 1.97.1 "*) ;;
    *) pipeline_fail 'Rust 1.97.1 is required' ;;
esac
case "$(cargo --version)" in
    "cargo 1.97.1 "*) ;;
    *) pipeline_fail 'Cargo 1.97.1 is required' ;;
esac

set -- cargo clippy --manifest-path "$PIPELINE_ROOT/Cargo.toml"
for clippy_package in $clippy_packages; do
    set -- "$@" --package "$clippy_package"
done
set -- "$@" --all-targets --locked
if [ "$clippy_keep_going" = 1 ]; then
    set -- "$@" --keep-going
fi
if [ "$clippy_offline" = 1 ]; then
    set -- "$@" --offline
    export CARGO_NET_OFFLINE=true
fi
export CARGO_BUILD_WARNINGS=deny
set -- "$@" -- \
    -F unsafe_code -D clippy::all -D clippy::pedantic \
    -D clippy::dbg_macro -D clippy::todo -D clippy::unimplemented \
    -D clippy::unwrap_used -D clippy::expect_used
printf '%s\n' '==> clippy for selected Rust packages'
exec "$@"
