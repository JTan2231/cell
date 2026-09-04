#!/bin/sh

set -eu

workspace_manifest="$PIPELINE_ROOT/Cargo.toml"
registry=$(mktemp -d "${TMPDIR:-/tmp}/cell-semantics-catalog.XXXXXX")
cleanup() {
    rm -rf "$registry"
}
trap cleanup 0
trap 'exit 129' 1
trap 'exit 130' 2
trap 'exit 143' 15

ln -s "$PIPELINE_ROOT/semantics/chancery" "$registry/semantics"
ln -s "$PIPELINE_ROOT/annals/chancery/annals" "$registry/annals"
ln -s "$PIPELINE_ROOT/conversations/chancery" "$registry/conversations"
ln -s "$PIPELINE_ROOT/nucleus/chancery" "$registry/nucleus"
ln -s "$PIPELINE_ROOT/clockwork/chancery" "$registry/clockwork"
ln -s "$PIPELINE_ROOT/chancery/provider" "$registry/chancery"
ln -s "$PIPELINE_ROOT/email/chancery" "$registry/email"

catalog=$(cargo run --manifest-path "$workspace_manifest" \
    --package chancery --locked --offline --quiet -- \
    --registry "$registry" --json list)
for entry_id in \
    semantics.repository.explore \
    semantics.project.operate \
    semantics.develop.change
do
    case "$catalog" in
        *"\"id\":\"$entry_id\""*) ;;
        *) printf 'catalog entry missing: %s\n' "$entry_id" >&2; exit 1 ;;
    esac
done

for entry_id in semantics.project.operate semantics.develop.change; do
    resolution=$(cargo run --manifest-path "$workspace_manifest" \
        --package chancery --locked --offline --quiet -- \
        --registry "$registry" --json resolve "$entry_id") || true
    case "$resolution" in
        *'"dependency_closure_status":"complete"'*) ;;
        *) printf 'dependency closure is incomplete: %s\n' "$entry_id" >&2; exit 1 ;;
    esac
    case "$resolution" in
        *'"id":"clockwork.schedule.operate"'*) ;;
        *) printf 'Clockwork schedule dependency is absent: %s\n' "$entry_id" >&2; exit 1 ;;
    esac
    case "$resolution" in
        *'"issues":[]'*) ;;
        *) printf 'dependency compatibility failed: %s\n' "$entry_id" >&2; exit 1 ;;
    esac
done
