#!/bin/sh

set -eu

PATH=/usr/bin:/bin:/usr/sbin:/sbin
export PATH
umask 077

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
LABEL=org.semantics.worker
CLOCKWORK_KEY=semantics/worker
SOURCE_FRONTEND="$SCRIPT_DIR/semantics"
SOURCE_RUNNER="$SCRIPT_DIR/semantics-worker"
SOURCE_DEFINITION="$SCRIPT_DIR/semantics-worker.clockwork.toml.in"
SOURCE_UNINSTALLER="$SCRIPT_DIR/uninstall-user.sh"
if [ -d "$SCRIPT_DIR/../share/chancery/semantics" ]; then
    SOURCE_CHANCERY="$SCRIPT_DIR/../share/chancery/semantics"
else
    SOURCE_CHANCERY="$SCRIPT_DIR/../../chancery"
fi

binary_path=
clockwork_path=
install_home=${HOME:-}
launchctl_path=/bin/launchctl
final_decisions_watermark=
final_decisions_watermark_set=0
keep_maintenance=0

fail() {
    printf 'semantics user deploy: %s\n' "$*" >&2
    exit 1
}

usage() {
    printf '%s\n' 'Usage: deploy-user.sh --binary ABSOLUTE_PATH --clockwork ABSOLUTE_PATH [--home ABSOLUTE_PATH] [--launchctl ABSOLUTE_PATH] [--final-decisions-watermark OPAQUE_CURSOR] [--keep-maintenance]'
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --binary) [ "$#" -ge 2 ] || fail '--binary requires a path'; binary_path=$2; shift 2 ;;
        --clockwork) [ "$#" -ge 2 ] || fail '--clockwork requires a path'; clockwork_path=$2; shift 2 ;;
        --home) [ "$#" -ge 2 ] || fail '--home requires a path'; install_home=$2; shift 2 ;;
        --launchctl) [ "$#" -ge 2 ] || fail '--launchctl requires a path'; launchctl_path=$2; shift 2 ;;
        --final-decisions-watermark)
            [ "$#" -ge 2 ] || fail '--final-decisions-watermark requires an opaque cursor'
            [ "$final_decisions_watermark_set" -eq 0 ] \
                || fail '--final-decisions-watermark may be supplied only once'
            final_decisions_watermark=$2
            final_decisions_watermark_set=1
            shift 2
            ;;
        --keep-maintenance) keep_maintenance=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *) fail "unknown argument: $1" ;;
    esac
done

[ -n "$binary_path" ] || fail '--binary is required'
[ -n "$clockwork_path" ] || fail '--clockwork is required'
[ "$final_decisions_watermark_set" -eq 0 ] || [ -n "$final_decisions_watermark" ] \
    || fail '--final-decisions-watermark must not be empty'
[ "$final_decisions_watermark_set" -eq 0 ] || [ "$keep_maintenance" -eq 1 ] \
    || fail '--final-decisions-watermark requires --keep-maintenance'
case "$binary_path" in /*) ;; *) fail 'binary must be absolute' ;; esac
case "$clockwork_path" in /*) ;; *) fail 'clockwork must be absolute' ;; esac
case "$install_home" in /*) ;; *) fail 'home must be absolute' ;; esac
case "$launchctl_path" in /*) ;; *) fail 'launchctl must be absolute' ;; esac
case "$install_home" in *'&'*|*'<'*|*'>'*|*'|'*|*'"'*|*'\'*|*'
'*) fail 'home contains characters unsupported by schedule rendering' ;; esac
[ -d "$install_home" ] && [ ! -L "$install_home" ] || fail 'home is not a regular directory'
operator_uid=$(id -u)
[ "$operator_uid" -ne 0 ] || fail 'run as the Semantics operator, not root'
[ "$(stat -f '%u' "$install_home")" -eq "$operator_uid" ] \
    || fail 'home is not owned by the Semantics operator'
[ -f "$binary_path" ] && [ ! -L "$binary_path" ] && [ -x "$binary_path" ] \
    || fail 'candidate is not an executable regular file'
[ -e "$clockwork_path" ] && [ -x "$clockwork_path" ] \
    || fail 'Clockwork executable is unavailable'
[ -x "$launchctl_path" ] && [ ! -L "$launchctl_path" ] || fail 'launchctl is unavailable'
[ -x /usr/sbin/lsof ] || fail 'lsof is unavailable'
[ -x /usr/bin/perl ] || fail 'perl is unavailable'
[ -x /usr/bin/shlock ] || fail 'shlock is unavailable'
for source in "$SOURCE_FRONTEND" "$SOURCE_RUNNER" "$SOURCE_DEFINITION" "$SOURCE_UNINSTALLER"; do
    [ -f "$source" ] && [ ! -L "$source" ] || fail "missing packaged file: $source"
done

validate_bundle() {
    bundle=$1
    role=$2
    [ -d "$bundle" ] && [ ! -L "$bundle" ] \
        || fail "Chancery provider is not a regular directory: $bundle"
    for relative in \
        provider.json \
        entries/repository-explore.json entries/project-operate.json entries/develop-change.json \
        manuals/repository-explore.md manuals/project-operate.md manuals/develop-change.md
    do
        [ -f "$bundle/$relative" ] && [ ! -L "$bundle/$relative" ] \
            || fail "Chancery provider file is missing: $bundle/$relative"
    done
    [ "$(find "$bundle" -type f | awk 'END { print NR }')" -eq 7 ] \
        || fail "Chancery provider has unexpected files: $bundle"
    if find "$bundle" -type l -print | grep -q .; then
        fail "Chancery provider contains a symbolic link: $bundle"
    fi
    if find "$bundle" ! -type d ! -type f -print | grep -q .; then
        fail "Chancery provider contains a non-file entry: $bundle"
    fi
    schema_version=$(/usr/bin/plutil -extract schema_version raw \
        "$bundle/provider.json" 2>/dev/null) \
        || fail "Chancery provider schema is unreadable: $bundle"
    case "$role:$schema_version" in
        source:3|installed:2|installed:3) ;;
        *) fail "Chancery provider schema $schema_version is not valid for $role bundle" ;;
    esac
    provider_id=$(/usr/bin/plutil -extract provider.id raw \
        "$bundle/provider.json" 2>/dev/null) \
        || fail "Chancery provider ID is unreadable: $bundle"
    [ "$provider_id" = semantics ] || fail 'Chancery provider ID is not semantics'
    for entry_spec in \
        repository-explore.json:semantics.repository.explore \
        project-operate.json:semantics.project.operate \
        develop-change.json:semantics.develop.change
    do
        entry_file=${entry_spec%%:*}
        entry_id=${entry_spec#*:}
        actual_entry_id=$(/usr/bin/plutil -extract id raw \
            "$bundle/entries/$entry_file" 2>/dev/null) \
            || fail "Chancery provider entry ID is unreadable: $entry_file"
        [ "$actual_entry_id" = "$entry_id" ] \
            || fail "Chancery provider entry is missing: $entry_id"
    done
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
        fail "Semantics log path is not a regular file: $log_path"
    fi
    [ -e "$log_path" ] || return 0
    [ "$(stat -f '%u' "$log_path")" -eq "$operator_uid" ] \
        || fail "Semantics log is not owned by the operator: $log_path"
    [ "$(stat -f '%l' "$log_path")" -eq 1 ] \
        || fail "Semantics log must not be hard-linked: $log_path"
    chmod 0600 "$log_path" \
        || fail "unable to make the Semantics log private: $log_path"
}

validate_private_database_file() {
    state_path=$1
    description=$2
    if [ -L "$state_path" ] \
        || { [ -e "$state_path" ] && [ ! -f "$state_path" ]; }
    then
        fail "$description must be a regular non-symbolic-link file: $state_path"
    fi
    [ -e "$state_path" ] || return 0
    [ "$(stat -f '%u' "$state_path")" -eq "$operator_uid" ] \
        || fail "$description is not owned by the Semantics operator: $state_path"
    [ "$(stat -f '%Lp' "$state_path")" = 600 ] \
        || fail "$description permissions must be exactly 0600: $state_path"
    [ "$(stat -f '%l' "$state_path")" -eq 1 ] \
        || fail "$description must not be hard-linked: $state_path"
}

preflight_private_database_files() {
    validate_private_database_file "$DATABASE_PATH" 'database'
    for suffix in wal shm journal; do
        validate_private_database_file "$DATABASE_PATH-$suffix" 'database sidecar'
        if [ ! -e "$DATABASE_PATH" ] && [ -f "$DATABASE_PATH-$suffix" ]; then
            fail "database sidecar exists without its database: $DATABASE_PATH-$suffix"
        fi
    done
    if [ -e "$MAINTENANCE_HOLD_RECEIPT" ] || [ -L "$MAINTENANCE_HOLD_RECEIPT" ]; then
        validate_private_database_file "$MAINTENANCE_HOLD_RECEIPT" \
            'maintenance hold receipt'
        [ -e "$MAINTENANCE_MARKER" ] && [ ! -L "$MAINTENANCE_MARKER" ] \
            || fail 'maintenance hold receipt has no matching maintenance gate'
    fi
}

validate_bundle "$SOURCE_CHANCERY" source

STATE_DIR="$install_home/Library/Application Support/Semantics"
INSTALL_DIR="$STATE_DIR/install"
RELEASES_DIR="$INSTALL_DIR/releases"
CURRENT_LINK="$INSTALL_DIR/current"
PREVIOUS_LINK="$INSTALL_DIR/previous"
LOCK_DIR="$INSTALL_DIR/.update-lock"
DEPLOYMENT_BACKUPS_DIR="$STATE_DIR/backups/deployments"
LAST_UPDATE_PATH="$INSTALL_DIR/last-update.txt"
CLI_DIR="$install_home/.local/bin"
CLI_PATH="$CLI_DIR/semantics"
AGENT_DIR="$install_home/Library/LaunchAgents"
PLIST_PATH="$AGENT_DIR/$LABEL.plist"
LOG_DIR="$install_home/Library/Logs/Semantics"
DATABASE_PATH="$STATE_DIR/semantics.db"
MAINTENANCE_MARKER="$STATE_DIR/.clockwork-maintenance"
MAINTENANCE_HOLD_RECEIPT="$STATE_DIR/.deployment-maintenance.json"
PROVIDERS_DIR="$install_home/Library/Application Support/Chancery/providers"
PROVIDER_LINK="$PROVIDERS_DIR/semantics"
CHANCERY_CATALOG_LOCK="${PROVIDERS_DIR%/providers}/.catalog-update-lock"
SERVICE_DOMAIN="gui/$operator_uid"
SERVICE_TARGET="$SERVICE_DOMAIN/$LABEL"
EXPECTED_CLI="$INSTALL_DIR/current/bin/semantics"
EXPECTED_PROVIDER="$INSTALL_DIR/current/share/chancery/semantics"

for directory in "$STATE_DIR" "$INSTALL_DIR" "$RELEASES_DIR" "$LOG_DIR" \
    "$STATE_DIR/backups" "$DEPLOYMENT_BACKUPS_DIR"
do
    [ ! -L "$directory" ] || fail "refusing symbolic-link directory: $directory"
    [ ! -e "$directory" ] || [ -d "$directory" ] || fail "directory path is occupied: $directory"
    [ -d "$directory" ] || install -d -m 0700 "$directory"
done
for directory in "$CLI_DIR" "$AGENT_DIR" "$PROVIDERS_DIR"; do
    [ ! -L "$directory" ] || fail "refusing symbolic-link directory: $directory"
    [ ! -e "$directory" ] || [ -d "$directory" ] || fail "directory path is occupied: $directory"
    [ -d "$directory" ] || install -d -m 0755 "$directory"
done
preflight_private_database_files
candidate_version=$("$binary_path" --version) || fail 'unable to read candidate version'
case "$candidate_version" in
    'semantics '*) version=${candidate_version#semantics } ;;
    *) fail "unexpected candidate version: $candidate_version" ;;
esac
provider_version=$(/usr/bin/plutil -extract provider.release raw \
    "$SOURCE_CHANCERY/provider.json" 2>/dev/null) \
    || fail 'Chancery provider release is unreadable'
[ "$provider_version" = "$version" ] \
    || fail "provider release $provider_version does not match candidate $version"
# Defer catchable termination across the atomic mkdir until the full cleanup
# trap owns the newly acquired directory lock.
trap '' HUP INT TERM
mkdir "$LOCK_DIR" 2>/dev/null || fail 'another Semantics deployment is active'

temporary=
temporary_definition=
transaction_dir=
worker_lock_ready="$INSTALL_DIR/.worker-lock-ready.$$"
worker_lock_stop="$INSTALL_DIR/.worker-lock-stop.$$"
old_current=
old_previous=
old_cli=
old_provider=
old_plist=
prior_clockwork_digest=
prior_clockwork_enabled=0
clockwork_disabled=0
clockwork_switched=0
candidate_definition_digest=
release=
switched=0
committed=0
service_was_loaded=0
service_stopped=0
legacy_plist_removed=0
cli_suspended=0
database_touched=0
database_was_absent=0
retain_transaction=0
retain_current_for_recovery=0
rollback_snapshot=
rollback_snapshot_created=0
old_last_update=0
receipt_changed=0
worker_lock_pid=
maintenance_created=0
maintenance_preexisting=0
maintenance_owned=0
maintenance_retained=0
hold_existed=0
hold_changed=0
catalog_lock_created=0

release_worker_lock() {
    [ -n "$worker_lock_pid" ] || return 0
    : >"$worker_lock_stop" || return 1
    if wait "$worker_lock_pid"; then lock_status=0; else lock_status=$?; fi
    worker_lock_pid=
    rm -f "$worker_lock_ready" "$worker_lock_stop"
    return "$lock_status"
}

release_catalog_lock() {
    if [ "$catalog_lock_created" -eq 1 ] \
        && [ -f "$CHANCERY_CATALOG_LOCK" ] \
        && [ ! -L "$CHANCERY_CATALOG_LOCK" ] \
        && [ "$(sed -n '1p' "$CHANCERY_CATALOG_LOCK" 2>/dev/null || true)" = "$$" ]
    then
        rm -f "$CHANCERY_CATALOG_LOCK" >/dev/null 2>&1 || true
    fi
}

acquire_catalog_lock() {
    [ ! -L "$CHANCERY_CATALOG_LOCK" ] \
        || fail "Chancery catalog writer lock is a symbolic link: $CHANCERY_CATALOG_LOCK"
    if [ -e "$CHANCERY_CATALOG_LOCK" ] \
        && [ ! -f "$CHANCERY_CATALOG_LOCK" ]
    then
        fail "Chancery catalog writer lock is not safely recoverable: $CHANCERY_CATALOG_LOCK"
    fi
    catalog_lock_created=1
    /usr/bin/shlock -p "$$" -f "$CHANCERY_CATALOG_LOCK" \
        || fail "another Chancery catalog writer is active: $CHANCERY_CATALOG_LOCK"
}

cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    set +e
    rollback_ready=1
    if [ "$status" -ne 0 ] && [ "$committed" -eq 0 ]; then
        # Clockwork is the only scheduler allowed after handoff. Disable the
        # candidate before restoring product state so no new activation can
        # enter the rollback window.
        if [ "$clockwork_disabled" -eq 1 ] || [ "$clockwork_switched" -eq 1 ]; then
            HOME="$install_home" "$clockwork_path" --json binding disable \
                "$CLOCKWORK_KEY" >/dev/null 2>&1 || rollback_ready=0
        fi
        # Clockwork cannot clear a selected definition back to null. Once a
        # formerly unselected binding may have selected the candidate, retain
        # maintenance plus its private release selector as recovery evidence
        # instead of claiming a complete rollback to the old release.
        if [ "$clockwork_switched" -eq 1 ] && [ -z "$prior_clockwork_digest" ]; then
            rollback_ready=0
            retain_current_for_recovery=1
        fi
        public_cli_was_present=0
        if [ "$switched" -eq 1 ] && { [ -e "$CLI_PATH" ] || [ -L "$CLI_PATH" ]; }; then
            public_cli_was_present=1
            rm -f "$CLI_PATH" || rollback_ready=0
        fi
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
                install -m 0600 "$transaction_dir/semantics.db" "$DATABASE_PATH" \
                    || rollback_ready=0
                for suffix in wal shm journal; do
                    [ ! -f "$transaction_dir/semantics.db-$suffix" ] || \
                        install -m 0600 "$transaction_dir/semantics.db-$suffix" "$DATABASE_PATH-$suffix" \
                        || rollback_ready=0
                done
            fi
        fi
        if [ "$legacy_plist_removed" -eq 1 ] && [ "$rollback_ready" -eq 1 ]; then
            if [ -n "$old_plist" ]; then
                cp -p "$old_plist" "$PLIST_PATH" || rollback_ready=0
            else
                rm -f "$PLIST_PATH" || rollback_ready=0
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
        if [ "$rollback_ready" -eq 1 ] \
            && { [ "$clockwork_disabled" -eq 1 ] || [ "$clockwork_switched" -eq 1 ]; }
        then
            if [ "$prior_clockwork_enabled" -eq 1 ]; then
                HOME="$install_home" "$clockwork_path" --json binding switch \
                    "$CLOCKWORK_KEY" "$prior_clockwork_digest" >/dev/null 2>&1 \
                    || rollback_ready=0
            elif [ -n "$prior_clockwork_digest" ]; then
                HOME="$install_home" "$clockwork_path" --json binding disable \
                    "$CLOCKWORK_KEY" --select "$prior_clockwork_digest" >/dev/null 2>&1 \
                    || rollback_ready=0
            else
                HOME="$install_home" "$clockwork_path" --json binding disable \
                    "$CLOCKWORK_KEY" >/dev/null 2>&1 || rollback_ready=0
            fi
        fi
        if [ "$rollback_ready" -eq 1 ] && [ "$prior_clockwork_enabled" -eq 0 ] \
            && [ "$service_stopped" -eq 1 ] \
            && [ "$service_was_loaded" -eq 1 ] && [ -n "$old_plist" ]; then
            "$launchctl_path" bootstrap "$SERVICE_DOMAIN" "$PLIST_PATH" >/dev/null 2>&1 \
                || rollback_ready=0
        fi
        if [ "$rollback_ready" -eq 1 ] && ! release_worker_lock; then
            rollback_ready=0
        fi
        if [ "$rollback_ready" -eq 1 ] && [ "$hold_changed" -eq 1 ]; then
            if [ "$hold_existed" -eq 1 ]; then
                install -m 0600 "$transaction_dir/maintenance-hold.before" \
                    "$MAINTENANCE_HOLD_RECEIPT" || rollback_ready=0
            else
                rm -f "$MAINTENANCE_HOLD_RECEIPT" || rollback_ready=0
            fi
        fi
        if [ "$rollback_ready" -eq 1 ] && [ "$maintenance_created" -eq 1 ]; then
            maintenance_marker_is_owned && rm -f "$MAINTENANCE_MARKER" \
                && [ ! -e "$MAINTENANCE_MARKER" ] && [ ! -L "$MAINTENANCE_MARKER" ] \
                || rollback_ready=0
            [ "$rollback_ready" -eq 0 ] || maintenance_created=0
        fi
        if [ "$rollback_ready" -eq 0 ]; then
            # The release-independent maintenance marker remains after the
            # worker flock is released, so even an unproven loaded Clockwork
            # projection cannot enter Semantics domain work.
            [ "$hold_changed" -eq 0 ] || retain_current_for_recovery=1
            HOME="$install_home" "$clockwork_path" --json binding disable \
                "$CLOCKWORK_KEY" >/dev/null 2>&1 || true
            "$launchctl_path" bootout "$SERVICE_TARGET" >/dev/null 2>&1 || true
            rm -f "$CLI_PATH" "$PROVIDER_LINK" "$PREVIOUS_LINK" "$PLIST_PATH"
            [ "$retain_current_for_recovery" -eq 1 ] || rm -f "$CURRENT_LINK"
            retain_transaction=1
            printf '%s\n' 'semantics user deploy: rollback could not prove scheduler/database quiescence or restore every owned artifact; domain admission is maintenance-gated, scheduler cleanup was attempted, and public selectors were removed' >&2
            printf 'semantics user deploy: private rollback backup retained at %s\n' "$transaction_dir" >&2
            release_worker_lock >/dev/null 2>&1 || true
        fi
        if [ "$rollback_snapshot_created" -eq 1 ]; then
            rm -rf "$rollback_snapshot"
        fi
        if [ "$receipt_changed" -eq 1 ]; then
            if [ "$old_last_update" -eq 1 ]; then
                install -m 0600 "$transaction_dir/last-update.txt" "$LAST_UPDATE_PATH"
            else
                rm -f "$LAST_UPDATE_PATH"
            fi
        fi
    fi
    [ -z "$temporary" ] || rm -rf "$temporary"
    [ -z "$temporary_definition" ] || rm -f "$temporary_definition"
    release_worker_lock >/dev/null 2>&1 || true
    rm -f "$worker_lock_ready" "$worker_lock_stop"
    [ -z "$old_plist" ] || rm -f "$old_plist"
    [ "$retain_transaction" -eq 1 ] || [ -z "$transaction_dir" ] || rm -rf "$transaction_dir"
    release_catalog_lock
    rmdir "$LOCK_DIR" >/dev/null 2>&1 || true
    exit "$status"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

temporary=$(mktemp -d "$INSTALL_DIR/.candidate.XXXXXX") \
    || fail 'unable to create the Semantics release staging directory'
temporary_definition=$(mktemp "$INSTALL_DIR/.worker-definition.XXXXXX") \
    || fail 'unable to create the Clockwork definition staging file'
transaction_dir=$(mktemp -d "$INSTALL_DIR/.transaction.XXXXXX") \
    || fail 'unable to create the Semantics transaction directory'

if [ -L "$CURRENT_LINK" ]; then old_current=$(readlink "$CURRENT_LINK"); elif [ -e "$CURRENT_LINK" ]; then fail "$CURRENT_LINK must be a symbolic link"; fi
if [ -L "$PREVIOUS_LINK" ]; then old_previous=$(readlink "$PREVIOUS_LINK"); elif [ -e "$PREVIOUS_LINK" ]; then fail "$PREVIOUS_LINK must be a symbolic link"; fi
if [ -L "$CLI_PATH" ]; then old_cli=$(readlink "$CLI_PATH"); elif [ -e "$CLI_PATH" ]; then fail "$CLI_PATH exists and is not a symbolic link"; fi
if [ -L "$PROVIDER_LINK" ]; then old_provider=$(readlink "$PROVIDER_LINK"); elif [ -e "$PROVIDER_LINK" ]; then fail "$PROVIDER_LINK exists and is not a symbolic link"; fi
if [ -L "$LAST_UPDATE_PATH" ] || { [ -e "$LAST_UPDATE_PATH" ] && [ ! -f "$LAST_UPDATE_PATH" ]; }; then
    fail 'last-update receipt is not a regular file'
fi
if [ -f "$LAST_UPDATE_PATH" ]; then
    old_last_update=1
    install -m 0600 "$LAST_UPDATE_PATH" "$transaction_dir/last-update.txt"
fi

validate_release_selector() {
    selector=$1
    printf '%s\n' "$selector" | grep -Eq '^releases/[0-9a-f]{64}$' \
        || fail "invalid Semantics release selector: $selector"
    selected_release="$INSTALL_DIR/$selector"
    selected_id=${selector#releases/}
    selected_manifest="$selected_release/manifest.txt"
    [ -d "$selected_release" ] && [ ! -L "$selected_release" ] \
        || fail "selected Semantics release is unavailable: $selector"
    [ -f "$selected_manifest" ] && [ ! -L "$selected_manifest" ] \
        || fail "selected Semantics release has no owned manifest: $selector"
    [ "$(awk 'END { print NR }' "$selected_manifest")" -eq 10 ] \
        || fail "selected Semantics release manifest is not canonical: $selector"
    selected_format=$(sed -n '1s/^format=//p' "$selected_manifest")
    case "$selected_format" in
        1|2) ;;
        *) fail "selected Semantics release manifest format is unsupported: $selector" ;;
    esac
    selected_manifest_id=$(sed -n '2s/^release_id=//p' "$selected_manifest")
    selected_version=$(sed -n '3s/^version=//p' "$selected_manifest")
    selected_binary_hash=$(sed -n '4s/^binary_sha256=//p' "$selected_manifest")
    selected_frontend_hash=$(sed -n '5s/^frontend_sha256=//p' "$selected_manifest")
    selected_runner_hash=$(sed -n '6s/^runner_sha256=//p' "$selected_manifest")
    if [ "$selected_format" -eq 1 ]; then
        selected_schedule_hash=$(sed -n '7s/^plist_sha256=//p' "$selected_manifest")
    else
        selected_schedule_hash=$(sed -n '7s/^clockwork_template_sha256=//p' "$selected_manifest")
    fi
    selected_deployer_hash=$(sed -n '8s/^deployer_sha256=//p' "$selected_manifest")
    selected_uninstaller_hash=$(sed -n '9s/^uninstaller_sha256=//p' "$selected_manifest")
    selected_chancery_hash=$(sed -n '10s/^chancery_sha256=//p' "$selected_manifest")
    printf '%s\n' "$selected_manifest_id" "$selected_binary_hash" "$selected_frontend_hash" \
        "$selected_runner_hash" "$selected_schedule_hash" "$selected_deployer_hash" \
        "$selected_uninstaller_hash" "$selected_chancery_hash" \
        | grep -Eqv '^[0-9a-f]{64}$' \
        && fail "selected Semantics release manifest hashes are invalid: $selector"
    printf '%s\n' "$selected_version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+([+-][0-9A-Za-z.-]+)?$' \
        || fail "selected Semantics release version is invalid: $selector"
    [ "$selected_manifest_id" = "$selected_id" ] \
        || fail "selected Semantics release manifest does not match: $selector"
    for owned_file in \
        "$selected_release/libexec/semantics" \
        "$selected_release/bin/semantics" \
        "$selected_release/bin/semantics-worker" \
        "$selected_release/package/semantics" \
        "$selected_release/package/semantics-worker" \
        "$selected_release/package/deploy-user.sh" \
        "$selected_release/package/uninstall-user.sh"
    do
        [ -f "$owned_file" ] && [ ! -L "$owned_file" ] \
            || fail "selected Semantics release is incomplete: $selector"
    done
    if [ "$selected_format" -eq 1 ]; then
        selected_schedule_file="$selected_release/package/$LABEL.plist"
    else
        selected_schedule_file="$selected_release/package/semantics-worker.clockwork.toml.in"
    fi
    [ -f "$selected_schedule_file" ] && [ ! -L "$selected_schedule_file" ] \
        || fail "selected Semantics release has no owned schedule template: $selector"
    validate_bundle "$selected_release/share/chancery/semantics" installed
    actual_binary_hash=$(shasum -a 256 "$selected_release/libexec/semantics" | awk '{print $1}')
    actual_frontend_hash=$(shasum -a 256 "$selected_release/bin/semantics" | awk '{print $1}')
    actual_runner_hash=$(shasum -a 256 "$selected_release/bin/semantics-worker" | awk '{print $1}')
    actual_schedule_hash=$(shasum -a 256 "$selected_schedule_file" | awk '{print $1}')
    actual_deployer_hash=$(shasum -a 256 "$selected_release/package/deploy-user.sh" | awk '{print $1}')
    actual_uninstaller_hash=$(shasum -a 256 "$selected_release/package/uninstall-user.sh" | awk '{print $1}')
    actual_chancery_hash=$(bundle_hash "$selected_release/share/chancery/semantics")
    [ "$actual_binary_hash" = "$selected_binary_hash" ] || fail "selected Semantics binary is tampered: $selector"
    [ "$actual_frontend_hash" = "$selected_frontend_hash" ] || fail "selected Semantics frontend is tampered: $selector"
    [ "$(shasum -a 256 "$selected_release/package/semantics" | awk '{print $1}')" = "$selected_frontend_hash" ] \
        || fail "selected packaged Semantics frontend is tampered: $selector"
    [ "$actual_runner_hash" = "$selected_runner_hash" ] || fail "selected Semantics runner is tampered: $selector"
    [ "$(shasum -a 256 "$selected_release/package/semantics-worker" | awk '{print $1}')" = "$selected_runner_hash" ] \
        || fail "selected packaged Semantics runner is tampered: $selector"
    [ "$actual_schedule_hash" = "$selected_schedule_hash" ] || fail "selected Semantics schedule template is tampered: $selector"
    [ "$actual_deployer_hash" = "$selected_deployer_hash" ] || fail "selected Semantics deployer is tampered: $selector"
    [ "$actual_uninstaller_hash" = "$selected_uninstaller_hash" ] || fail "selected Semantics uninstaller is tampered: $selector"
    [ "$actual_chancery_hash" = "$selected_chancery_hash" ] || fail "selected Semantics provider is tampered: $selector"
    actual_release_id=$(printf '%s\n' "$actual_binary_hash" "$actual_frontend_hash" \
        "$actual_runner_hash" "$actual_schedule_hash" "$actual_deployer_hash" \
        "$actual_uninstaller_hash" "$actual_chancery_hash" | shasum -a 256 | awk '{print $1}')
    [ "$actual_release_id" = "$selected_id" ] \
        || fail "selected Semantics release content ID does not match: $selector"
}

if [ -n "$old_current" ]; then
    validate_release_selector "$old_current"
    current_release_format=$selected_format
    current_schedule_template=$selected_schedule_file
    current_clockwork_release=$selected_release
    current_clockwork_release_id=$selected_id
    current_clockwork_runner_hash=$selected_runner_hash
    [ -z "$old_previous" ] || validate_release_selector "$old_previous"
    [ -z "$old_cli" ] || [ "$old_cli" = "$EXPECTED_CLI" ] \
        || fail "installed command is not owned by Semantics: $CLI_PATH"
elif [ -n "$old_previous" ] || [ -n "$old_cli" ] || [ -n "$old_provider" ]; then
    fail 'installed selectors have no current Semantics release'
fi
[ -z "$old_provider" ] || [ "$old_provider" = "$EXPECTED_PROVIDER" ] \
    || fail "provider selector is not owned by Semantics: $PROVIDER_LINK"

prove_owned_clockwork_definition() {
    definition_digest=$1
    [ -n "${current_clockwork_release:-}" ] \
        || fail 'selected Clockwork binding has no current Semantics release'
    [ "$current_release_format" -eq 2 ] \
        || fail 'selected Clockwork binding cannot be owned by a legacy Semantics release'
    definition_show="$transaction_dir/clockwork-definition.json"
    HOME="$install_home" "$clockwork_path" --json definition show "$definition_digest" \
        >"$definition_show" 2>"$definition_show.stderr" \
        || fail 'unable to inspect the selected Semantics Clockwork definition'
    [ "$(/usr/bin/plutil -extract ok raw "$definition_show" 2>/dev/null)" = true ] \
        && [ "$(/usr/bin/plutil -extract data.digest raw "$definition_show" 2>/dev/null)" = "$definition_digest" ] \
        && [ "$(/usr/bin/plutil -extract data.key raw "$definition_show" 2>/dev/null)" = "$CLOCKWORK_KEY" ] \
        && [ "$(/usr/bin/plutil -extract data.manifest.schema_version raw "$definition_show" 2>/dev/null)" = 1 ] \
        && [ "$(/usr/bin/plutil -extract data.manifest.key raw "$definition_show" 2>/dev/null)" = "$CLOCKWORK_KEY" ] \
        && [ "$(/usr/bin/plutil -extract data.manifest.release_id raw "$definition_show" 2>/dev/null)" = "$current_clockwork_release_id" ] \
        && [ "$(/usr/bin/plutil -extract data.manifest.release_root raw "$definition_show" 2>/dev/null)" = "$current_clockwork_release" ] \
        && [ "$(/usr/bin/plutil -extract data.manifest.authority raw "$definition_show" 2>/dev/null)" = current-user-background ] \
        && [ "$(/usr/bin/plutil -extract data.manifest.overlap raw "$definition_show" 2>/dev/null)" = skip ] \
        && [ "$(/usr/bin/plutil -extract data.manifest.cwd raw "$definition_show" 2>/dev/null)" = "$STATE_DIR" ] \
        && [ "$(/usr/bin/plutil -extract data.manifest.schedule.kind raw "$definition_show" 2>/dev/null)" = interval ] \
        && [ "$(/usr/bin/plutil -extract data.manifest.schedule.seconds raw "$definition_show" 2>/dev/null)" = 60 ] \
        && [ "$(/usr/bin/plutil -extract data.manifest.schedule.run_at_load raw "$definition_show" 2>/dev/null)" = false ] \
        && [ "$(/usr/bin/plutil -extract data.manifest.launch.kind raw "$definition_show" 2>/dev/null)" = interpreted ] \
        && [ "$(/usr/bin/plutil -extract data.manifest.launch.interpreter raw "$definition_show" 2>/dev/null)" = /bin/sh ] \
        && [ "$(/usr/bin/plutil -extract data.manifest.launch.interpreter_sha256 raw "$definition_show" 2>/dev/null)" = "$interpreter_hash" ] \
        && [ "$(/usr/bin/plutil -extract data.manifest.launch.script raw "$definition_show" 2>/dev/null)" = "$current_clockwork_release/bin/semantics-worker" ] \
        && [ "$(/usr/bin/plutil -extract data.manifest.launch.script_sha256 raw "$definition_show" 2>/dev/null)" = "$current_clockwork_runner_hash" ] \
        && [ "$(/usr/bin/plutil -extract data.manifest.environment.HOME raw "$definition_show" 2>/dev/null)" = "$install_home" ] \
        && [ "$(/usr/bin/plutil -extract data.manifest.output.stdout raw "$definition_show" 2>/dev/null)" = "$LOG_DIR/worker.stdout.log" ] \
        && [ "$(/usr/bin/plutil -extract data.manifest.output.stderr raw "$definition_show" 2>/dev/null)" = "$LOG_DIR/worker.stderr.log" ] \
        || fail 'selected Clockwork definition is not owned by the current Semantics release'
    if /usr/bin/plutil -extract data.manifest.timeout_seconds raw "$definition_show" >/dev/null 2>&1 \
        || /usr/bin/plutil -extract data.manifest.arguments.0 raw "$definition_show" >/dev/null 2>&1; then
        fail 'selected Clockwork definition adds unsupported timeout or arguments'
    fi
    environment_keys=$(/usr/bin/plutil -extract data.manifest.environment xml1 -o - \
        "$definition_show" 2>/dev/null | awk '/<key>/{count++} END {print count+0}')
    [ "$environment_keys" -eq 1 ] \
        || fail 'selected Clockwork definition contains foreign environment entries'
}

interpreter_hash=$(shasum -a 256 /bin/sh | awk '{print $1}')

if clockwork_show=$(HOME="$install_home" "$clockwork_path" --json \
    binding show "$CLOCKWORK_KEY" 2>"$transaction_dir/clockwork-show.stderr")
then
    clockwork_compact=$(printf '%s' "$clockwork_show" | tr -d '[:space:]')
    prior_clockwork_digest=$(printf '%s\n' "$clockwork_compact" | sed -n \
        's/.*"definition_digest":"\([0-9a-f]\{64\}\)".*/\1/p')
    case "$clockwork_compact" in
        *'"enabled":true'*)
            prior_clockwork_enabled=1
            [ -n "$prior_clockwork_digest" ] \
                || fail 'enabled Clockwork binding has no definition digest'
            ;;
        *'"enabled":false'*)
            prior_clockwork_enabled=0
            if [ -z "$prior_clockwork_digest" ]; then
                printf '%s\n' "$clockwork_compact" | grep -F '"definition_digest":null' >/dev/null \
                    || fail 'disabled Clockwork binding has an invalid definition digest'
            fi
            ;;
        *) fail 'Clockwork returned an invalid binding document' ;;
    esac
    [ -z "$prior_clockwork_digest" ] \
        || prove_owned_clockwork_definition "$prior_clockwork_digest"
else
    grep -F '"code":"binding_not_found"' "$transaction_dir/clockwork-show.stderr" >/dev/null \
        || fail 'unable to inspect the Clockwork binding'
fi

if [ -L "$PLIST_PATH" ]; then fail "LaunchAgent must not be a symbolic link: $PLIST_PATH"; fi
if [ -e "$PLIST_PATH" ] && [ ! -f "$PLIST_PATH" ]; then fail "LaunchAgent path is occupied: $PLIST_PATH"; fi
if [ -f "$PLIST_PATH" ]; then
    [ -n "$old_current" ] || fail 'LaunchAgent has no owned Semantics release'
    [ "$current_release_format" -eq 1 ] \
        || fail 'legacy LaunchAgent is not owned by the current Semantics release'
    [ "$(stat -f '%u' "$PLIST_PATH")" -eq "$operator_uid" ] \
        || fail 'legacy LaunchAgent is not owned by the Semantics operator'
    [ "$(stat -f '%Lp' "$PLIST_PATH")" = 644 ] \
        || fail 'legacy LaunchAgent permissions are not owned by Semantics'
    expected_legacy_plist_hash=$(sed \
        -e "s|__SEMANTICS_WORKER_RUNNER__|$INSTALL_DIR/current/bin/semantics-worker|g" \
        -e "s|__SEMANTICS_STATE_DIR__|$STATE_DIR|g" \
        -e "s|__SEMANTICS_HOME__|$install_home|g" \
        -e "s|__SEMANTICS_WORKER_STDOUT__|$LOG_DIR/worker.stdout.log|g" \
        -e "s|__SEMANTICS_WORKER_STDERR__|$LOG_DIR/worker.stderr.log|g" \
        "$current_schedule_template" | shasum -a 256 | awk '{print $1}')
    [ "$(shasum -a 256 "$PLIST_PATH" | awk '{print $1}')" = \
        "$expected_legacy_plist_hash" ] \
        || fail 'legacy LaunchAgent bytes do not match the current Semantics release'
    old_plist=$(mktemp "$INSTALL_DIR/.old-worker-plist.XXXXXX")
    cp -p "$PLIST_PATH" "$old_plist"
    cp -p "$PLIST_PATH" "$transaction_dir/prior-worker.plist"
fi
if "$launchctl_path" print "$SERVICE_TARGET" >/dev/null 2>&1; then
    service_was_loaded=1
    [ -n "$old_plist" ] || fail 'loaded Semantics label has no owned recoverable plist'
fi
[ "$prior_clockwork_enabled" -eq 0 ] || [ "$service_was_loaded" -eq 0 ] \
    || fail 'Clockwork and the legacy Semantics LaunchAgent are both active'

binary_hash=$(shasum -a 256 "$binary_path" | awk '{print $1}')
frontend_hash=$(shasum -a 256 "$SOURCE_FRONTEND" | awk '{print $1}')
runner_hash=$(shasum -a 256 "$SOURCE_RUNNER" | awk '{print $1}')
definition_template_hash=$(shasum -a 256 "$SOURCE_DEFINITION" | awk '{print $1}')
deployer_hash=$(shasum -a 256 "$0" | awk '{print $1}')
uninstaller_hash=$(shasum -a 256 "$SOURCE_UNINSTALLER" | awk '{print $1}')
chancery_hash=$(bundle_hash "$SOURCE_CHANCERY")
release_id=$(printf '%s\n' "$binary_hash" "$frontend_hash" "$runner_hash" "$definition_template_hash" \
    "$deployer_hash" "$uninstaller_hash" "$chancery_hash" | shasum -a 256 | awk '{print $1}')
release="$RELEASES_DIR/$release_id"

if [ -L "$release" ]; then
    fail "existing release must not be a symbolic link: $release_id"
elif [ -e "$release" ] && [ ! -d "$release" ]; then
    fail "existing release is not a directory: $release_id"
elif [ -d "$release" ]; then
    validate_release_selector "releases/$release_id"
    [ "$(shasum -a 256 "$release/libexec/semantics" | awk '{print $1}')" = "$binary_hash" ] || fail 'existing release binary is tampered'
    [ "$(shasum -a 256 "$release/bin/semantics" | awk '{print $1}')" = "$frontend_hash" ] || fail 'existing release frontend is tampered'
    [ "$(shasum -a 256 "$release/bin/semantics-worker" | awk '{print $1}')" = "$runner_hash" ] || fail 'existing release runner is tampered'
    [ "$(shasum -a 256 "$release/package/semantics-worker.clockwork.toml.in" | awk '{print $1}')" = "$definition_template_hash" ] || fail 'existing release Clockwork template is tampered'
    [ "$(shasum -a 256 "$release/package/deploy-user.sh" | awk '{print $1}')" = "$deployer_hash" ] || fail 'existing release deployer is tampered'
    [ "$(shasum -a 256 "$release/package/uninstall-user.sh" | awk '{print $1}')" = "$uninstaller_hash" ] || fail 'existing release uninstaller is tampered'
    [ "$(bundle_hash "$release/share/chancery/semantics")" = "$chancery_hash" ] || fail 'existing release provider is tampered'
else
    install -d -m 0755 "$temporary/bin" "$temporary/libexec" "$temporary/package" "$temporary/share/chancery"
    install -m 0755 "$binary_path" "$temporary/libexec/semantics"
    install -m 0755 "$SOURCE_FRONTEND" "$temporary/bin/semantics"
    install -m 0755 "$SOURCE_RUNNER" "$temporary/bin/semantics-worker"
    install -m 0755 "$SOURCE_FRONTEND" "$temporary/package/semantics"
    install -m 0755 "$SOURCE_RUNNER" "$temporary/package/semantics-worker"
    install -m 0755 "$0" "$temporary/package/deploy-user.sh"
    install -m 0755 "$SOURCE_UNINSTALLER" "$temporary/package/uninstall-user.sh"
    install -m 0644 "$SOURCE_DEFINITION" \
        "$temporary/package/semantics-worker.clockwork.toml.in"
    cp -R "$SOURCE_CHANCERY" "$temporary/share/chancery/semantics"
    {
        printf '%s\n' 'format=2'
        printf 'release_id=%s\n' "$release_id"
        printf 'version=%s\n' "$version"
        printf 'binary_sha256=%s\n' "$binary_hash"
        printf 'frontend_sha256=%s\n' "$frontend_hash"
        printf 'runner_sha256=%s\n' "$runner_hash"
        printf 'clockwork_template_sha256=%s\n' "$definition_template_hash"
        printf 'deployer_sha256=%s\n' "$deployer_hash"
        printf 'uninstaller_sha256=%s\n' "$uninstaller_hash"
        printf 'chancery_sha256=%s\n' "$chancery_hash"
    } >"$temporary/manifest.txt"
    chmod 0444 "$temporary/manifest.txt"
    chmod -R go-w "$temporary"
    mv "$temporary" "$release"
    temporary=$(mktemp -d "$INSTALL_DIR/.candidate.XXXXXX")
    validate_release_selector "releases/$release_id"
fi

if [ -x /opt/homebrew/bin/codex ]; then
    codex_path=/opt/homebrew/bin/codex
elif [ -x "$install_home/.local/bin/codex" ]; then
    codex_path="$install_home/.local/bin/codex"
else
    fail 'Codex executable is unavailable'
fi
[ -x "$install_home/.local/bin/annals" ] || fail 'Annals executable is unavailable'
[ -f "$install_home/Library/Application Support/Annals/decisions/config.toml" ] \
    && [ ! -L "$install_home/Library/Application Support/Annals/decisions/config.toml" ] \
    || fail 'Annals decisions config is unavailable'

engage_maintenance \
    || fail 'Semantics maintenance gate is invalid or unavailable'
if [ -e "$MAINTENANCE_HOLD_RECEIPT" ]; then
    validate_private_database_file "$MAINTENANCE_HOLD_RECEIPT" \
        'maintenance hold receipt'
    maintenance_marker_is_owned \
        || fail 'maintenance hold receipt gate is invalid'
    receipt_key_count=$(/usr/bin/plutil -convert xml1 -o - \
        "$MAINTENANCE_HOLD_RECEIPT" 2>/dev/null \
        | awk '/<key>/{count++} END {print count+0}')
    [ "$receipt_key_count" -eq 4 ] \
        && [ "$(/usr/bin/plutil -extract version raw \
            "$MAINTENANCE_HOLD_RECEIPT" 2>/dev/null)" = 1 ] \
        && [ "$(/usr/bin/plutil -extract key raw \
            "$MAINTENANCE_HOLD_RECEIPT" 2>/dev/null)" = "$CLOCKWORK_KEY" ] \
        && [ "$(/usr/bin/plutil -extract release_id raw \
            "$MAINTENANCE_HOLD_RECEIPT" 2>/dev/null)" = \
            "${current_clockwork_release_id:-}" ] \
        && [ "$(/usr/bin/plutil -extract definition_digest raw \
            "$MAINTENANCE_HOLD_RECEIPT" 2>/dev/null)" = "$prior_clockwork_digest" ] \
        && [ -n "${current_clockwork_release_id:-}" ] \
        && [ -n "$prior_clockwork_digest" ] \
        || fail 'maintenance hold receipt does not match current owned state'
    install -m 0600 "$MAINTENANCE_HOLD_RECEIPT" \
        "$transaction_dir/maintenance-hold.before"
    hold_existed=1
    maintenance_owned=1
elif [ "$maintenance_created" -eq 1 ]; then
    maintenance_owned=1
else
    maintenance_preexisting=1
fi
prepare_private_log "$LOG_DIR/worker.stdout.log"
prepare_private_log "$LOG_DIR/worker.stderr.log"

sed \
    -e "s|__RELEASE_ID__|$release_id|g" \
    -e "s|__RELEASE_ROOT__|$release|g" \
    -e "s|__SEMANTICS_STATE__|$STATE_DIR|g" \
    -e "s|__SEMANTICS_HOME__|$install_home|g" \
    -e "s|__SEMANTICS_LOGS__|$LOG_DIR|g" \
    -e "s|__INTERPRETER_SHA256__|$interpreter_hash|g" \
    -e "s|__RUNNER_SHA256__|$runner_hash|g" \
    "$release/package/semantics-worker.clockwork.toml.in" >"$temporary_definition"
chmod 0600 "$temporary_definition"
definition_output=$(HOME="$install_home" "$clockwork_path" --json \
    definition register "$temporary_definition") \
    || fail 'Clockwork rejected the candidate worker definition'
definition_compact=$(printf '%s' "$definition_output" | tr -d '[:space:]')
candidate_definition_digest=$(printf '%s\n' "$definition_compact" | sed -n \
    's/.*"digest":"\([0-9a-f]\{64\}\)".*/\1/p')
[ -n "$candidate_definition_digest" ] \
    || fail 'Clockwork returned no candidate definition digest'

# Treat the transition as changed before calling Clockwork because an error
# may still mean Clockwork failed closed with the binding disabled.
clockwork_disabled=1
HOME="$install_home" "$clockwork_path" --json binding disable \
    "$CLOCKWORK_KEY" >/dev/null \
    || fail 'unable to disable the Clockwork worker binding'
if [ "$service_was_loaded" -eq 1 ]; then
    # A failing bootout can still have stopped the service; make rollback
    # prove restoration instead of assuming no transition occurred.
    service_stopped=1
    "$launchctl_path" bootout "$SERVICE_TARGET" >/dev/null \
        || fail 'unable to stop the owned worker service'
fi
if [ -n "$old_plist" ]; then
    rm -f "$PLIST_PATH"
    legacy_plist_removed=1
fi
if [ -n "$old_cli" ]; then
    rm -f "$CLI_PATH"
    cli_suspended=1
fi

WORKER_LOCK_PATH="$DATABASE_PATH.worker.lock"
if [ -L "$WORKER_LOCK_PATH" ]; then
    fail 'worker lock must not be a symbolic link'
elif [ -e "$WORKER_LOCK_PATH" ] && [ ! -f "$WORKER_LOCK_PATH" ]; then
    fail 'worker lock must be a regular file'
fi
rm -f "$worker_lock_ready" "$worker_lock_stop"
/usr/bin/perl -MFcntl=:flock -e '
    my ($path, $ready, $stop) = @ARGV;
    open(my $lock, ">>", $path) or exit 70;
    flock($lock, LOCK_EX | LOCK_NB) or exit 75;
    open(my $signal, ">", $ready) or exit 71;
    print {$signal} "ready\n"; close($signal) or exit 72;
    while (!-e $stop) { select(undef, undef, undef, 0.05); }
' "$WORKER_LOCK_PATH" "$worker_lock_ready" "$worker_lock_stop" &
worker_lock_pid=$!
worker_lock_attempt=0
while [ ! -f "$worker_lock_ready" ]; do
    if ! kill -0 "$worker_lock_pid" >/dev/null 2>&1; then
        if wait "$worker_lock_pid"; then worker_lock_status=0; else worker_lock_status=$?; fi
        worker_lock_pid=
        case "$worker_lock_status" in
            75) fail 'another Semantics worker is active' ;;
            *) fail 'unable to acquire the Semantics worker lock' ;;
        esac
    fi
    worker_lock_attempt=$((worker_lock_attempt + 1))
    [ "$worker_lock_attempt" -le 40 ] || fail 'timed out acquiring the Semantics worker lock'
    /bin/sleep 0.05
done

{
    printf 'current=%s\n' "$old_current"
    printf 'previous=%s\n' "$old_previous"
    printf 'cli=%s\n' "$old_cli"
    printf 'provider=%s\n' "$old_provider"
    printf 'service_was_loaded=%s\n' "$service_was_loaded"
    printf 'clockwork_enabled=%s\n' "$prior_clockwork_enabled"
    printf 'clockwork_definition=%s\n' "$prior_clockwork_digest"
} >"$transaction_dir/prior-install.txt"
chmod 0600 "$transaction_dir/prior-install.txt"

preflight_private_database_files
if [ ! -e "$DATABASE_PATH" ]; then
    database_was_absent=1
fi
for suffix in wal shm journal; do
    sidecar="$DATABASE_PATH-$suffix"
    [ ! -f "$sidecar" ] || [ "$database_was_absent" -eq 0 ] \
        || fail "database sidecar exists without its database: $sidecar"
done

require_database_quiescent() {
    [ "$database_was_absent" -eq 0 ] || return 0
    if /usr/sbin/lsof -t -- "$DATABASE_PATH" >/dev/null 2>&1; then
        fail 'database is open by another Semantics process'
    else
        lsof_status=$?
        [ "$lsof_status" -eq 1 ] || fail 'unable to verify database quiescence'
    fi
}
require_database_quiescent
if [ "$database_was_absent" -eq 0 ]; then
    install -m 0600 "$DATABASE_PATH" "$transaction_dir/semantics.db"
    for suffix in wal shm journal; do
        [ ! -f "$DATABASE_PATH-$suffix" ] || \
            install -m 0600 "$DATABASE_PATH-$suffix" "$transaction_dir/semantics.db-$suffix"
    done
fi
require_database_quiescent

database_touched=1
if [ "$final_decisions_watermark_set" -eq 1 ]; then
    /usr/bin/env -i \
        HOME="$install_home" \
        PATH=/usr/bin:/bin:/usr/sbin:/sbin \
        CONVERSATIONS_CODEX="$codex_path" \
        SEMANTICS_ANNALS="$install_home/.local/bin/annals" \
        SEMANTICS_ANNALS_CONFIG="$install_home/Library/Application Support/Annals/decisions/config.toml" \
        "$release/libexec/semantics" --database "$DATABASE_PATH" --json \
            project activate-annals \
            --final-decisions-watermark "$final_decisions_watermark" \
            >"$transaction_dir/annals-activation.json" \
            2>"$transaction_dir/annals-activation.stderr" \
        || fail 'candidate rejected the asserted final Decisions watermark or Annals activation'
fi
doctor_output=$(/usr/bin/env -i \
    HOME="$install_home" \
    PATH=/usr/bin:/bin:/usr/sbin:/sbin \
    CONVERSATIONS_CODEX="$codex_path" \
    SEMANTICS_ANNALS="$install_home/.local/bin/annals" \
    SEMANTICS_ANNALS_CONFIG="$install_home/Library/Application Support/Annals/decisions/config.toml" \
    "$release/libexec/semantics" --database "$DATABASE_PATH" --json doctor \
    2>"$transaction_dir/doctor.stderr") \
    || fail 'candidate doctor failed'
doctor_compact=$(printf '%s' "$doctor_output" | tr -d '[:space:]')
case "$doctor_compact" in *'"ok":true'*) ;; *) fail 'candidate doctor did not report ok' ;; esac
for check_name in database participation_markers annals_decision_feed conversations_exact_cwd nucleus_reconciliation; do
    case "$doctor_compact" in
        *"\"name\":\"$check_name\",\"ok\":true"*) ;;
        *) fail "candidate doctor did not prove $check_name" ;;
    esac
done
case "$doctor_compact" in *'"detail":"schema2at'*) ;; *) fail 'candidate doctor did not prove schema version 2' ;; esac

# Record the exact owned hold before selector or scheduler publication. A later
# invocation can therefore prove and release a retained handoff, while an
# unrelated pre-existing marker remains unclaimed.
if [ "$maintenance_owned" -eq 1 ]; then
    maintenance_hold_next="$transaction_dir/maintenance-hold.next"
    {
        printf '{\n'
        printf '  "version": 1,\n'
        printf '  "key": "%s",\n' "$CLOCKWORK_KEY"
        printf '  "release_id": "%s",\n' "$release_id"
        printf '  "definition_digest": "%s"\n' "$candidate_definition_digest"
        printf '}\n'
    } >"$maintenance_hold_next"
    chmod 0600 "$maintenance_hold_next"
    hold_changed=1
    mv "$maintenance_hold_next" "$MAINTENANCE_HOLD_RECEIPT"
    validate_private_database_file "$MAINTENANCE_HOLD_RECEIPT" \
        'maintenance hold receipt'
fi

acquire_catalog_lock
switched=1
if [ -n "$old_current" ] && [ "$old_current" != "releases/$release_id" ]; then
    atomic_symlink "$old_current" "$PREVIOUS_LINK"
fi
atomic_symlink "releases/$release_id" "$CURRENT_LINK"
atomic_symlink "$EXPECTED_PROVIDER" "$PROVIDER_LINK"
atomic_symlink "$EXPECTED_CLI" "$CLI_PATH"
cli_suspended=0
clockwork_switched=1
HOME="$install_home" "$clockwork_path" --json binding switch \
    "$CLOCKWORK_KEY" "$candidate_definition_digest" >/dev/null \
    || fail 'Clockwork rejected the worker binding switch'

completed_at=$(date -u '+%Y-%m-%dT%H:%M:%SZ')
if [ "$maintenance_owned" -eq 0 ] || [ "$keep_maintenance" -eq 1 ]; then
    maintenance_retained=1
fi
if [ -n "$old_current" ] && [ "$old_current" != "releases/$release_id" ]; then
    rollback_snapshot="$DEPLOYMENT_BACKUPS_DIR/pre-$release_id-$$"
    rollback_stage="$transaction_dir/rollback-snapshot"
    install -d -m 0700 "$rollback_stage"
    install -m 0600 "$transaction_dir/prior-install.txt" \
        "$rollback_stage/prior-install.txt"
    if [ -f "$transaction_dir/prior-worker.plist" ]; then
        install -m 0600 "$transaction_dir/prior-worker.plist" \
            "$rollback_stage/prior-worker.plist"
    fi
    if [ -f "$transaction_dir/semantics.db" ]; then
        install -m 0600 "$transaction_dir/semantics.db" \
            "$rollback_stage/semantics.db"
        for suffix in wal shm journal; do
            [ ! -f "$transaction_dir/semantics.db-$suffix" ] || \
                install -m 0600 "$transaction_dir/semantics.db-$suffix" \
                    "$rollback_stage/semantics.db-$suffix"
        done
    fi
    mv "$rollback_stage" "$rollback_snapshot"
    rollback_snapshot_created=1
fi
receipt="$LAST_UPDATE_PATH.tmp.$$"
{
    printf 'release=releases/%s\n' "$release_id"
    printf 'previous=%s\n' "$old_current"
    printf 'clockwork_definition=%s\n' "$candidate_definition_digest"
    printf 'previous_clockwork_definition=%s\n' "$prior_clockwork_digest"
    printf 'previous_clockwork_enabled=%s\n' "$prior_clockwork_enabled"
    printf 'legacy_launchagent_loaded=%s\n' "$service_was_loaded"
    printf 'maintenance_preexisting=%s\n' "$maintenance_preexisting"
    printf 'maintenance_owned=%s\n' "$maintenance_owned"
    printf 'maintenance_retained=%s\n' "$maintenance_retained"
    printf 'rollback_snapshot=%s\n' "$rollback_snapshot"
    printf 'completed_at=%s\n' "$completed_at"
} >"$receipt"
chmod 0600 "$receipt"
mv -f "$receipt" "$INSTALL_DIR/last-update.txt"
receipt_changed=1
release_worker_lock || fail 'unable to release the Semantics worker lock'

committed=1
maintenance_marker_is_owned \
    || fail 'committed Semantics but the maintenance gate changed'
if [ "$maintenance_owned" -eq 1 ] && [ "$keep_maintenance" -eq 0 ]; then
    rm -f "$MAINTENANCE_HOLD_RECEIPT" \
        || fail 'committed Semantics but could not clear its maintenance receipt'
    rm -f "$MAINTENANCE_MARKER" \
        || fail 'committed Semantics but could not clear its maintenance gate'
    [ ! -e "$MAINTENANCE_MARKER" ] && [ ! -L "$MAINTENANCE_MARKER" ] \
        || fail 'committed Semantics but its maintenance gate remains'
    maintenance_created=0
fi
printf 'installed semantics %s (%s)\n' "$version" "$release_id"
