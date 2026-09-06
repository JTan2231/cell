#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
temporary=$(mktemp -d "${TMPDIR:-/tmp}/annals-user-frontend-test.XXXXXX")

cleanup() {
    rm -rf "$temporary"
}
trap cleanup EXIT HUP INT TERM

home="$temporary/Operator Home"
state="$home/Library/Application Support/Annals"
payload="$state/install/current/libexec/annals"
capture="$temporary/capture"
mkdir -p "$(dirname "$payload")"

cat >"$payload" <<'EOF'
#!/bin/sh
printf 'config=%s\n' "${ANNALS_CONFIG-<unset>}" >"$ANNALS_TEST_CAPTURE"
printf 'library=%s\n' "${ANNALS_LIBRARY-<unset>}" >>"$ANNALS_TEST_CAPTURE"
for argument in "$@"; do
    printf 'argument=%s\n' "$argument" >>"$ANNALS_TEST_CAPTURE"
done
EOF
chmod 0755 "$payload"

(
    unset ANNALS_CONFIG ANNALS_LIBRARY
    HOME="$home" ANNALS_TEST_CAPTURE="$capture" \
        "$SCRIPT_DIR/annals-user" search 'two words'
)
grep -Fx "config=$state/config.toml" "$capture" >/dev/null
grep -Fx 'argument=search' "$capture" >/dev/null
grep -Fx 'argument=two words' "$capture" >/dev/null

(
    unset ANNALS_CONFIG ANNALS_LIBRARY
    HOME="$home" ANNALS_TEST_CAPTURE="$capture" \
        "$SCRIPT_DIR/annals-user" --library ./scratch.db stats
)
grep -Fx 'config=<unset>' "$capture" >/dev/null
grep -Fx 'argument=--library' "$capture" >/dev/null

(
    unset ANNALS_CONFIG ANNALS_LIBRARY
    HOME="$home" ANNALS_TEST_CAPTURE="$capture" ANNALS_CONFIG=./custom.toml \
        "$SCRIPT_DIR/annals-user" stats
)
grep -Fx 'config=./custom.toml' "$capture" >/dev/null

(
    unset ANNALS_CONFIG ANNALS_LIBRARY
    HOME="$home" ANNALS_TEST_CAPTURE="$capture" ANNALS_LIBRARY=./environment.db \
        "$SCRIPT_DIR/annals-user" stats
)
grep -Fx 'config=<unset>' "$capture" >/dev/null
grep -Fx 'library=./environment.db' "$capture" >/dev/null

printf '%s\n' 'user frontend test passed'
