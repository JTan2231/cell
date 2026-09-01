#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
temporary=$(mktemp -d "${TMPDIR:-/tmp}/semantics-frontend.XXXXXX")
trap 'rm -rf "$temporary"' EXIT HUP INT TERM
home="$temporary/Home"
mkdir -p "$home/Library/Application Support/Semantics/install/current/libexec"
cat >"$home/Library/Application Support/Semantics/install/current/libexec/semantics" <<'EOF'
#!/bin/sh
printf 'argc=%s\n' "$#"
for argument in "$@"; do printf 'arg=%s\n' "$argument"; done
EOF
chmod 0755 "$home/Library/Application Support/Semantics/install/current/libexec/semantics"
output=$(HOME="$home" "$SCRIPT_DIR/semantics" repository show sample --revision 3)
expected='argc=5
arg=repository
arg=show
arg=sample
arg=--revision
arg=3'
[ "$output" = "$expected" ]
