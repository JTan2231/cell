#!/bin/sh

set -eu

PATH=/usr/bin:/bin:/usr/sbin:/sbin
export PATH
umask 077

SERVICE_LABEL=org.annals.inbox
SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
SOURCE_LEGACY_DAEMON_PLIST="$SCRIPT_DIR/org.annals.inbox.plist"
SOURCE_LEGACY_AGENT_PLIST="$SCRIPT_DIR/org.annals.inbox.agent.plist"

binary_path=
usage_binary_path=
nucleus_path=
nucleus_socket=
clockwork_path=
legacy_prefix=${ANNALS_MIGRATION_LEGACY_PREFIX:-}
legacy_state_override=${ANNALS_MIGRATION_LEGACY_STATE:-}
launchctl_path=${ANNALS_MIGRATION_LAUNCHCTL:-/bin/launchctl}
dscl_path=${ANNALS_MIGRATION_DSCL:-/usr/bin/dscl}
operator_runner=${ANNALS_MIGRATION_OPERATOR_RUNNER:-/usr/bin/sudo}
deploy_path=${ANNALS_MIGRATION_DEPLOY:-$SCRIPT_DIR/deploy-user.sh}

usage() {
    cat <<'EOF'
Usage: migrate-to-user.sh --binary ABSOLUTE_PATH --usage-binary ABSOLUTE_PATH \
  --nucleus ABSOLUTE_PATH --nucleus-socket ABSOLUTE_PATH \
  --clockwork ABSOLUTE_PATH [OPTIONS]

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

# Full rendered comparisons make any extra launchd behavior foreign even when
# the visible label and executable tuple still look like Annals.
legacy_daemon_plist_matches_expected() {
    ownership_candidate=$1
    [ -f "$ownership_candidate" ] && [ ! -L "$ownership_candidate" ] \
        && [ "$(stat -f '%u' "$ownership_candidate")" -eq "$invoking_uid" ] \
        && [ "$(stat -f '%Lp' "$ownership_candidate")" = 644 ] \
        && [ -f "$SOURCE_LEGACY_DAEMON_PLIST" ] \
        && [ ! -L "$SOURCE_LEGACY_DAEMON_PLIST" ] \
        || return 1

    ownership_expected_dir=$(mktemp -d /tmp/annals-legacy-daemon.XXXXXX) \
        || return 1
    ownership_expected="$ownership_expected_dir/$SERVICE_LABEL.plist"
    if install -m 0600 "$SOURCE_LEGACY_DAEMON_PLIST" "$ownership_expected" \
        && plutil -replace UserName -string "$operator" "$ownership_expected" \
        && plutil -replace GroupName -string "$operator_group" "$ownership_expected" \
        && cmp -s "$ownership_expected" "$ownership_candidate"
    then
        ownership_matched=0
    else
        ownership_matched=1
    fi
    rm -f "$ownership_expected" >/dev/null 2>&1 || true
    rmdir "$ownership_expected_dir" >/dev/null 2>&1 || true
    return "$ownership_matched"
}

legacy_agent_plist_matches_expected() {
    ownership_candidate=$1
    [ -f "$ownership_candidate" ] && [ ! -L "$ownership_candidate" ] \
        && [ "$(stat -f '%u' "$ownership_candidate")" -eq "$operator_uid" ] \
        && [ "$(stat -f '%Lp' "$ownership_candidate")" = 600 ] \
        && [ -f "$SOURCE_LEGACY_AGENT_PLIST" ] \
        && [ ! -L "$SOURCE_LEGACY_AGENT_PLIST" ] \
        || return 1

    ownership_expected_dir=$(mktemp -d /tmp/annals-legacy-agent.XXXXXX) \
        || return 1
    ownership_expected="$ownership_expected_dir/$SERVICE_LABEL.plist"
    if install -m 0600 "$SOURCE_LEGACY_AGENT_PLIST" "$ownership_expected" \
        && plutil -remove ProgramArguments.0 "$ownership_expected" \
        && plutil -insert ProgramArguments.0 -string "$USER_CLI" \
            "$ownership_expected" \
        && plutil -replace WorkingDirectory -string "$TARGET_STATE" \
            "$ownership_expected" \
        && plutil -replace EnvironmentVariables.HOME -string "$operator_home" \
            "$ownership_expected" \
        && plutil -replace StandardOutPath \
            -string "$TARGET_STATE/log/inbox.stdout.log" "$ownership_expected" \
        && plutil -replace StandardErrorPath \
            -string "$TARGET_STATE/log/inbox.stderr.log" "$ownership_expected" \
        && cmp -s "$ownership_expected" "$ownership_candidate"
    then
        ownership_matched=0
    else
        ownership_matched=1
    fi
    rm -f "$ownership_expected" >/dev/null 2>&1 || true
    rmdir "$ownership_expected_dir" >/dev/null 2>&1 || true
    return "$ownership_matched"
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --binary) binary_path=${2:?}; shift 2 ;;
        --usage-binary) usage_binary_path=${2:?}; shift 2 ;;
        --nucleus) nucleus_path=${2:?}; shift 2 ;;
        --nucleus-socket) nucleus_socket=${2:?}; shift 2 ;;
        --clockwork) clockwork_path=${2:?}; shift 2 ;;
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
invoking_uid=$(id -u)
if [ "$invoking_uid" -ne 0 ] && [ -z "$legacy_prefix" ]; then
    fail 'run this migration with sudo'
fi

[ -n "$usage_binary_path" ] || fail '--usage-binary is required'
for value_name in \
    binary_path usage_binary_path nucleus_path nucleus_socket clockwork_path \
    launchctl_path dscl_path operator_runner deploy_path
do
    eval "value=\${$value_name}"
    [ -n "$value" ] || fail "$value_name is required"
    case "$value" in /*) ;; *) fail "$value_name must be absolute" ;; esac
done
for executable in "$binary_path" "$usage_binary_path" "$launchctl_path" \
    "$dscl_path" "$operator_runner" "$deploy_path"
do
    [ -f "$executable" ] && [ -x "$executable" ] && [ ! -L "$executable" ] \
        || fail "required executable is unavailable: $executable"
done
if [ ! -x "$clockwork_path" ] \
    || { [ ! -f "$clockwork_path" ] && [ ! -L "$clockwork_path" ]; }
then
    fail "Clockwork executable is unavailable: $clockwork_path"
fi
if [ ! -x "$nucleus_path" ] || { [ ! -f "$nucleus_path" ] && [ ! -L "$nucleus_path" ]; }; then
    fail "Nucleus executable is unavailable: $nucleus_path"
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
    operator=$(plutil -extract UserName raw -o - "$LEGACY_PLIST") \
        || fail 'unable to read the legacy operator'
fi
case "$operator" in *[!A-Za-z0-9._-]*|'') fail 'invalid legacy operator' ;; esac
operator_uid=$(id -u "$operator") || fail "operator does not exist: $operator"
[ "$operator_uid" -ne 0 ] || fail 'legacy operator must not be root'
operator_group=$(id -gn "$operator")
if [ "$recovery_phase" != committed ]; then
    legacy_daemon_plist_matches_expected "$LEGACY_PLIST" \
        || fail "legacy LaunchDaemon is not the exact Annals-owned plist: $LEGACY_PLIST"
    home_record=$($dscl_path . -read "/Users/$operator" NFSHomeDirectory) \
        || fail "unable to resolve the home for $operator"
    operator_home=${home_record#*:}
    operator_home=$(printf '%s\n' "$operator_home" | sed 's/^[[:space:]]*//')
fi
case "$operator_home" in /*) ;; *) fail "invalid operator home: $operator_home" ;; esac
[ -d "$operator_home" ] && [ ! -L "$operator_home" ] \
    || fail "operator home is unavailable: $operator_home"
[ "$(stat -f '%u' "$operator_home")" -eq "$operator_uid" ] \
    || fail "operator home is not owned by $operator"

TARGET_STATE="$operator_home/Library/Application Support/Annals"
USER_PLIST="$operator_home/Library/LaunchAgents/$SERVICE_LABEL.plist"
USER_CLI="$operator_home/.local/bin/annals"
USER_USAGE_CLI="$operator_home/.local/bin/annals-usage"
USER_TARGET="gui/$operator_uid/$SERVICE_LABEL"
MAINTENANCE_MARKER="$TARGET_STATE/spool/.maintenance"
LEGACY_PAUSED_MARKER="$LEGACY_STATE/spool/.paused"
CLOCKWORK_HANDOFF="$TARGET_STATE/install/.migration-annals-inbox.clockwork.toml"

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

inspect_legacy_system_service() {
    legacy_system_service_loaded=0
    if legacy_system_service_result=$("$launchctl_path" print \
        "$SYSTEM_TARGET" 2>&1)
    then
        legacy_system_service_loaded=1
        return 0
    fi
    printf '%s\n' "$legacy_system_service_result" \
        | grep -F 'Could not find service' >/dev/null
}

inspect_clockwork_binding() {
    inspected_clockwork_present=0
    inspected_clockwork_enabled=0
    inspected_clockwork_digest=
    if inspected_clockwork_result=$(run_as_operator "$clockwork_path" --json \
        binding show annals/inbox 2>&1)
    then
        inspected_clockwork_present=1
        inspected_clockwork_compact=$(printf '%s' "$inspected_clockwork_result" \
            | tr -d '[:space:]')
        case "$inspected_clockwork_compact" in
            *'"key":"annals/inbox"'*) ;;
            *) return 1 ;;
        esac
        case "$inspected_clockwork_compact" in
            *'"definition_digest":null'*) ;;
            *'"definition_digest":"'*)
                inspected_clockwork_digest=$(printf '%s\n' \
                    "$inspected_clockwork_compact" | sed -n \
                    's/.*"definition_digest":"\([0-9a-f]\{64\}\)".*/\1/p')
                [ -n "$inspected_clockwork_digest" ] || return 1
                ;;
            *) return 1 ;;
        esac
        case "$inspected_clockwork_compact" in
            *'"enabled":true'*) inspected_clockwork_enabled=1 ;;
            *'"enabled":false'*) inspected_clockwork_enabled=0 ;;
            *) return 1 ;;
        esac
        [ "$inspected_clockwork_enabled" -eq 0 ] \
            || [ -n "$inspected_clockwork_digest" ] \
            || return 1
        return 0
    fi
    printf '%s\n' "$inspected_clockwork_result" \
        | grep -F '"code":"binding_not_found"' >/dev/null
}

clockwork_binding_is_empty_and_disabled() {
    [ "$inspected_clockwork_present" -eq 0 ] \
        || { [ "$inspected_clockwork_enabled" -eq 0 ] \
            && [ -z "$inspected_clockwork_digest" ]; }
}

ensure_clockwork_disabled() {
    if inspect_clockwork_binding && clockwork_binding_is_empty_and_disabled; then
        return 0
    fi
    printf '%s\n' \
        'annals migration: cannot prove annals/inbox is unselected and disabled; legacy schedule remains stopped' \
        >&2
    return 1
}

finish_clockwork_handoff() {
    [ -f "$CLOCKWORK_HANDOFF" ] && [ ! -L "$CLOCKWORK_HANDOFF" ] \
        || fail "committed migration has no Clockwork definition handoff: $CLOCKWORK_HANDOFF"

    definition_output=$(run_as_operator "$clockwork_path" --json \
        definition register "$CLOCKWORK_HANDOFF") \
        || fail 'Clockwork rejected the committed inbox definition'
    definition_compact=$(printf '%s' "$definition_output" | tr -d '[:space:]')
    definition_digest=$(printf '%s\n' "$definition_compact" | sed -n \
        's/.*"digest":"\([0-9a-f]\{64\}\)".*/\1/p')
    [ -n "$definition_digest" ] \
        || fail 'Clockwork returned no committed definition digest'

    receipt="$TARGET_STATE/install/last-update.json"
    [ -f "$receipt" ] && [ ! -L "$receipt" ] \
        || fail "committed migration has no Annals deployment receipt: $receipt"
    recorded_definition=$(sed -n \
        's/^  "clockwork_definition": \(.*\),$/\1/p' "$receipt")
    case "$recorded_definition" in
        null)
            temporary_receipt="$receipt.migration.$$"
            awk -v digest="$definition_digest" '
                $0 == "  \"clockwork_definition\": null," {
                    print "  \"clockwork_definition\": \"" digest "\","; replaced++; next
                }
                { print }
                END { if (replaced != 1) exit 1 }
            ' "$receipt" >"$temporary_receipt" \
                || fail 'unable to record the committed Clockwork definition'
            chmod 0600 "$temporary_receipt"
            chown "$operator:$operator_group" "$temporary_receipt"
            mv -f "$temporary_receipt" "$receipt"
            ;;
        "\"$definition_digest\"") ;;
        *) fail 'Annals deployment receipt selects another Clockwork definition' ;;
    esac

    inspect_clockwork_binding \
        || fail 'unable to inspect the committed annals/inbox binding'
    if [ -n "$inspected_clockwork_digest" ] \
        && [ "$inspected_clockwork_digest" != "$definition_digest" ]
    then
        fail 'annals/inbox selects another definition during committed migration'
    fi
    if [ "$inspected_clockwork_enabled" -eq 1 ]; then
        [ "$inspected_clockwork_digest" = "$definition_digest" ] \
            || fail 'enabled annals/inbox has no exact committed Annals definition'
        return 0
    fi

    run_as_operator "$clockwork_path" --json binding switch \
        annals/inbox "$definition_digest" >/dev/null \
        || fail 'Clockwork rejected the committed inbox binding switch'
}

restore_legacy() {
    ensure_clockwork_disabled || return 1
    if { [ -e "$USER_PLIST" ] || [ -L "$USER_PLIST" ]; } \
        && ! legacy_agent_plist_matches_expected "$USER_PLIST"
    then
        printf 'annals migration: refusing non-owned user LaunchAgent during rollback: %s\n' \
            "$USER_PLIST" >&2
        return 1
    fi
    "$launchctl_path" bootout "$USER_TARGET" >/dev/null 2>&1 || true
    if [ -e "$USER_PLIST" ] || [ -L "$USER_PLIST" ]; then
        legacy_agent_plist_matches_expected "$USER_PLIST" || {
            printf 'annals migration: refusing non-owned user LaunchAgent during rollback: %s\n' \
                "$USER_PLIST" >&2
            return 1
        }
        rm -f "$USER_PLIST" || return 1
    fi
    rm -f "$USER_CLI" "$USER_USAGE_CLI"
    if [ ! -d "$LEGACY_STATE" ] && [ -d "$TARGET_STATE" ]; then
        mv "$TARGET_STATE" "$LEGACY_STATE"
    fi
    if [ -d "$LEGACY_STATE" ]; then
        rm -f "$LEGACY_STATE/install/.migration-annals-inbox.clockwork.toml"
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
        legacy_daemon_plist_matches_expected "$LEGACY_PLIST" || {
            printf 'annals migration: refusing non-owned legacy LaunchDaemon during rollback: %s\n' \
                "$LEGACY_PLIST" >&2
            return 1
        }
        "$launchctl_path" enable "$SYSTEM_TARGET" >/dev/null 2>&1 || true
        if ! "$launchctl_path" print "$SYSTEM_TARGET" >/dev/null 2>&1; then
            "$launchctl_path" bootstrap system "$LEGACY_PLIST" >/dev/null
        fi
        "$launchctl_path" kickstart "$SYSTEM_TARGET" >/dev/null 2>&1 || true
    fi
    rm -rf "$TRANSACTION_DIR"
}

retire_legacy() {
    if { [ -e "$LEGACY_PLIST" ] || [ -L "$LEGACY_PLIST" ]; } \
        && ! legacy_daemon_plist_matches_expected "$LEGACY_PLIST"
    then
        fail "refusing to retire a non-owned legacy LaunchDaemon: $LEGACY_PLIST"
    fi
    inspect_legacy_system_service \
        || fail "unable to inspect the legacy service: $SYSTEM_TARGET"
    if [ "$legacy_system_service_loaded" -eq 1 ]; then
        "$launchctl_path" bootout "$SYSTEM_TARGET" >/dev/null 2>&1 \
            || fail "unable to boot out the legacy service: $SYSTEM_TARGET"
    fi
    inspect_legacy_system_service \
        || fail "unable to prove the legacy service absent: $SYSTEM_TARGET"
    [ "$legacy_system_service_loaded" -eq 0 ] \
        || fail "legacy service is still loaded: $SYSTEM_TARGET"
    if [ -e "$LEGACY_PLIST" ] || [ -L "$LEGACY_PLIST" ]; then
        legacy_daemon_plist_matches_expected "$LEGACY_PLIST" \
            || fail "refusing to remove a non-owned legacy LaunchDaemon: $LEGACY_PLIST"
        rm -f "$LEGACY_PLIST"
    fi
    rm -f "$LEGACY_FRONTEND" "$LEGACY_PAYLOAD"
    rmdir "$(dirname "$LEGACY_PAYLOAD")" >/dev/null 2>&1 || true
}

committed=0
cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    if [ "$status" -ne 0 ] && [ "$committed" -eq 0 ] \
        && [ -d "$TRANSACTION_DIR" ]; then
        phase_at_failure=$(sed -n '1p' "$TRANSACTION_DIR/phase" 2>/dev/null || true)
        if [ "$phase_at_failure" != committed ]; then
            set +e
            restore_legacy
            set -e
        fi
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
            finish_clockwork_handoff
            retire_legacy
            rm -f "$MAINTENANCE_MARKER"
            rm -rf "$TRANSACTION_DIR"
            rm -f "$CLOCKWORK_HANDOFF"
            printf '%s\n' 'Annals migration recovery completed.'
            exit 0
            ;;
        prepared|stopped|moved|rewritten)
            restore_legacy
            ;;
        *) fail "invalid migration transaction: $TRANSACTION_DIR" ;;
    esac
fi

# The system job is the only schedule this migration knows how to recover.
# Accept only no binding or Clockwork's unselected disabled tombstone before
# touching the legacy service or state. A selected digest belongs to some
# installation lifecycle that this migration has no authority to replace.
inspect_clockwork_binding \
    || fail 'unable to inspect the annals/inbox Clockwork binding'
clockwork_binding_is_empty_and_disabled \
    || fail 'annals/inbox already selects a Clockwork definition'

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
[ ! -e "$USER_USAGE_CLI" ] && [ ! -L "$USER_USAGE_CLI" ] \
    || fail "user usage command already exists: $USER_USAGE_CLI"
grep -Fx 'library = "/Library/Application Support/Annals/annals.db"' \
    "$LEGACY_STATE/config.toml" >/dev/null \
    || fail 'legacy config has a nonstandard library path'
grep -Fx 'root = "/Library/Application Support/Annals/spool"' \
    "$LEGACY_STATE/config.toml" >/dev/null \
    || fail 'legacy config has a nonstandard inbox path'
if [ -L "$LEGACY_PAUSED_MARKER" ] \
    || { [ -e "$LEGACY_PAUSED_MARKER" ] && [ ! -f "$LEGACY_PAUSED_MARKER" ]; }
then
    fail "invalid legacy inbox pause marker: $LEGACY_PAUSED_MARKER"
fi

run_as_operator "$binary_path" --version >/dev/null
run_as_operator "$usage_binary_path" --version >/dev/null
run_as_operator "$LEGACY_FRONTEND" stats >/dev/null \
    || fail 'the legacy library cannot be inspected'

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

# The child may commit its user-state cutover, but it must return an inert
# definition while this outer transaction can still move that state back.
run_as_operator "$deploy_path" \
    --binary "$binary_path" \
    --usage-binary "$usage_binary_path" \
    --nucleus "$nucleus_path" \
    --nucleus-socket "$nucleus_socket" \
    --clockwork "$clockwork_path" \
    --home "$operator_home" \
    --launchctl "$launchctl_path" \
    --fresh-state \
    --migration-clockwork-handoff

[ -f "$CLOCKWORK_HANDOFF" ] && [ ! -L "$CLOCKWORK_HANDOFF" ] \
    || fail 'Annals deployer returned no Clockwork definition handoff'
# From this phase onward TARGET_STATE is permanent. Registration and the
# RunAtLoad-capable binding switch are now recoverable, and maintenance stays
# in place until the legacy artifacts are retired and the binding is selected.
write_phase committed
committed=1
finish_clockwork_handoff
retire_legacy
rm -f "$MAINTENANCE_MARKER"
rm -rf "$TRANSACTION_DIR"
rm -f "$CLOCKWORK_HANDOFF"

printf '%s\n' 'Annals was migrated to the user-owned installation.'
printf 'Operator: %s\n' "$operator"
printf 'State:    %s\n' "$TARGET_STATE"
printf 'Command:  %s\n' "$USER_CLI"
printf 'Usage:    %s\n' "$USER_USAGE_CLI"
printf 'Schedule: %s\n' 'annals/inbox'
