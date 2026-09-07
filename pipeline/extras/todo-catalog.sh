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

catalog_fail() {
    printf '%s\n' "$3" | python3 -c '
import json
import sys

message, field = sys.argv[1:]
try:
    data = json.load(sys.stdin)["data"]
    if "entries" in data:
        entry = next((item for item in data["entries"]
                      if item.get("id") == "todo.concern.capture-and-route"), None)
    else:
        entry = data.get("entry")
    observed = entry.get(field, "<missing field>") if entry else "<missing entry>"
    if not isinstance(observed, str):
        observed = "<invalid field type>"
    rendered = json.dumps(observed, ensure_ascii=True)
    if len(rendered) > 240:
        rendered = rendered[:240] + "... (truncated)"
except (ValueError, KeyError, TypeError, AttributeError, RecursionError):
    rendered = "<invalid catalog JSON>"
print(f"ci.sh: {message}; observed {field}={rendered}", file=sys.stderr)
' "$1" "$2"
    exit 1
}

ln -s "$PIPELINE_ROOT/todo/chancery" "$registry/todo"
ln -s "$PIPELINE_ROOT/nucleus/chancery" "$registry/nucleus"

catalog=$(cargo run --manifest-path "$workspace_manifest" \
    --package chancery --locked --quiet -- \
    --registry "$registry" --json list)
case "$catalog" in
    *'"id":"todo.concern.capture-and-route"'*) ;;
    *)
        catalog_fail 'Todo catalog does not contain concern capture' id "$catalog"
        ;;
esac
case "$catalog" in
    *'"title":"Save and research a concern for later"'*) ;;
    *)
        catalog_fail 'Todo catalog omits the concern-capture title' title "$catalog"
        ;;
esac
case "$catalog" in
    *'"summary":"Save a concern and its source. Research a pending proposal to attach it, create or revise a todo, unify duplicates, defer or dismiss it."'*) ;;
    *)
        catalog_fail 'Todo catalog omits the concern-capture summary' summary "$catalog"
        ;;
esac

shown=$(cargo run --manifest-path "$workspace_manifest" \
    --package chancery --locked --quiet -- \
    --registry "$registry" --json show todo.concern.capture-and-route)
case "$shown" in
    *'"id":"todo.concern.capture-and-route"'*) ;;
    *)
        catalog_fail 'Todo concern-capture contract cannot be shown' id "$shown"
        ;;
esac
printf '%s\n' 'Todo catalog regression passed'
