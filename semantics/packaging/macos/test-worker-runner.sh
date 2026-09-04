#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
temporary=$(mktemp -d "${TMPDIR:-/tmp}/semantics-runner.XXXXXX")
trap 'rm -rf "$temporary"' EXIT HUP INT TERM
home="$temporary/Home"
release="$temporary/release"
mkdir -p "$home/.local/bin" "$home/Library/Application Support/Semantics" \
    "$home/Library/Application Support/Annals/decisions" \
    "$release/bin" "$release/libexec"
: >"$home/Library/Application Support/Annals/decisions/config.toml"
cp "$SCRIPT_DIR/semantics-worker" "$release/bin/semantics-worker"
if [ -x /opt/homebrew/bin/codex ]; then
    printf '%s\n' /opt/homebrew/bin/codex >"$home/expected-codex"
else
    printf '%s\n' "$home/.local/bin/codex" >"$home/expected-codex"
fi
for binary in codex annals; do
    cat >"$home/.local/bin/$binary" <<'EOF'
#!/bin/sh
exit 0
EOF
    chmod 0755 "$home/.local/bin/$binary"
done
cat >"$release/libexec/semantics" <<'EOF'
#!/bin/sh
set -eu
[ "$#" -eq 3 ]
[ "$1" = --json ]
[ "$2" = intake ]
[ "$3" = run ]
[ "$PATH" = /usr/bin:/bin:/usr/sbin:/sbin ]
[ "$CONVERSATIONS_CODEX" = "$(cat "$HOME/expected-codex")" ]
[ "$SEMANTICS_ANNALS" = "$HOME/.local/bin/annals" ]
[ "$SEMANTICS_ANNALS_CONFIG" = "$HOME/Library/Application Support/Annals/decisions/config.toml" ]
[ "$SEMANTICS_DATABASE" = "$HOME/Library/Application Support/Semantics/semantics.db" ]
env | LC_ALL=C sort >"$HOME/environment"
: >"$HOME/created"
printf '%s\n' '{"already_running":false,"events_seen":0}'
EOF
chmod 0755 "$release/bin/semantics-worker" "$release/libexec/semantics"
if ! HOME="$home" "$release/bin/semantics-worker" >"$temporary/stdout" 2>"$temporary/stderr"; then
    sed 's/^/worker test: /' "$temporary/stderr" >&2
    exit 1
fi
[ ! -s "$temporary/stderr" ]
grep -Fx '{"already_running":false,"events_seen":0}' "$temporary/stdout" >/dev/null
[ "$(stat -f '%Lp' "$home/created")" = 600 ]
grep -Fx "CONVERSATIONS_CODEX=$(cat "$home/expected-codex")" "$home/environment" >/dev/null
grep -Fx "SEMANTICS_ANNALS=$home/.local/bin/annals" "$home/environment" >/dev/null
grep -Fx "SEMANTICS_ANNALS_CONFIG=$home/Library/Application Support/Annals/decisions/config.toml" "$home/environment" >/dev/null
grep -Fx "SEMANTICS_DATABASE=$home/Library/Application Support/Semantics/semantics.db" "$home/environment" >/dev/null
! grep -E 'TOKEN|KEY|SECRET|SSH|NPM|CARGO' "$home/environment" >/dev/null

maintenance="$home/Library/Application Support/Semantics/.clockwork-maintenance"
: >"$maintenance"
chmod 0600 "$maintenance"
rm -f "$home/created"
HOME="$home" "$release/bin/semantics-worker" \
    >"$temporary/maintenance.stdout" 2>"$temporary/maintenance.stderr"
[ ! -e "$home/created" ]
grep -F 'maintenance gate is active' "$temporary/maintenance.stderr" >/dev/null
chmod 0644 "$maintenance"
if HOME="$home" "$release/bin/semantics-worker" \
    >/dev/null 2>"$temporary/private-maintenance"; then
    printf '%s\n' 'worker runner accepted a non-private maintenance gate' >&2
    exit 1
fi
grep -F 'maintenance gate is not private' "$temporary/private-maintenance" >/dev/null
chmod 0600 "$maintenance"
ln "$maintenance" "$temporary/maintenance-link"
if HOME="$home" "$release/bin/semantics-worker" \
    >/dev/null 2>"$temporary/linked-maintenance"; then
    printf '%s\n' 'worker runner accepted a hard-linked maintenance gate' >&2
    exit 1
fi
grep -F 'maintenance gate is not private' "$temporary/linked-maintenance" >/dev/null
rm -f "$temporary/maintenance-link"
rm -f "$maintenance"
mkdir "$maintenance"
if HOME="$home" "$release/bin/semantics-worker" >/dev/null 2>"$temporary/invalid-maintenance"; then
    printf '%s\n' 'worker runner accepted an invalid maintenance gate' >&2
    exit 1
fi
grep -F 'maintenance gate is invalid' "$temporary/invalid-maintenance" >/dev/null
rmdir "$maintenance"

rm -f "$home/.local/bin/annals"
if HOME="$home" "$release/bin/semantics-worker" >/dev/null 2>"$temporary/missing"; then
    printf '%s\n' 'worker runner accepted a missing Annals executable' >&2
    exit 1
fi
grep -F 'Annals executable is unavailable' "$temporary/missing" >/dev/null
