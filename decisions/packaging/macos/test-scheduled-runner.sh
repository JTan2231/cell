#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
temporary=$(mktemp -d "${TMPDIR:-/tmp}/decisions-runner.XXXXXX")
trap 'rm -rf "$temporary"' EXIT HUP INT TERM
home="$temporary/Home"
capture="$home/capture"
mkdir -p "$home/.local/bin"
cat >"$home/.local/bin/decisions" <<'EOF'
#!/bin/sh
env | sort >"$HOME/capture"
printf '%s\n' "$@" >>"$HOME/capture"
EOF
chmod 0755 "$home/.local/bin/decisions"
HOME="$home" SECRET_MUST_NOT_LEAK=value \
    /bin/zsh "$SCRIPT_DIR/decisions-daily-email"
grep -Fx 'daily' "$capture" >/dev/null
grep -Fx 'run' "$capture" >/dev/null
grep -Fx -- '--scheduled' "$capture" >/dev/null
grep -Fx "DECISIONS_DATABASE=$home/Library/Application Support/Decisions/decisions.db" "$capture" >/dev/null
grep -F 'CONVERSATIONS_CODEX=' "$capture" >/dev/null
! grep -F 'SECRET_MUST_NOT_LEAK=' "$capture" >/dev/null
! grep -F 'RESEND_API_KEY=' "$capture" >/dev/null
printf '%s\n' 'scheduled runner test passed'
