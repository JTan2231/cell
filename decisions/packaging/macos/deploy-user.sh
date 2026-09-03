#!/bin/sh

set -eu

PATH=/usr/bin:/bin:/usr/sbin:/sbin
export PATH
umask 077

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
DAILY_LABEL=org.decisions.daily-email
OBSERVER_LABEL=org.decisions.observer
DAILY_CLOCKWORK_KEY=decisions/daily-email
OBSERVER_CLOCKWORK_KEY=decisions/observer
SOURCE_FRONTEND="$SCRIPT_DIR/decisions"
SOURCE_DAILY_RUNNER="$SCRIPT_DIR/decisions-daily-email"
SOURCE_OBSERVER_RUNNER="$SCRIPT_DIR/decisions-observer"
SOURCE_DAILY_DEFINITION="$SCRIPT_DIR/decisions-daily-email.clockwork.toml.in"
SOURCE_OBSERVER_DEFINITION="$SCRIPT_DIR/decisions-observer.clockwork.toml.in"
SOURCE_HOOKS="$SCRIPT_DIR/hooks.json"
SOURCE_UNINSTALLER="$SCRIPT_DIR/uninstall-user.sh"
if [ -d "$SCRIPT_DIR/../share/chancery/decisions" ]; then
    SOURCE_CHANCERY="$SCRIPT_DIR/../share/chancery/decisions"
else
    SOURCE_CHANCERY="$SCRIPT_DIR/../../chancery"
fi

binary_path=
clockwork_path=
install_home=${HOME:-}
launchctl_path=/bin/launchctl

fail() {
    printf 'decisions user deploy: %s\n' "$*" >&2
    exit 1
}

usage() {
    printf '%s\n' 'Usage: deploy-user.sh --binary ABSOLUTE_PATH --clockwork ABSOLUTE_PATH [--home ABSOLUTE_PATH] [--launchctl ABSOLUTE_PATH]'
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --binary) [ "$#" -ge 2 ] || fail '--binary requires a path'; binary_path=$2; shift 2 ;;
        --clockwork) [ "$#" -ge 2 ] || fail '--clockwork requires a path'; clockwork_path=$2; shift 2 ;;
        --home) [ "$#" -ge 2 ] || fail '--home requires a path'; install_home=$2; shift 2 ;;
        --launchctl) [ "$#" -ge 2 ] || fail '--launchctl requires a path'; launchctl_path=$2; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *) fail "unknown argument: $1" ;;
    esac
done

[ -n "$binary_path" ] || fail '--binary is required'
[ -n "$clockwork_path" ] || fail '--clockwork is required'
case "$binary_path" in /*) ;; *) fail 'binary must be absolute' ;; esac
case "$clockwork_path" in /*) ;; *) fail 'clockwork must be absolute' ;; esac
case "$install_home" in /*) ;; *) fail 'home must be absolute' ;; esac
case "$launchctl_path" in /*) ;; *) fail 'launchctl must be absolute' ;; esac
case "$install_home" in *'&'*|*'<'*|*'>'*|*'|'*|*'"'*|*'\'*|*'
'*) fail 'home contains characters unsupported by schedule rendering' ;; esac
[ -d "$install_home" ] && [ ! -L "$install_home" ] || fail 'home is not a regular directory'
operator_uid=$(id -u)
[ "$operator_uid" -ne 0 ] || fail 'run as the Decisions operator, not root'
[ "$(stat -f '%u' "$install_home")" -eq "$operator_uid" ] \
    || fail 'home is not owned by the Decisions operator'
[ -f "$binary_path" ] && [ ! -L "$binary_path" ] && [ -x "$binary_path" ] || fail 'candidate is not an executable regular file'
[ -e "$clockwork_path" ] && [ -x "$clockwork_path" ] || fail 'Clockwork executable is unavailable'
[ -x "$launchctl_path" ] && [ ! -L "$launchctl_path" ] || fail 'launchctl is unavailable'
[ -x /usr/sbin/lsof ] || fail 'lsof is unavailable'
for source in "$SOURCE_FRONTEND" "$SOURCE_DAILY_RUNNER" "$SOURCE_OBSERVER_RUNNER" \
    "$SOURCE_DAILY_DEFINITION" "$SOURCE_OBSERVER_DEFINITION" \
    "$SOURCE_HOOKS" "$SOURCE_UNINSTALLER"
do
    [ -f "$source" ] && [ ! -L "$source" ] || fail "missing packaged file: $source"
done
validate_bundle() {
    bundle=$1
    [ -d "$bundle" ] && [ ! -L "$bundle" ] || fail "Chancery provider is not a regular directory: $bundle"
    [ -f "$bundle/provider.json" ] && [ ! -L "$bundle/provider.json" ] || fail "Chancery provider manifest is missing: $bundle"
    if find "$bundle" -type l -print | grep -q .; then fail "Chancery provider contains a symbolic link: $bundle"; fi
    if find "$bundle" ! -type d ! -type f -print | grep -q .; then fail "Chancery provider contains a non-file entry: $bundle"; fi
}

bundle_hash() {
    bundle=$1
    (
        cd "$bundle"
        find . -type f -print | LC_ALL=C sort | while IFS= read -r file; do
            printf 'path=%s\n' "$file"
            shasum -a 256 "$file"
        done
    ) | shasum -a 256 | awk '{print $1}'
}

maintenance_marker_is_owned() {
    [ -f "$MAINTENANCE_MARKER" ] && [ ! -L "$MAINTENANCE_MARKER" ] \
        && [ "$(stat -f '%u' "$MAINTENANCE_MARKER")" -eq "$operator_uid" ] \
        && [ "$(stat -f '%Lp' "$MAINTENANCE_MARKER")" = 600 ] \
        && [ "$(stat -f '%l' "$MAINTENANCE_MARKER")" -eq 1 ]
}

engage_maintenance() {
    if [ -L "$MAINTENANCE_MARKER" ] \
        || { [ -e "$MAINTENANCE_MARKER" ] && [ ! -f "$MAINTENANCE_MARKER" ]; }
    then
        return 1
    fi
    if [ -e "$MAINTENANCE_MARKER" ]; then
        maintenance_marker_is_owned
        return
    fi
    (set -C; : >"$MAINTENANCE_MARKER") || return 1
    maintenance_created=1
    chmod 0600 "$MAINTENANCE_MARKER" || return 1
    maintenance_marker_is_owned
}

prepare_private_log() {
    log_path=$1
    if [ -L "$log_path" ] || { [ -e "$log_path" ] && [ ! -f "$log_path" ]; }; then
        fail "Decisions log path is not a regular file: $log_path"
    fi
    [ -e "$log_path" ] || return 0
    [ "$(stat -f '%u' "$log_path")" -eq "$operator_uid" ] \
        || fail "Decisions log is not owned by the operator: $log_path"
    [ "$(stat -f '%l' "$log_path")" -eq 1 ] \
        || fail "Decisions log must not be hard-linked: $log_path"
    chmod 0600 "$log_path" \
        || fail "unable to make the Decisions log private: $log_path"
}

render_legacy_plist() {
    legacy_kind=$1
    legacy_template=$2
    legacy_output=$3
    [ -f "$legacy_template" ] && [ ! -L "$legacy_template" ] || return 1
    case "$legacy_kind" in
        daily)
            legacy_runner="$INSTALL_DIR/current/bin/decisions-daily-email"
            legacy_stdout="$LOG_DIR/daily-email.stdout.log"
            legacy_stderr="$LOG_DIR/daily-email.stderr.log"
            sed \
                -e "s|__DECISIONS_RUNNER__|$legacy_runner|g" \
                -e "s|__DECISIONS_STATE_DIR__|$STATE_DIR|g" \
                -e "s|__DECISIONS_HOME__|$install_home|g" \
                -e "s|__DECISIONS_STDOUT__|$legacy_stdout|g" \
                -e "s|__DECISIONS_STDERR__|$legacy_stderr|g" \
                "$legacy_template" >"$legacy_output"
            ;;
        observer)
            legacy_runner="$INSTALL_DIR/current/bin/decisions-observer"
            legacy_stdout="$LOG_DIR/observer.stdout.log"
            legacy_stderr="$LOG_DIR/observer.stderr.log"
            sed \
                -e "s|__DECISIONS_OBSERVER_RUNNER__|$legacy_runner|g" \
                -e "s|__DECISIONS_STATE_DIR__|$STATE_DIR|g" \
                -e "s|__DECISIONS_HOME__|$install_home|g" \
                -e "s|__DECISIONS_OBSERVER_STDOUT__|$legacy_stdout|g" \
                -e "s|__DECISIONS_OBSERVER_STDERR__|$legacy_stderr|g" \
                "$legacy_template" >"$legacy_output"
            ;;
        *) return 1 ;;
    esac
}

legacy_plist_matches_expected() {
    legacy_candidate=$1
    legacy_expected=$2
    [ -f "$legacy_candidate" ] && [ ! -L "$legacy_candidate" ] \
        && [ "$(stat -f '%u' "$legacy_candidate")" -eq "$operator_uid" ] \
        && [ "$(stat -f '%Lp' "$legacy_candidate")" = 644 ] \
        && [ -f "$legacy_expected" ] && [ ! -L "$legacy_expected" ] \
        && cmp -s "$legacy_expected" "$legacy_candidate"
}

restore_legacy_plist() {
    legacy_backup=$1
    legacy_expected=$2
    legacy_destination=$3
    legacy_plist_matches_expected "$legacy_backup" "$legacy_expected" \
        && [ ! -e "$legacy_destination" ] && [ ! -L "$legacy_destination" ] \
        && cp -p "$legacy_backup" "$legacy_destination" \
        && legacy_plist_matches_expected "$legacy_destination" "$legacy_expected"
}

validate_bundle "$SOURCE_CHANCERY"

atomic_symlink() {
    target=$1
    path=$2
    temporary_link="$path.tmp.$$"
    rm -f "$temporary_link"
    ln -s "$target" "$temporary_link"
    if mv -fh "$temporary_link" "$path" 2>/dev/null; then
        return 0
    fi
    mv -fT "$temporary_link" "$path"
}

candidate_version=$("$binary_path" --version) || fail 'unable to read candidate version'
case "$candidate_version" in 'decisions '*) version=${candidate_version#decisions } ;; *) fail "unexpected candidate version: $candidate_version" ;; esac
provider_version=$(awk -F '"' '/"release"[[:space:]]*:/ { print $4; exit }' "$SOURCE_CHANCERY/provider.json")
[ "$provider_version" = "$version" ] || fail "provider release $provider_version does not match candidate $version"

STATE_DIR="$install_home/Library/Application Support/Decisions"
INSTALL_DIR="$STATE_DIR/install"
RELEASES_DIR="$INSTALL_DIR/releases"
CURRENT_LINK="$INSTALL_DIR/current"
PREVIOUS_LINK="$INSTALL_DIR/previous"
LOCK_DIR="$INSTALL_DIR/.update-lock"
CLI_DIR="$install_home/.local/bin"
CLI_PATH="$CLI_DIR/decisions"
AGENT_DIR="$install_home/Library/LaunchAgents"
DAILY_PLIST="$AGENT_DIR/$DAILY_LABEL.plist"
OBSERVER_PLIST="$AGENT_DIR/$OBSERVER_LABEL.plist"
LOG_DIR="$install_home/Library/Logs/Decisions"
DATABASE_PATH="$STATE_DIR/decisions.db"
MAINTENANCE_MARKER="$STATE_DIR/.clockwork-maintenance"
CODEX_DIR="$install_home/.codex"
HOOKS_PATH="$CODEX_DIR/hooks.json"
PROVIDERS_DIR="$install_home/Library/Application Support/Chancery/providers"
PROVIDER_LINK="$PROVIDERS_DIR/decisions"
SERVICE_DOMAIN="gui/$operator_uid"
DAILY_TARGET="$SERVICE_DOMAIN/$DAILY_LABEL"
OBSERVER_TARGET="$SERVICE_DOMAIN/$OBSERVER_LABEL"

for directory in "$STATE_DIR" "$INSTALL_DIR" "$RELEASES_DIR" "$LOG_DIR"; do
    [ ! -L "$directory" ] || fail "refusing symbolic-link directory: $directory"
    [ ! -e "$directory" ] || [ -d "$directory" ] || fail "directory path is occupied: $directory"
    if [ ! -d "$directory" ]; then install -d -m 0700 "$directory"; fi
done
for directory in "$CLI_DIR" "$AGENT_DIR" "$PROVIDERS_DIR"; do
    [ ! -L "$directory" ] || fail "refusing symbolic-link directory: $directory"
    [ ! -e "$directory" ] || [ -d "$directory" ] || fail "directory path is occupied: $directory"
    if [ ! -d "$directory" ]; then install -d -m 0755 "$directory"; fi
done
if [ -L "$CODEX_DIR" ]; then fail "refusing symbolic-link directory: $CODEX_DIR"; fi
if [ -e "$CODEX_DIR" ] && [ ! -d "$CODEX_DIR" ]; then fail "directory path is occupied: $CODEX_DIR"; fi
if [ ! -d "$CODEX_DIR" ]; then install -d -m 0700 "$CODEX_DIR"; fi
# Defer catchable termination across the atomic mkdir until the full cleanup
# trap owns the newly acquired directory lock.
trap '' HUP INT TERM
mkdir "$LOCK_DIR" 2>/dev/null || fail 'another Decisions deployment is active'
temporary=
temporary_daily_definition=
temporary_observer_definition=
transaction_dir=
old_current=
old_previous=
old_cli=
old_provider=
old_daily_plist=
old_observer_plist=
expected_old_daily_plist=
expected_old_observer_plist=
old_hooks=
prior_daily_clockwork_digest=
prior_observer_clockwork_digest=
prior_daily_clockwork_enabled=0
prior_observer_clockwork_enabled=0
old_clockwork_release=
old_clockwork_release_id=
old_clockwork_format=
old_daily_clockwork_runner_hash=
old_observer_clockwork_runner_hash=
clockwork_disabled=0
daily_clockwork_switched=0
observer_clockwork_switched=0
candidate_daily_definition_digest=
candidate_observer_definition_digest=
switched=0
committed=0
daily_was_loaded=0
observer_was_loaded=0
daily_service_stopped=0
observer_service_stopped=0
daily_plist_changed=0
observer_plist_changed=0
hooks_changed=0
cli_suspended=0
database_touched=0
database_was_absent=0
retain_transaction=0
maintenance_created=0
cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    set +e
    if [ "$status" -ne 0 ] && [ "$committed" -eq 0 ]; then
        rollback_ready=1
        if [ "$clockwork_disabled" -eq 1 ] || [ "$daily_clockwork_switched" -eq 1 ] \
            || [ "$observer_clockwork_switched" -eq 1 ]; then
            HOME="$install_home" "$clockwork_path" --json binding disable \
                "$OBSERVER_CLOCKWORK_KEY" >/dev/null 2>&1 || rollback_ready=0
            HOME="$install_home" "$clockwork_path" --json binding disable \
                "$DAILY_CLOCKWORK_KEY" >/dev/null 2>&1 || rollback_ready=0
        fi
        # Clockwork can restore an exact disabled digest, but it cannot clear a
        # selection back to null. Once a previously unselected binding has been
        # switched, do not roll selectors back to a release that cannot own the
        # retained candidate digest.
        if { [ "$daily_clockwork_switched" -eq 1 ] \
                && [ -z "$prior_daily_clockwork_digest" ]; } \
            || { [ "$observer_clockwork_switched" -eq 1 ] \
                && [ -z "$prior_observer_clockwork_digest" ]; }
        then
            rollback_ready=0
        fi
        if [ "$observer_plist_changed" -eq 1 ]; then
            "$launchctl_path" bootout "$OBSERVER_TARGET" >/dev/null 2>&1 || true
        fi
        if [ "$daily_plist_changed" -eq 1 ]; then
            "$launchctl_path" bootout "$DAILY_TARGET" >/dev/null 2>&1 || true
        fi
        public_cli_was_present=0
        if [ "$switched" -eq 1 ] && { [ -e "$CLI_PATH" ] || [ -L "$CLI_PATH" ]; }; then
            public_cli_was_present=1
            rm -f "$CLI_PATH" || rollback_ready=0
        fi
        if [ "$public_cli_was_present" -eq 1 ]; then /bin/sleep 3; fi
        if [ "$database_touched" -eq 1 ] && [ -f "$DATABASE_PATH" ]; then
            if /usr/sbin/lsof -t -- "$DATABASE_PATH" >/dev/null 2>&1; then
                rollback_ready=0
            else
                rollback_lsof_status=$?
                [ "$rollback_lsof_status" -eq 1 ] || rollback_ready=0
            fi
        fi
        if [ "$database_touched" -eq 1 ] && [ "$rollback_ready" -eq 1 ]; then
            rm -f "$DATABASE_PATH" "$DATABASE_PATH-wal" "$DATABASE_PATH-shm" "$DATABASE_PATH-journal" \
                || rollback_ready=0
            if [ "$database_was_absent" -eq 0 ]; then
                install -m 0600 "$transaction_dir/decisions.db" "$DATABASE_PATH" \
                    || rollback_ready=0
                for suffix in wal shm journal; do
                    [ ! -f "$transaction_dir/decisions.db-$suffix" ] || \
                        install -m 0600 "$transaction_dir/decisions.db-$suffix" "$DATABASE_PATH-$suffix" \
                        || rollback_ready=0
                done
            fi
        fi
        if [ "$daily_plist_changed" -eq 1 ] && [ "$rollback_ready" -eq 1 ]; then
            if [ -n "$old_daily_plist" ]; then
                restore_legacy_plist "$old_daily_plist" \
                    "$expected_old_daily_plist" "$DAILY_PLIST" || rollback_ready=0
            else
                rm -f "$DAILY_PLIST" || rollback_ready=0
            fi
        fi
        if [ "$observer_plist_changed" -eq 1 ] && [ "$rollback_ready" -eq 1 ]; then
            if [ -n "$old_observer_plist" ]; then
                restore_legacy_plist "$old_observer_plist" \
                    "$expected_old_observer_plist" "$OBSERVER_PLIST" || rollback_ready=0
            else
                rm -f "$OBSERVER_PLIST" || rollback_ready=0
            fi
        fi
        if [ "$hooks_changed" -eq 1 ] && [ "$rollback_ready" -eq 1 ]; then
            if [ -n "$old_hooks" ]; then
                cp -p "$old_hooks" "$HOOKS_PATH" || rollback_ready=0
            else
                rm -f "$HOOKS_PATH" || rollback_ready=0
            fi
        fi
        if [ "$switched" -eq 1 ] && [ "$rollback_ready" -eq 1 ]; then
            if [ -n "$old_current" ]; then
                atomic_symlink "$old_current" "$CURRENT_LINK" || rollback_ready=0
            else
                rm -f "$CURRENT_LINK" || rollback_ready=0
            fi
            if [ -n "$old_previous" ]; then
                atomic_symlink "$old_previous" "$PREVIOUS_LINK" || rollback_ready=0
            else
                rm -f "$PREVIOUS_LINK" || rollback_ready=0
            fi
            if [ -n "$old_provider" ]; then
                atomic_symlink "$old_provider" "$PROVIDER_LINK" || rollback_ready=0
            else
                rm -f "$PROVIDER_LINK" || rollback_ready=0
            fi
        fi
        if [ "$rollback_ready" -eq 1 ] && { [ "$switched" -eq 1 ] || [ "$cli_suspended" -eq 1 ]; }; then
            if [ -n "$old_cli" ]; then
                atomic_symlink "$old_cli" "$CLI_PATH" || rollback_ready=0
            else
                rm -f "$CLI_PATH" || rollback_ready=0
            fi
        fi
        if [ "$rollback_ready" -eq 1 ] && [ "$clockwork_disabled" -eq 1 ]; then
            if [ "$prior_daily_clockwork_enabled" -eq 1 ] \
                && [ -n "$prior_daily_clockwork_digest" ]; then
                HOME="$install_home" "$clockwork_path" --json binding switch \
                    "$DAILY_CLOCKWORK_KEY" "$prior_daily_clockwork_digest" \
                    >/dev/null 2>&1 || rollback_ready=0
            elif [ -n "$prior_daily_clockwork_digest" ]; then
                HOME="$install_home" "$clockwork_path" --json binding disable \
                    "$DAILY_CLOCKWORK_KEY" --select "$prior_daily_clockwork_digest" \
                    >/dev/null 2>&1 || rollback_ready=0
            else
                HOME="$install_home" "$clockwork_path" --json binding disable \
                    "$DAILY_CLOCKWORK_KEY" >/dev/null 2>&1 || rollback_ready=0
            fi
            if [ "$prior_observer_clockwork_enabled" -eq 1 ] \
                && [ -n "$prior_observer_clockwork_digest" ]; then
                HOME="$install_home" "$clockwork_path" --json binding switch \
                    "$OBSERVER_CLOCKWORK_KEY" "$prior_observer_clockwork_digest" \
                    >/dev/null 2>&1 || rollback_ready=0
            elif [ -n "$prior_observer_clockwork_digest" ]; then
                HOME="$install_home" "$clockwork_path" --json binding disable \
                    "$OBSERVER_CLOCKWORK_KEY" --select "$prior_observer_clockwork_digest" \
                    >/dev/null 2>&1 || rollback_ready=0
            else
                HOME="$install_home" "$clockwork_path" --json binding disable \
                    "$OBSERVER_CLOCKWORK_KEY" >/dev/null 2>&1 || rollback_ready=0
            fi
        fi
        if [ "$rollback_ready" -eq 1 ] && [ "$prior_daily_clockwork_enabled" -eq 0 ] \
            && [ "$daily_service_stopped" -eq 1 ] && [ "$daily_was_loaded" -eq 1 ] && [ -n "$old_daily_plist" ]; then
            legacy_plist_matches_expected "$DAILY_PLIST" "$expected_old_daily_plist" \
                && "$launchctl_path" bootstrap "$SERVICE_DOMAIN" "$DAILY_PLIST" >/dev/null 2>&1 \
                || rollback_ready=0
        fi
        if [ "$rollback_ready" -eq 1 ] && [ "$prior_observer_clockwork_enabled" -eq 0 ] \
            && [ "$observer_service_stopped" -eq 1 ] && [ "$observer_was_loaded" -eq 1 ] && [ -n "$old_observer_plist" ]; then
            legacy_plist_matches_expected "$OBSERVER_PLIST" "$expected_old_observer_plist" \
                && "$launchctl_path" bootstrap "$SERVICE_DOMAIN" "$OBSERVER_PLIST" >/dev/null 2>&1 \
                || rollback_ready=0
        fi
        if [ "$rollback_ready" -eq 1 ] && [ "$maintenance_created" -eq 1 ]; then
            maintenance_marker_is_owned && rm -f "$MAINTENANCE_MARKER" \
                && [ ! -e "$MAINTENANCE_MARKER" ] && [ ! -L "$MAINTENANCE_MARKER" ] \
                || rollback_ready=0
            [ "$rollback_ready" -eq 0 ] || maintenance_created=0
        fi
        if [ "$rollback_ready" -eq 0 ]; then
            HOME="$install_home" "$clockwork_path" --json binding disable \
                "$OBSERVER_CLOCKWORK_KEY" >/dev/null 2>&1 || true
            HOME="$install_home" "$clockwork_path" --json binding disable \
                "$DAILY_CLOCKWORK_KEY" >/dev/null 2>&1 || true
            "$launchctl_path" bootout "$OBSERVER_TARGET" >/dev/null 2>&1 || true
            "$launchctl_path" bootout "$DAILY_TARGET" >/dev/null 2>&1 || true
            maintenance_evidence='maintenance gate could not be proven'
            maintenance_marker_is_owned \
                && maintenance_evidence='a valid maintenance gate is retained'
            public_command_evidence='public command removal was attempted'
            if rm -f "$CLI_PATH" \
                && [ ! -e "$CLI_PATH" ] && [ ! -L "$CLI_PATH" ]; then
                public_command_evidence='the public command is disabled'
            fi
            retain_transaction=1
            printf 'decisions user deploy: rollback could not prove quiescence or restore every owned artifact; %s, scheduler cleanup was attempted, and %s\n' \
                "$maintenance_evidence" "$public_command_evidence" >&2
            printf 'decisions user deploy: private rollback backup retained at %s\n' "$transaction_dir" >&2
        fi
    fi
    [ -z "$temporary" ] || rm -rf "$temporary"
    [ -z "$temporary_daily_definition" ] || rm -f "$temporary_daily_definition"
    [ -z "$temporary_observer_definition" ] || rm -f "$temporary_observer_definition"
    [ "$retain_transaction" -eq 1 ] || [ -z "$transaction_dir" ] || rm -rf "$transaction_dir"
    rmdir "$LOCK_DIR" >/dev/null 2>&1 || true
    exit "$status"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

temporary=$(mktemp -d "$INSTALL_DIR/.candidate.XXXXXX") \
    || fail 'unable to create the Decisions release staging directory'
temporary_daily_definition=$(mktemp "$INSTALL_DIR/.daily-clockwork.XXXXXX") \
    || fail 'unable to create the daily Clockwork definition staging file'
temporary_observer_definition=$(mktemp "$INSTALL_DIR/.observer-clockwork.XXXXXX") \
    || fail 'unable to create the observer Clockwork definition staging file'
transaction_dir=$(mktemp -d "$INSTALL_DIR/.transaction.XXXXXX") \
    || fail 'unable to create the Decisions transaction directory'

if [ -L "$CURRENT_LINK" ]; then old_current=$(readlink "$CURRENT_LINK"); elif [ -e "$CURRENT_LINK" ]; then fail "$CURRENT_LINK must be a symbolic link"; fi
if [ -L "$PREVIOUS_LINK" ]; then old_previous=$(readlink "$PREVIOUS_LINK"); elif [ -e "$PREVIOUS_LINK" ]; then fail "$PREVIOUS_LINK must be a symbolic link"; fi
if [ -L "$CLI_PATH" ]; then old_cli=$(readlink "$CLI_PATH"); elif [ -e "$CLI_PATH" ]; then fail "$CLI_PATH exists and is not a symbolic link"; fi
if [ -L "$PROVIDER_LINK" ]; then old_provider=$(readlink "$PROVIDER_LINK"); elif [ -e "$PROVIDER_LINK" ]; then fail "$PROVIDER_LINK exists and is not a symbolic link"; fi
expected_cli="$INSTALL_DIR/current/bin/decisions"
expected_provider="$INSTALL_DIR/current/share/chancery/decisions"
validate_release_selector() {
    selector=$1
    printf '%s\n' "$selector" | grep -Eq '^releases/[0-9a-f]{64}$' \
        || fail "invalid Decisions release selector: $selector"
    selected_release="$INSTALL_DIR/$selector"
    [ -d "$selected_release" ] && [ ! -L "$selected_release" ] \
        || fail "selected Decisions release is unavailable: $selector"
    [ -f "$selected_release/manifest.txt" ] && [ ! -L "$selected_release/manifest.txt" ] \
        || fail "selected Decisions release has no owned manifest: $selector"
    selector_id=${selector#releases/}
    selected_manifest="$selected_release/manifest.txt"
    selected_format=$(sed -n '1s/^format=//p' "$selected_manifest")
    [ "$(awk 'END { print NR }' "$selected_manifest")" -eq 13 ] \
        || fail "selected Decisions release manifest is not canonical: $selector"
    case "$selected_format" in
        2|3) ;;
        *) fail "selected Decisions release manifest format is unsupported: $selector" ;;
    esac
    selected_manifest_release=$(sed -n '2s/^release_id=//p' "$selected_manifest")
    selected_version=$(sed -n '3s/^version=//p' "$selected_manifest")
    selected_binary_hash=$(sed -n '4s/^binary_sha256=//p' "$selected_manifest")
    selected_frontend_hash=$(sed -n '5s/^frontend_sha256=//p' "$selected_manifest")
    selected_daily_runner_hash=$(sed -n '6s/^daily_runner_sha256=//p' "$selected_manifest")
    selected_observer_runner_hash=$(sed -n '7s/^observer_runner_sha256=//p' "$selected_manifest")
    if [ "$selected_format" -eq 2 ]; then
        selected_daily_schedule_hash=$(sed -n '8s/^daily_plist_sha256=//p' "$selected_manifest")
        selected_observer_schedule_hash=$(sed -n '9s/^observer_plist_sha256=//p' "$selected_manifest")
    else
        selected_daily_schedule_hash=$(sed -n '8s/^daily_clockwork_definition_sha256=//p' "$selected_manifest")
        selected_observer_schedule_hash=$(sed -n '9s/^observer_clockwork_definition_sha256=//p' "$selected_manifest")
    fi
    selected_hooks_hash=$(sed -n '10s/^hooks_sha256=//p' "$selected_manifest")
    selected_deployer_hash=$(sed -n '11s/^deployer_sha256=//p' "$selected_manifest")
    selected_uninstaller_hash=$(sed -n '12s/^uninstaller_sha256=//p' "$selected_manifest")
    selected_chancery_hash=$(sed -n '13s/^chancery_sha256=//p' "$selected_manifest")
    printf '%s\n' "$selected_manifest_release" "$selected_binary_hash" "$selected_frontend_hash" \
        "$selected_daily_runner_hash" "$selected_observer_runner_hash" \
        "$selected_daily_schedule_hash" "$selected_observer_schedule_hash" "$selected_hooks_hash" \
        "$selected_deployer_hash" \
        "$selected_uninstaller_hash" "$selected_chancery_hash" \
        | grep -Eqv '^[0-9a-f]{64}$' \
        && fail "selected Decisions release manifest hashes are invalid: $selector"
    printf '%s\n' "$selected_version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+([+-][0-9A-Za-z.-]+)?$' \
        || fail "selected Decisions release version is invalid: $selector"
    [ "$selected_manifest_release" = "$selector_id" ] \
        || fail "selected Decisions release manifest does not match: $selector"
    for owned_file in \
        "$selected_release/libexec/decisions" \
        "$selected_release/bin/decisions" \
        "$selected_release/bin/decisions-daily-email" \
        "$selected_release/bin/decisions-observer" \
        "$selected_release/package/decisions" \
        "$selected_release/package/decisions-daily-email" \
        "$selected_release/package/decisions-observer" \
        "$selected_release/package/deploy-user.sh" \
        "$selected_release/package/uninstall-user.sh" \
        "$selected_release/package/hooks.json"
    do
        [ -f "$owned_file" ] && [ ! -L "$owned_file" ] \
            || fail "selected Decisions release is incomplete: $selector"
    done
    if [ "$selected_format" -eq 2 ]; then
        selected_daily_schedule_file="$selected_release/package/$DAILY_LABEL.plist"
        selected_observer_schedule_file="$selected_release/package/$OBSERVER_LABEL.plist"
    else
        selected_daily_schedule_file="$selected_release/package/decisions-daily-email.clockwork.toml.in"
        selected_observer_schedule_file="$selected_release/package/decisions-observer.clockwork.toml.in"
    fi
    for schedule_file in "$selected_daily_schedule_file" "$selected_observer_schedule_file"; do
        [ -f "$schedule_file" ] && [ ! -L "$schedule_file" ] \
            || fail "selected Decisions release has no owned schedule template: $selector"
    done
    validate_bundle "$selected_release/share/chancery/decisions"
    actual_binary_hash=$(shasum -a 256 "$selected_release/libexec/decisions" | awk '{print $1}')
    actual_frontend_hash=$(shasum -a 256 "$selected_release/bin/decisions" | awk '{print $1}')
    actual_daily_runner_hash=$(shasum -a 256 "$selected_release/bin/decisions-daily-email" | awk '{print $1}')
    actual_observer_runner_hash=$(shasum -a 256 "$selected_release/bin/decisions-observer" | awk '{print $1}')
    actual_daily_schedule_hash=$(shasum -a 256 "$selected_daily_schedule_file" | awk '{print $1}')
    actual_observer_schedule_hash=$(shasum -a 256 "$selected_observer_schedule_file" | awk '{print $1}')
    actual_hooks_hash=$(shasum -a 256 "$selected_release/package/hooks.json" | awk '{print $1}')
    actual_deployer_hash=$(shasum -a 256 "$selected_release/package/deploy-user.sh" | awk '{print $1}')
    actual_uninstaller_hash=$(shasum -a 256 "$selected_release/package/uninstall-user.sh" | awk '{print $1}')
    actual_chancery_hash=$(bundle_hash "$selected_release/share/chancery/decisions")
    [ "$actual_binary_hash" = "$selected_binary_hash" ] \
        || fail "selected Decisions release binary is tampered: $selector"
    [ "$actual_frontend_hash" = "$selected_frontend_hash" ] \
        || fail "selected Decisions release frontend is tampered: $selector"
    [ "$(shasum -a 256 "$selected_release/package/decisions" | awk '{print $1}')" = "$selected_frontend_hash" ] \
        || fail "selected Decisions release packaged frontend is tampered: $selector"
    [ "$actual_daily_runner_hash" = "$selected_daily_runner_hash" ] \
        || fail "selected Decisions release daily runner is tampered: $selector"
    [ "$(shasum -a 256 "$selected_release/package/decisions-daily-email" | awk '{print $1}')" = "$selected_daily_runner_hash" ] \
        || fail "selected Decisions release packaged daily runner is tampered: $selector"
    [ "$actual_observer_runner_hash" = "$selected_observer_runner_hash" ] \
        || fail "selected Decisions release observer runner is tampered: $selector"
    [ "$(shasum -a 256 "$selected_release/package/decisions-observer" | awk '{print $1}')" = "$selected_observer_runner_hash" ] \
        || fail "selected Decisions release packaged observer runner is tampered: $selector"
    [ "$actual_daily_schedule_hash" = "$selected_daily_schedule_hash" ] \
        || fail "selected Decisions release daily schedule template is tampered: $selector"
    [ "$actual_observer_schedule_hash" = "$selected_observer_schedule_hash" ] \
        || fail "selected Decisions release observer schedule template is tampered: $selector"
    [ "$actual_hooks_hash" = "$selected_hooks_hash" ] \
        || fail "selected Decisions release hook definition is tampered: $selector"
    [ "$actual_deployer_hash" = "$selected_deployer_hash" ] \
        || fail "selected Decisions release deployer is tampered: $selector"
    [ "$actual_uninstaller_hash" = "$selected_uninstaller_hash" ] \
        || fail "selected Decisions release uninstaller is tampered: $selector"
    [ "$actual_chancery_hash" = "$selected_chancery_hash" ] \
        || fail "selected Decisions release provider is tampered: $selector"
    actual_release_id=$(printf '%s\n' "$actual_binary_hash" "$actual_frontend_hash" \
        "$actual_daily_runner_hash" "$actual_observer_runner_hash" \
        "$actual_daily_schedule_hash" "$actual_observer_schedule_hash" "$actual_hooks_hash" \
        "$actual_deployer_hash" "$actual_uninstaller_hash" "$actual_chancery_hash" \
        | shasum -a 256 | awk '{print $1}')
    [ "$actual_release_id" = "$selector_id" ] \
        || fail "selected Decisions release content ID does not match: $selector"
}

prove_owned_clockwork_definition() {
    definition_key=$1
    definition_digest=$2
    definition_runner=$3
    definition_runner_hash=$4
    definition_name=$5
    [ -n "$old_clockwork_release" ] \
        || fail "$definition_name Clockwork binding has no current Decisions release"
    [ "$old_clockwork_format" = 3 ] \
        || fail "$definition_name Clockwork binding cannot be owned by a legacy release"
    definition_show="$transaction_dir/$definition_name-clockwork-definition.json"
    HOME="$install_home" "$clockwork_path" --json definition show "$definition_digest" \
        >"$definition_show" 2>"$definition_show.stderr" \
        || fail "unable to inspect the selected $definition_name Clockwork definition"
    [ "$(plutil -extract ok raw "$definition_show" 2>/dev/null)" = true ] \
        && [ "$(plutil -extract data.digest raw "$definition_show" 2>/dev/null)" = "$definition_digest" ] \
        && [ "$(plutil -extract data.key raw "$definition_show" 2>/dev/null)" = "$definition_key" ] \
        && [ "$(plutil -extract data.manifest.schema_version raw "$definition_show" 2>/dev/null)" = 1 ] \
        && [ "$(plutil -extract data.manifest.key raw "$definition_show" 2>/dev/null)" = "$definition_key" ] \
        && [ "$(plutil -extract data.manifest.release_id raw "$definition_show" 2>/dev/null)" = "$old_clockwork_release_id" ] \
        && [ "$(plutil -extract data.manifest.release_root raw "$definition_show" 2>/dev/null)" = "$old_clockwork_release" ] \
        && [ "$(plutil -extract data.manifest.authority raw "$definition_show" 2>/dev/null)" = current-user-background ] \
        && [ "$(plutil -extract data.manifest.overlap raw "$definition_show" 2>/dev/null)" = skip ] \
        && [ "$(plutil -extract data.manifest.cwd raw "$definition_show" 2>/dev/null)" = "$STATE_DIR" ] \
        && [ "$(plutil -extract data.manifest.launch.kind raw "$definition_show" 2>/dev/null)" = interpreted ] \
        && [ "$(plutil -extract data.manifest.launch.interpreter raw "$definition_show" 2>/dev/null)" = /bin/sh ] \
        && [ "$(plutil -extract data.manifest.launch.interpreter_sha256 raw "$definition_show" 2>/dev/null)" = "$interpreter_hash" ] \
        && [ "$(plutil -extract data.manifest.launch.script raw "$definition_show" 2>/dev/null)" = "$definition_runner" ] \
        && [ "$(plutil -extract data.manifest.launch.script_sha256 raw "$definition_show" 2>/dev/null)" = "$definition_runner_hash" ] \
        && [ "$(plutil -extract data.manifest.environment.HOME raw "$definition_show" 2>/dev/null)" = "$install_home" ] \
        || fail "$definition_name Clockwork definition identity is not owned by the current Decisions release"
    if plutil -extract data.manifest.timeout_seconds raw "$definition_show" >/dev/null 2>&1 \
        || plutil -extract data.manifest.arguments.0 raw "$definition_show" >/dev/null 2>&1; then
        fail "$definition_name Clockwork definition adds unsupported timeout or arguments"
    fi
    environment_keys=$(plutil -extract data.manifest.environment xml1 -o - \
        "$definition_show" 2>/dev/null | awk '/<key>/{count++} END {print count+0}')
    [ "$environment_keys" -eq 1 ] \
        || fail "$definition_name Clockwork definition contains foreign environment entries"
    case "$definition_name" in
        daily)
            [ "$(plutil -extract data.manifest.schedule.kind raw "$definition_show" 2>/dev/null)" = local-calendar ] \
                && [ "$(plutil -extract data.manifest.schedule.hour raw "$definition_show" 2>/dev/null)" = 9 ] \
                && [ "$(plutil -extract data.manifest.schedule.minute raw "$definition_show" 2>/dev/null)" = 0 ] \
                && [ "$(plutil -extract data.manifest.schedule.run_at_load raw "$definition_show" 2>/dev/null)" = false ] \
                && [ "$(plutil -extract data.manifest.output.stdout raw "$definition_show" 2>/dev/null)" = "$LOG_DIR/daily-email.stdout.log" ] \
                && [ "$(plutil -extract data.manifest.output.stderr raw "$definition_show" 2>/dev/null)" = "$LOG_DIR/daily-email.stderr.log" ] \
                || fail 'daily Clockwork definition schedule or output is not owned by Decisions'
            ;;
        observer)
            [ "$(plutil -extract data.manifest.schedule.kind raw "$definition_show" 2>/dev/null)" = interval ] \
                && [ "$(plutil -extract data.manifest.schedule.seconds raw "$definition_show" 2>/dev/null)" = 60 ] \
                && [ "$(plutil -extract data.manifest.schedule.run_at_load raw "$definition_show" 2>/dev/null)" = false ] \
                && [ "$(plutil -extract data.manifest.output.stdout raw "$definition_show" 2>/dev/null)" = "$LOG_DIR/observer.stdout.log" ] \
                && [ "$(plutil -extract data.manifest.output.stderr raw "$definition_show" 2>/dev/null)" = "$LOG_DIR/observer.stderr.log" ] \
                || fail 'observer Clockwork definition schedule or output is not owned by Decisions'
            ;;
        *) fail 'internal Clockwork definition ownership selector is invalid' ;;
    esac
}

interpreter_hash=$(shasum -a 256 /bin/sh | awk '{print $1}')
if [ -n "$old_current" ]; then
    validate_release_selector "$old_current"
    old_clockwork_release="$INSTALL_DIR/$old_current"
    old_clockwork_release_id=${old_current#releases/}
    old_clockwork_format=$selected_format
    old_daily_clockwork_runner_hash=$selected_daily_runner_hash
    old_observer_clockwork_runner_hash=$selected_observer_runner_hash
    if [ -n "$old_previous" ]; then validate_release_selector "$old_previous"; fi
    [ -z "$old_cli" ] || [ "$old_cli" = "$expected_cli" ] || fail "installed command is not owned by Decisions: $CLI_PATH"
elif [ -n "$old_previous" ] || [ -n "$old_cli" ] || [ -n "$old_provider" ]; then
    fail 'installed selectors have no current Decisions release'
fi
[ -z "$old_provider" ] || [ "$old_provider" = "$expected_provider" ] || fail "provider selector is not owned by Decisions: $PROVIDER_LINK"
if daily_clockwork_show=$(HOME="$install_home" "$clockwork_path" --json \
    binding show "$DAILY_CLOCKWORK_KEY" 2>"$transaction_dir/daily-clockwork-show.stderr")
then
    daily_clockwork_compact=$(printf '%s' "$daily_clockwork_show" | tr -d '[:space:]')
    case "$daily_clockwork_compact" in
        *'"enabled":true'*) prior_daily_clockwork_enabled=1 ;;
        *'"enabled":false'*) prior_daily_clockwork_enabled=0 ;;
        *) fail 'Clockwork returned an invalid daily binding document' ;;
    esac
    prior_daily_clockwork_digest=$(printf '%s\n' "$daily_clockwork_compact" | sed -n \
        's/.*"definition_digest":"\([0-9a-f]\{64\}\)".*/\1/p')
    if [ -z "$prior_daily_clockwork_digest" ]; then
        [ "$prior_daily_clockwork_enabled" -eq 0 ] \
            && printf '%s\n' "$daily_clockwork_compact" | grep -F '"definition_digest":null' >/dev/null \
            || fail 'Clockwork daily binding has an invalid definition digest'
    fi
    if [ -n "$prior_daily_clockwork_digest" ]; then
        prove_owned_clockwork_definition "$DAILY_CLOCKWORK_KEY" \
            "$prior_daily_clockwork_digest" \
            "$old_clockwork_release/bin/decisions-daily-email" \
            "$old_daily_clockwork_runner_hash" daily
    fi
else
    grep -F '"code":"binding_not_found"' "$transaction_dir/daily-clockwork-show.stderr" >/dev/null \
        || fail 'unable to inspect the Clockwork daily binding'
fi
if observer_clockwork_show=$(HOME="$install_home" "$clockwork_path" --json \
    binding show "$OBSERVER_CLOCKWORK_KEY" 2>"$transaction_dir/observer-clockwork-show.stderr")
then
    observer_clockwork_compact=$(printf '%s' "$observer_clockwork_show" | tr -d '[:space:]')
    case "$observer_clockwork_compact" in
        *'"enabled":true'*) prior_observer_clockwork_enabled=1 ;;
        *'"enabled":false'*) prior_observer_clockwork_enabled=0 ;;
        *) fail 'Clockwork returned an invalid observer binding document' ;;
    esac
    prior_observer_clockwork_digest=$(printf '%s\n' "$observer_clockwork_compact" | sed -n \
        's/.*"definition_digest":"\([0-9a-f]\{64\}\)".*/\1/p')
    if [ -z "$prior_observer_clockwork_digest" ]; then
        [ "$prior_observer_clockwork_enabled" -eq 0 ] \
            && printf '%s\n' "$observer_clockwork_compact" | grep -F '"definition_digest":null' >/dev/null \
            || fail 'Clockwork observer binding has an invalid definition digest'
    fi
    if [ -n "$prior_observer_clockwork_digest" ]; then
        prove_owned_clockwork_definition "$OBSERVER_CLOCKWORK_KEY" \
            "$prior_observer_clockwork_digest" \
            "$old_clockwork_release/bin/decisions-observer" \
            "$old_observer_clockwork_runner_hash" observer
    fi
else
    grep -F '"code":"binding_not_found"' "$transaction_dir/observer-clockwork-show.stderr" >/dev/null \
        || fail 'unable to inspect the Clockwork observer binding'
fi
if [ -L "$DAILY_PLIST" ]; then fail "LaunchAgent must not be a symbolic link: $DAILY_PLIST"; fi
if [ -e "$DAILY_PLIST" ] && [ ! -f "$DAILY_PLIST" ]; then fail "LaunchAgent path is occupied: $DAILY_PLIST"; fi
if [ -f "$DAILY_PLIST" ]; then
    [ -n "$old_current" ] || fail "LaunchAgent has no owned Decisions release"
    [ "$old_clockwork_format" = 2 ] \
        || fail "legacy daily LaunchAgent is not owned by the current Decisions release"
    expected_old_daily_plist="$transaction_dir/expected-old-daily.plist"
    render_legacy_plist daily \
        "$old_clockwork_release/package/$DAILY_LABEL.plist" \
        "$expected_old_daily_plist" \
        || fail "unable to render the current release's legacy daily LaunchAgent"
    legacy_plist_matches_expected "$DAILY_PLIST" "$expected_old_daily_plist" \
        || fail "legacy daily LaunchAgent bytes, owner, or mode are not owned by Decisions"
    old_daily_plist="$transaction_dir/prior-daily.plist"
    install -m 0644 "$DAILY_PLIST" "$old_daily_plist"
    legacy_plist_matches_expected "$old_daily_plist" "$expected_old_daily_plist" \
        || fail "unable to preserve the exact legacy daily LaunchAgent"
fi
if [ -L "$OBSERVER_PLIST" ]; then fail "LaunchAgent must not be a symbolic link: $OBSERVER_PLIST"; fi
if [ -e "$OBSERVER_PLIST" ] && [ ! -f "$OBSERVER_PLIST" ]; then fail "LaunchAgent path is occupied: $OBSERVER_PLIST"; fi
if [ -f "$OBSERVER_PLIST" ]; then
    [ -n "$old_current" ] || fail "observer LaunchAgent has no owned Decisions release"
    [ "$old_clockwork_format" = 2 ] \
        || fail "legacy observer LaunchAgent is not owned by the current Decisions release"
    expected_old_observer_plist="$transaction_dir/expected-old-observer.plist"
    render_legacy_plist observer \
        "$old_clockwork_release/package/$OBSERVER_LABEL.plist" \
        "$expected_old_observer_plist" \
        || fail "unable to render the current release's legacy observer LaunchAgent"
    legacy_plist_matches_expected "$OBSERVER_PLIST" "$expected_old_observer_plist" \
        || fail "legacy observer LaunchAgent bytes, owner, or mode are not owned by Decisions"
    old_observer_plist="$transaction_dir/prior-observer.plist"
    install -m 0644 "$OBSERVER_PLIST" "$old_observer_plist"
    legacy_plist_matches_expected "$old_observer_plist" "$expected_old_observer_plist" \
        || fail "unable to preserve the exact legacy observer LaunchAgent"
fi
if [ -L "$HOOKS_PATH" ]; then fail "Codex hooks file must not be a symbolic link: $HOOKS_PATH"; fi
if [ -e "$HOOKS_PATH" ] && [ ! -f "$HOOKS_PATH" ]; then fail "Codex hooks path is occupied: $HOOKS_PATH"; fi
if [ -f "$HOOKS_PATH" ]; then
    [ -n "$old_current" ] || fail "refusing to replace foreign Codex hooks: $HOOKS_PATH"
    cmp -s "$HOOKS_PATH" "$INSTALL_DIR/$old_current/package/hooks.json" \
        || fail "refusing to replace foreign or modified Codex hooks: $HOOKS_PATH"
    old_hooks="$transaction_dir/prior-hooks.json"
    install -m 0600 "$HOOKS_PATH" "$old_hooks"
fi
if "$launchctl_path" print "$DAILY_TARGET" >/dev/null 2>&1; then
    daily_was_loaded=1
    [ -n "$old_daily_plist" ] || fail "loaded Decisions daily label has no owned recoverable plist"
fi
if "$launchctl_path" print "$OBSERVER_TARGET" >/dev/null 2>&1; then
    observer_was_loaded=1
    [ -n "$old_observer_plist" ] || fail "loaded Decisions observer label has no owned recoverable plist"
fi
[ "$prior_daily_clockwork_enabled" -eq 0 ] || [ "$daily_was_loaded" -eq 0 ] \
    || fail 'Clockwork and the legacy Decisions daily LaunchAgent are both active'
[ "$prior_observer_clockwork_enabled" -eq 0 ] || [ "$observer_was_loaded" -eq 0 ] \
    || fail 'Clockwork and the legacy Decisions observer LaunchAgent are both active'
[ "$prior_daily_clockwork_enabled" -eq 0 ] || [ -z "$old_daily_plist" ] \
    || fail 'Clockwork and legacy Decisions daily schedule files coexist'
[ "$prior_observer_clockwork_enabled" -eq 0 ] || [ -z "$old_observer_plist" ] \
    || fail 'Clockwork and legacy Decisions observer schedule files coexist'

binary_hash=$(shasum -a 256 "$binary_path" | awk '{print $1}')
frontend_hash=$(shasum -a 256 "$SOURCE_FRONTEND" | awk '{print $1}')
daily_runner_hash=$(shasum -a 256 "$SOURCE_DAILY_RUNNER" | awk '{print $1}')
observer_runner_hash=$(shasum -a 256 "$SOURCE_OBSERVER_RUNNER" | awk '{print $1}')
daily_definition_hash=$(shasum -a 256 "$SOURCE_DAILY_DEFINITION" | awk '{print $1}')
observer_definition_hash=$(shasum -a 256 "$SOURCE_OBSERVER_DEFINITION" | awk '{print $1}')
hooks_hash=$(shasum -a 256 "$SOURCE_HOOKS" | awk '{print $1}')
deployer_hash=$(shasum -a 256 "$0" | awk '{print $1}')
uninstaller_hash=$(shasum -a 256 "$SOURCE_UNINSTALLER" | awk '{print $1}')
chancery_hash=$(bundle_hash "$SOURCE_CHANCERY")
release_id=$(printf '%s\n' "$binary_hash" "$frontend_hash" "$daily_runner_hash" \
    "$observer_runner_hash" "$daily_definition_hash" "$observer_definition_hash" "$hooks_hash" \
    "$deployer_hash" "$uninstaller_hash" "$chancery_hash" | shasum -a 256 | awk '{print $1}')
release="$RELEASES_DIR/$release_id"

if [ -L "$release" ]; then
    fail "existing release must not be a symbolic link: $release_id"
elif [ -e "$release" ] && [ ! -d "$release" ]; then
    fail "existing release is not a directory: $release_id"
elif [ -d "$release" ]; then
    validate_release_selector "releases/$release_id"
    [ "$(shasum -a 256 "$release/libexec/decisions" | awk '{print $1}')" = "$binary_hash" ] || fail "existing release binary is tampered: $release_id"
    [ "$(shasum -a 256 "$release/bin/decisions" | awk '{print $1}')" = "$frontend_hash" ] || fail "existing release frontend is tampered: $release_id"
    [ "$(shasum -a 256 "$release/bin/decisions-daily-email" | awk '{print $1}')" = "$daily_runner_hash" ] || fail "existing release daily runner is tampered: $release_id"
    [ "$(shasum -a 256 "$release/bin/decisions-observer" | awk '{print $1}')" = "$observer_runner_hash" ] || fail "existing release observer runner is tampered: $release_id"
    [ "$(shasum -a 256 "$release/package/decisions-daily-email.clockwork.toml.in" | awk '{print $1}')" = "$daily_definition_hash" ] || fail "existing release daily Clockwork definition is tampered: $release_id"
    [ "$(shasum -a 256 "$release/package/decisions-observer.clockwork.toml.in" | awk '{print $1}')" = "$observer_definition_hash" ] || fail "existing release observer Clockwork definition is tampered: $release_id"
    [ "$(shasum -a 256 "$release/package/hooks.json" | awk '{print $1}')" = "$hooks_hash" ] || fail "existing release hooks are tampered: $release_id"
    [ "$(shasum -a 256 "$release/package/deploy-user.sh" | awk '{print $1}')" = "$deployer_hash" ] || fail "existing release deployer is tampered: $release_id"
    [ "$(shasum -a 256 "$release/package/uninstall-user.sh" | awk '{print $1}')" = "$uninstaller_hash" ] || fail "existing release uninstaller is tampered: $release_id"
    [ "$(bundle_hash "$release/share/chancery/decisions")" = "$chancery_hash" ] || fail "existing release provider is tampered: $release_id"
else
    install -d -m 0755 "$temporary/bin" "$temporary/libexec" "$temporary/package" "$temporary/share/chancery"
    install -m 0755 "$binary_path" "$temporary/libexec/decisions"
    install -m 0755 "$SOURCE_FRONTEND" "$temporary/bin/decisions"
    install -m 0755 "$SOURCE_DAILY_RUNNER" "$temporary/bin/decisions-daily-email"
    install -m 0755 "$SOURCE_OBSERVER_RUNNER" "$temporary/bin/decisions-observer"
    install -m 0755 "$SOURCE_FRONTEND" "$temporary/package/decisions"
    install -m 0755 "$SOURCE_DAILY_RUNNER" "$temporary/package/decisions-daily-email"
    install -m 0755 "$SOURCE_OBSERVER_RUNNER" "$temporary/package/decisions-observer"
    install -m 0755 "$0" "$temporary/package/deploy-user.sh"
    install -m 0755 "$SOURCE_UNINSTALLER" "$temporary/package/uninstall-user.sh"
    install -m 0644 "$SOURCE_DAILY_DEFINITION" "$temporary/package/decisions-daily-email.clockwork.toml.in"
    install -m 0644 "$SOURCE_OBSERVER_DEFINITION" "$temporary/package/decisions-observer.clockwork.toml.in"
    install -m 0644 "$SOURCE_HOOKS" "$temporary/package/hooks.json"
    cp -R "$SOURCE_CHANCERY" "$temporary/share/chancery/decisions"
    {
        printf '%s\n' 'format=3'
        printf 'release_id=%s\n' "$release_id"
        printf 'version=%s\n' "$version"
        printf 'binary_sha256=%s\n' "$binary_hash"
        printf 'frontend_sha256=%s\n' "$frontend_hash"
        printf 'daily_runner_sha256=%s\n' "$daily_runner_hash"
        printf 'observer_runner_sha256=%s\n' "$observer_runner_hash"
        printf 'daily_clockwork_definition_sha256=%s\n' "$daily_definition_hash"
        printf 'observer_clockwork_definition_sha256=%s\n' "$observer_definition_hash"
        printf 'hooks_sha256=%s\n' "$hooks_hash"
        printf 'deployer_sha256=%s\n' "$deployer_hash"
        printf 'uninstaller_sha256=%s\n' "$uninstaller_hash"
        printf 'chancery_sha256=%s\n' "$chancery_hash"
    } >"$temporary/manifest.txt"
    chmod 0444 "$temporary/manifest.txt"
    chmod -R go-w "$temporary"
    mv "$temporary" "$release"
    temporary=$(mktemp -d "$INSTALL_DIR/.candidate.XXXXXX")
fi

engage_maintenance \
    || fail 'Decisions maintenance gate is invalid or unavailable'
prepare_private_log "$LOG_DIR/daily-email.stdout.log"
prepare_private_log "$LOG_DIR/daily-email.stderr.log"
prepare_private_log "$LOG_DIR/observer.stdout.log"
prepare_private_log "$LOG_DIR/observer.stderr.log"

sed \
    -e "s|__RELEASE_ID__|$release_id|g" \
    -e "s|__RELEASE_ROOT__|$release|g" \
    -e "s|__DECISIONS_STATE__|$STATE_DIR|g" \
    -e "s|__DECISIONS_HOME__|$install_home|g" \
    -e "s|__DECISIONS_LOGS__|$LOG_DIR|g" \
    -e "s|__INTERPRETER_SHA256__|$interpreter_hash|g" \
    -e "s|__RUNNER_SHA256__|$daily_runner_hash|g" \
    "$release/package/decisions-daily-email.clockwork.toml.in" \
    >"$temporary_daily_definition"
chmod 0600 "$temporary_daily_definition"
daily_definition_output=$(HOME="$install_home" "$clockwork_path" --json \
    definition register "$temporary_daily_definition") \
    || fail 'Clockwork rejected the candidate daily definition'
daily_definition_compact=$(printf '%s' "$daily_definition_output" | tr -d '[:space:]')
candidate_daily_definition_digest=$(printf '%s\n' "$daily_definition_compact" | sed -n \
    's/.*"digest":"\([0-9a-f]\{64\}\)".*/\1/p')
[ -n "$candidate_daily_definition_digest" ] \
    || fail 'Clockwork returned no candidate daily definition digest'

sed \
    -e "s|__RELEASE_ID__|$release_id|g" \
    -e "s|__RELEASE_ROOT__|$release|g" \
    -e "s|__DECISIONS_STATE__|$STATE_DIR|g" \
    -e "s|__DECISIONS_HOME__|$install_home|g" \
    -e "s|__DECISIONS_LOGS__|$LOG_DIR|g" \
    -e "s|__INTERPRETER_SHA256__|$interpreter_hash|g" \
    -e "s|__RUNNER_SHA256__|$observer_runner_hash|g" \
    "$release/package/decisions-observer.clockwork.toml.in" \
    >"$temporary_observer_definition"
chmod 0600 "$temporary_observer_definition"
observer_definition_output=$(HOME="$install_home" "$clockwork_path" --json \
    definition register "$temporary_observer_definition") \
    || fail 'Clockwork rejected the candidate observer definition'
observer_definition_compact=$(printf '%s' "$observer_definition_output" | tr -d '[:space:]')
candidate_observer_definition_digest=$(printf '%s\n' "$observer_definition_compact" | sed -n \
    's/.*"digest":"\([0-9a-f]\{64\}\)".*/\1/p')
[ -n "$candidate_observer_definition_digest" ] \
    || fail 'Clockwork returned no candidate observer definition digest'

rollback() {
    fail "$1"
}

if [ -x /opt/homebrew/bin/codex ]; then
    codex_path=/opt/homebrew/bin/codex
elif [ -x "$install_home/.local/bin/codex" ]; then
    codex_path="$install_home/.local/bin/codex"
else
    rollback 'Codex executable is unavailable'
fi

{
    printf 'current=%s\n' "$old_current"
    printf 'previous=%s\n' "$old_previous"
    printf 'daily_legacy_loaded=%s\n' "$daily_was_loaded"
    printf 'observer_legacy_loaded=%s\n' "$observer_was_loaded"
    printf 'daily_clockwork_enabled=%s\n' "$prior_daily_clockwork_enabled"
    printf 'daily_clockwork_definition=%s\n' "$prior_daily_clockwork_digest"
    printf 'observer_clockwork_enabled=%s\n' "$prior_observer_clockwork_enabled"
    printf 'observer_clockwork_definition=%s\n' "$prior_observer_clockwork_digest"
    printf 'maintenance_created=%s\n' "$maintenance_created"
} >"$transaction_dir/prior-install.txt"
chmod 0600 "$transaction_dir/prior-install.txt"

clockwork_disabled=1
HOME="$install_home" "$clockwork_path" --json binding disable \
    "$OBSERVER_CLOCKWORK_KEY" >/dev/null \
    || rollback 'unable to disable the Clockwork observer binding'
HOME="$install_home" "$clockwork_path" --json binding disable \
    "$DAILY_CLOCKWORK_KEY" >/dev/null \
    || rollback 'unable to disable the Clockwork daily binding'
if [ "$observer_was_loaded" -eq 1 ]; then
    legacy_plist_matches_expected "$OBSERVER_PLIST" "$expected_old_observer_plist" \
        || rollback 'legacy observer LaunchAgent changed before stop'
    observer_service_stopped=1
    "$launchctl_path" bootout "$OBSERVER_TARGET" >/dev/null \
        || rollback 'unable to stop the owned observer service'
fi
if [ "$daily_was_loaded" -eq 1 ]; then
    legacy_plist_matches_expected "$DAILY_PLIST" "$expected_old_daily_plist" \
        || rollback 'legacy daily LaunchAgent changed before stop'
    daily_service_stopped=1
    "$launchctl_path" bootout "$DAILY_TARGET" >/dev/null \
        || rollback 'unable to stop the owned daily service'
fi
if [ -n "$old_observer_plist" ]; then
    legacy_plist_matches_expected "$OBSERVER_PLIST" "$expected_old_observer_plist" \
        || rollback 'legacy observer LaunchAgent changed before removal'
    observer_plist_changed=1
    rm -f "$OBSERVER_PLIST" || rollback 'unable to remove the legacy observer LaunchAgent'
fi
if [ -n "$old_daily_plist" ]; then
    legacy_plist_matches_expected "$DAILY_PLIST" "$expected_old_daily_plist" \
        || rollback 'legacy daily LaunchAgent changed before removal'
    daily_plist_changed=1
    rm -f "$DAILY_PLIST" || rollback 'unable to remove the legacy daily LaunchAgent'
fi

# The synchronous Stop hook is also a database writer. Keep its public command
# unavailable across the quiescent backup, candidate migration, and baseline
# activation. Any turn completed during this short cutover is recovered by the
# post-baseline reconciliation path.
if [ -n "$old_cli" ]; then
    rm -f "$CLI_PATH"
    cli_suspended=1
    /bin/sleep 3
fi

if [ -L "$DATABASE_PATH" ]; then
    rollback 'database must not be a symbolic link'
elif [ -e "$DATABASE_PATH" ] && [ ! -f "$DATABASE_PATH" ]; then
    rollback 'database must be a regular file'
elif [ ! -e "$DATABASE_PATH" ]; then
    database_was_absent=1
fi
for suffix in wal shm journal; do
    sidecar="$DATABASE_PATH-$suffix"
    if [ -L "$sidecar" ]; then
        rollback "database sidecar must not be a symbolic link: $sidecar"
    elif [ -e "$sidecar" ] && [ ! -f "$sidecar" ]; then
        rollback "database sidecar must be a regular file: $sidecar"
    elif [ -f "$sidecar" ] && [ "$database_was_absent" -eq 1 ]; then
        rollback "database sidecar exists without its database: $sidecar"
    fi
done
require_database_quiescent() {
    [ "$database_was_absent" -eq 0 ] || return 0
    if open_pids=$(/usr/sbin/lsof -t -- "$DATABASE_PATH" 2>/dev/null); then
        rollback 'database is open by another Decisions process'
    else
        lsof_status=$?
        [ "$lsof_status" -eq 1 ] || rollback 'unable to verify database quiescence'
    fi
}
require_database_quiescent
if [ "$database_was_absent" -eq 0 ]; then
    install -m 0600 "$DATABASE_PATH" "$transaction_dir/decisions.db"
    for suffix in wal shm journal; do
        [ ! -f "$DATABASE_PATH-$suffix" ] || \
            install -m 0600 "$DATABASE_PATH-$suffix" "$transaction_dir/decisions.db-$suffix"
    done
fi
require_database_quiescent

switched=1
if [ -n "$old_current" ] && [ "$old_current" != "releases/$release_id" ]; then
    atomic_symlink "$old_current" "$PREVIOUS_LINK"
fi
atomic_symlink "releases/$release_id" "$CURRENT_LINK"
atomic_symlink "$expected_provider" "$PROVIDER_LINK"

database_touched=1
doctor_output=$(HOME="$install_home" CONVERSATIONS_CODEX="$codex_path" "$release/libexec/decisions" \
    --database "$DATABASE_PATH" \
    --email-binary "$install_home/.local/bin/email" \
    --json doctor) || rollback 'candidate doctor failed'
doctor_compact=$(printf '%s' "$doctor_output" | tr -d '[:space:]')
case "$doctor_compact" in
    *'"schema_version":3'*) ;;
    *) rollback 'candidate doctor did not prove Decisions schema version 3' ;;
esac
watermark_output=$(HOME="$install_home" "$release/libexec/decisions" \
    --database "$DATABASE_PATH" --json events watermark) \
    || rollback 'candidate lifecycle stream watermark failed'
watermark_compact=$(printf '%s' "$watermark_output" | tr -d '[:space:]')
case "$watermark_compact" in
    *'"stream":"decisions.lifecycle"'*'"envelope_version":1'*'"cursor":"'*) ;;
    *) rollback 'candidate lifecycle stream contract is invalid' ;;
esac

HOME="$install_home" CONVERSATIONS_CODEX="$codex_path" "$release/libexec/decisions" \
    --database "$DATABASE_PATH" observe activate >/dev/null \
    || rollback 'unable to establish the observer activation baseline'

hooks_changed=1
install -m 0600 "$SOURCE_HOOKS" "$HOOKS_PATH"
atomic_symlink "$expected_cli" "$CLI_PATH"
cli_suspended=0

daily_clockwork_switched=1
HOME="$install_home" "$clockwork_path" --json binding switch \
    "$DAILY_CLOCKWORK_KEY" "$candidate_daily_definition_digest" >/dev/null \
    || rollback 'Clockwork rejected the daily binding switch'
observer_clockwork_switched=1
HOME="$install_home" "$clockwork_path" --json binding switch \
    "$OBSERVER_CLOCKWORK_KEY" "$candidate_observer_definition_digest" >/dev/null \
    || rollback 'Clockwork rejected the observer binding switch'

# All database, selector, hook, and Clockwork transitions have now committed.
# End rollback authority before removing the release-independent gate; if gate
# removal fails, the committed installation remains safely maintenance-gated.
maintenance_marker_is_owned \
    || rollback 'Decisions maintenance gate changed before commit'
committed=1
rm -f "$MAINTENANCE_MARKER" \
    || fail 'committed Decisions but could not clear the maintenance gate'
[ ! -e "$MAINTENANCE_MARKER" ] && [ ! -L "$MAINTENANCE_MARKER" ] \
    || fail 'committed Decisions but the maintenance gate remains'
maintenance_created=0
printf 'installed decisions %s (%s)\n' "$version" "$release_id"
