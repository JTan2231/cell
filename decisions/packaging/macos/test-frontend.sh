#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
temporary=$(mktemp -d "${TMPDIR:-/tmp}/krisis-frontend.XXXXXX")
trap 'rm -rf "$temporary"' EXIT HUP INT TERM
home="$temporary/Home"
payload="$home/Library/Application Support/Decisions/install/current/libexec/krisis"
capture="$temporary/capture"
mkdir -p "$(dirname "$payload")"
cat >"$payload" <<'EOF'
#!/bin/sh
printf '%s\n' "$@" >"$KRISIS_TEST_CAPTURE"
EOF
chmod 0755 "$payload"
HOME="$home" KRISIS_TEST_CAPTURE="$capture" /bin/sh "$SCRIPT_DIR/krisis" observe status --date 2026-09-03
[ "$(tr '\n' ' ' <"$capture")" = 'observe status --date 2026-09-03 ' ]
printf '%s\n' 'Krisis frontend test passed'
