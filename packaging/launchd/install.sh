#!/bin/sh

set -eu

PATH=/usr/bin:/bin:/usr/sbin:/sbin
export PATH

umask 077

SERVICE_LABEL=org.annals.inbox
SERVICE_TARGET=system/$SERVICE_LABEL
STATE_DIR='/Library/Application Support/Annals'
CODEX_HOME="$STATE_DIR/codex-home"
SPOOL_DIR="$STATE_DIR/spool"
CONFIG_PATH="$STATE_DIR/config.toml"
LIBRARY_PATH="$STATE_DIR/annals.db"
PAYLOAD_DIR=/usr/local/libexec/annals
INSTALL_PAYLOAD="$PAYLOAD_DIR/annals"
INSTALL_FRONTEND=/usr/local/bin/annals
INSTALL_PLIST=/Library/LaunchDaemons/org.annals.inbox.plist
SCRIPT_DIR=$(CDPATH= cd "$(dirname "$0")" && pwd)
SOURCE_CONFIG="$SCRIPT_DIR/annals.toml"
SOURCE_FRONTEND="$SCRIPT_DIR/annals"
SOURCE_PLIST="$SCRIPT_DIR/org.annals.inbox.plist"

operator=
operator_group=
binary_path=
codex_path=
no_start=0
temporary_config=
temporary_frontend=
temporary_payload=
temporary_plist=

usage() {
    cat <<'EOF'
Usage: install.sh --operator USER --binary ABSOLUTE_PATH --codex ABSOLUTE_PATH [--no-start]

Install or update the single-operator macOS Annals LaunchDaemon. The operator
owns the private state and can use the installed `annals` command without sudo.
Existing state must already belong to the selected operator; this installer
does not migrate installations owned by another account.
EOF
}

fail() {
    printf 'annals installer: %s\n' "$*" >&2
    exit 1
}

cleanup() {
    for path in \
        "$temporary_config" \
        "$temporary_frontend" \
        "$temporary_payload" \
        "$temporary_plist"
    do
        if [ -n "$path" ]; then
            rm -f "$path"
        fi
    done
}

trap cleanup EXIT HUP INT TERM

while [ "$#" -gt 0 ]; do
    case "$1" in
        --operator)
            [ "$#" -ge 2 ] || fail '--operator requires a user name'
            operator=$2
            shift 2
            ;;
        --binary)
            [ "$#" -ge 2 ] || fail '--binary requires a path'
            binary_path=$2
            shift 2
            ;;
        --codex)
            [ "$#" -ge 2 ] || fail '--codex requires a path'
            codex_path=$2
            shift 2
            ;;
        --no-start)
            no_start=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            fail "unknown argument: $1"
            ;;
    esac
done

[ "$(id -u)" -eq 0 ] || fail 'run this installer with sudo'
[ "$(uname -s)" = Darwin ] || fail 'this installer supports macOS only'
[ -n "$operator" ] || fail '--operator is required'
[ -n "$binary_path" ] || fail '--binary is required'
[ -n "$codex_path" ] || fail '--codex is required'

case "$operator" in
    *[!A-Za-z0-9._-]*|'') fail "invalid operator user name: $operator" ;;
esac
id "$operator" >/dev/null 2>&1 || fail "operator account does not exist: $operator"
operator_uid=$(id -u "$operator")
[ "$operator_uid" -ne 0 ] || fail 'the operator must not be root'
[ "$(id -un "$operator_uid")" = "$operator" ] \
    || fail "operator does not resolve canonically: $operator"
operator_group=$(id -gn "$operator")

case "$binary_path" in
    /*) ;;
    *) fail '--binary must be an absolute path' ;;
esac
case "$codex_path" in
    /*) ;;
    *) fail '--codex must be an absolute path' ;;
esac

[ -f "$binary_path" ] && [ ! -L "$binary_path" ] && [ -x "$binary_path" ] \
    || fail "Annals binary is not an executable regular file: $binary_path"
[ -f "$codex_path" ] || [ -L "$codex_path" ] \
    || fail "Codex executable does not exist: $codex_path"
[ -x "$codex_path" ] || fail "Codex path is not executable: $codex_path"
[ -f "$SOURCE_CONFIG" ] && [ ! -L "$SOURCE_CONFIG" ] \
    || fail "missing packaged configuration: $SOURCE_CONFIG"
[ -f "$SOURCE_FRONTEND" ] && [ ! -L "$SOURCE_FRONTEND" ] \
    || fail "missing packaged frontend: $SOURCE_FRONTEND"
[ -f "$SOURCE_PLIST" ] && [ ! -L "$SOURCE_PLIST" ] \
    || fail "missing packaged LaunchDaemon: $SOURCE_PLIST"

for command in install launchctl plutil sh stat sudo; do
    command -v "$command" >/dev/null 2>&1 || fail "required command not found: $command"
done

"$binary_path" --version >/dev/null
"$codex_path" --version >/dev/null
sh -n "$SOURCE_FRONTEND"
plutil -lint "$SOURCE_PLIST" >/dev/null
[ "$(plutil -extract Label raw -o - "$SOURCE_PLIST")" = "$SERVICE_LABEL" ] \
    || fail "packaged plist label is not $SERVICE_LABEL"

codex_lines=$(printf '%s\n' "$codex_path" | wc -l | tr -d ' ')
[ "$codex_lines" -eq 1 ] || fail 'the Codex path must not contain a newline'
case "$codex_path" in
    *\"*) fail 'the Codex path must not contain a double quote' ;;
esac

check_directory() {
    path=$1
    if [ -L "$path" ]; then
        fail "refusing symlink at directory path: $path"
    fi
    if [ -e "$path" ] && [ ! -d "$path" ]; then
        fail "expected a directory: $path"
    fi
}

check_file() {
    path=$1
    if [ -L "$path" ]; then
        fail "refusing symlink at file path: $path"
    fi
    if [ -e "$path" ] && [ ! -f "$path" ]; then
        fail "expected a regular file: $path"
    fi
}

for path in \
    "$STATE_DIR" \
    "$CODEX_HOME" \
    "$STATE_DIR/log" \
    "$SPOOL_DIR" \
    "$SPOOL_DIR/incoming" \
    "$SPOOL_DIR/processing" \
    "$SPOOL_DIR/done" \
    "$SPOOL_DIR/failed" \
    /usr/local/bin \
    /usr/local/libexec \
    "$PAYLOAD_DIR"
do
    check_directory "$path"
done

for path in \
    "$CONFIG_PATH" \
    "$LIBRARY_PATH" \
    "$CODEX_HOME/config.toml" \
    "$CODEX_HOME/auth.json" \
    "$INSTALL_PAYLOAD" \
    "$INSTALL_FRONTEND" \
    "$INSTALL_PLIST"
do
    check_file "$path"
done

if [ -d "$STATE_DIR" ]; then
    state_owner=$(stat -f '%Su' "$STATE_DIR")
    [ "$state_owner" = "$operator" ] \
        || fail "$STATE_DIR belongs to $state_owner, not $operator; remove the test installation before reinstalling"
fi

if [ -f "$INSTALL_PLIST" ]; then
    installed_operator=$(plutil -extract UserName raw -o - "$INSTALL_PLIST") \
        || fail "unable to read the installed operator from $INSTALL_PLIST"
    [ "$installed_operator" = "$operator" ] \
        || fail "the installed service belongs to $installed_operator, not $operator"
fi

run_as_operator() {
    (
        cd "$STATE_DIR" \
            || fail "unable to enter operator state directory: $STATE_DIR"
        sudo -u "$operator" env -i \
            HOME="$STATE_DIR" \
            CODEX_HOME="$CODEX_HOME" \
            PATH=/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin \
            USER="$operator" \
            LOGNAME="$operator" \
            "$@"
    )
}

assert_existing_inbox_idle() {
    if [ -x "$INSTALL_FRONTEND" ] && [ -f "$CONFIG_PATH" ]; then
        status_json=$(run_as_operator "$INSTALL_FRONTEND" --json inbox status) \
            || fail 'unable to inspect the existing inbox; the service remains disabled'
        if printf '%s\n' "$status_json" | grep -q '"locked":true'; then
            fail 'an inbox worker is active; rerun after it finishes'
        fi
    fi
}

if launchctl print "$SERVICE_TARGET" >/dev/null 2>&1; then
    launchctl disable "$SERVICE_TARGET"
    sleep 1
    assert_existing_inbox_idle
    launchctl bootout "$SERVICE_TARGET"
fi

if [ ! -d /usr/local/bin ]; then
    install -d -o root -g wheel -m 0755 /usr/local/bin
fi
if [ ! -d /usr/local/libexec ]; then
    install -d -o root -g wheel -m 0755 /usr/local/libexec
fi
if [ ! -d "$PAYLOAD_DIR" ]; then
    install -d -o root -g wheel -m 0755 "$PAYLOAD_DIR"
fi

for path in \
    "$STATE_DIR" \
    "$CODEX_HOME" \
    "$STATE_DIR/log" \
    "$SPOOL_DIR" \
    "$SPOOL_DIR/incoming" \
    "$SPOOL_DIR/processing" \
    "$SPOOL_DIR/done" \
    "$SPOOL_DIR/failed"
do
    install -d -o "$operator" -g "$operator_group" -m 0700 "$path"
done

temporary_payload="$PAYLOAD_DIR/.annals.install.$$"
install -o root -g wheel -m 0755 "$binary_path" "$temporary_payload"
mv -f "$temporary_payload" "$INSTALL_PAYLOAD"
temporary_payload=

temporary_frontend=/usr/local/bin/.annals.install.$$
install -o root -g wheel -m 0755 "$SOURCE_FRONTEND" "$temporary_frontend"
mv -f "$temporary_frontend" "$INSTALL_FRONTEND"
temporary_frontend=

if [ ! -e "$CONFIG_PATH" ]; then
    temporary_config=$(mktemp "${TMPDIR:-/tmp}/annals-config.XXXXXX")
    escaped_codex=$(printf '%s\n' "$codex_path" | sed 's/[\\&|]/\\&/g')
    sed "s|codex = \"/usr/local/bin/codex\"|codex = \"$escaped_codex\"|" \
        "$SOURCE_CONFIG" >"$temporary_config"
    grep -F "codex = \"$codex_path\"" "$temporary_config" >/dev/null \
        || fail 'unable to render the Codex path into the Annals configuration'
    install -o "$operator" -g "$operator_group" -m 0600 \
        "$temporary_config" "$CONFIG_PATH"
    rm -f "$temporary_config"
    temporary_config=
else
    config_owner=$(stat -f '%Su' "$CONFIG_PATH")
    [ "$config_owner" = "$operator" ] \
        || fail "$CONFIG_PATH belongs to $config_owner, not $operator"
    grep -F "codex = \"$codex_path\"" "$CONFIG_PATH" >/dev/null \
        || fail "the retained configuration does not select the supplied Codex path: $codex_path"
    printf 'annals installer: retaining existing configuration %s\n' "$CONFIG_PATH"
fi

codex_config="$CODEX_HOME/config.toml"
if [ ! -e "$codex_config" ]; then
    printf '%s\n' 'cli_auth_credentials_store = "file"' >"$codex_config"
elif grep -Eq '^[[:space:]]*cli_auth_credentials_store[[:space:]]*=' "$codex_config"; then
    grep -Eq '^[[:space:]]*cli_auth_credentials_store[[:space:]]*=[[:space:]]*"file"[[:space:]]*$' \
        "$codex_config" \
        || fail "$codex_config must set cli_auth_credentials_store to \"file\""
else
    printf '\n%s\n' 'cli_auth_credentials_store = "file"' >>"$codex_config"
fi
chown "$operator:$operator_group" "$codex_config"
chmod 0600 "$codex_config"

temporary_plist=/Library/LaunchDaemons/.org.annals.inbox.plist.$$
install -o root -g wheel -m 0644 "$SOURCE_PLIST" "$temporary_plist"
plutil -replace UserName -string "$operator" "$temporary_plist"
plutil -replace GroupName -string "$operator_group" "$temporary_plist"
plutil -lint "$temporary_plist" >/dev/null
mv -f "$temporary_plist" "$INSTALL_PLIST"
temporary_plist=

launchctl disable "$SERVICE_TARGET" >/dev/null 2>&1 || true

run_as_operator "$codex_path" --version >/dev/null \
    || fail "$operator cannot execute $codex_path"

if ! run_as_operator "$codex_path" login status >/dev/null 2>&1; then
    if [ -t 0 ] && [ -t 1 ]; then
        printf '%s\n' 'Annals requires a Codex login in its state-local Codex home.'
        run_as_operator "$codex_path" login --device-auth \
            || fail 'Codex device authentication did not complete; the service remains disabled'
    else
        fail "Codex is not authenticated for $operator; rerun interactively to complete device login"
    fi
fi

run_as_operator "$codex_path" login status >/dev/null \
    || fail "Codex login verification failed for $operator"
[ -f "$CODEX_HOME/auth.json" ] && [ ! -L "$CODEX_HOME/auth.json" ] \
    || fail "Codex login did not create a regular $CODEX_HOME/auth.json"
chown "$operator:$operator_group" "$CODEX_HOME/auth.json"
chmod 0600 "$CODEX_HOME/auth.json"

if [ ! -e "$LIBRARY_PATH" ]; then
    run_as_operator "$INSTALL_FRONTEND" init
fi
run_as_operator "$INSTALL_FRONTEND" validate >/dev/null
run_as_operator "$INSTALL_FRONTEND" inbox status >/dev/null

if [ "$no_start" -eq 1 ]; then
    printf '%s\n' 'Annals is installed and validated; scheduling remains disabled (--no-start).'
else
    launchctl enable "$SERVICE_TARGET"
    if ! launchctl bootstrap system "$INSTALL_PLIST"; then
        launchctl disable "$SERVICE_TARGET" >/dev/null 2>&1 || true
        fail 'unable to bootstrap the LaunchDaemon; the service remains disabled'
    fi
    launchctl kickstart "$SERVICE_TARGET"
    launchctl print "$SERVICE_TARGET" >/dev/null \
        || fail 'LaunchDaemon verification failed'
    printf '%s\n' 'Annals is installed, validated, and scheduled with launchd.'
fi

printf 'Version:  %s\n' "$("$INSTALL_PAYLOAD" --version)"
printf 'Operator: %s\n' "$operator"
printf 'Service:  %s\n' "$SERVICE_TARGET"
printf 'Config:   %s\n' "$CONFIG_PATH"
printf 'Library:  %s\n' "$LIBRARY_PATH"
printf 'Inbox:    %s\n' "$SPOOL_DIR/incoming"
printf 'Logs:     %s\n' "$STATE_DIR/log"
