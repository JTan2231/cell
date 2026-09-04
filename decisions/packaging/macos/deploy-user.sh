#!/bin/sh

set -eu

PATH=/usr/bin:/bin:/usr/sbin:/sbin
export PATH
umask 077

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
ACTIVE_CLOCKWORK_KEY=krisis/observer
LEGACY_OBSERVER_CLOCKWORK_KEY=decisions/observer
LEGACY_DAILY_CLOCKWORK_KEY=decisions/daily-email
OBSERVER_LABEL=org.decisions.observer
DAILY_LABEL=org.decisions.daily-email
SOURCE_FRONTEND="$SCRIPT_DIR/krisis"
SOURCE_RUNNER="$SCRIPT_DIR/krisis-observer"
SOURCE_DEFINITION="$SCRIPT_DIR/krisis-observer.clockwork.toml.in"
SOURCE_HOOKS="$SCRIPT_DIR/hooks.json"
SOURCE_UNINSTALLER="$SCRIPT_DIR/uninstall-user.sh"
if [ -d "$SCRIPT_DIR/../share/chancery/krisis" ]; then
    SOURCE_CHANCERY="$SCRIPT_DIR/../share/chancery/krisis"
    SOURCE_LEGACY_CHANCERY="$SCRIPT_DIR/../share/chancery/decisions"
else
    SOURCE_CHANCERY="$SCRIPT_DIR/../../chancery"
    SOURCE_LEGACY_CHANCERY="$SCRIPT_DIR/../../chancery-legacy"
fi

binary_path=
clockwork_path=
annals_path=
annals_config=
annals_library_id=
install_home=${HOME:-}
launchctl_path=/bin/launchctl
final_cutover=0
keep_maintenance=0
release_maintenance=0

fail() {
    printf 'krisis user deploy: %s\n' "$*" >&2
    exit 1
}

usage() {
    printf '%s\n' 'Usage: deploy-user.sh --binary ABSOLUTE_KRISIS --clockwork ABSOLUTE_CLOCKWORK --annals ABSOLUTE_ANNALS --annals-config ABSOLUTE_CONFIG --annals-library-id LOWERCASE_32_HEX [--home ABSOLUTE_HOME] [--launchctl ABSOLUTE_LAUNCHCTL] [--final-cutover [--keep-maintenance] | --release-maintenance]'
    printf '%s\n' 'Without --final-cutover, installs and registers the candidate while retaining the maintenance gate.'
    printf '%s\n' '--release-maintenance idempotently releases only the exact authenticated hold created for the current installation.'
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --binary) [ "$#" -ge 2 ] || fail '--binary requires a path'; binary_path=$2; shift 2 ;;
        --clockwork) [ "$#" -ge 2 ] || fail '--clockwork requires a path'; clockwork_path=$2; shift 2 ;;
        --annals) [ "$#" -ge 2 ] || fail '--annals requires a path'; annals_path=$2; shift 2 ;;
        --annals-config) [ "$#" -ge 2 ] || fail '--annals-config requires a path'; annals_config=$2; shift 2 ;;
        --annals-library-id) [ "$#" -ge 2 ] || fail '--annals-library-id requires an ID'; annals_library_id=$2; shift 2 ;;
        --home) [ "$#" -ge 2 ] || fail '--home requires a path'; install_home=$2; shift 2 ;;
        --launchctl) [ "$#" -ge 2 ] || fail '--launchctl requires a path'; launchctl_path=$2; shift 2 ;;
        --final-cutover) final_cutover=1; shift ;;
        --keep-maintenance) keep_maintenance=1; shift ;;
        --release-maintenance) release_maintenance=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *) fail "unknown argument: $1" ;;
    esac
done

[ "$keep_maintenance" -eq 0 ] || [ "$final_cutover" -eq 1 ] \
    || fail '--keep-maintenance requires --final-cutover'
[ "$release_maintenance" -eq 0 ] || { [ "$final_cutover" -eq 0 ] && [ "$keep_maintenance" -eq 0 ]; } \
    || fail '--release-maintenance cannot be combined with final-cutover options'

for named_path in "$binary_path" "$clockwork_path" "$annals_path" "$annals_config" "$install_home" "$launchctl_path"; do
    case "$named_path" in /*) ;; *) fail 'all binary, config, home, and launchctl paths must be absolute' ;; esac
done
case "$annals_library_id" in
    [0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f]) ;;
    *) fail 'annals library ID must be exactly 32 lowercase hexadecimal characters' ;;
esac
case "$install_home$annals_path$annals_config" in *'"'*|*'\'*|*'|'*|*'&'*|*'
'*) fail 'home and Annals paths contain characters unsupported by manifest rendering' ;; esac
operator_uid=$(id -u)
[ "$operator_uid" -ne 0 ] || fail 'run as the Krisis operator, not root'
[ -d "$install_home" ] && [ ! -L "$install_home" ] || fail 'home is not a regular directory'
[ "$(stat -f '%u' "$install_home")" -eq "$operator_uid" ] || fail 'home is not owned by the operator'
for executable in "$binary_path" "$clockwork_path" "$annals_path"; do
    [ -f "$executable" ] && [ ! -L "$executable" ] && [ -x "$executable" ] || fail "executable is unavailable: $executable"
done
[ -f "$annals_config" ] && [ ! -L "$annals_config" ] || fail 'Annals config is not a regular file'
if [ "$final_cutover" -eq 1 ] || [ "$release_maintenance" -eq 1 ]; then
    [ -f "$launchctl_path" ] && [ ! -L "$launchctl_path" ] && [ -x "$launchctl_path" ] || fail 'launchctl is unavailable'
fi
if [ "$final_cutover" -eq 1 ]; then
    [ -x /usr/sbin/lsof ] || fail 'lsof is unavailable'
fi
for source in "$SOURCE_FRONTEND" "$SOURCE_RUNNER" "$SOURCE_DEFINITION" "$SOURCE_HOOKS" "$SOURCE_UNINSTALLER"; do
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

atomic_symlink() {
    target=$1
    path=$2
    temporary_link="$path.tmp.$$"
    rm -f "$temporary_link"
    ln -s "$target" "$temporary_link"
    if mv -fh "$temporary_link" "$path" 2>/dev/null; then return 0; fi
    mv -fT "$temporary_link" "$path"
}

maintenance_marker_is_owned() {
    [ -f "$MAINTENANCE_MARKER" ] && [ ! -L "$MAINTENANCE_MARKER" ] \
        && [ "$(stat -f '%u' "$MAINTENANCE_MARKER")" -eq "$operator_uid" ] \
        && [ "$(stat -f '%Lp' "$MAINTENANCE_MARKER")" = 600 ] \
        && [ "$(stat -f '%l' "$MAINTENANCE_MARKER")" -eq 1 ]
}

hold_receipt_is_private() {
    [ -f "$HOLD_RECEIPT" ] && [ ! -L "$HOLD_RECEIPT" ] \
        && [ "$(stat -f '%u' "$HOLD_RECEIPT")" -eq "$operator_uid" ] \
        && [ "$(stat -f '%Lp' "$HOLD_RECEIPT")" = 600 ] \
        && [ "$(stat -f '%l' "$HOLD_RECEIPT")" -eq 1 ] \
        && [ "$(awk 'END {print NR}' "$HOLD_RECEIPT")" -eq 9 ] \
        && [ "$(sed -n '1p' "$HOLD_RECEIPT")" = format=1 ] \
        && [ "$(sed -n '2p' "$HOLD_RECEIPT")" = "key=$ACTIVE_CLOCKWORK_KEY" ]
}

hold_receipt_matches_gate() {
    hold_receipt_is_private && maintenance_marker_is_owned \
        && [ "$(sed -n '8s/^gate_device=//p' "$HOLD_RECEIPT")" = "$(stat -f '%d' "$MAINTENANCE_MARKER")" ] \
        && [ "$(sed -n '9s/^gate_inode=//p' "$HOLD_RECEIPT")" = "$(stat -f '%i' "$MAINTENANCE_MARKER")" ]
}

hold_receipt_matches_candidate() {
    hold_receipt_is_private \
        && [ "$(sed -n '3s/^release_id=//p' "$HOLD_RECEIPT")" = "$release_id" ] \
        && [ "$(sed -n '4s/^definition_digest=//p' "$HOLD_RECEIPT")" = "$candidate_definition_digest" ] \
        && [ "$(sed -n '5s/^annals_binary=//p' "$HOLD_RECEIPT")" = "$annals_path" ] \
        && [ "$(sed -n '6s/^annals_config=//p' "$HOLD_RECEIPT")" = "$annals_config" ] \
        && [ "$(sed -n '7s/^annals_library_id=//p' "$HOLD_RECEIPT")" = "$annals_library_id" ]
}

write_hold_receipt() {
    hold_next="$TRANSACTION/krisis-maintenance-hold.next"
    {
        printf '%s\n' 'format=1'
        printf 'key=%s\n' "$ACTIVE_CLOCKWORK_KEY"
        printf 'release_id=%s\n' "$release_id"
        printf 'definition_digest=%s\n' "$candidate_definition_digest"
        printf 'annals_binary=%s\n' "$annals_path"
        printf 'annals_config=%s\n' "$annals_config"
        printf 'annals_library_id=%s\n' "$annals_library_id"
        printf 'gate_device=%s\n' "$(stat -f '%d' "$MAINTENANCE_MARKER")"
        printf 'gate_inode=%s\n' "$(stat -f '%i' "$MAINTENANCE_MARKER")"
    } >"$hold_next"
    chmod 0600 "$hold_next"
    hold_receipt_changed=1
    install -m 0600 "$hold_next" "$HOLD_RECEIPT"
    hold_receipt_matches_candidate && hold_receipt_matches_gate
}

restore_hold_receipt() {
    if [ "$hold_receipt_changed" -eq 0 ]; then return 0; fi
    if [ "$hold_receipt_existed" -eq 1 ]; then
        install -m 0600 "$old_hold_receipt" "$HOLD_RECEIPT"
    else
        rm -f "$HOLD_RECEIPT"
    fi
}

engage_maintenance() {
    if [ -L "$MAINTENANCE_MARKER" ] || { [ -e "$MAINTENANCE_MARKER" ] && [ ! -f "$MAINTENANCE_MARKER" ]; }; then
        return 1
    fi
    if [ -e "$MAINTENANCE_MARKER" ]; then maintenance_marker_is_owned; return; fi
    (set -C; : >"$MAINTENANCE_MARKER") || return 1
    maintenance_created=1
    chmod 0600 "$MAINTENANCE_MARKER" || return 1
    maintenance_marker_is_owned
}

validate_private_database_file() {
    private_path=$1
    private_label=$2
    [ "$(stat -f '%u' "$private_path")" -eq "$operator_uid" ] \
        && [ "$(stat -f '%Lp' "$private_path")" = 600 ] \
        && [ "$(stat -f '%l' "$private_path")" -eq 1 ] \
        || fail "$private_label is not an exclusive private operator file"
}

validate_existing_database_paths() {
    if [ -L "$DATABASE_PATH" ] || { [ -e "$DATABASE_PATH" ] && [ ! -f "$DATABASE_PATH" ]; }; then fail 'database path is unsafe'; fi
    if [ -f "$DATABASE_PATH" ]; then validate_private_database_file "$DATABASE_PATH" database; fi
    for validated_suffix in wal shm journal; do
        validated_sidecar="$DATABASE_PATH-$validated_suffix"
        if [ -L "$validated_sidecar" ] || { [ -e "$validated_sidecar" ] && [ ! -f "$validated_sidecar" ]; }; then fail "database sidecar is unsafe: $validated_sidecar"; fi
        [ -f "$DATABASE_PATH" ] || [ ! -e "$validated_sidecar" ] || fail "database sidecar exists without database: $validated_sidecar"
        [ ! -f "$validated_sidecar" ] || validate_private_database_file "$validated_sidecar" "database $validated_suffix sidecar"
    done
}

candidate_version=$("$binary_path" --version) || fail 'unable to read candidate version'
case "$candidate_version" in 'krisis '*) version=${candidate_version#krisis } ;; *) fail "unexpected candidate version: $candidate_version" ;; esac
provider_version=$(awk -F '"' '/"release"[[:space:]]*:/ { print $4; exit }' "$SOURCE_CHANCERY/provider.json")
legacy_provider_version=$(awk -F '"' '/"release"[[:space:]]*:/ { print $4; exit }' "$SOURCE_LEGACY_CHANCERY/provider.json")
[ "$provider_version" = "$version" ] || fail 'candidate and Krisis provider versions differ'
[ "$legacy_provider_version" = "$version" ] || fail 'candidate and Decisions compatibility provider versions differ'
grep -F '"id": "krisis"' "$SOURCE_CHANCERY/provider.json" >/dev/null || fail 'provider identity is not krisis'
grep -F '"id": "decisions"' "$SOURCE_LEGACY_CHANCERY/provider.json" >/dev/null || fail 'compatibility provider identity is not decisions'
validate_bundle "$SOURCE_CHANCERY"
validate_bundle "$SOURCE_LEGACY_CHANCERY"

STATE_DIR="$install_home/Library/Application Support/Decisions"
INSTALL_DIR="$STATE_DIR/install"
RELEASES_DIR="$INSTALL_DIR/releases"
CURRENT_LINK="$INSTALL_DIR/current"
PREVIOUS_LINK="$INSTALL_DIR/previous"
LOCK_DIR="$INSTALL_DIR/.update-lock"
BINDING_RECEIPT="$INSTALL_DIR/krisis-observer-binding.txt"
HOLD_RECEIPT="$INSTALL_DIR/krisis-maintenance-hold.txt"
LOG_DIR="$install_home/Library/Logs/Decisions"
DATABASE_PATH="$STATE_DIR/decisions.db"
MAINTENANCE_MARKER="$STATE_DIR/.clockwork-maintenance"
CLI_DIR="$install_home/.local/bin"
CLI_PATH="$CLI_DIR/krisis"
LEGACY_CLI_PATH="$CLI_DIR/decisions"
PROVIDERS_DIR="$install_home/Library/Application Support/Chancery/providers"
PROVIDER_LINK="$PROVIDERS_DIR/krisis"
LEGACY_PROVIDER_LINK="$PROVIDERS_DIR/decisions"
HOOKS_PATH="$install_home/.codex/hooks.json"
AGENT_DIR="$install_home/Library/LaunchAgents"
OBSERVER_PLIST="$AGENT_DIR/$OBSERVER_LABEL.plist"
DAILY_PLIST="$AGENT_DIR/$DAILY_LABEL.plist"
SERVICE_DOMAIN="gui/$operator_uid"
OBSERVER_TARGET="$SERVICE_DOMAIN/$OBSERVER_LABEL"
DAILY_TARGET="$SERVICE_DOMAIN/$DAILY_LABEL"

for directory in "$STATE_DIR" "$INSTALL_DIR" "$RELEASES_DIR" "$LOG_DIR"; do
    [ ! -L "$directory" ] || fail "refusing symbolic-link directory: $directory"
    [ ! -e "$directory" ] || [ -d "$directory" ] || fail "directory path is occupied: $directory"
    [ -d "$directory" ] || install -d -m 0700 "$directory"
done
for directory in "$CLI_DIR" "$PROVIDERS_DIR" "$install_home/.codex" "$AGENT_DIR"; do
    [ ! -L "$directory" ] || fail "refusing symbolic-link directory: $directory"
    [ ! -e "$directory" ] || [ -d "$directory" ] || fail "directory path is occupied: $directory"
    [ -d "$directory" ] || install -d -m 0755 "$directory"
done

trap '' HUP INT TERM
mkdir "$LOCK_DIR" 2>/dev/null || fail 'another Krisis deployment is active'
TEMPORARY=
TRANSACTION=
maintenance_created=0
maintenance_preexisting=0
maintenance_owned=0
committed=0
mutation_started=0
retain_transaction=0
old_current=
old_previous=
old_cli=
old_legacy_cli=
old_provider=
old_legacy_provider=
old_hooks=
old_observer_plist=
old_daily_plist=
old_binding_receipt=
old_hold_receipt=
expected_old_observer_plist=
expected_old_daily_plist=
selectors_switched=0
public_suspended=0
hooks_changed=0
database_touched=0
database_was_absent=0
observer_plist_changed=0
daily_plist_changed=0
observer_service_stopped=0
daily_service_stopped=0
observer_was_loaded=0
daily_was_loaded=0
touched_active=0
touched_legacy_observer=0
touched_legacy_daily=0
active_switched=0
binding_receipt_changed=0
hold_receipt_existed=0
hold_receipt_changed=0
prior_active_exists=0
prior_active_enabled=0
prior_active_digest=
prior_legacy_observer_exists=0
prior_legacy_observer_enabled=0
prior_legacy_observer_digest=
prior_legacy_daily_exists=0
prior_legacy_daily_enabled=0
prior_legacy_daily_digest=

restore_binding() {
    restore_key=$1
    restore_exists=$2
    restore_enabled=$3
    restore_digest=$4
    if [ "$restore_exists" -eq 0 ] || [ -z "$restore_digest" ]; then return 2; fi
    if [ "$restore_enabled" -eq 1 ]; then
        HOME="$install_home" "$clockwork_path" --json binding switch "$restore_key" "$restore_digest" >/dev/null 2>&1
    else
        HOME="$install_home" "$clockwork_path" --json binding disable "$restore_key" --select "$restore_digest" >/dev/null 2>&1
    fi
}

cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    set +e
    rollback_ready=1
    if [ "$status" -ne 0 ] && [ "$committed" -eq 0 ] && [ "$mutation_started" -eq 0 ]; then
        if [ "$maintenance_created" -eq 1 ]; then
            maintenance_marker_is_owned && rm -f "$MAINTENANCE_MARKER" || retain_transaction=1
        fi
        if [ "$retain_transaction" -eq 0 ]; then
            restore_hold_receipt || retain_transaction=1
        fi
        [ -z "$TEMPORARY" ] || rm -rf "$TEMPORARY"
        if [ -n "$TRANSACTION" ] && [ "$retain_transaction" -eq 0 ]; then rm -rf "$TRANSACTION"; fi
        rmdir "$LOCK_DIR" 2>/dev/null
        exit "$status"
    fi
    if [ "$status" -ne 0 ] && [ "$committed" -eq 0 ] && [ "$final_cutover" -eq 1 ]; then
        if [ "$active_switched" -eq 1 ]; then
            HOME="$install_home" "$clockwork_path" --json binding disable "$ACTIVE_CLOCKWORK_KEY" >/dev/null 2>&1 || rollback_ready=0
        fi
        if [ "$binding_receipt_changed" -eq 1 ]; then
            if [ -n "$old_binding_receipt" ]; then
                install -m 0600 "$old_binding_receipt" "$BINDING_RECEIPT" || rollback_ready=0
            else
                rm -f "$BINDING_RECEIPT" || rollback_ready=0
            fi
        fi
        if [ "$touched_legacy_daily" -eq 1 ]; then
            restore_binding "$LEGACY_DAILY_CLOCKWORK_KEY" "$prior_legacy_daily_exists" "$prior_legacy_daily_enabled" "$prior_legacy_daily_digest" || rollback_ready=0
        fi
        if [ "$touched_legacy_observer" -eq 1 ]; then
            restore_binding "$LEGACY_OBSERVER_CLOCKWORK_KEY" "$prior_legacy_observer_exists" "$prior_legacy_observer_enabled" "$prior_legacy_observer_digest" || rollback_ready=0
        fi
        if [ "$touched_active" -eq 1 ]; then
            restore_binding "$ACTIVE_CLOCKWORK_KEY" "$prior_active_exists" "$prior_active_enabled" "$prior_active_digest"
            restore_status=$?
            [ "$restore_status" -eq 0 ] || rollback_ready=0
        fi
        if [ "$observer_plist_changed" -eq 1 ]; then "$launchctl_path" bootout "$OBSERVER_TARGET" >/dev/null 2>&1 || true; fi
        if [ "$daily_plist_changed" -eq 1 ]; then "$launchctl_path" bootout "$DAILY_TARGET" >/dev/null 2>&1 || true; fi
        if [ "$public_suspended" -eq 1 ] || [ "$selectors_switched" -eq 1 ]; then
            rm -f "$CLI_PATH" "$LEGACY_CLI_PATH"
        fi
        if [ "$selectors_switched" -eq 1 ]; then
            rm -f "$PROVIDER_LINK" "$LEGACY_PROVIDER_LINK"
        fi
        if [ "$database_touched" -eq 1 ] && [ -f "$DATABASE_PATH" ]; then
            if /usr/sbin/lsof -t -- "$DATABASE_PATH" >/dev/null 2>&1; then rollback_ready=0; else [ "$?" -eq 1 ] || rollback_ready=0; fi
        fi
        if [ "$database_touched" -eq 1 ] && [ "$rollback_ready" -eq 1 ]; then
            rm -f "$DATABASE_PATH" "$DATABASE_PATH-wal" "$DATABASE_PATH-shm" "$DATABASE_PATH-journal" || rollback_ready=0
            if [ "$database_was_absent" -eq 0 ]; then
                install -m 0600 "$TRANSACTION/decisions.db" "$DATABASE_PATH" || rollback_ready=0
                for suffix in wal shm journal; do
                    [ ! -f "$TRANSACTION/decisions.db-$suffix" ] || install -m 0600 "$TRANSACTION/decisions.db-$suffix" "$DATABASE_PATH-$suffix" || rollback_ready=0
                done
            fi
        fi
        if [ "$rollback_ready" -eq 1 ] && [ "$selectors_switched" -eq 1 ]; then
            [ -z "$old_current" ] && rm -f "$CURRENT_LINK" || atomic_symlink "$old_current" "$CURRENT_LINK" || rollback_ready=0
            [ -z "$old_previous" ] && rm -f "$PREVIOUS_LINK" || atomic_symlink "$old_previous" "$PREVIOUS_LINK" || rollback_ready=0
            [ -z "$old_provider" ] || atomic_symlink "$old_provider" "$PROVIDER_LINK" || rollback_ready=0
            [ -z "$old_legacy_provider" ] || atomic_symlink "$old_legacy_provider" "$LEGACY_PROVIDER_LINK" || rollback_ready=0
        fi
        if [ "$rollback_ready" -eq 1 ] && { [ "$public_suspended" -eq 1 ] || [ "$selectors_switched" -eq 1 ]; }; then
            [ -z "$old_cli" ] || atomic_symlink "$old_cli" "$CLI_PATH" || rollback_ready=0
            [ -z "$old_legacy_cli" ] || atomic_symlink "$old_legacy_cli" "$LEGACY_CLI_PATH" || rollback_ready=0
        fi
        if [ "$rollback_ready" -eq 1 ] && [ "$hooks_changed" -eq 1 ]; then
            if [ -n "$old_hooks" ]; then install -m 0600 "$old_hooks" "$HOOKS_PATH" || rollback_ready=0; else rm -f "$HOOKS_PATH" || rollback_ready=0; fi
        fi
        if [ "$rollback_ready" -eq 1 ] && [ "$observer_plist_changed" -eq 1 ]; then
            if [ -n "$old_observer_plist" ]; then install -m 0644 "$old_observer_plist" "$OBSERVER_PLIST" || rollback_ready=0; else rm -f "$OBSERVER_PLIST" || rollback_ready=0; fi
        fi
        if [ "$rollback_ready" -eq 1 ] && [ "$daily_plist_changed" -eq 1 ]; then
            if [ -n "$old_daily_plist" ]; then install -m 0644 "$old_daily_plist" "$DAILY_PLIST" || rollback_ready=0; else rm -f "$DAILY_PLIST" || rollback_ready=0; fi
        fi
        if [ "$rollback_ready" -eq 1 ] && [ "$observer_service_stopped" -eq 1 ] && [ "$observer_was_loaded" -eq 1 ]; then
            "$launchctl_path" bootstrap "$SERVICE_DOMAIN" "$OBSERVER_PLIST" >/dev/null 2>&1 || rollback_ready=0
        fi
        if [ "$rollback_ready" -eq 1 ] && [ "$daily_service_stopped" -eq 1 ] && [ "$daily_was_loaded" -eq 1 ]; then
            "$launchctl_path" bootstrap "$SERVICE_DOMAIN" "$DAILY_PLIST" >/dev/null 2>&1 || rollback_ready=0
        fi
        if [ "$rollback_ready" -eq 1 ] && [ "$maintenance_created" -eq 1 ]; then
            maintenance_marker_is_owned && rm -f "$MAINTENANCE_MARKER" || rollback_ready=0
        fi
        if [ "$rollback_ready" -eq 1 ] && [ "$maintenance_created" -eq 0 ]; then
            maintenance_marker_is_owned || rollback_ready=0
        fi
        if [ "$rollback_ready" -eq 1 ]; then
            restore_hold_receipt || rollback_ready=0
        fi
        if [ "$rollback_ready" -eq 0 ] && [ "$mutation_started" -eq 1 ]; then
            retain_transaction=1
            printf '%s\n' "krisis user deploy: rollback could not restore every owned artifact; maintenance recovery is retained at $TRANSACTION" >&2
        fi
    fi
    [ -z "$TEMPORARY" ] || rm -rf "$TEMPORARY"
    if [ -n "$TRANSACTION" ] && [ "$retain_transaction" -eq 0 ]; then rm -rf "$TRANSACTION"; fi
    rmdir "$LOCK_DIR" 2>/dev/null
    exit "$status"
}
trap cleanup EXIT HUP INT TERM

if [ -e "$MAINTENANCE_MARKER" ] || [ -L "$MAINTENANCE_MARKER" ]; then
    maintenance_preexisting=1
fi
if [ "$release_maintenance" -eq 0 ]; then
    engage_maintenance || fail 'maintenance gate is invalid or unavailable'
elif [ "$maintenance_preexisting" -eq 1 ]; then
    maintenance_marker_is_owned || fail 'maintenance gate is invalid or unavailable'
fi
TRANSACTION=$(mktemp -d "$INSTALL_DIR/.transaction.XXXXXX") || fail 'unable to create transaction directory'
TEMPORARY=$(mktemp -d "$INSTALL_DIR/.candidate.XXXXXX") || fail 'unable to create candidate directory'
if [ -L "$HOLD_RECEIPT" ] || { [ -e "$HOLD_RECEIPT" ] && [ ! -f "$HOLD_RECEIPT" ]; }; then fail 'installed maintenance hold receipt is unsafe'; fi
if [ -f "$HOLD_RECEIPT" ]; then
    hold_receipt_is_private || fail 'installed maintenance hold receipt is invalid'
    hold_receipt_existed=1
    old_hold_receipt="$TRANSACTION/prior-maintenance-hold.txt"
    install -m 0600 "$HOLD_RECEIPT" "$old_hold_receipt"
fi
if [ "$maintenance_preexisting" -eq 1 ]; then
    [ "$hold_receipt_existed" -eq 1 ] \
        || fail 'pre-existing maintenance gate has no Krisis hold receipt'
    hold_receipt_matches_gate \
        || fail 'pre-existing maintenance gate differs from its Krisis hold receipt'
    maintenance_owned=1
fi
if [ -L "$BINDING_RECEIPT" ] || { [ -e "$BINDING_RECEIPT" ] && [ ! -f "$BINDING_RECEIPT" ]; }; then fail 'installed observer binding receipt is unsafe'; fi
if [ -f "$BINDING_RECEIPT" ]; then
    [ "$(stat -f '%u' "$BINDING_RECEIPT")" -eq "$operator_uid" ] \
        && [ "$(stat -f '%Lp' "$BINDING_RECEIPT")" = 600 ] \
        && [ "$(stat -f '%l' "$BINDING_RECEIPT")" -eq 1 ] \
        && [ "$(awk 'END {print NR}' "$BINDING_RECEIPT")" -eq 6 ] \
        && [ "$(sed -n '1p' "$BINDING_RECEIPT")" = format=1 ] \
        || fail 'installed observer binding receipt is invalid'
    old_binding_receipt="$TRANSACTION/prior-observer-binding.txt"
    install -m 0600 "$BINDING_RECEIPT" "$old_binding_receipt"
fi

binary_hash=$(shasum -a 256 "$binary_path" | awk '{print $1}')
frontend_hash=$(shasum -a 256 "$SOURCE_FRONTEND" | awk '{print $1}')
runner_hash=$(shasum -a 256 "$SOURCE_RUNNER" | awk '{print $1}')
definition_hash=$(shasum -a 256 "$SOURCE_DEFINITION" | awk '{print $1}')
hooks_hash=$(shasum -a 256 "$SOURCE_HOOKS" | awk '{print $1}')
deployer_hash=$(shasum -a 256 "$0" | awk '{print $1}')
uninstaller_hash=$(shasum -a 256 "$SOURCE_UNINSTALLER" | awk '{print $1}')
provider_hash=$(bundle_hash "$SOURCE_CHANCERY")
legacy_provider_hash=$(bundle_hash "$SOURCE_LEGACY_CHANCERY")
release_id=$(printf '%s\n' "$binary_hash" "$frontend_hash" "$runner_hash" "$definition_hash" "$hooks_hash" "$deployer_hash" "$uninstaller_hash" "$provider_hash" "$legacy_provider_hash" | shasum -a 256 | awk '{print $1}')
release="$RELEASES_DIR/$release_id"

validate_release_tree() {
    release_tree=$1
    release_format=$2
    if find "$release_tree" -type l -print | grep -q .; then fail "release contains a symbolic link: $release_tree"; fi
    if find "$release_tree" ! -type d ! -type f -print | grep -q .; then fail "release contains a non-file entry: $release_tree"; fi
    if find "$release_tree" -type d -empty -print | grep -q .; then fail "release contains an uncommitted empty directory: $release_tree"; fi
    if ! find "$release_tree" -type d -o -type f | while IFS= read -r release_entry; do
        [ "$(stat -f '%u' "$release_entry")" -eq "$operator_uid" ] || exit 1
        release_mode=$(stat -f '%Lp' "$release_entry")
        release_other=${release_mode#"${release_mode%?}"}
        release_without_other=${release_mode%?}
        release_group=${release_without_other#"${release_without_other%?}"}
        case "$release_group$release_other" in
            00|01|04|05|10|11|14|15|40|41|44|45|50|51|54|55) ;;
            *) exit 1 ;;
        esac
        [ ! -f "$release_entry" ] || [ "$(stat -f '%l' "$release_entry")" -eq 1 ] || exit 1
    done; then
        fail "release is writable outside the operator or contains shared files: $release_tree"
    fi
    if ! find "$release_tree" -type f -print | while IFS= read -r release_file; do
        relative=${release_file#"$release_tree/"}
        case "$release_format:$relative" in
            4:manifest.txt|4:libexec/krisis|4:bin/krisis|4:bin/krisis-observer|4:package/krisis|4:package/krisis-observer|4:package/deploy-user.sh|4:package/uninstall-user.sh|4:package/krisis-observer.clockwork.toml.in|4:package/hooks.json|4:share/chancery/krisis/*|4:share/chancery/decisions/*) ;;
            2:manifest.txt|2:libexec/decisions|2:bin/decisions|2:bin/decisions-daily-email|2:bin/decisions-observer|2:package/decisions|2:package/decisions-daily-email|2:package/decisions-observer|2:package/deploy-user.sh|2:package/uninstall-user.sh|2:package/hooks.json|2:package/org.decisions.daily-email.plist|2:package/org.decisions.observer.plist|2:share/chancery/decisions/*) ;;
            3:manifest.txt|3:libexec/decisions|3:bin/decisions|3:bin/decisions-daily-email|3:bin/decisions-observer|3:package/decisions|3:package/decisions-daily-email|3:package/decisions-observer|3:package/deploy-user.sh|3:package/uninstall-user.sh|3:package/hooks.json|3:package/decisions-daily-email.clockwork.toml.in|3:package/decisions-observer.clockwork.toml.in|3:share/chancery/decisions/*) ;;
            *) exit 1 ;;
        esac
    done; then
        fail "release contains a path outside its canonical layout: $release_tree"
    fi
    case "$release_format" in
        4)
            validate_bundle "$release_tree/share/chancery/krisis"
            validate_bundle "$release_tree/share/chancery/decisions"
            ;;
        2|3) validate_bundle "$release_tree/share/chancery/decisions" ;;
    esac
}

validate_release_selector() {
    selector=$1
    printf '%s\n' "$selector" | grep -Eq '^releases/[0-9a-f]{64}$' || fail "invalid release selector: $selector"
    selected_release="$INSTALL_DIR/$selector"
    [ -d "$selected_release" ] && [ ! -L "$selected_release" ] || fail "selected release is unavailable: $selector"
    selected_manifest="$selected_release/manifest.txt"
    [ -f "$selected_manifest" ] && [ ! -L "$selected_manifest" ] || fail "selected release has no manifest: $selector"
    selected_format=$(sed -n '1s/^format=//p' "$selected_manifest")
    selected_release_id=$(sed -n '2s/^release_id=//p' "$selected_manifest")
    selected_version=$(sed -n '3s/^version=//p' "$selected_manifest")
    [ "$selected_release_id" = "${selector#releases/}" ] || fail "selected release manifest does not match: $selector"
    validate_release_tree "$selected_release" "$selected_format"
    case "$selected_format" in
        4)
            [ "$(awk 'END {print NR}' "$selected_manifest")" -eq 12 ] || fail "Krisis release manifest is not canonical: $selector"
            selected_binary_hash=$(sed -n '4s/^binary_sha256=//p' "$selected_manifest")
            selected_frontend_hash=$(sed -n '5s/^frontend_sha256=//p' "$selected_manifest")
            selected_observer_runner_hash=$(sed -n '6s/^observer_runner_sha256=//p' "$selected_manifest")
            selected_observer_definition_hash=$(sed -n '7s/^observer_clockwork_definition_sha256=//p' "$selected_manifest")
            selected_hooks_hash=$(sed -n '8s/^hooks_sha256=//p' "$selected_manifest")
            selected_deployer_hash=$(sed -n '9s/^deployer_sha256=//p' "$selected_manifest")
            selected_uninstaller_hash=$(sed -n '10s/^uninstaller_sha256=//p' "$selected_manifest")
            selected_provider_hash=$(sed -n '11s/^krisis_chancery_sha256=//p' "$selected_manifest")
            selected_legacy_provider_hash=$(sed -n '12s/^decisions_chancery_sha256=//p' "$selected_manifest")
            [ "$(shasum -a 256 "$selected_release/libexec/krisis" | awk '{print $1}')" = "$selected_binary_hash" ] || fail "selected Krisis binary is tampered: $selector"
            [ "$(shasum -a 256 "$selected_release/bin/krisis" | awk '{print $1}')" = "$selected_frontend_hash" ] || fail "selected Krisis frontend is tampered: $selector"
            [ "$(shasum -a 256 "$selected_release/bin/krisis-observer" | awk '{print $1}')" = "$selected_observer_runner_hash" ] || fail "selected Krisis runner is tampered: $selector"
            if [ -f "$selected_release/package/krisis" ] \
                && [ -f "$selected_release/package/krisis-observer" ]; then
                [ "$(shasum -a 256 "$selected_release/package/krisis" | awk '{print $1}')" = "$selected_frontend_hash" ] || fail "selected packaged Krisis frontend is tampered: $selector"
                [ "$(shasum -a 256 "$selected_release/package/krisis-observer" | awk '{print $1}')" = "$selected_observer_runner_hash" ] || fail "selected packaged Krisis runner is tampered: $selector"
            elif [ ! -e "$selected_release/package/krisis" ] \
                && [ ! -e "$selected_release/package/krisis-observer" ]; then
                case "$selected_version" in
                    0.4.0|0.4.1) ;;
                    *) fail "selected packaged Krisis executables are missing: $selector" ;;
                esac
            else
                fail "selected packaged Krisis executables are incomplete: $selector"
            fi
            [ "$(shasum -a 256 "$selected_release/package/krisis-observer.clockwork.toml.in" | awk '{print $1}')" = "$selected_observer_definition_hash" ] || fail "selected Krisis definition is tampered: $selector"
            [ "$(shasum -a 256 "$selected_release/package/hooks.json" | awk '{print $1}')" = "$selected_hooks_hash" ] || fail "selected Krisis hooks are tampered: $selector"
            [ "$(shasum -a 256 "$selected_release/package/deploy-user.sh" | awk '{print $1}')" = "$selected_deployer_hash" ] || fail "selected Krisis deployer is tampered: $selector"
            [ "$(shasum -a 256 "$selected_release/package/uninstall-user.sh" | awk '{print $1}')" = "$selected_uninstaller_hash" ] || fail "selected Krisis uninstaller is tampered: $selector"
            [ "$(bundle_hash "$selected_release/share/chancery/krisis")" = "$selected_provider_hash" ] || fail "selected Krisis provider is tampered: $selector"
            [ "$(bundle_hash "$selected_release/share/chancery/decisions")" = "$selected_legacy_provider_hash" ] || fail "selected Decisions compatibility provider is tampered: $selector"
            actual_id=$(printf '%s\n' "$selected_binary_hash" "$selected_frontend_hash" "$selected_observer_runner_hash" "$selected_observer_definition_hash" "$selected_hooks_hash" "$selected_deployer_hash" "$selected_uninstaller_hash" "$selected_provider_hash" "$selected_legacy_provider_hash" | shasum -a 256 | awk '{print $1}')
            ;;
        2|3)
            [ "$(awk 'END {print NR}' "$selected_manifest")" -eq 13 ] || fail "legacy Decisions release manifest is not canonical: $selector"
            selected_binary_hash=$(sed -n '4s/^binary_sha256=//p' "$selected_manifest")
            selected_frontend_hash=$(sed -n '5s/^frontend_sha256=//p' "$selected_manifest")
            selected_daily_runner_hash=$(sed -n '6s/^daily_runner_sha256=//p' "$selected_manifest")
            selected_observer_runner_hash=$(sed -n '7s/^observer_runner_sha256=//p' "$selected_manifest")
            if [ "$selected_format" -eq 2 ]; then
                selected_daily_schedule_hash=$(sed -n '8s/^daily_plist_sha256=//p' "$selected_manifest")
                selected_observer_schedule_hash=$(sed -n '9s/^observer_plist_sha256=//p' "$selected_manifest")
                selected_daily_schedule="$selected_release/package/$DAILY_LABEL.plist"
                selected_observer_schedule="$selected_release/package/$OBSERVER_LABEL.plist"
            else
                selected_daily_schedule_hash=$(sed -n '8s/^daily_clockwork_definition_sha256=//p' "$selected_manifest")
                selected_observer_schedule_hash=$(sed -n '9s/^observer_clockwork_definition_sha256=//p' "$selected_manifest")
                selected_daily_schedule="$selected_release/package/decisions-daily-email.clockwork.toml.in"
                selected_observer_schedule="$selected_release/package/decisions-observer.clockwork.toml.in"
            fi
            selected_hooks_hash=$(sed -n '10s/^hooks_sha256=//p' "$selected_manifest")
            selected_deployer_hash=$(sed -n '11s/^deployer_sha256=//p' "$selected_manifest")
            selected_uninstaller_hash=$(sed -n '12s/^uninstaller_sha256=//p' "$selected_manifest")
            selected_provider_hash=$(sed -n '13s/^chancery_sha256=//p' "$selected_manifest")
            [ "$(shasum -a 256 "$selected_release/libexec/decisions" | awk '{print $1}')" = "$selected_binary_hash" ] || fail "selected Decisions binary is tampered: $selector"
            [ "$(shasum -a 256 "$selected_release/bin/decisions" | awk '{print $1}')" = "$selected_frontend_hash" ] || fail "selected Decisions frontend is tampered: $selector"
            [ "$(shasum -a 256 "$selected_release/bin/decisions-daily-email" | awk '{print $1}')" = "$selected_daily_runner_hash" ] || fail "selected Decisions daily runner is tampered: $selector"
            [ "$(shasum -a 256 "$selected_release/bin/decisions-observer" | awk '{print $1}')" = "$selected_observer_runner_hash" ] || fail "selected Decisions observer runner is tampered: $selector"
            [ "$(shasum -a 256 "$selected_daily_schedule" | awk '{print $1}')" = "$selected_daily_schedule_hash" ] || fail "selected Decisions daily schedule is tampered: $selector"
            [ "$(shasum -a 256 "$selected_observer_schedule" | awk '{print $1}')" = "$selected_observer_schedule_hash" ] || fail "selected Decisions observer schedule is tampered: $selector"
            [ "$(shasum -a 256 "$selected_release/package/hooks.json" | awk '{print $1}')" = "$selected_hooks_hash" ] || fail "selected Decisions hooks are tampered: $selector"
            [ "$(shasum -a 256 "$selected_release/package/deploy-user.sh" | awk '{print $1}')" = "$selected_deployer_hash" ] || fail "selected Decisions deployer is tampered: $selector"
            [ "$(shasum -a 256 "$selected_release/package/uninstall-user.sh" | awk '{print $1}')" = "$selected_uninstaller_hash" ] || fail "selected Decisions uninstaller is tampered: $selector"
            [ "$(bundle_hash "$selected_release/share/chancery/decisions")" = "$selected_provider_hash" ] || fail "selected Decisions provider is tampered: $selector"
            actual_id=$(printf '%s\n' "$selected_binary_hash" "$selected_frontend_hash" "$selected_daily_runner_hash" "$selected_observer_runner_hash" "$selected_daily_schedule_hash" "$selected_observer_schedule_hash" "$selected_hooks_hash" "$selected_deployer_hash" "$selected_uninstaller_hash" "$selected_provider_hash" | shasum -a 256 | awk '{print $1}')
            ;;
        *) fail "selected release format is unsupported: $selector" ;;
    esac
    [ "$actual_id" = "$selected_release_id" ] || fail "selected release content identity is invalid: $selector"
}

if [ -L "$CURRENT_LINK" ]; then old_current=$(readlink "$CURRENT_LINK"); elif [ -e "$CURRENT_LINK" ]; then fail 'current release selector is not a symbolic link'; fi
if [ -L "$PREVIOUS_LINK" ]; then old_previous=$(readlink "$PREVIOUS_LINK"); elif [ -e "$PREVIOUS_LINK" ]; then fail 'previous release selector is not a symbolic link'; fi
if [ -n "$old_current" ]; then
    validate_release_selector "$old_current"
    old_release="$selected_release"
    old_release_id="$selected_release_id"
    old_release_format="$selected_format"
    old_observer_runner_hash="$selected_observer_runner_hash"
    if [ "$old_release_format" -eq 2 ] || [ "$old_release_format" -eq 3 ]; then old_daily_runner_hash="$selected_daily_runner_hash"; fi
else
    old_release=
    old_release_id=
    old_release_format=
    old_observer_runner_hash=
    old_daily_runner_hash=
fi
if [ -n "$old_previous" ]; then validate_release_selector "$old_previous"; fi

for selector in "$CLI_PATH" "$LEGACY_CLI_PATH" "$PROVIDER_LINK" "$LEGACY_PROVIDER_LINK"; do
    if [ -e "$selector" ] || [ -L "$selector" ]; then [ -L "$selector" ] || fail "public selector is not a symbolic link: $selector"; fi
done
if [ -L "$CLI_PATH" ]; then old_cli=$(readlink "$CLI_PATH"); fi
if [ -L "$LEGACY_CLI_PATH" ]; then old_legacy_cli=$(readlink "$LEGACY_CLI_PATH"); fi
if [ -L "$PROVIDER_LINK" ]; then old_provider=$(readlink "$PROVIDER_LINK"); fi
if [ -L "$LEGACY_PROVIDER_LINK" ]; then old_legacy_provider=$(readlink "$LEGACY_PROVIDER_LINK"); fi
if [ -z "$old_current" ]; then
    [ -z "$old_cli$old_legacy_cli$old_provider$old_legacy_provider$old_previous" ] || fail 'installed selectors have no current owned release'
elif [ "$old_release_format" -eq 4 ]; then
    [ -z "$old_cli" ] || [ "$old_cli" = "$CURRENT_LINK/bin/krisis" ] || fail 'Krisis command selector is foreign'
    [ -z "$old_legacy_cli" ] || fail 'legacy Decisions command selector is unexpected for Krisis'
    [ -z "$old_provider" ] || [ "$old_provider" = "$CURRENT_LINK/share/chancery/krisis" ] || fail 'Krisis provider selector is foreign'
    [ -z "$old_legacy_provider" ] || [ "$old_legacy_provider" = "$CURRENT_LINK/share/chancery/decisions" ] || fail 'Decisions compatibility provider selector is foreign'
else
    [ -z "$old_cli" ] || fail 'Krisis command cannot be owned by a legacy Decisions release'
    [ -z "$old_legacy_cli" ] || [ "$old_legacy_cli" = "$CURRENT_LINK/bin/decisions" ] || fail 'Decisions command selector is foreign'
    [ -z "$old_provider" ] || fail 'Krisis provider cannot be owned by a legacy Decisions release'
    [ -z "$old_legacy_provider" ] || [ "$old_legacy_provider" = "$CURRENT_LINK/share/chancery/decisions" ] || fail 'Decisions provider selector is foreign'
fi

if [ -e "$release" ]; then
    validate_release_selector "releases/$release_id"
else
    install -d -m 0755 "$TEMPORARY/bin" "$TEMPORARY/libexec" "$TEMPORARY/package" "$TEMPORARY/share/chancery"
    install -m 0755 "$binary_path" "$TEMPORARY/libexec/krisis"
    install -m 0755 "$SOURCE_FRONTEND" "$TEMPORARY/bin/krisis"
    install -m 0755 "$SOURCE_RUNNER" "$TEMPORARY/bin/krisis-observer"
    install -m 0755 "$SOURCE_FRONTEND" "$TEMPORARY/package/krisis"
    install -m 0755 "$SOURCE_RUNNER" "$TEMPORARY/package/krisis-observer"
    install -m 0755 "$0" "$TEMPORARY/package/deploy-user.sh"
    install -m 0755 "$SOURCE_UNINSTALLER" "$TEMPORARY/package/uninstall-user.sh"
    install -m 0644 "$SOURCE_DEFINITION" "$TEMPORARY/package/krisis-observer.clockwork.toml.in"
    install -m 0600 "$SOURCE_HOOKS" "$TEMPORARY/package/hooks.json"
    cp -R "$SOURCE_CHANCERY" "$TEMPORARY/share/chancery/krisis"
    cp -R "$SOURCE_LEGACY_CHANCERY" "$TEMPORARY/share/chancery/decisions"
    {
        printf '%s\n' 'format=4'
        printf 'release_id=%s\nversion=%s\n' "$release_id" "$version"
        printf 'binary_sha256=%s\nfrontend_sha256=%s\n' "$binary_hash" "$frontend_hash"
        printf 'observer_runner_sha256=%s\nobserver_clockwork_definition_sha256=%s\n' "$runner_hash" "$definition_hash"
        printf 'hooks_sha256=%s\ndeployer_sha256=%s\nuninstaller_sha256=%s\n' "$hooks_hash" "$deployer_hash" "$uninstaller_hash"
        printf 'krisis_chancery_sha256=%s\ndecisions_chancery_sha256=%s\n' "$provider_hash" "$legacy_provider_hash"
    } >"$TEMPORARY/manifest.txt"
    chmod 0444 "$TEMPORARY/manifest.txt"
    chmod -R go-w "$TEMPORARY"
    mv "$TEMPORARY" "$release"
    TEMPORARY=$(mktemp -d "$INSTALL_DIR/.candidate.XXXXXX")
    validate_release_selector "releases/$release_id"
fi

interpreter_hash=$(shasum -a 256 /bin/sh | awk '{print $1}')
rendered_definition="$TRANSACTION/krisis-observer.toml"
sed -e "s|__RELEASE_ID__|$release_id|g" \
    -e "s|__RELEASE_ROOT__|$release|g" \
    -e "s|__KRISIS_STATE__|$STATE_DIR|g" \
    -e "s|__KRISIS_HOME__|$install_home|g" \
    -e "s|__KRISIS_LOGS__|$LOG_DIR|g" \
    -e "s|__KRISIS_ANNALS_BINARY__|$annals_path|g" \
    -e "s|__KRISIS_ANNALS_CONFIG__|$annals_config|g" \
    -e "s|__KRISIS_ANNALS_LIBRARY_ID__|$annals_library_id|g" \
    -e "s|__INTERPRETER_SHA256__|$interpreter_hash|g" \
    -e "s|__RUNNER_SHA256__|$runner_hash|g" \
    "$release/package/krisis-observer.clockwork.toml.in" >"$rendered_definition"
chmod 0600 "$rendered_definition"
prepare_private_log() {
    log_path=$1
    if [ -L "$log_path" ] || { [ -e "$log_path" ] && [ ! -f "$log_path" ]; }; then fail "log path is unsafe: $log_path"; fi
    [ -e "$log_path" ] || return 0
    [ "$(stat -f '%u' "$log_path")" -eq "$operator_uid" ] \
        && [ "$(stat -f '%l' "$log_path")" -eq 1 ] \
        || fail "log is not exclusively operator-owned: $log_path"
    chmod 0600 "$log_path" || fail "unable to make log private: $log_path"
}
prepare_private_log "$LOG_DIR/observer.stdout.log"
prepare_private_log "$LOG_DIR/observer.stderr.log"

definition_output=$(HOME="$install_home" "$clockwork_path" --json definition register "$rendered_definition") || fail 'Clockwork rejected the observer definition'
candidate_definition_digest=$(printf '%s' "$definition_output" | tr -d '[:space:]' | sed -n 's/.*"digest":"\([0-9a-f]\{64\}\)".*/\1/p')
[ -n "$candidate_definition_digest" ] || fail 'Clockwork returned no definition digest'

prove_definition() {
    proof_key=$1
    proof_digest=$2
    proof_release=$3
    proof_release_id=$4
    proof_runner=$5
    proof_runner_hash=$6
    proof_kind=$7
    proof_file="$TRANSACTION/$proof_kind-definition.json"
    HOME="$install_home" "$clockwork_path" --json definition show "$proof_digest" >"$proof_file" 2>"$proof_file.stderr" || fail "unable to inspect $proof_kind Clockwork definition"
    [ "$(plutil -extract ok raw "$proof_file" 2>/dev/null)" = true ] \
        && [ "$(plutil -extract data.digest raw "$proof_file" 2>/dev/null)" = "$proof_digest" ] \
        && [ "$(plutil -extract data.key raw "$proof_file" 2>/dev/null)" = "$proof_key" ] \
        && [ "$(plutil -extract data.manifest.key raw "$proof_file" 2>/dev/null)" = "$proof_key" ] \
        && [ "$(plutil -extract data.manifest.schema_version raw "$proof_file" 2>/dev/null)" = 1 ] \
        && [ "$(plutil -extract data.manifest.release_id raw "$proof_file" 2>/dev/null)" = "$proof_release_id" ] \
        && [ "$(plutil -extract data.manifest.release_root raw "$proof_file" 2>/dev/null)" = "$proof_release" ] \
        && [ "$(plutil -extract data.manifest.authority raw "$proof_file" 2>/dev/null)" = current-user-background ] \
        && [ "$(plutil -extract data.manifest.overlap raw "$proof_file" 2>/dev/null)" = skip ] \
        && [ "$(plutil -extract data.manifest.cwd raw "$proof_file" 2>/dev/null)" = "$STATE_DIR" ] \
        && [ "$(plutil -extract data.manifest.launch.kind raw "$proof_file" 2>/dev/null)" = interpreted ] \
        && [ "$(plutil -extract data.manifest.launch.interpreter raw "$proof_file" 2>/dev/null)" = /bin/sh ] \
        && [ "$(plutil -extract data.manifest.launch.interpreter_sha256 raw "$proof_file" 2>/dev/null)" = "$interpreter_hash" ] \
        && [ "$(plutil -extract data.manifest.launch.script raw "$proof_file" 2>/dev/null)" = "$proof_runner" ] \
        && [ "$(plutil -extract data.manifest.launch.script_sha256 raw "$proof_file" 2>/dev/null)" = "$proof_runner_hash" ] \
        || fail "$proof_kind Clockwork definition is not release-owned"
    direct_key_count() {
        plutil -extract "$1" xml1 -o - "$proof_file" 2>/dev/null | awk '
            /<dict>/ { depth++; next }
            /<\/dict>/ { depth--; next }
            depth == 1 && /<key>/ { count++ }
            END { print count+0 }
        '
    }
    [ "$(direct_key_count data.manifest)" -eq 12 ] \
        && [ "$(direct_key_count data.manifest.launch)" -eq 5 ] \
        && [ "$(direct_key_count data.manifest.output)" -eq 2 ] \
        || fail "$proof_kind Clockwork definition contains foreign manifest fields"
    if plutil -extract data.manifest.arguments.0 raw "$proof_file" >/dev/null 2>&1 \
        || plutil -extract data.manifest.timeout_seconds raw "$proof_file" >/dev/null 2>&1; then
        fail "$proof_kind Clockwork definition adds arguments or a timeout"
    fi
    case "$proof_kind" in
        active|legacy-observer)
            [ "$(plutil -extract data.manifest.schedule.kind raw "$proof_file" 2>/dev/null)" = interval ] \
                && [ "$(plutil -extract data.manifest.schedule.seconds raw "$proof_file" 2>/dev/null)" = 60 ] \
                && [ "$(plutil -extract data.manifest.output.stdout raw "$proof_file" 2>/dev/null)" = "$LOG_DIR/observer.stdout.log" ] \
                && [ "$(plutil -extract data.manifest.output.stderr raw "$proof_file" 2>/dev/null)" = "$LOG_DIR/observer.stderr.log" ] \
                || fail "$proof_kind Clockwork schedule is not release-owned"
            [ "$(direct_key_count data.manifest.schedule)" -eq 3 ] \
                || fail "$proof_kind Clockwork schedule contains foreign fields"
            ;;
        legacy-daily)
            [ "$(plutil -extract data.manifest.schedule.kind raw "$proof_file" 2>/dev/null)" = local-calendar ] \
                && [ "$(plutil -extract data.manifest.schedule.hour raw "$proof_file" 2>/dev/null)" = 9 ] \
                && [ "$(plutil -extract data.manifest.schedule.minute raw "$proof_file" 2>/dev/null)" = 0 ] \
                && [ "$(plutil -extract data.manifest.output.stdout raw "$proof_file" 2>/dev/null)" = "$LOG_DIR/daily-email.stdout.log" ] \
                && [ "$(plutil -extract data.manifest.output.stderr raw "$proof_file" 2>/dev/null)" = "$LOG_DIR/daily-email.stderr.log" ] \
                || fail 'legacy daily Clockwork schedule is not release-owned'
            [ "$(direct_key_count data.manifest.schedule)" -eq 4 ] \
                || fail 'legacy daily Clockwork schedule contains foreign fields'
            ;;
    esac
    [ "$(plutil -extract data.manifest.schedule.run_at_load raw "$proof_file" 2>/dev/null)" = false ] || fail "$proof_kind run-at-load changed"
    if [ "$proof_kind" = active ]; then
        [ "$(plutil -extract data.manifest.environment.HOME raw "$proof_file" 2>/dev/null)" = "$install_home" ] \
            && [ "$(plutil -extract data.manifest.environment.KRISIS_ANNALS_BINARY raw "$proof_file" 2>/dev/null)" = "$annals_path" ] \
            && [ "$(plutil -extract data.manifest.environment.KRISIS_ANNALS_CONFIG raw "$proof_file" 2>/dev/null)" = "$annals_config" ] \
            && [ "$(plutil -extract data.manifest.environment.KRISIS_ANNALS_LIBRARY_ID raw "$proof_file" 2>/dev/null)" = "$annals_library_id" ] \
            || fail 'active Clockwork Annals target differs from this cutover'
        environment_count=$(plutil -extract data.manifest.environment xml1 -o - "$proof_file" 2>/dev/null | awk '/<key>/{count++} END {print count+0}')
        [ "$environment_count" -eq 4 ] || fail 'active Clockwork definition contains foreign environment entries'
    else
        [ "$(plutil -extract data.manifest.environment.HOME raw "$proof_file" 2>/dev/null)" = "$install_home" ] || fail "$proof_kind HOME differs"
        environment_count=$(plutil -extract data.manifest.environment xml1 -o - "$proof_file" 2>/dev/null | awk '/<key>/{count++} END {print count+0}')
        [ "$environment_count" -eq 1 ] || fail "$proof_kind Clockwork definition contains foreign environment entries"
    fi
}

prove_definition "$ACTIVE_CLOCKWORK_KEY" "$candidate_definition_digest" "$release" "$release_id" "$release/bin/krisis-observer" "$runner_hash" active

if [ "$maintenance_preexisting" -eq 1 ]; then
    hold_receipt_matches_candidate \
        || fail 'Krisis maintenance hold belongs to a different release, definition, or Annals target'
elif [ "$release_maintenance" -eq 0 ]; then
    write_hold_receipt || fail 'unable to authenticate the Krisis maintenance hold'
    maintenance_owned=1
else
    [ "$hold_receipt_existed" -eq 1 ] \
        && hold_receipt_matches_candidate \
        || fail 'no authenticated Krisis maintenance hold is available to release'
fi

if [ "$keep_maintenance" -eq 1 ]; then
    [ "$maintenance_preexisting" -eq 1 ] && [ "$maintenance_owned" -eq 1 ] \
        || fail '--final-cutover --keep-maintenance requires a successful separate prepare'
fi

if [ "$final_cutover" -eq 0 ]; then
    [ "$release_maintenance" -eq 1 ] || {
        committed=1
        printf 'prepared krisis %s (%s); authenticated maintenance hold remains engaged at %s; definition %s\n' "$version" "$release_id" "$HOLD_RECEIPT" "$candidate_definition_digest"
        exit 0
    }
fi

snapshot_binding() {
    snapshot_key=$1
    snapshot_name=$2
    snapshot_file="$TRANSACTION/$snapshot_name-binding.json"
    snapshot_error="$snapshot_file.stderr"
    if HOME="$install_home" "$clockwork_path" --json binding show "$snapshot_key" >"$snapshot_file" 2>"$snapshot_error"; then
        [ "$(plutil -extract ok raw "$snapshot_file" 2>/dev/null)" = true ] || fail "$snapshot_name binding response is invalid"
        [ "$(plutil -extract data.key raw "$snapshot_file" 2>/dev/null)" = "$snapshot_key" ] || fail "$snapshot_name binding key changed"
        snapshot_enabled=$(plutil -extract data.enabled raw "$snapshot_file" 2>/dev/null) || fail "$snapshot_name binding has no enabled state"
        case "$snapshot_enabled" in true) snapshot_enabled=1 ;; false) snapshot_enabled=0 ;; *) fail "$snapshot_name binding enabled state is invalid" ;; esac
        snapshot_digest=$(plutil -extract data.definition_digest raw "$snapshot_file" 2>/dev/null || true)
        if [ -n "$snapshot_digest" ]; then printf '%s\n' "$snapshot_digest" | grep -Eq '^[0-9a-f]{64}$' || fail "$snapshot_name binding digest is invalid"; fi
        [ "$snapshot_enabled" -eq 0 ] || [ -n "$snapshot_digest" ] || fail "$snapshot_name enabled binding has no definition"
        eval "prior_${snapshot_name}_exists=1"
        eval "prior_${snapshot_name}_enabled=$snapshot_enabled"
        eval "prior_${snapshot_name}_digest=\$snapshot_digest"
    else
        grep -F '"code":"binding_not_found"' "$snapshot_error" >/dev/null || fail "unable to inspect $snapshot_name Clockwork binding"
    fi
}

snapshot_binding "$ACTIVE_CLOCKWORK_KEY" active
snapshot_binding "$LEGACY_OBSERVER_CLOCKWORK_KEY" legacy_observer
snapshot_binding "$LEGACY_DAILY_CLOCKWORK_KEY" legacy_daily
[ "$prior_active_enabled" -eq 0 ] || [ "$prior_legacy_observer_enabled" -eq 0 ] \
    || fail 'Krisis and legacy Decisions observers are both enabled'

assert_binding_state() {
    assert_key=$1
    assert_name=$2
    assert_exists=$3
    assert_enabled=$4
    assert_digest=$5
    assert_file="$TRANSACTION/assert-$assert_name-binding.json"
    if HOME="$install_home" "$clockwork_path" --json binding show "$assert_key" >"$assert_file" 2>"$assert_file.stderr"; then
        [ "$assert_exists" -eq 1 ] || fail "$assert_name Clockwork binding appeared during cutover"
        actual_enabled=$(plutil -extract data.enabled raw "$assert_file" 2>/dev/null) || fail "$assert_name Clockwork binding became invalid"
        case "$actual_enabled" in true) actual_enabled=1 ;; false) actual_enabled=0 ;; *) fail "$assert_name Clockwork binding became invalid" ;; esac
        actual_digest=$(plutil -extract data.definition_digest raw "$assert_file" 2>/dev/null || true)
        [ "$(plutil -extract data.key raw "$assert_file" 2>/dev/null)" = "$assert_key" ] \
            && [ "$actual_enabled" -eq "$assert_enabled" ] \
            && [ "$actual_digest" = "$assert_digest" ] \
            || fail "$assert_name Clockwork binding changed during cutover"
    else
        [ "$assert_exists" -eq 0 ] && grep -F '"code":"binding_not_found"' "$assert_file.stderr" >/dev/null \
            || fail "$assert_name Clockwork binding disappeared during cutover"
    fi
}

if [ -n "$prior_active_digest" ]; then
    if [ "$prior_active_digest" = "$candidate_definition_digest" ]; then
        prove_definition "$ACTIVE_CLOCKWORK_KEY" "$prior_active_digest" "$release" "$release_id" "$release/bin/krisis-observer" "$runner_hash" active
    elif [ "$old_release_format" = 4 ]; then
        prove_definition "$ACTIVE_CLOCKWORK_KEY" "$prior_active_digest" "$old_release" "$old_release_id" "$old_release/bin/krisis-observer" "$old_observer_runner_hash" active
    else
        fail 'selected Krisis binding is foreign to the candidate and current release'
    fi
    [ -n "$old_binding_receipt" ] || fail 'selected Krisis binding has no installed ownership receipt'
    [ "$(sed -n '2s/^release_id=//p' "$old_binding_receipt")" = "$old_release_id" ] \
        && [ "$(sed -n '3s/^definition_digest=//p' "$old_binding_receipt")" = "$prior_active_digest" ] \
        && [ "$(sed -n '4s/^annals_binary=//p' "$old_binding_receipt")" = "$annals_path" ] \
        && [ "$(sed -n '5s/^annals_config=//p' "$old_binding_receipt")" = "$annals_config" ] \
        && [ "$(sed -n '6s/^annals_library_id=//p' "$old_binding_receipt")" = "$annals_library_id" ] \
        || fail 'selected Krisis binding differs from its installed ownership receipt'
fi
if [ "$prior_legacy_observer_enabled" -eq 1 ]; then
    [ "$old_release_format" = 3 ] || fail 'enabled legacy observer binding is not owned by the current Decisions release'
    prove_definition "$LEGACY_OBSERVER_CLOCKWORK_KEY" "$prior_legacy_observer_digest" "$old_release" "$old_release_id" "$old_release/bin/decisions-observer" "$old_observer_runner_hash" legacy-observer
fi
if [ "$prior_legacy_daily_enabled" -eq 1 ]; then
    [ "$old_release_format" = 3 ] || fail 'enabled legacy daily binding is not owned by the current Decisions release'
    prove_definition "$LEGACY_DAILY_CLOCKWORK_KEY" "$prior_legacy_daily_digest" "$old_release" "$old_release_id" "$old_release/bin/decisions-daily-email" "$old_daily_runner_hash" legacy-daily
fi

render_legacy_plist() {
    plist_kind=$1
    template=$2
    output=$3
    case "$plist_kind" in
        observer) sed -e "s|__DECISIONS_OBSERVER_RUNNER__|$old_release/bin/decisions-observer|g" -e "s|__DECISIONS_STATE_DIR__|$STATE_DIR|g" -e "s|__DECISIONS_HOME__|$install_home|g" -e "s|__DECISIONS_OBSERVER_STDOUT__|$LOG_DIR/observer.stdout.log|g" -e "s|__DECISIONS_OBSERVER_STDERR__|$LOG_DIR/observer.stderr.log|g" "$template" >"$output" ;;
        daily) sed -e "s|__DECISIONS_RUNNER__|$old_release/bin/decisions-daily-email|g" -e "s|__DECISIONS_STATE_DIR__|$STATE_DIR|g" -e "s|__DECISIONS_HOME__|$install_home|g" -e "s|__DECISIONS_STDOUT__|$LOG_DIR/daily-email.stdout.log|g" -e "s|__DECISIONS_STDERR__|$LOG_DIR/daily-email.stderr.log|g" "$template" >"$output" ;;
    esac
}

inspect_legacy_plist() {
    plist_kind=$1
    plist_path=$2
    if [ -L "$plist_path" ] || { [ -e "$plist_path" ] && [ ! -f "$plist_path" ]; }; then fail "legacy $plist_kind LaunchAgent path is unsafe"; fi
    [ -f "$plist_path" ] || return 0
    [ "$old_release_format" = 2 ] || fail "legacy $plist_kind LaunchAgent is not owned by the current Decisions release"
    expected="$TRANSACTION/expected-$plist_kind.plist"
    render_legacy_plist "$plist_kind" "$old_release/package/org.decisions.$plist_kind.plist" "$expected"
    cmp -s "$plist_path" "$expected" || fail "legacy $plist_kind LaunchAgent is foreign or modified"
    [ "$(stat -f '%u' "$plist_path")" -eq "$operator_uid" ] && [ "$(stat -f '%Lp' "$plist_path")" = 644 ] || fail "legacy $plist_kind LaunchAgent ownership is invalid"
    backup="$TRANSACTION/prior-$plist_kind.plist"
    install -m 0644 "$plist_path" "$backup"
    if [ "$plist_kind" = observer ]; then old_observer_plist=$backup; expected_old_observer_plist=$expected; else old_daily_plist=$backup; expected_old_daily_plist=$expected; fi
}

inspect_legacy_plist observer "$OBSERVER_PLIST"
inspect_legacy_plist daily "$DAILY_PLIST"
if "$launchctl_path" print "$OBSERVER_TARGET" >/dev/null 2>&1; then observer_was_loaded=1; [ -n "$old_observer_plist" ] || fail 'loaded observer service has no owned recoverable plist'; fi
if "$launchctl_path" print "$DAILY_TARGET" >/dev/null 2>&1; then daily_was_loaded=1; [ -n "$old_daily_plist" ] || fail 'loaded daily service has no owned recoverable plist'; fi
[ "$prior_legacy_observer_enabled" -eq 0 ] || [ "$observer_was_loaded" -eq 0 ] || fail 'legacy observer is active in both Clockwork and launchd'
[ "$prior_legacy_daily_enabled" -eq 0 ] || [ "$daily_was_loaded" -eq 0 ] || fail 'legacy daily schedule is active in both Clockwork and launchd'

if [ -L "$HOOKS_PATH" ] || { [ -e "$HOOKS_PATH" ] && [ ! -f "$HOOKS_PATH" ]; }; then fail 'Codex hooks path is unsafe'; fi
if [ -f "$HOOKS_PATH" ]; then
    [ -n "$old_release" ] && cmp -s "$HOOKS_PATH" "$old_release/package/hooks.json" || fail 'refusing to replace a foreign or modified Codex hooks file'
    old_hooks="$TRANSACTION/prior-hooks.json"
    install -m 0600 "$HOOKS_PATH" "$old_hooks"
fi

release_held_maintenance() {
    hold_receipt_matches_candidate \
        || fail 'maintenance hold receipt changed before release'
    if [ ! -e "$MAINTENANCE_MARKER" ] && [ ! -L "$MAINTENANCE_MARKER" ]; then
        return 0
    fi
    hold_receipt_matches_gate \
        || fail 'maintenance gate is not the exact authenticated Krisis hold'
    rm -f "$MAINTENANCE_MARKER" \
        || fail 'committed Krisis but could not release its authenticated maintenance hold'
    [ ! -e "$MAINTENANCE_MARKER" ] && [ ! -L "$MAINTENANCE_MARKER" ] \
        || fail 'committed Krisis but a maintenance gate remains'
}

if [ "$release_maintenance" -eq 1 ]; then
    [ "$old_release_format" = 4 ] && [ "$old_release_id" = "$release_id" ] \
        && [ "$old_current" = "releases/$release_id" ] \
        || fail 'current release is not the held Krisis release'
    [ "$old_cli" = "$CURRENT_LINK/bin/krisis" ] \
        && [ -z "$old_legacy_cli" ] \
        && [ "$old_provider" = "$CURRENT_LINK/share/chancery/krisis" ] \
        && [ "$old_legacy_provider" = "$CURRENT_LINK/share/chancery/decisions" ] \
        || fail 'current public selectors do not match the held Krisis release'
    [ -n "$old_hooks" ] && cmp -s "$HOOKS_PATH" "$release/package/hooks.json" \
        || fail 'current hook does not match the held Krisis release'
    [ "$prior_active_exists" -eq 1 ] && [ "$prior_active_enabled" -eq 1 ] \
        && [ "$prior_active_digest" = "$candidate_definition_digest" ] \
        || fail 'current Krisis binding does not match the held definition'
    [ "$prior_legacy_observer_enabled" -eq 0 ] \
        && [ "$prior_legacy_daily_enabled" -eq 0 ] \
        && [ "$observer_was_loaded" -eq 0 ] && [ "$daily_was_loaded" -eq 0 ] \
        || fail 'legacy Decisions execution is not fully disabled'
    [ -n "$old_binding_receipt" ] \
        || fail 'current Krisis binding has no installed ownership receipt'
    release_held_maintenance
    committed=1
    printf 'released authenticated Krisis maintenance hold for %s (%s)\n' "$version" "$release_id"
    exit 0
fi

if [ -x /opt/homebrew/bin/codex ]; then codex_path=/opt/homebrew/bin/codex; elif [ -x "$install_home/.local/bin/codex" ]; then codex_path="$install_home/.local/bin/codex"; else fail 'Codex executable is unavailable'; fi

validate_existing_database_paths
mutation_started=1
if [ "$prior_active_enabled" -eq 1 ]; then assert_binding_state "$ACTIVE_CLOCKWORK_KEY" active "$prior_active_exists" "$prior_active_enabled" "$prior_active_digest"; touched_active=1; HOME="$install_home" "$clockwork_path" --json binding disable "$ACTIVE_CLOCKWORK_KEY" >/dev/null || fail 'unable to disable the owned Krisis binding'; fi
if [ "$prior_legacy_observer_enabled" -eq 1 ]; then assert_binding_state "$LEGACY_OBSERVER_CLOCKWORK_KEY" legacy-observer "$prior_legacy_observer_exists" "$prior_legacy_observer_enabled" "$prior_legacy_observer_digest"; touched_legacy_observer=1; HOME="$install_home" "$clockwork_path" --json binding disable "$LEGACY_OBSERVER_CLOCKWORK_KEY" >/dev/null || fail 'unable to disable the owned legacy observer binding'; fi
if [ "$prior_legacy_daily_enabled" -eq 1 ]; then assert_binding_state "$LEGACY_DAILY_CLOCKWORK_KEY" legacy-daily "$prior_legacy_daily_exists" "$prior_legacy_daily_enabled" "$prior_legacy_daily_digest"; touched_legacy_daily=1; HOME="$install_home" "$clockwork_path" --json binding disable "$LEGACY_DAILY_CLOCKWORK_KEY" >/dev/null || fail 'unable to disable the owned legacy daily binding'; fi
if [ "$observer_was_loaded" -eq 1 ]; then cmp -s "$OBSERVER_PLIST" "$expected_old_observer_plist" || fail 'legacy observer plist changed before stop'; observer_service_stopped=1; "$launchctl_path" bootout "$OBSERVER_TARGET" >/dev/null || fail 'unable to stop the owned legacy observer'; fi
if [ "$daily_was_loaded" -eq 1 ]; then cmp -s "$DAILY_PLIST" "$expected_old_daily_plist" || fail 'legacy daily plist changed before stop'; daily_service_stopped=1; "$launchctl_path" bootout "$DAILY_TARGET" >/dev/null || fail 'unable to stop the owned legacy daily schedule'; fi
if [ -n "$old_observer_plist" ]; then cmp -s "$OBSERVER_PLIST" "$expected_old_observer_plist" || fail 'legacy observer plist changed before removal'; observer_plist_changed=1; rm -f "$OBSERVER_PLIST" || fail 'unable to remove the owned legacy observer plist'; fi
if [ -n "$old_daily_plist" ]; then cmp -s "$DAILY_PLIST" "$expected_old_daily_plist" || fail 'legacy daily plist changed before removal'; daily_plist_changed=1; rm -f "$DAILY_PLIST" || fail 'unable to remove the owned legacy daily plist'; fi

rm -f "$CLI_PATH" "$LEGACY_CLI_PATH"
public_suspended=1
hooks_changed=1
rm -f "$HOOKS_PATH"
/bin/sleep 3
if [ ! -e "$DATABASE_PATH" ]; then database_was_absent=1; fi
validate_existing_database_paths
if [ "$database_was_absent" -eq 0 ]; then
    if /usr/sbin/lsof -t -- "$DATABASE_PATH" >/dev/null 2>&1; then fail 'database remains open after writer shutdown'; else [ "$?" -eq 1 ] || fail 'unable to prove database quiescence'; fi
    install -m 0600 "$DATABASE_PATH" "$TRANSACTION/decisions.db"
    for suffix in wal shm journal; do [ ! -f "$DATABASE_PATH-$suffix" ] || install -m 0600 "$DATABASE_PATH-$suffix" "$TRANSACTION/decisions.db-$suffix"; done
fi

selectors_switched=1
if [ -n "$old_current" ] && [ "$old_current" != "releases/$release_id" ]; then atomic_symlink "$old_current" "$PREVIOUS_LINK"; fi
atomic_symlink "releases/$release_id" "$CURRENT_LINK"
atomic_symlink "$CURRENT_LINK/share/chancery/krisis" "$PROVIDER_LINK"
atomic_symlink "$CURRENT_LINK/share/chancery/decisions" "$LEGACY_PROVIDER_LINK"
database_touched=1
doctor_output=$(/usr/bin/env -i HOME="$install_home" PATH=/usr/bin:/bin:/usr/sbin:/sbin CONVERSATIONS_CODEX="$codex_path" "$release/libexec/krisis" --database "$DATABASE_PATH" --annals-binary "$annals_path" --annals-config "$annals_config" --annals-library-id "$annals_library_id" --json doctor) || fail 'candidate doctor failed'
doctor_compact=$(printf '%s' "$doctor_output" | tr -d '[:space:]')
printf '%s\n' "$doctor_compact" | grep -F '"schema_version":4' >/dev/null || fail 'doctor did not prove schema 4'
printf '%s\n' "$doctor_compact" | grep -F "\"annals_library_id\":\"$annals_library_id\"" >/dev/null || fail 'doctor did not prove the dedicated Annals target'
/usr/bin/env -i HOME="$install_home" PATH=/usr/bin:/bin:/usr/sbin:/sbin CONVERSATIONS_CODEX="$codex_path" "$release/libexec/krisis" --database "$DATABASE_PATH" observe activate >/dev/null || fail 'unable to activate the Krisis baseline'
validate_private_database_file "$DATABASE_PATH" database
for suffix in wal shm journal; do [ ! -f "$DATABASE_PATH-$suffix" ] || validate_private_database_file "$DATABASE_PATH-$suffix" "database $suffix sidecar"; done
install -m 0600 "$release/package/hooks.json" "$HOOKS_PATH"
atomic_symlink "$CURRENT_LINK/bin/krisis" "$CLI_PATH"

candidate_binding_receipt="$TRANSACTION/candidate-observer-binding.txt"
{
    printf '%s\n' 'format=1'
    printf 'release_id=%s\n' "$release_id"
    printf 'definition_digest=%s\n' "$candidate_definition_digest"
    printf 'annals_binary=%s\n' "$annals_path"
    printf 'annals_config=%s\n' "$annals_config"
    printf 'annals_library_id=%s\n' "$annals_library_id"
} >"$candidate_binding_receipt"
chmod 0600 "$candidate_binding_receipt"
binding_receipt_changed=1
install -m 0600 "$candidate_binding_receipt" "$BINDING_RECEIPT"

touched_active=1
active_switched=1
if [ "$prior_active_enabled" -eq 1 ]; then
    assert_binding_state "$ACTIVE_CLOCKWORK_KEY" active "$prior_active_exists" 0 "$prior_active_digest"
else
    assert_binding_state "$ACTIVE_CLOCKWORK_KEY" active "$prior_active_exists" "$prior_active_enabled" "$prior_active_digest"
fi
HOME="$install_home" "$clockwork_path" --json binding switch "$ACTIVE_CLOCKWORK_KEY" "$candidate_definition_digest" >/dev/null || fail 'Clockwork rejected the Krisis binding switch'
selected_binding="$TRANSACTION/selected-active-binding.json"
HOME="$install_home" "$clockwork_path" --json binding show "$ACTIVE_CLOCKWORK_KEY" >"$selected_binding" 2>"$selected_binding.stderr" || fail 'unable to verify the selected Krisis binding'
[ "$(plutil -extract ok raw "$selected_binding" 2>/dev/null)" = true ] \
    && [ "$(plutil -extract data.key raw "$selected_binding" 2>/dev/null)" = "$ACTIVE_CLOCKWORK_KEY" ] \
    && [ "$(plutil -extract data.enabled raw "$selected_binding" 2>/dev/null)" = true ] \
    && [ "$(plutil -extract data.definition_digest raw "$selected_binding" 2>/dev/null)" = "$candidate_definition_digest" ] \
    || fail 'Krisis binding did not select the exact candidate definition'
prove_definition "$ACTIVE_CLOCKWORK_KEY" "$candidate_definition_digest" "$release" "$release_id" "$release/bin/krisis-observer" "$runner_hash" active
if [ "$prior_legacy_observer_enabled" -eq 1 ]; then
    assert_binding_state "$LEGACY_OBSERVER_CLOCKWORK_KEY" legacy-observer "$prior_legacy_observer_exists" 0 "$prior_legacy_observer_digest"
else
    assert_binding_state "$LEGACY_OBSERVER_CLOCKWORK_KEY" legacy-observer "$prior_legacy_observer_exists" "$prior_legacy_observer_enabled" "$prior_legacy_observer_digest"
fi
if [ "$prior_legacy_daily_enabled" -eq 1 ]; then
    assert_binding_state "$LEGACY_DAILY_CLOCKWORK_KEY" legacy-daily "$prior_legacy_daily_exists" 0 "$prior_legacy_daily_digest"
else
    assert_binding_state "$LEGACY_DAILY_CLOCKWORK_KEY" legacy-daily "$prior_legacy_daily_exists" "$prior_legacy_daily_enabled" "$prior_legacy_daily_digest"
fi
hold_receipt_matches_candidate && hold_receipt_matches_gate \
    || fail 'authenticated Krisis maintenance hold changed before cutover commit'
committed=1
if [ "$keep_maintenance" -eq 0 ]; then
    release_held_maintenance
fi
rm -rf "$TRANSACTION"
TRANSACTION=
if [ "$keep_maintenance" -eq 1 ]; then
    printf 'installed krisis %s (%s); authenticated maintenance hold retained at %s\n' "$version" "$release_id" "$HOLD_RECEIPT"
else
    printf 'installed krisis %s (%s); authenticated maintenance hold released\n' "$version" "$release_id"
fi
