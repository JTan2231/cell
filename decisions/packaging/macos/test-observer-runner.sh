#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
temporary=$(mktemp -d "${TMPDIR:-/tmp}/krisis-observer.XXXXXX")
trap 'rm -rf "$temporary"' EXIT HUP INT TERM
home="$temporary/Home"
capture="$home/capture"
state="$home/Library/Application Support/Decisions"
maintenance="$state/.clockwork-maintenance"
release="$temporary/release"
annals="$temporary/annals"
config="$temporary/config.toml"
mkdir -p "$home/.local/bin" "$state" "$release/bin" "$release/libexec"
cp "$SCRIPT_DIR/krisis-observer" "$release/bin/krisis-observer"
cat >"$release/libexec/krisis" <<'EOF'
#!/bin/sh
if [ -f "$HOME/fail" ]; then
    printf '%s\n' "private failure at $HOME/Library/Application Support/Decisions/decisions.db" >&2
    exit 42
fi
env | sort >"$HOME/capture"
printf '%s\n' "$@" >>"$HOME/capture"
EOF
cat >"$annals" <<'EOF'
#!/bin/sh
exit 0
EOF
cat >"$home/.local/bin/codex" <<'EOF'
#!/bin/sh
exit 0
EOF
: >"$config"
chmod 0755 "$release/bin/krisis-observer" "$release/libexec/krisis" "$annals" "$home/.local/bin/codex"
HOME="$home" KRISIS_ANNALS_BINARY="$annals" KRISIS_ANNALS_CONFIG="$config" \
    KRISIS_ANNALS_LIBRARY_ID=0123456789abcdef0123456789abcdef \
    SECRET_MUST_NOT_LEAK=value RESEND_API_KEY=must-not-leak \
    /bin/sh "$release/bin/krisis-observer"
grep -Fx 'observe' "$capture" >/dev/null
grep -Fx 'process' "$capture" >/dev/null
grep -Fx "KRISIS_DATABASE=$home/Library/Application Support/Decisions/decisions.db" "$capture" >/dev/null
grep -Fx "KRISIS_ANNALS_BINARY=$annals" "$capture" >/dev/null
grep -Fx "KRISIS_ANNALS_CONFIG=$config" "$capture" >/dev/null
grep -Fx 'KRISIS_ANNALS_LIBRARY_ID=0123456789abcdef0123456789abcdef' "$capture" >/dev/null
grep -F 'CONVERSATIONS_CODEX=' "$capture" >/dev/null
! grep -F 'SECRET_MUST_NOT_LEAK=' "$capture" >/dev/null
! grep -F 'RESEND_API_KEY=' "$capture" >/dev/null

: >"$home/fail"
if HOME="$home" KRISIS_ANNALS_BINARY="$annals" KRISIS_ANNALS_CONFIG="$config" \
    KRISIS_ANNALS_LIBRARY_ID=0123456789abcdef0123456789abcdef \
    /bin/sh "$release/bin/krisis-observer" >"$temporary/failure.out" 2>"$temporary/failure.err"; then
    printf '%s\n' 'observer hid a failed Krisis process' >&2
    exit 1
fi
[ ! -s "$temporary/failure.out" ]
[ "$(cat "$temporary/failure.err")" = 'krisis observer: processing failed' ]
! grep -F "$home" "$temporary/failure.err" >/dev/null
rm -f "$home/fail"

: >"$maintenance"
chmod 0600 "$maintenance"
rm -f "$capture"
HOME="$home" KRISIS_ANNALS_BINARY="$annals" KRISIS_ANNALS_CONFIG="$config" \
    KRISIS_ANNALS_LIBRARY_ID=0123456789abcdef0123456789abcdef \
    /bin/sh "$release/bin/krisis-observer" >"$temporary/gate.out" 2>"$temporary/gate.err"
[ ! -e "$capture" ]
grep -F 'maintenance gate is active' "$temporary/gate.err" >/dev/null
chmod 0644 "$maintenance"
if HOME="$home" KRISIS_ANNALS_BINARY="$annals" KRISIS_ANNALS_CONFIG="$config" \
    KRISIS_ANNALS_LIBRARY_ID=0123456789abcdef0123456789abcdef \
    /bin/sh "$release/bin/krisis-observer" >/dev/null 2>"$temporary/private.err"; then
    printf '%s\n' 'observer accepted a non-private maintenance gate' >&2
    exit 1
fi
grep -F 'maintenance gate is not private' "$temporary/private.err" >/dev/null
printf '%s\n' 'Krisis observer runner test passed'
