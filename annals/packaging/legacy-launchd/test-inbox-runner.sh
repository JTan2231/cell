#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
temporary=$(mktemp -d "${TMPDIR:-/tmp}/annals-inbox-runner.XXXXXX")
trap 'rm -rf "$temporary"' EXIT HUP INT TERM
release="$temporary/release"
capture="$temporary/capture"
mkdir -p "$release/bin" "$release/libexec"
cp "$SCRIPT_DIR/annals-inbox" "$release/bin/annals-inbox"
cat >"$release/libexec/annals" <<'EOF'
#!/bin/sh
set -eu
printf 'program=%s\n' "$0" >"$ANNALS_TEST_CAPTURE"
printf 'argc=%s\n' "$#" >>"$ANNALS_TEST_CAPTURE"
for argument in "$@"; do printf 'arg=%s\n' "$argument" >>"$ANNALS_TEST_CAPTURE"; done
EOF
chmod 0755 "$release/bin/annals-inbox" "$release/libexec/annals"
ANNALS_TEST_CAPTURE="$capture" "$release/bin/annals-inbox"
expected="program=$release/bin/../libexec/annals
argc=3
arg=--quiet
arg=inbox
arg=run"
[ "$(cat "$capture")" = "$expected" ]
