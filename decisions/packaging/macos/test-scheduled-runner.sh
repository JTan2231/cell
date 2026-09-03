#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
temporary=$(mktemp -d "${TMPDIR:-/tmp}/decisions-runner.XXXXXX")
trap 'rm -rf "$temporary"' EXIT HUP INT TERM
home="$temporary/Home"
capture="$home/capture"
state="$home/Library/Application Support/Decisions"
maintenance="$state/.clockwork-maintenance"
release="$temporary/release"
mkdir -p "$home/.local/bin" "$state" "$release/bin" "$release/libexec"
cp "$SCRIPT_DIR/decisions-daily-email" "$release/bin/decisions-daily-email"
cat >"$release/libexec/decisions" <<'EOF'
#!/bin/sh
env | sort >"$HOME/capture"
printf '%s\n' "$@" >>"$HOME/capture"
EOF
chmod 0755 "$release/bin/decisions-daily-email" "$release/libexec/decisions"
HOME="$home" SECRET_MUST_NOT_LEAK=value \
    /bin/sh "$release/bin/decisions-daily-email"
grep -Fx 'daily' "$capture" >/dev/null
grep -Fx 'run' "$capture" >/dev/null
grep -Fx -- '--scheduled' "$capture" >/dev/null
grep -Fx "DECISIONS_DATABASE=$home/Library/Application Support/Decisions/decisions.db" "$capture" >/dev/null
grep -F 'CONVERSATIONS_CODEX=' "$capture" >/dev/null
! grep -F 'SECRET_MUST_NOT_LEAK=' "$capture" >/dev/null
! grep -F 'RESEND_API_KEY=' "$capture" >/dev/null

: >"$maintenance"
chmod 0600 "$maintenance"
rm -f "$capture"
HOME="$home" /bin/sh "$release/bin/decisions-daily-email" \
    >"$temporary/maintenance.stdout" 2>"$temporary/maintenance.stderr"
[ ! -e "$capture" ]
grep -F 'maintenance gate is active' "$temporary/maintenance.stderr" >/dev/null
chmod 0644 "$maintenance"
if HOME="$home" /bin/sh "$release/bin/decisions-daily-email" \
    >/dev/null 2>"$temporary/private-maintenance.stderr"; then
    printf '%s\n' 'daily runner accepted a non-private maintenance gate' >&2
    exit 1
fi
grep -F 'maintenance gate is not private' "$temporary/private-maintenance.stderr" >/dev/null
chmod 0600 "$maintenance"
ln "$maintenance" "$temporary/maintenance-link"
if HOME="$home" /bin/sh "$release/bin/decisions-daily-email" \
    >/dev/null 2>"$temporary/linked-maintenance.stderr"; then
    printf '%s\n' 'daily runner accepted a hard-linked maintenance gate' >&2
    exit 1
fi
grep -F 'maintenance gate is not private' "$temporary/linked-maintenance.stderr" >/dev/null
rm -f "$temporary/maintenance-link"
rm -f "$maintenance"
mkdir "$maintenance"
if HOME="$home" /bin/sh "$release/bin/decisions-daily-email" \
    >/dev/null 2>"$temporary/invalid-maintenance.stderr"; then
    printf '%s\n' 'daily runner accepted an invalid maintenance gate' >&2
    exit 1
fi
grep -F 'maintenance gate is invalid' "$temporary/invalid-maintenance.stderr" >/dev/null
printf '%s\n' 'scheduled runner test passed'
