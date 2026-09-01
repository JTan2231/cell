#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
temporary=$(mktemp -d "${TMPDIR:-/tmp}/semantics-runner.XXXXXX")
trap 'rm -rf "$temporary"' EXIT HUP INT TERM
home="$temporary/Home"
mkdir -p "$home/.local/bin" "$home/Library/Application Support/Semantics"
if [ -x /opt/homebrew/bin/codex ]; then
    printf '%s\n' /opt/homebrew/bin/codex >"$home/expected-codex"
else
    printf '%s\n' "$home/.local/bin/codex" >"$home/expected-codex"
fi
for binary in codex decisions; do
    cat >"$home/.local/bin/$binary" <<'EOF'
#!/bin/sh
exit 0
EOF
    chmod 0755 "$home/.local/bin/$binary"
done
cat >"$home/.local/bin/semantics" <<'EOF'
#!/bin/sh
set -eu
[ "$#" -eq 3 ]
[ "$1" = --json ]
[ "$2" = intake ]
[ "$3" = run ]
[ "$PATH" = /usr/bin:/bin:/usr/sbin:/sbin ]
[ "$CONVERSATIONS_CODEX" = "$(cat "$HOME/expected-codex")" ]
[ "$SEMANTICS_DECISIONS" = "$HOME/.local/bin/decisions" ]
[ "$SEMANTICS_DATABASE" = "$HOME/Library/Application Support/Semantics/semantics.db" ]
env | LC_ALL=C sort >"$HOME/environment"
: >"$HOME/created"
printf '%s\n' '{"already_running":false,"events_seen":0}'
EOF
chmod 0755 "$home/.local/bin/semantics"
if ! HOME="$home" "$SCRIPT_DIR/semantics-worker" >"$temporary/stdout" 2>"$temporary/stderr"; then
    sed 's/^/worker test: /' "$temporary/stderr" >&2
    exit 1
fi
[ ! -s "$temporary/stderr" ]
grep -Fx '{"already_running":false,"events_seen":0}' "$temporary/stdout" >/dev/null
[ "$(stat -f '%Lp' "$home/created")" = 600 ]
grep -Fx "CONVERSATIONS_CODEX=$(cat "$home/expected-codex")" "$home/environment" >/dev/null
grep -Fx "SEMANTICS_DECISIONS=$home/.local/bin/decisions" "$home/environment" >/dev/null
grep -Fx "SEMANTICS_DATABASE=$home/Library/Application Support/Semantics/semantics.db" "$home/environment" >/dev/null
! grep -E 'TOKEN|KEY|SECRET|SSH|NPM|CARGO' "$home/environment" >/dev/null
rm -f "$home/.local/bin/decisions"
if HOME="$home" "$SCRIPT_DIR/semantics-worker" >/dev/null 2>"$temporary/missing"; then
    printf '%s\n' 'worker runner accepted a missing Decisions executable' >&2
    exit 1
fi
grep -F 'Decisions executable is unavailable' "$temporary/missing" >/dev/null
