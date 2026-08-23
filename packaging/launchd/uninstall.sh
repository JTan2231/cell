#!/bin/sh

set -eu

PATH=/usr/bin:/bin:/usr/sbin:/sbin
export PATH

umask 077

SERVICE_LABEL=org.annals.inbox
SERVICE_TARGET=system/$SERVICE_LABEL
STATE_DIR='/Library/Application Support/Annals'
CONFIG_PATH="$STATE_DIR/config.toml"
INSTALL_FRONTEND=/usr/local/bin/annals
INSTALL_USAGE_FRONTEND=/usr/local/bin/annals-usage
INSTALL_PAYLOAD=/usr/local/libexec/annals/annals
INSTALL_USAGE_PAYLOAD=/usr/local/libexec/annals/annals-usage
INSTALL_PLIST=/Library/LaunchDaemons/org.annals.inbox.plist

fail() {
    printf 'annals uninstaller: %s\n' "$*" >&2
    exit 1
}

[ "$#" -eq 0 ] || fail 'this uninstaller accepts no arguments'
[ "$(id -u)" -eq 0 ] || fail 'run this uninstaller with sudo'
[ "$(uname -s)" = Darwin ] || fail 'this uninstaller supports macOS only'

operator=
operator_group=
if [ -f "$INSTALL_PLIST" ] && [ ! -L "$INSTALL_PLIST" ]; then
    operator=$(plutil -extract UserName raw -o - "$INSTALL_PLIST") \
        || fail "unable to read the operator from $INSTALL_PLIST"
    operator_group=$(plutil -extract GroupName raw -o - "$INSTALL_PLIST") \
        || fail "unable to read the operator group from $INSTALL_PLIST"
    id "$operator" >/dev/null 2>&1 \
        || fail "the installed operator no longer exists: $operator"
fi

run_as_operator() {
    (
        cd "$STATE_DIR" \
            || fail "unable to enter operator state directory: $STATE_DIR"
        sudo -u "$operator" env -i \
            HOME="$STATE_DIR" \
            CODEX_HOME="$STATE_DIR/codex-home" \
            PATH=/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin \
            USER="$operator" \
            LOGNAME="$operator" \
            "$@"
    )
}

assert_inbox_idle() {
    if [ -n "$operator" ] \
        && [ -x "$INSTALL_FRONTEND" ] \
        && [ -f "$CONFIG_PATH" ]; then
        status_json=$(run_as_operator "$INSTALL_FRONTEND" --json inbox status) \
            || fail 'unable to inspect the inbox; the service remains disabled'
        if printf '%s\n' "$status_json" | grep -q '"locked":true'; then
            fail 'an inbox worker is active; rerun after it finishes'
        fi
    fi
}

if launchctl print "$SERVICE_TARGET" >/dev/null 2>&1; then
    launchctl disable "$SERVICE_TARGET"
    sleep 1
    assert_inbox_idle
    launchctl bootout "$SERVICE_TARGET"
else
    launchctl disable "$SERVICE_TARGET" >/dev/null 2>&1 || true
fi

rm -f "$INSTALL_PLIST"
rm -f "$INSTALL_FRONTEND"
rm -f "$INSTALL_USAGE_FRONTEND"
rm -f "$INSTALL_PAYLOAD"
rm -f "$INSTALL_USAGE_PAYLOAD"

printf '%s\n' 'Annals scheduling and installed program files were removed, including annals-usage.'
if [ -n "$operator" ]; then
    printf 'Operator: %s (%s)\n' "$operator" "$operator_group"
fi
printf '%s\n' 'All library and usage state was retained:'
printf '  %s\n' "$STATE_DIR"
printf '%s\n' 'Remove retained state manually only after making any required backup.'
