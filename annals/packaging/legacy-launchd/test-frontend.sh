#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
SOURCE_FRONTEND="$SCRIPT_DIR/annals"
SOURCE_USAGE_FRONTEND="$SCRIPT_DIR/annals-usage"
temporary=$(mktemp -d "${TMPDIR:-/tmp}/annals-frontend-test.XXXXXX")

cleanup() {
    rm -rf "$temporary"
}
trap cleanup EXIT HUP INT TERM

payload="$temporary/payload"
frontend="$temporary/annals"
capture="$temporary/capture"

sed "s|^PAYLOAD=.*|PAYLOAD='$payload'|" "$SOURCE_FRONTEND" >"$frontend"
chmod 0755 "$frontend"

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
    ANNALS_TEST_CAPTURE=$capture "$frontend" search 'two words'
)
grep -Fx 'config=/Library/Application Support/Annals/config.toml' "$capture" >/dev/null
grep -Fx 'argument=search' "$capture" >/dev/null
grep -Fx 'argument=two words' "$capture" >/dev/null

(
    unset ANNALS_CONFIG ANNALS_LIBRARY
    ANNALS_TEST_CAPTURE=$capture "$frontend" --library ./scratch.db stats
)
grep -Fx 'config=<unset>' "$capture" >/dev/null
grep -Fx 'argument=--library' "$capture" >/dev/null

(
    unset ANNALS_CONFIG ANNALS_LIBRARY
    ANNALS_TEST_CAPTURE=$capture ANNALS_CONFIG=./custom.toml \
        "$frontend" stats
)
grep -Fx 'config=./custom.toml' "$capture" >/dev/null

(
    unset ANNALS_CONFIG ANNALS_LIBRARY
    ANNALS_TEST_CAPTURE=$capture ANNALS_LIBRARY=./environment.db \
        "$frontend" stats
)
grep -Fx 'config=<unset>' "$capture" >/dev/null
grep -Fx 'library=./environment.db' "$capture" >/dev/null

usage_frontend="$temporary/annals-usage"
sed "s|^PAYLOAD=.*|PAYLOAD='$payload'|" \
    "$SOURCE_USAGE_FRONTEND" >"$usage_frontend"
chmod 0755 "$usage_frontend"

cat >"$payload" <<'EOF'
#!/bin/sh
printf 'usage_config=%s\n' "${ANNALS_USAGE_CONFIG-<unset>}" >"$ANNALS_TEST_CAPTURE"
printf 'annals_config=%s\n' "${ANNALS_CONFIG-<unset>}" >>"$ANNALS_TEST_CAPTURE"
for argument in "$@"; do
    printf 'argument=%s\n' "$argument" >>"$ANNALS_TEST_CAPTURE"
done
EOF
chmod 0755 "$payload"

(
    unset ANNALS_CONFIG ANNALS_USAGE_CONFIG
    ANNALS_TEST_CAPTURE=$capture "$usage_frontend" report --limit 2
)
grep -Fx 'usage_config=/Library/Application Support/Annals/usage.toml' \
    "$capture" >/dev/null
grep -Fx 'argument=report' "$capture" >/dev/null

(
    unset ANNALS_CONFIG ANNALS_USAGE_CONFIG
    ANNALS_TEST_CAPTURE=$capture "$usage_frontend" report --config ./usage.toml
)
grep -Fx 'usage_config=<unset>' "$capture" >/dev/null

(
    unset ANNALS_CONFIG ANNALS_USAGE_CONFIG
    ANNALS_TEST_CAPTURE=$capture ANNALS_USAGE_CONFIG=./environment-usage.toml \
        "$usage_frontend" budget
)
grep -Fx 'usage_config=./environment-usage.toml' "$capture" >/dev/null

(
    unset ANNALS_CONFIG ANNALS_USAGE_CONFIG
    ANNALS_TEST_CAPTURE=$capture ANNALS_CONFIG=./annals.toml \
        "$usage_frontend" app-server --stdio
)
grep -Fx 'usage_config=<unset>' "$capture" >/dev/null
grep -Fx 'annals_config=./annals.toml' "$capture" >/dev/null
