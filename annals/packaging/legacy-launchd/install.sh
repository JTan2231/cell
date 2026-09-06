#!/bin/sh

set -eu

PATH=/usr/bin:/bin:/usr/sbin:/sbin
export PATH

umask 077

SERVICE_LABEL=org.annals.inbox
SERVICE_TARGET=system/$SERVICE_LABEL
STATE_DIR='/Library/Application Support/Annals'
SPOOL_DIR="$STATE_DIR/spool"
MAINTENANCE_MARKER="$SPOOL_DIR/.maintenance"
PAUSED_MARKER="$SPOOL_DIR/.paused"
CONFIG_PATH="$STATE_DIR/config.toml"
USAGE_CONFIG_PATH="$STATE_DIR/usage.toml"
LIBRARY_PATH="$STATE_DIR/annals.db"
PAYLOAD_DIR=/usr/local/libexec/annals
INSTALL_PAYLOAD="$PAYLOAD_DIR/annals"
INSTALL_USAGE_PAYLOAD="$PAYLOAD_DIR/annals-usage"
INSTALL_FRONTEND=/usr/local/bin/annals
INSTALL_USAGE_FRONTEND=/usr/local/bin/annals-usage
INSTALL_PLIST=/Library/LaunchDaemons/org.annals.inbox.plist
SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
SOURCE_CONFIG="$SCRIPT_DIR/annals.toml"
SOURCE_FRONTEND="$SCRIPT_DIR/annals"
SOURCE_USAGE_FRONTEND="$SCRIPT_DIR/annals-usage"
SOURCE_PLIST="$SCRIPT_DIR/org.annals.inbox.plist"

operator=
operator_group=
binary_path=
usage_binary_path=
nucleus_path=
nucleus_socket=
no_start=0
temporary_config=
temporary_frontend=
temporary_payload=
temporary_plist=
temporary_usage_config=
temporary_usage_frontend=
temporary_usage_payload=
transaction_dir=
old_config=0
old_frontend=0
old_payload=0
old_plist=0
old_usage_config=0
old_usage_frontend=0
old_usage_payload=0
service_was_loaded=0
service_booted_out=0
service_started=0
marker_created=0
switched=0
committed=0

usage() {
    cat <<'EOF'
Usage: install.sh --operator USER --binary ABSOLUTE_PATH \
  --usage-binary ABSOLUTE_PATH --nucleus ABSOLUTE_PATH \
  --nucleus-socket ABSOLUTE_PATH [--no-start]

Install or update the single-operator macOS Annals LaunchDaemon. The operator
owns the private state and can use the installed `annals` command without sudo.
Existing state must already belong to the selected operator; this installer
does not migrate installations owned by another account. Nucleus must already
be running and authenticated at the supplied socket.
EOF
}

fail() {
    printf 'annals installer: %s\n' "$*" >&2
    exit 1
}

cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    set +e
    if [ "$status" -ne 0 ] && [ "$service_started" -eq 1 ]; then
        launchctl disable "$SERVICE_TARGET" >/dev/null 2>&1 || true
        launchctl bootout "$SERVICE_TARGET" >/dev/null 2>&1 || true
        service_started=0
    fi
    if [ "$status" -ne 0 ] && [ "$switched" -eq 1 ] \
        && [ "$committed" -eq 0 ]
    then
        restore_file "$old_payload" payload "$INSTALL_PAYLOAD"
        restore_file "$old_usage_payload" usage-payload "$INSTALL_USAGE_PAYLOAD"
        restore_file "$old_frontend" frontend "$INSTALL_FRONTEND"
        restore_file "$old_usage_frontend" usage-frontend "$INSTALL_USAGE_FRONTEND"
        restore_file "$old_config" config.toml "$CONFIG_PATH"
        restore_file "$old_usage_config" usage.toml "$USAGE_CONFIG_PATH"
        restore_file "$old_plist" launchd.plist "$INSTALL_PLIST"
    fi
    if [ "$marker_created" -eq 1 ]; then
        rm -f "$MAINTENANCE_MARKER"
        marker_created=0
    fi
    if [ "$status" -ne 0 ] && [ "$service_was_loaded" -eq 1 ]; then
        launchctl disable "$SERVICE_TARGET" >/dev/null 2>&1 || true
        if [ "$service_booted_out" -eq 1 ]; then
            launchctl bootout "$SERVICE_TARGET" >/dev/null 2>&1 || true
        fi
        launchctl enable "$SERVICE_TARGET" >/dev/null 2>&1 || true
        if [ "$service_booted_out" -eq 1 ] && [ -f "$INSTALL_PLIST" ]; then
            launchctl bootstrap system "$INSTALL_PLIST" >/dev/null 2>&1 || true
        fi
        launchctl kickstart "$SERVICE_TARGET" >/dev/null 2>&1 || true
    fi
    for path in \
        "$temporary_config" \
        "$temporary_frontend" \
        "$temporary_payload" \
        "$temporary_plist" \
        "$temporary_usage_config" \
        "$temporary_usage_frontend" \
        "$temporary_usage_payload"
    do
        if [ -n "$path" ]; then
            rm -f "$path"
        fi
    done
    if [ -n "$transaction_dir" ]; then
        rm -f "$transaction_dir"/*
        rmdir "$transaction_dir" >/dev/null 2>&1 || true
    fi
    exit "$status"
}

restore_file() {
    existed=$1
    backup_name=$2
    target=$3
    if [ "$existed" -eq 1 ]; then
        cp -p "$transaction_dir/$backup_name" "$target"
    else
        rm -f "$target"
    fi
}

trap cleanup EXIT
trap 'exit 1' HUP INT TERM

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
        --usage-binary)
            [ "$#" -ge 2 ] || fail '--usage-binary requires a path'
            usage_binary_path=$2
            shift 2
            ;;
        --nucleus)
            [ "$#" -ge 2 ] || fail '--nucleus requires a path'
            nucleus_path=$2
            shift 2
            ;;
        --nucleus-socket)
            [ "$#" -ge 2 ] || fail '--nucleus-socket requires a path'
            nucleus_socket=$2
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
[ -n "$usage_binary_path" ] || fail '--usage-binary is required'
[ -n "$nucleus_path" ] || fail '--nucleus is required'
[ -n "$nucleus_socket" ] || fail '--nucleus-socket is required'

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
case "$usage_binary_path" in
    /*) ;;
    *) fail '--usage-binary must be an absolute path' ;;
esac
case "$nucleus_path" in
    /*) ;;
    *) fail '--nucleus must be an absolute path' ;;
esac
case "$nucleus_socket" in
    /*) ;;
    *) fail '--nucleus-socket must be an absolute path' ;;
esac

[ -f "$binary_path" ] && [ ! -L "$binary_path" ] && [ -x "$binary_path" ] \
    || fail "Annals binary is not an executable regular file: $binary_path"
[ -f "$usage_binary_path" ] && [ ! -L "$usage_binary_path" ] \
    && [ -x "$usage_binary_path" ] \
    || fail "Annals usage binary is not an executable regular file: $usage_binary_path"
[ -f "$nucleus_path" ] || [ -L "$nucleus_path" ] \
    || fail "Nucleus executable does not exist: $nucleus_path"
[ -x "$nucleus_path" ] || fail "Nucleus path is not executable: $nucleus_path"
[ "$usage_binary_path" != "$nucleus_path" ] \
    || fail 'the Annals usage binary and Nucleus executable must differ'
case "$nucleus_path" in
    "$INSTALL_USAGE_PAYLOAD"|"$INSTALL_USAGE_FRONTEND")
        fail 'the Nucleus path must not select the installed Annals usage command'
        ;;
esac
[ -f "$SOURCE_CONFIG" ] && [ ! -L "$SOURCE_CONFIG" ] \
    || fail "missing packaged configuration: $SOURCE_CONFIG"
[ -f "$SOURCE_FRONTEND" ] && [ ! -L "$SOURCE_FRONTEND" ] \
    || fail "missing packaged frontend: $SOURCE_FRONTEND"
[ -f "$SOURCE_USAGE_FRONTEND" ] && [ ! -L "$SOURCE_USAGE_FRONTEND" ] \
    || fail "missing packaged usage frontend: $SOURCE_USAGE_FRONTEND"
[ -f "$SOURCE_PLIST" ] && [ ! -L "$SOURCE_PLIST" ] \
    || fail "missing packaged LaunchDaemon: $SOURCE_PLIST"

for command in awk cp install launchctl plutil sh stat sudo; do
    command -v "$command" >/dev/null 2>&1 || fail "required command not found: $command"
done

"$binary_path" --version >/dev/null
usage_version=$("$usage_binary_path" --version)
case "$usage_version" in
    'annals-usage '*) ;;
    *) fail "unexpected Annals usage binary version output: $usage_version" ;;
esac
nucleus_version=$("$nucleus_path" --version)
case "$nucleus_version" in
    'annals-usage '*) fail 'the supplied Nucleus path resolves to annals-usage' ;;
esac
sh -n "$SOURCE_FRONTEND"
sh -n "$SOURCE_USAGE_FRONTEND"
plutil -lint "$SOURCE_PLIST" >/dev/null
[ "$(plutil -extract Label raw -o - "$SOURCE_PLIST")" = "$SERVICE_LABEL" ] \
    || fail "packaged plist label is not $SERVICE_LABEL"

for config_value in "$nucleus_path" "$nucleus_socket"; do
    value_lines=$(printf '%s\n' "$config_value" | wc -l | tr -d ' ')
    [ "$value_lines" -eq 1 ] || fail 'a Nucleus path must not contain a newline'
    case "$config_value" in
        *\"*|*\\*) fail 'a Nucleus path contains characters unsupported by config rendering' ;;
    esac
done

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
    "$STATE_DIR/log" \
    "$SPOOL_DIR" \
    "$SPOOL_DIR/incoming" \
    "$SPOOL_DIR/queued" \
    "$SPOOL_DIR/processing" \
    "$SPOOL_DIR/done" \
    "$SPOOL_DIR/duplicates" \
    "$SPOOL_DIR/failed" \
    "$SPOOL_DIR/skipped" \
    /usr/local/bin \
    /usr/local/libexec \
    "$PAYLOAD_DIR"
do
    check_directory "$path"
done

for path in \
    "$CONFIG_PATH" \
    "$USAGE_CONFIG_PATH" \
    "$MAINTENANCE_MARKER" \
    "$PAUSED_MARKER" \
    "$LIBRARY_PATH" \
    "$INSTALL_PAYLOAD" \
    "$INSTALL_USAGE_PAYLOAD" \
    "$INSTALL_FRONTEND" \
    "$INSTALL_USAGE_FRONTEND" \
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

transaction_dir=$(mktemp -d "${TMPDIR:-/tmp}/annals-system-install.XXXXXX")
if [ -f "$INSTALL_PAYLOAD" ]; then
    old_payload=1
    cp -p "$INSTALL_PAYLOAD" "$transaction_dir/payload"
fi
if [ -f "$INSTALL_USAGE_PAYLOAD" ]; then
    old_usage_payload=1
    cp -p "$INSTALL_USAGE_PAYLOAD" "$transaction_dir/usage-payload"
fi
if [ -f "$INSTALL_FRONTEND" ]; then
    old_frontend=1
    cp -p "$INSTALL_FRONTEND" "$transaction_dir/frontend"
fi
if [ -f "$INSTALL_USAGE_FRONTEND" ]; then
    old_usage_frontend=1
    cp -p "$INSTALL_USAGE_FRONTEND" "$transaction_dir/usage-frontend"
fi
if [ -f "$CONFIG_PATH" ]; then
    old_config=1
    cp -p "$CONFIG_PATH" "$transaction_dir/config.toml"
fi
if [ -f "$USAGE_CONFIG_PATH" ]; then
    old_usage_config=1
    cp -p "$USAGE_CONFIG_PATH" "$transaction_dir/usage.toml"
fi
if [ -f "$INSTALL_PLIST" ]; then
    old_plist=1
    cp -p "$INSTALL_PLIST" "$transaction_dir/launchd.plist"
fi

run_as_operator() {
    (
        cd "$STATE_DIR" \
            || fail "unable to enter operator state directory: $STATE_DIR"
        sudo -u "$operator" env -i \
            HOME="$STATE_DIR" \
            ANNALS_USAGE_CONFIG="$USAGE_CONFIG_PATH" \
            PATH=/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin \
            USER="$operator" \
            LOGNAME="$operator" \
            "$@"
    )
}

wait_for_existing_inbox() {
    wait_seconds=${ANNALS_UPDATE_WAIT_SECONDS:-3900}
    case "$wait_seconds" in
        ''|*[!0-9]*) fail 'ANNALS_UPDATE_WAIT_SECONDS must be a nonnegative integer' ;;
    esac
    waited=0
    while :; do
        status_json=$(run_as_operator "$INSTALL_FRONTEND" --json inbox status) \
            || fail 'unable to inspect the existing inbox'
        if printf '%s\n' "$status_json" | grep -q '"locked":false'; then
            break
        fi
        [ "$waited" -lt "$wait_seconds" ] \
            || fail "inbox did not become idle within $wait_seconds seconds"
        sleep 1
        waited=$((waited + 1))
    done
}

if launchctl print "$SERVICE_TARGET" >/dev/null 2>&1; then
    service_was_loaded=1
    if [ ! -e "$MAINTENANCE_MARKER" ]; then
        install -o "$operator" -g "$operator_group" -m 0600 \
            /dev/null "$MAINTENANCE_MARKER"
        marker_created=1
    fi
    launchctl disable "$SERVICE_TARGET"
    wait_for_existing_inbox
    launchctl bootout "$SERVICE_TARGET"
    service_booted_out=1
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
    "$STATE_DIR/log" \
    "$SPOOL_DIR" \
    "$SPOOL_DIR/incoming" \
    "$SPOOL_DIR/queued" \
    "$SPOOL_DIR/processing" \
    "$SPOOL_DIR/done" \
    "$SPOOL_DIR/duplicates" \
    "$SPOOL_DIR/failed" \
    "$SPOOL_DIR/skipped"
do
    install -d -o "$operator" -g "$operator_group" -m 0700 "$path"
done

temporary_payload="$PAYLOAD_DIR/.annals.install.$$"
install -o root -g wheel -m 0755 "$binary_path" "$temporary_payload"
switched=1
mv -f "$temporary_payload" "$INSTALL_PAYLOAD"
temporary_payload=

temporary_usage_payload="$PAYLOAD_DIR/.annals-usage.install.$$"
install -o root -g wheel -m 0755 \
    "$usage_binary_path" "$temporary_usage_payload"
mv -f "$temporary_usage_payload" "$INSTALL_USAGE_PAYLOAD"
temporary_usage_payload=

temporary_frontend=/usr/local/bin/.annals.install.$$
install -o root -g wheel -m 0755 "$SOURCE_FRONTEND" "$temporary_frontend"
mv -f "$temporary_frontend" "$INSTALL_FRONTEND"
temporary_frontend=

temporary_usage_frontend=/usr/local/bin/.annals-usage.install.$$
install -o root -g wheel -m 0755 \
    "$SOURCE_USAGE_FRONTEND" "$temporary_usage_frontend"
mv -f "$temporary_usage_frontend" "$INSTALL_USAGE_FRONTEND"
temporary_usage_frontend=

temporary_config="$STATE_DIR/.config.toml.install.$$"
if [ ! -e "$CONFIG_PATH" ]; then
    config_source=$SOURCE_CONFIG
else
    config_owner=$(stat -f '%Su' "$CONFIG_PATH")
    [ "$config_owner" = "$operator" ] \
        || fail "$CONFIG_PATH belongs to $config_owner, not $operator"
    config_source=$CONFIG_PATH
fi
if ! awk -v socket="$nucleus_socket" '
    BEGIN {
        in_liaison = 0
        saw_liaison = 0
        selected = 0
    }
    /^\[liaison\][[:space:]]*$/ {
        in_liaison = 1
        saw_liaison = 1
        print
        next
    }
    /^\[/ {
        if (in_liaison && selected == 0) {
            print "nucleus_socket = \"" socket "\""
            selected = 1
        }
        in_liaison = 0
    }
    in_liaison && /^[[:space:]]*(codex|nucleus_socket)[[:space:]]*=/ {
        if (selected == 0) {
            print "nucleus_socket = \"" socket "\""
            selected = 1
        }
        next
    }
    { print }
    END {
        if (in_liaison && selected == 0) {
            print "nucleus_socket = \"" socket "\""
            selected = 1
        }
        if (!saw_liaison || selected != 1) {
            exit 1
        }
    }
' "$config_source" >"$temporary_config"
then
    fail "unable to select Nucleus in $config_source"
fi
chown "$operator:$operator_group" "$temporary_config"
chmod 0600 "$temporary_config"
grep -Fqx "nucleus_socket = \"$nucleus_socket\"" "$temporary_config" \
    || fail 'candidate Annals configuration does not select Nucleus'
mv -f "$temporary_config" "$CONFIG_PATH"
temporary_config=

if [ -e "$USAGE_CONFIG_PATH" ]; then
    usage_config_owner=$(stat -f '%Su' "$USAGE_CONFIG_PATH")
    [ "$usage_config_owner" = "$operator" ] \
        || fail "$USAGE_CONFIG_PATH belongs to $usage_config_owner, not $operator"
fi
temporary_usage_config="$STATE_DIR/.usage.toml.install.$$"
{
    printf 'nucleus = "%s"\n' "$nucleus_path"
    printf 'nucleus_socket = "%s"\n' "$nucleus_socket"
    printf 'library = "%s"\n' "$LIBRARY_PATH"
    printf 'spool = "%s"\n' "$SPOOL_DIR"
} >"$temporary_usage_config"
chown "$operator:$operator_group" "$temporary_usage_config"
chmod 0600 "$temporary_usage_config"
mv -f "$temporary_usage_config" "$USAGE_CONFIG_PATH"
temporary_usage_config=

temporary_plist=/Library/LaunchDaemons/.org.annals.inbox.plist.$$
install -o root -g wheel -m 0644 "$SOURCE_PLIST" "$temporary_plist"
plutil -replace UserName -string "$operator" "$temporary_plist"
plutil -replace GroupName -string "$operator_group" "$temporary_plist"
plutil -lint "$temporary_plist" >/dev/null
mv -f "$temporary_plist" "$INSTALL_PLIST"
temporary_plist=

launchctl disable "$SERVICE_TARGET" >/dev/null 2>&1 || true

run_as_operator "$nucleus_path" --version >/dev/null \
    || fail "$operator cannot execute $nucleus_path"

run_as_operator "$INSTALL_USAGE_FRONTEND" --version >/dev/null \
    || fail "$operator cannot execute the installed Annals usage companion"
run_as_operator "$INSTALL_USAGE_FRONTEND" doctor \
    --config "$USAGE_CONFIG_PATH" >/dev/null \
    || fail "Nucleus readiness or authentication failed for $operator; repair Nucleus, then rerun the installer"

if [ ! -e "$LIBRARY_PATH" ]; then
    run_as_operator "$INSTALL_FRONTEND" init
fi
run_as_operator "$INSTALL_FRONTEND" stats >/dev/null
run_as_operator "$INSTALL_FRONTEND" inbox status >/dev/null
run_as_operator "$INSTALL_USAGE_FRONTEND" report --limit 0 >/dev/null

if [ "$no_start" -eq 1 ]; then
    if [ "$marker_created" -eq 1 ]; then
        rm -f "$MAINTENANCE_MARKER"
        marker_created=0
    fi
    printf '%s\n' 'Annals is installed and verified; scheduling remains disabled (--no-start).'
else
    if [ "$marker_created" -eq 1 ]; then
        rm -f "$MAINTENANCE_MARKER"
        marker_created=0
    fi
    launchctl enable "$SERVICE_TARGET"
    if ! launchctl bootstrap system "$INSTALL_PLIST"; then
        launchctl disable "$SERVICE_TARGET" >/dev/null 2>&1 || true
        fail 'unable to bootstrap the LaunchDaemon; the service remains disabled'
    fi
    service_started=1
    launchctl kickstart "$SERVICE_TARGET"
    launchctl print "$SERVICE_TARGET" >/dev/null \
        || fail 'LaunchDaemon verification failed'
    printf '%s\n' 'Annals is installed, verified, and scheduled with launchd.'
fi

# A live-only Annals Usage installation has no private ledger. This is the last
# fallible state change before commit, so earlier failures still leave the old
# release's ledger available to its rollback path.
rm -f \
    "$STATE_DIR/usage.db" \
    "$STATE_DIR/usage.db-wal" \
    "$STATE_DIR/usage.db-shm"

committed=1

printf 'Version:  %s\n' "$("$INSTALL_PAYLOAD" --version)"
printf 'Operator: %s\n' "$operator"
printf 'Service:  %s\n' "$SERVICE_TARGET"
printf 'Config:   %s\n' "$CONFIG_PATH"
printf 'Usage:    %s\n' "$USAGE_CONFIG_PATH"
printf 'Library:  %s\n' "$LIBRARY_PATH"
printf 'Inbox:    %s\n' "$SPOOL_DIR/incoming"
printf 'Logs:     %s\n' "$STATE_DIR/log"
