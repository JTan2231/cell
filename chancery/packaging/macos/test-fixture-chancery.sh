#!/bin/sh

set -eu

case "${1:-}" in
    --version) printf '%s\n' 'chancery @VERSION@' ;;
    --help) printf '%s\n' 'fixture Chancery help' ;;
    validate)
        [ -d "${2:-}" ] || exit 1
        printf '%s\n' 'Valid provider fixture'
        ;;
    marker) printf '%s\n' '@MARKER@' ;;
    *) exit 2 ;;
esac
