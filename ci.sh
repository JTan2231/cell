#!/bin/sh

set -eu

ROOT=$(CDPATH='' cd "$(dirname "$0")" && pwd)

usage() {
    printf '%s\n' 'Usage: ./ci.sh [nucleus|annals|todo|chancery|weaver]...'
}

if [ "$#" -eq 0 ]; then
    set -- nucleus annals todo chancery weaver
fi

nucleus_selected=0
annals_selected=0
todo_selected=0
chancery_selected=0
weaver_selected=0
for project in "$@"; do
    case "$project" in
        nucleus) nucleus_selected=1 ;;
        annals) annals_selected=1 ;;
        todo) todo_selected=1 ;;
        chancery) chancery_selected=1 ;;
        weaver) weaver_selected=1 ;;
        *)
            usage >&2
            exit 2
            ;;
    esac
done

for project in "$@"; do
    printf '==> %s CI\n' "$project"
    "$ROOT/$project/ci.sh"
done

if [ "$nucleus_selected$annals_selected$todo_selected$chancery_selected$weaver_selected" = \
    11111 ]
then
    printf '%s\n' '==> integrated Chancery source catalog'
    (
        catalog_registry=$(mktemp -d "${TMPDIR:-/tmp}/cell-catalog.XXXXXX")
        catalog_registry=$(CDPATH='' cd "$catalog_registry" && pwd)
        cleanup_catalog_registry() {
            rm -rf "$catalog_registry"
        }
        trap cleanup_catalog_registry EXIT
        trap 'exit 1' HUP INT TERM

        ln -s "$ROOT/chancery/provider" "$catalog_registry/chancery"
        ln -s "$ROOT/nucleus/chancery" "$catalog_registry/nucleus"
        ln -s "$ROOT/annals/chancery/annals" "$catalog_registry/annals"
        ln -s "$ROOT/annals/chancery/annals-usage" \
            "$catalog_registry/annals-usage"
        ln -s "$ROOT/todo/chancery" "$catalog_registry/todo"
        ln -s "$ROOT/weaver/chancery" "$catalog_registry/weaver"

        chancery_candidate="$ROOT/target/release/chancery"
        [ -f "$chancery_candidate" ] && [ -x "$chancery_candidate" ] || {
            printf 'ci.sh: Chancery release candidate is unavailable: %s\n' \
                "$chancery_candidate" >&2
            exit 1
        }
        "$chancery_candidate" --registry "$catalog_registry" doctor
        "$chancery_candidate" --registry "$catalog_registry" --json list \
            >/dev/null
    )
fi

printf '%s\n' 'ci.sh: all selected project gates are green'
