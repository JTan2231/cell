#!/bin/sh

set -eu

PATH=/usr/bin:/bin:/usr/sbin:/sbin
export PATH
umask 077

SERVICE_LABEL=org.annals.inbox
SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)

binary_path=
codex_path=
legacy_prefix=${ANNALS_MIGRATION_LEGACY_PREFIX:-}
legacy_state_override=${ANNALS_MIGRATION_LEGACY_STATE:-}
launchctl_path=${ANNALS_MIGRATION_LAUNCHCTL:-/bin/launchctl}
dscl_path=${ANNALS_MIGRATION_DSCL:-/usr/bin/dscl}
operator_runner=${ANNALS_MIGRATION_OPERATOR_RUNNER:-/usr/bin/sudo}
deploy_path=${ANNALS_MIGRATION_DEPLOY:-$SCRIPT_DIR/deploy-user.sh}

usage() {
    cat <<'EOF'
Usage: migrate-to-user.sh --binary ABSOLUTE_PATH --codex ABSOLUTE_PATH [OPTIONS]

Move the legacy system Annals installation into the selected operator's home
and deploy the complete user-owned installation. Run this command as root.

Options used by the fixture test:
  --legacy-prefix ABSOLUTE_PATH
  --legacy-state ABSOLUTE_PATH
  --launchctl ABSOLUTE_PATH
  --dscl ABSOLUTE_PATH
  --operator-runner ABSOLUTE_PATH
  --deploy ABSOLUTE_PATH
EOF
}

fail() {
    printf 'annals migration: %s\n' "$*" >&2
    exit 1
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --binary) binary_path=${2:?}; shift 2 ;;
        --codex) codex_path=${2:?}; shift 2 ;;
        --legacy-prefix) legacy_prefix=${2:?}; shift 2 ;;
        --legacy-state) legacy_state_override=${2:?}; shift 2 ;;
        --launchctl) launchctl_path=${2:?}; shift 2 ;;
        --dscl) dscl_path=${2:?}; shift 2 ;;
        --operator-runner) operator_runner=${2:?}; shift 2 ;;
        --deploy) deploy_path=${2:?}; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *) fail "unknown argument: $1" ;;
    esac
done

case "$legacy_prefix" in
    '') ;;
    /*) [ "$legacy_prefix" != / ] || fail '--legacy-prefix must not be /' ;;
    *) fail '--legacy-prefix must be absolute' ;;
esac
if [ "$(id -u)" -ne 0 ] && [ -z "$legacy_prefix" ]; then
    fail 'run this migration with sudo'
fi

for value_name in binary_path codex_path launchctl_path dscl_path operator_runner deploy_path; do
    eval "value=\${$value_name}"
    [ -n "$value" ] || fail "$value_name is required"
    case "$value" in /*) ;; *) fail "$value_name must be absolute" ;; esac
done
for executable in "$binary_path" "$launchctl_path" \
    "$dscl_path" "$operator_runner" "$deploy_path"
do
    [ -f "$executable" ] && [ -x "$executable" ] && [ ! -L "$executable" ] \
        || fail "required executable is unavailable: $executable"
done
if [ ! -x "$codex_path" ] || { [ ! -f "$codex_path" ] && [ ! -L "$codex_path" ]; }; then
    fail "Codex executable is unavailable: $codex_path"
fi

LEGACY_FRONTEND="$legacy_prefix/usr/local/bin/annals"
LEGACY_PAYLOAD="$legacy_prefix/usr/local/libexec/annals/annals"
LEGACY_PLIST="$legacy_prefix/Library/LaunchDaemons/$SERVICE_LABEL.plist"
if [ -n "$legacy_state_override" ]; then
    case "$legacy_state_override" in /*) ;; *) fail '--legacy-state must be absolute' ;; esac
    LEGACY_STATE=$legacy_state_override
else
    LEGACY_STATE="$legacy_prefix/Library/Application Support/Annals"
fi
TRANSACTION_DIR="$LEGACY_STATE.migrate-to-user"
SYSTEM_TARGET="system/$SERVICE_LABEL"

recovery_phase=
if [ -d "$TRANSACTION_DIR" ]; then
    recovery_phase=$(sed -n '1p' "$TRANSACTION_DIR/phase" 2>/dev/null || true)
fi
if [ "$recovery_phase" = committed ]; then
    operator=$(sed -n '1p' "$TRANSACTION_DIR/operator")
    operator_home=$(sed -n '1p' "$TRANSACTION_DIR/home")
else
    for path in "$LEGACY_FRONTEND" "$LEGACY_PAYLOAD" "$LEGACY_PLIST"; do
        [ -f "$path" ] && [ ! -L "$path" ] || fail "invalid legacy file: $path"
    done
    [ -d "$LEGACY_STATE" ] && [ ! -L "$LEGACY_STATE" ] \
        || { [ -d "$TRANSACTION_DIR" ] || fail "invalid legacy state: $LEGACY_STATE"; }
    [ "$(plutil -extract Label raw -o - "$LEGACY_PLIST")" = "$SERVICE_LABEL" ] \
        || fail "legacy plist label is not $SERVICE_LABEL"
    operator=$(plutil -extract UserName raw -o - "$LEGACY_PLIST") \
        || fail 'unable to read the legacy operator'
    home_record=$($dscl_path . -read "/Users/$operator" NFSHomeDirectory) \
        || fail "unable to resolve the home for $operator"
    operator_home=${home_record#*:}
    operator_home=$(printf '%s\n' "$operator_home" | sed 's/^[[:space:]]*//')
fi
case "$operator" in *[!A-Za-z0-9._-]*|'') fail 'invalid legacy operator' ;; esac
operator_uid=$(id -u "$operator") || fail "operator does not exist: $operator"
[ "$operator_uid" -ne 0 ] || fail 'legacy operator must not be root'
operator_group=$(id -gn "$operator")
case "$operator_home" in /*) ;; *) fail "invalid operator home: $operator_home" ;; esac
[ -d "$operator_home" ] && [ ! -L "$operator_home" ] \
    || fail "operator home is unavailable: $operator_home"
[ "$(stat -f '%u' "$operator_home")" -eq "$operator_uid" ] \
    || fail "operator home is not owned by $operator"

TARGET_STATE="$operator_home/Library/Application Support/Annals"
USER_PLIST="$operator_home/Library/LaunchAgents/$SERVICE_LABEL.plist"
USER_CLI="$operator_home/.local/bin/annals"
USER_TARGET="gui/$operator_uid/$SERVICE_LABEL"
MAINTENANCE_MARKER="$TARGET_STATE/spool/.maintenance"

run_as_operator() {
    "$operator_runner" -u "$operator" /usr/bin/env -i \
        HOME="$operator_home" \
        PATH=/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin \
        USER="$operator" LOGNAME="$operator" "$@"
}

write_phase() {
    printf '%s\n' "$1" >"$TRANSACTION_DIR/phase.tmp"
    mv -f "$TRANSACTION_DIR/phase.tmp" "$TRANSACTION_DIR/phase"
}

restore_legacy() {
    "$launchctl_path" bootout "$USER_TARGET" >/dev/null 2>&1 || true
    rm -f "$USER_PLIST" "$USER_CLI"
    if [ ! -d "$LEGACY_STATE" ] && [ -d "$TARGET_STATE" ]; then
        mv "$TARGET_STATE" "$LEGACY_STATE"
    fi
    if [ -d "$LEGACY_STATE" ]; then
        install -m 0600 "$TRANSACTION_DIR/config.toml" "$LEGACY_STATE/config.toml"
        chown "$operator:$operator_group" "$LEGACY_STATE/config.toml"
        rm -f "$LEGACY_STATE/spool/.maintenance"
        if [ "$(sed -n '1p' "$TRANSACTION_DIR/had-install")" = 0 ]; then
            rm -rf "$LEGACY_STATE/install"
        fi
        if [ "$(sed -n '1p' "$TRANSACTION_DIR/had-backups")" = 0 ]; then
            rm -rf "$LEGACY_STATE/backups"
        fi
    fi
    if [ "$(sed -n '1p' "$TRANSACTION_DIR/was-loaded")" = 1 ]; then
        "$launchctl_path" enable "$SYSTEM_TARGET" >/dev/null 2>&1 || true
        if ! "$launchctl_path" print "$SYSTEM_TARGET" >/dev/null 2>&1; then
            "$launchctl_path" bootstrap system "$LEGACY_PLIST" >/dev/null
        fi
        "$launchctl_path" kickstart "$SYSTEM_TARGET" >/dev/null 2>&1 || true
    fi
    rm -rf "$TRANSACTION_DIR"
}

retire_legacy() {
    "$launchctl_path" bootout "$SYSTEM_TARGET" >/dev/null 2>&1 || true
    rm -f "$LEGACY_PLIST" "$LEGACY_FRONTEND" "$LEGACY_PAYLOAD"
    rmdir "$(dirname "$LEGACY_PAYLOAD")" >/dev/null 2>&1 || true
}

committed=0
cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    if [ "$status" -ne 0 ] && [ "$committed" -eq 0 ] \
        && [ -d "$TRANSACTION_DIR" ]; then
        set +e
        restore_legacy
        set -e
    fi
    exit "$status"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

if [ -d "$TRANSACTION_DIR" ]; then
    case "$recovery_phase" in
        committed)
            committed=1
            [ -d "$TARGET_STATE" ] || fail 'committed migration has no user state'
            rm -f "$MAINTENANCE_MARKER"
            "$launchctl_path" kickstart "$USER_TARGET" >/dev/null 2>&1 || true
            retire_legacy
            rm -rf "$TRANSACTION_DIR"
            printf '%s\n' 'Annals migration recovery completed.'
            exit 0
            ;;
        prepared|stopped|moved|rewritten)
            restore_legacy
            ;;
        *) fail "invalid migration transaction: $TRANSACTION_DIR" ;;
    esac
fi

[ "$(stat -f '%u' "$LEGACY_STATE")" -eq "$operator_uid" ] \
    || fail "legacy state is not owned by $operator"
for path in config.toml annals.db codex-home spool log; do
    [ -e "$LEGACY_STATE/$path" ] && [ ! -L "$LEGACY_STATE/$path" ] \
        || fail "legacy state is incomplete: $LEGACY_STATE/$path"
done
[ ! -e "$TARGET_STATE" ] && [ ! -L "$TARGET_STATE" ] \
    || fail "user state already exists: $TARGET_STATE"
[ ! -e "$USER_PLIST" ] && [ ! -L "$USER_PLIST" ] \
    || fail "user LaunchAgent already exists: $USER_PLIST"
[ ! -e "$USER_CLI" ] && [ ! -L "$USER_CLI" ] \
    || fail "user command already exists: $USER_CLI"
grep -Fx 'library = "/Library/Application Support/Annals/annals.db"' \
    "$LEGACY_STATE/config.toml" >/dev/null \
    || fail 'legacy config has a nonstandard library path'
grep -Fx 'root = "/Library/Application Support/Annals/spool"' \
    "$LEGACY_STATE/config.toml" >/dev/null \
    || fail 'legacy config has a nonstandard inbox path'

run_as_operator "$binary_path" --version >/dev/null
run_as_operator "$LEGACY_FRONTEND" validate >/dev/null \
    || fail 'the legacy library is not valid'

install -d -m 0700 "$TRANSACTION_DIR"
install -m 0600 "$LEGACY_STATE/config.toml" "$TRANSACTION_DIR/config.toml"
printf '%s\n' "$operator" >"$TRANSACTION_DIR/operator"
printf '%s\n' "$operator_home" >"$TRANSACTION_DIR/home"
if "$launchctl_path" print "$SYSTEM_TARGET" >/dev/null 2>&1; then
    printf '%s\n' 1 >"$TRANSACTION_DIR/was-loaded"
else
    printf '%s\n' 0 >"$TRANSACTION_DIR/was-loaded"
fi
[ -e "$LEGACY_STATE/install" ] && had_install=1 || had_install=0
[ -e "$LEGACY_STATE/backups" ] && had_backups=1 || had_backups=0
printf '%s\n' "$had_install" >"$TRANSACTION_DIR/had-install"
printf '%s\n' "$had_backups" >"$TRANSACTION_DIR/had-backups"
write_phase prepared

"$launchctl_path" disable "$SYSTEM_TARGET" >/dev/null 2>&1 || true
if [ -e "$LEGACY_STATE/spool/.maintenance" ] \
    && { [ ! -f "$LEGACY_STATE/spool/.maintenance" ] \
        || [ -L "$LEGACY_STATE/spool/.maintenance" ]; }; then
    fail 'the legacy maintenance marker is invalid'
fi
: >"$LEGACY_STATE/spool/.maintenance"
wait_seconds=${ANNALS_UPDATE_WAIT_SECONDS:-3900}
case "$wait_seconds" in
    ''|*[!0-9]*) fail 'ANNALS_UPDATE_WAIT_SECONDS must be a nonnegative integer' ;;
esac
waited=0
while :; do
    status_json=$(run_as_operator "$LEGACY_FRONTEND" --json inbox status) \
        || fail 'unable to inspect the legacy inbox'
    if printf '%s\n' "$status_json" | grep -q '"locked":false'; then
        break
    fi
    [ "$waited" -lt "$wait_seconds" ] \
        || fail "legacy inbox did not become idle within $wait_seconds seconds"
    sleep 1
    waited=$((waited + 1))
done
"$launchctl_path" bootout "$SYSTEM_TARGET" >/dev/null 2>&1 || true
write_phase stopped

target_parent=$(dirname "$TARGET_STATE")
if [ -L "$target_parent" ]; then
    fail "refusing symlink at user state parent: $target_parent"
fi
if [ ! -d "$target_parent" ]; then
    run_as_operator /bin/mkdir -p "$target_parent"
    run_as_operator /bin/chmod 0700 "$target_parent"
fi
[ "$(stat -f '%u' "$target_parent")" -eq "$operator_uid" ] \
    || fail "user state parent is not owned by $operator"
[ "$(stat -f '%d' "$LEGACY_STATE")" = "$(stat -f '%d' "$target_parent")" ] \
    || fail 'legacy and user state must be on the same filesystem'
mv "$LEGACY_STATE" "$TARGET_STATE"
write_phase moved

temporary_config="$TARGET_STATE/.config.toml.migration.$$"
awk '
    $0 == "library = \"/Library/Application Support/Annals/annals.db\"" {
        print "library = \"annals.db\""; next
    }
    $0 == "root = \"/Library/Application Support/Annals/spool\"" {
        print "root = \"spool\""; next
    }
    { print }
' "$TARGET_STATE/config.toml" >"$temporary_config"
chmod 0600 "$temporary_config"
chown "$operator:$operator_group" "$temporary_config"
mv -f "$temporary_config" "$TARGET_STATE/config.toml"
: >"$MAINTENANCE_MARKER"
write_phase rewritten

if [ "${ANNALS_MIGRATION_TEST_CRASH_AFTER_MOVE:-0}" = 1 ] \
    && [ -n "$legacy_prefix" ]; then
    trap - EXIT HUP INT TERM
    exit 99
fi

run_as_operator "$deploy_path" \
    --binary "$binary_path" \
    --codex "$codex_path" \
    --home "$operator_home" \
    --launchctl "$launchctl_path"

write_phase committed
committed=1
rm -f "$MAINTENANCE_MARKER"
"$launchctl_path" kickstart "$USER_TARGET" >/dev/null
retire_legacy
rm -rf "$TRANSACTION_DIR"

printf '%s\n' 'Annals was migrated to the user-owned installation.'
printf 'Operator: %s\n' "$operator"
printf 'State:    %s\n' "$TARGET_STATE"
printf 'Command:  %s\n' "$USER_CLI"
printf 'Service:  %s\n' "$USER_TARGET"
