#!/bin/sh
# Private body: the public root CI obtains the shared heavy lane first.
set -eu
PIPELINE_ROOT=$(CDPATH='' cd "$(dirname "$0")/.." && pwd)
export PIPELINE_ROOT
. "$PIPELINE_ROOT/pipeline/lib.sh"
CARGO_PATH_PREFIX=
pipeline_bootstrap_cargo
exec cargo run --manifest-path "$PIPELINE_ROOT/Cargo.toml" \
    --package usher --locked --quiet -- check "$PIPELINE_ROOT"
