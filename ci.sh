#!/bin/sh

set -eu

ROOT=$(CDPATH='' cd "$(dirname "$0")" && pwd)

usage() {
    printf '%s\n' 'Usage: ./ci.sh [nucleus|annals|todo|weaver]...'
}

if [ "$#" -eq 0 ]; then
    set -- nucleus annals todo weaver
fi

for project in "$@"; do
    case "$project" in
        nucleus|annals|todo|weaver) ;;
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

printf '%s\n' 'ci.sh: all selected project gates are green'
