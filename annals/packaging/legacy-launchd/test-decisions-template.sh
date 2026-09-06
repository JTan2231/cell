#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
config_template="$SCRIPT_DIR/annals-decisions.toml.in"
schedule_template="$SCRIPT_DIR/annals-decisions-inbox.clockwork.toml.in"
runner="$SCRIPT_DIR/annals-inbox"
temporary=$(mktemp -d "${TMPDIR:-/tmp}/annals-decisions-template.XXXXXX")
trap 'rm -rf "$temporary"' EXIT HUP INT TERM

for source in "$config_template" "$schedule_template" "$runner"; do
    [ -f "$source" ] || {
        printf 'missing decisions-library packaging source: %s\n' "$source" >&2
        exit 1
    }
done

state="$temporary/Library/Application Support/Annals/decisions"
logs="$state/log"
release="$temporary/release"
home="$temporary/home"
mkdir -p "$logs" "$release/bin" "$release/libexec" "$home"

library_id=0123456789abcdef0123456789abcdef
nucleus_socket="$temporary/nucleus.sock"
sed \
    -e "s|__ANNALS_DECISIONS_STATE__|$state|g" \
    -e "s|__ANNALS_DECISIONS_LIBRARY_ID__|$library_id|g" \
    -e "s|__NUCLEUS_SOCKET__|$nucleus_socket|g" \
    "$config_template" >"$temporary/config.toml"

release_id=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
interpreter_hash=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
runner_hash=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
sed \
    -e "s|__RELEASE_ID__|$release_id|g" \
    -e "s|__RELEASE_ROOT__|$release|g" \
    -e "s|__INTERPRETER_SHA256__|$interpreter_hash|g" \
    -e "s|__RUNNER_SHA256__|$runner_hash|g" \
    -e "s|__ANNALS_DECISIONS_STATE__|$state|g" \
    -e "s|__ANNALS_DECISIONS_LOGS__|$logs|g" \
    -e "s|__ANNALS_HOME__|$home|g" \
    -e 's|__ANNALS_USER__|annals-test|g' \
    "$schedule_template" >"$temporary/schedule.toml"

if grep -E '__[A-Z0-9_]+__' "$temporary/config.toml" "$temporary/schedule.toml" >/dev/null; then
    printf '%s\n' 'unresolved decisions-library packaging placeholder' >&2
    exit 1
fi

grep -Fx "library = \"$state/annals.db\"" "$temporary/config.toml" >/dev/null
grep -Fx "root = \"$state/spool\"" "$temporary/config.toml" >/dev/null
grep -Fx "expected_library_id = \"$library_id\"" "$temporary/config.toml" >/dev/null
grep -Fx "nucleus_socket = \"$nucleus_socket\"" "$temporary/config.toml" >/dev/null

grep -Fx 'schema_version = 1' "$temporary/schedule.toml" >/dev/null
grep -Fx 'key = "annals/decisions-inbox"' "$temporary/schedule.toml" >/dev/null
grep -Fx "cwd = \"$state\"" "$temporary/schedule.toml" >/dev/null
grep -Fx 'seconds = 300' "$temporary/schedule.toml" >/dev/null
grep -Fx 'run_at_load = true' "$temporary/schedule.toml" >/dev/null
grep -Fx "ANNALS_CONFIG = \"$state/config.toml\"" "$temporary/schedule.toml" >/dev/null
grep -Fx "stdout = \"$logs/inbox.stdout.log\"" "$temporary/schedule.toml" >/dev/null
grep -Fx "stderr = \"$logs/inbox.stderr.log\"" "$temporary/schedule.toml" >/dev/null
if grep -F 'key = "annals/inbox"' "$temporary/schedule.toml" >/dev/null; then
    printf '%s\n' 'decisions schedule reused the primary binding key' >&2
    exit 1
fi

cp "$runner" "$release/bin/annals-inbox"
cat >"$release/libexec/annals" <<'EOF'
#!/bin/sh
set -eu
printf 'config=%s\n' "$ANNALS_CONFIG" >"$ANNALS_TEST_CAPTURE"
printf 'args=%s\n' "$*" >>"$ANNALS_TEST_CAPTURE"
EOF
chmod 0755 "$release/bin/annals-inbox" "$release/libexec/annals"
ANNALS_CONFIG="$state/config.toml" ANNALS_TEST_CAPTURE="$temporary/capture" \
    "$release/bin/annals-inbox"
expected="config=$state/config.toml
args=--quiet inbox run"
[ "$(cat "$temporary/capture")" = "$expected" ]
