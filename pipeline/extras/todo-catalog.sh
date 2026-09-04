#!/bin/sh

set -eu

workspace_manifest="$PIPELINE_ROOT/Cargo.toml"
registry=$(mktemp -d "${TMPDIR:-/tmp}/cell-todo-catalog.XXXXXX")
cleanup() {
    rm -rf "$registry"
}
trap cleanup 0
trap 'exit 129' 1
trap 'exit 130' 2
trap 'exit 143' 15

ln -s "$PIPELINE_ROOT/todo/chancery" "$registry/todo"
ln -s "$PIPELINE_ROOT/nucleus/chancery" "$registry/nucleus"

catalog=$(cargo run --manifest-path "$workspace_manifest" \
    --package chancery --locked --quiet -- \
    --registry "$registry" --json list)
case "$catalog" in
    *'"id":"todo.concern.capture-and-route"'*) ;;
    *)
        printf 'ci.sh: Todo catalog does not contain concern capture: %s\n' \
            "$catalog" >&2
        exit 1
        ;;
esac
case "$catalog" in
    *'"title":"Save and research a concern for later"'*) ;;
    *)
        printf 'ci.sh: Todo catalog omits the concern-capture title: %s\n' \
            "$catalog" >&2
        exit 1
        ;;
esac
case "$catalog" in
    *'"summary":"Save one actionable concern with its source, then research a pending proposal to attach it, create or revise a todo, unify duplicates, defer it, or dismiss it."'*) ;;
    *)
        printf 'ci.sh: Todo catalog omits the concern-capture summary: %s\n' \
            "$catalog" >&2
        exit 1
        ;;
esac

shown=$(cargo run --manifest-path "$workspace_manifest" \
    --package chancery --locked --quiet -- \
    --registry "$registry" --json show todo.concern.capture-and-route)
case "$shown" in
    *'"id":"todo.concern.capture-and-route"'*) ;;
    *)
        printf 'ci.sh: Todo concern-capture contract cannot be shown: %s\n' \
            "$shown" >&2
        exit 1
        ;;
esac
printf '%s\n' 'Todo catalog regression passed'
