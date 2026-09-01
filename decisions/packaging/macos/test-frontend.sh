#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
temporary=$(mktemp -d "${TMPDIR:-/tmp}/decisions-frontend.XXXXXX")
trap 'rm -rf "$temporary"' EXIT HUP INT TERM
home="$temporary/Home"
payload="$home/Library/Application Support/Decisions/install/current/libexec/decisions"
capture="$temporary/capture"
mkdir -p "$(dirname "$payload")"
cat >"$payload" <<'EOF'
#!/bin/sh
printf '%s\n' "$@" >"$DECISIONS_TEST_CAPTURE"
EOF
chmod 0755 "$payload"
HOME="$home" DECISIONS_TEST_CAPTURE="$capture" "$SCRIPT_DIR/decisions" daily preview --date 2026-08-31
[ "$(tr '\n' ' ' <"$capture")" = 'daily preview --date 2026-08-31 ' ]
printf '%s\n' 'frontend test passed'
