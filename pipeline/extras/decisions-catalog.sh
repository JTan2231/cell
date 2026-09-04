#!/bin/sh

set -eu

workspace_manifest="$PIPELINE_ROOT/Cargo.toml"
registry=$(mktemp -d "${TMPDIR:-/tmp}/cell-decisions-catalog.XXXXXX")
cleanup() {
    rm -rf "$registry"
}
trap cleanup 0
trap 'exit 129' 1
trap 'exit 130' 2
trap 'exit 143' 15

ln -s "$PIPELINE_ROOT/decisions/chancery" "$registry/krisis"
ln -s "$PIPELINE_ROOT/decisions/chancery-legacy" "$registry/decisions"
ln -s "$PIPELINE_ROOT/conversations/chancery" "$registry/conversations"
ln -s "$PIPELINE_ROOT/nucleus/chancery" "$registry/nucleus"
ln -s "$PIPELINE_ROOT/annals/chancery/annals" "$registry/annals"
ln -s "$PIPELINE_ROOT/clockwork/chancery" "$registry/clockwork"
ln -s "$PIPELINE_ROOT/semantics/chancery" "$registry/semantics"

catalog=$(cargo run --manifest-path "$workspace_manifest" \
    --package chancery --locked --quiet -- \
    --registry "$registry" --json list)
case "$catalog" in
    *'"id":"krisis.decision.capture"'*) ;;
    *) printf '%s\n' 'Krisis capture missing from catalog' >&2; exit 1 ;;
esac
case "$catalog" in
    *'"id":"decisions.lifecycle.consume"'*) ;;
    *) printf '%s\n' 'lifecycle stream missing from catalog' >&2; exit 1 ;;
esac

resolution=$(cargo run --manifest-path "$workspace_manifest" \
    --package chancery --locked --quiet -- \
    --registry "$registry" --json resolve krisis.decision.capture \
    --require completeness_and_freshness || true)
case "$resolution" in
    *'"status":"incomplete_declaration"'*) ;;
    *) printf '%s\n' 'Krisis capture resolution did not preserve declared gaps' >&2; exit 1 ;;
esac
case "$resolution" in
    *'"code":"promise_unspecified"'*) ;;
    *) printf '%s\n' 'Krisis capture resolution lost its explicit non-guarantees' >&2; exit 1 ;;
esac
