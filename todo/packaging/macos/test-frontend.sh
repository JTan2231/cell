#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
temporary=$(mktemp -d "${TMPDIR:-/tmp}/todo-frontend-test.XXXXXX")
cleanup() {
    rm -rf "$temporary"
}
trap cleanup EXIT HUP INT TERM

home="$temporary/Operator Home"
state="$home/Library/Application Support/Todo"
payload="$state/install/current/libexec/todo"
capture="$temporary/capture"
mkdir -p "$(dirname "$payload")"

cat >"$payload" <<'EOF'
#!/bin/sh
printf 'config=%s\n' "${TODO_CONFIG-<unset>}" >"$TODO_TEST_CAPTURE"
printf 'database=%s\n' "${TODO_DATABASE-<unset>}" >>"$TODO_TEST_CAPTURE"
for argument in "$@"; do
    printf 'argument=%s\n' "$argument" >>"$TODO_TEST_CAPTURE"
done
EOF
chmod 0755 "$payload"

(
    unset TODO_CONFIG TODO_DATABASE
    HOME="$home" TODO_TEST_CAPTURE="$capture" \
        "$SCRIPT_DIR/todo" search 'two words'
)
grep -Fx "config=$state/config.toml" "$capture" >/dev/null
grep -Fx 'argument=search' "$capture" >/dev/null
grep -Fx 'argument=two words' "$capture" >/dev/null

(
    unset TODO_CONFIG TODO_DATABASE
    HOME="$home" TODO_TEST_CAPTURE="$capture" \
        "$SCRIPT_DIR/todo" --database ./scratch.db list
)
grep -Fx 'config=<unset>' "$capture" >/dev/null
grep -Fx 'argument=--database' "$capture" >/dev/null

(
    unset TODO_CONFIG TODO_DATABASE
    HOME="$home" TODO_TEST_CAPTURE="$capture" TODO_CONFIG=./custom.toml \
        "$SCRIPT_DIR/todo" list
)
grep -Fx 'config=./custom.toml' "$capture" >/dev/null

(
    unset TODO_CONFIG TODO_DATABASE
    HOME="$home" TODO_TEST_CAPTURE="$capture" TODO_DATABASE=./environment.db \
        "$SCRIPT_DIR/todo" list
)
grep -Fx 'config=<unset>' "$capture" >/dev/null
grep -Fx 'database=./environment.db' "$capture" >/dev/null

printf '%s\n' 'frontend test passed'
