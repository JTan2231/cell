#!/bin/sh
# Private body: the public root CI obtains the shared heavy lane first.
set -eu
PIPELINE_ROOT=$(CDPATH='' cd "$(dirname "$0")/.." && pwd)
export PIPELINE_ROOT
. "$PIPELINE_ROOT/pipeline/lib.sh"
CARGO_PATH_PREFIX=
pipeline_bootstrap_cargo
printf '%s\n' '==> shared deployment admission primitive'
cargo fmt --manifest-path "$PIPELINE_ROOT/Cargo.toml" \
    --package cell-maintenance -- --check
cargo clippy --manifest-path "$PIPELINE_ROOT/Cargo.toml" \
    --package cell-maintenance --all-targets --locked -- \
    -D warnings -F unsafe_code -D clippy::all -D clippy::pedantic \
    -D clippy::dbg_macro -D clippy::todo -D clippy::unimplemented \
    -D clippy::unwrap_used -D clippy::expect_used
cargo test --manifest-path "$PIPELINE_ROOT/Cargo.toml" \
    --package cell-maintenance --locked
exec cargo run --manifest-path "$PIPELINE_ROOT/Cargo.toml" \
    --package usher --locked --quiet -- check "$PIPELINE_ROOT"
