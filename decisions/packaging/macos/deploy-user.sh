#!/bin/sh

set -eu

PATH=/usr/bin:/bin:/usr/sbin:/sbin
export PATH
umask 077

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
DAILY_LABEL=org.decisions.daily-email
OBSERVER_LABEL=org.decisions.observer
SOURCE_FRONTEND="$SCRIPT_DIR/decisions"
SOURCE_DAILY_RUNNER="$SCRIPT_DIR/decisions-daily-email"
SOURCE_OBSERVER_RUNNER="$SCRIPT_DIR/decisions-observer"
SOURCE_DAILY_PLIST="$SCRIPT_DIR/$DAILY_LABEL.plist"
SOURCE_OBSERVER_PLIST="$SCRIPT_DIR/$OBSERVER_LABEL.plist"
SOURCE_HOOKS="$SCRIPT_DIR/hooks.json"
SOURCE_UNINSTALLER="$SCRIPT_DIR/uninstall-user.sh"
if [ -d "$SCRIPT_DIR/../share/chancery/decisions" ]; then
    SOURCE_CHANCERY="$SCRIPT_DIR/../share/chancery/decisions"
else
    SOURCE_CHANCERY="$SCRIPT_DIR/../../chancery"
fi

binary_path=
install_home=${HOME:-}
launchctl_path=/bin/launchctl

fail() {
    printf 'decisions user deploy: %s\n' "$*" >&2
    exit 1
}

usage() {
    printf '%s\n' 'Usage: deploy-user.sh --binary ABSOLUTE_PATH [--home ABSOLUTE_PATH] [--launchctl ABSOLUTE_PATH]'
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --binary) [ "$#" -ge 2 ] || fail '--binary requires a path'; binary_path=$2; shift 2 ;;
        --home) [ "$#" -ge 2 ] || fail '--home requires a path'; install_home=$2; shift 2 ;;
        --launchctl) [ "$#" -ge 2 ] || fail '--launchctl requires a path'; launchctl_path=$2; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *) fail "unknown argument: $1" ;;
    esac
done

[ -n "$binary_path" ] || fail '--binary is required'
case "$binary_path" in /*) ;; *) fail 'binary must be absolute' ;; esac
case "$install_home" in /*) ;; *) fail 'home must be absolute' ;; esac
case "$launchctl_path" in /*) ;; *) fail 'launchctl must be absolute' ;; esac
case "$install_home" in *'&'*|*'<'*|*'>'*|*'|'*|*'
'*) fail 'home contains unsupported plist characters' ;; esac
[ -d "$install_home" ] && [ ! -L "$install_home" ] || fail 'home is not a regular directory'
[ -f "$binary_path" ] && [ ! -L "$binary_path" ] && [ -x "$binary_path" ] || fail 'candidate is not an executable regular file'
[ -x "$launchctl_path" ] && [ ! -L "$launchctl_path" ] || fail 'launchctl is unavailable'
[ -x /usr/sbin/lsof ] || fail 'lsof is unavailable'
for source in "$SOURCE_FRONTEND" "$SOURCE_DAILY_RUNNER" "$SOURCE_OBSERVER_RUNNER" \
    "$SOURCE_DAILY_PLIST" "$SOURCE_OBSERVER_PLIST" "$SOURCE_HOOKS" "$SOURCE_UNINSTALLER"
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
CODEX_DIR="$install_home/.codex"
HOOKS_PATH="$CODEX_DIR/hooks.json"
PROVIDERS_DIR="$install_home/Library/Application Support/Chancery/providers"
PROVIDER_LINK="$PROVIDERS_DIR/decisions"
SERVICE_DOMAIN="gui/$(id -u)"
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
mkdir "$LOCK_DIR" 2>/dev/null || fail 'another Decisions deployment is active'
temporary=$(mktemp -d "$INSTALL_DIR/.candidate.XXXXXX")
temporary_daily_plist=$(mktemp "$INSTALL_DIR/.daily-plist.XXXXXX")
temporary_observer_plist=$(mktemp "$INSTALL_DIR/.observer-plist.XXXXXX")
transaction_dir=$(mktemp -d "$INSTALL_DIR/.transaction.XXXXXX")
old_current=
old_previous=
old_cli=
old_provider=
old_daily_plist=
old_observer_plist=
old_hooks=
switched=0
committed=0
daily_was_loaded=0
observer_was_loaded=0
daily_service_stopped=0
observer_service_stopped=0
new_daily_service_loaded=0
new_observer_service_loaded=0
daily_plist_changed=0
observer_plist_changed=0
hooks_changed=0
cli_suspended=0
database_touched=0
database_was_absent=0
retain_transaction=0
cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    set +e
    if [ "$status" -ne 0 ] && [ "$committed" -eq 0 ]; then
        rollback_ready=1
        if [ "$new_observer_service_loaded" -eq 1 ] || [ "$observer_plist_changed" -eq 1 ]; then
            "$launchctl_path" bootout "$OBSERVER_TARGET" >/dev/null 2>&1 || true
        fi
        if [ "$new_daily_service_loaded" -eq 1 ] || [ "$daily_plist_changed" -eq 1 ]; then
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
                cp -p "$old_daily_plist" "$DAILY_PLIST" || rollback_ready=0
            else
                rm -f "$DAILY_PLIST" || rollback_ready=0
            fi
        fi
        if [ "$observer_plist_changed" -eq 1 ] && [ "$rollback_ready" -eq 1 ]; then
            if [ -n "$old_observer_plist" ]; then
                cp -p "$old_observer_plist" "$OBSERVER_PLIST" || rollback_ready=0
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
        if [ "$rollback_ready" -eq 1 ] && [ "$daily_service_stopped" -eq 1 ] && [ "$daily_was_loaded" -eq 1 ] && [ -n "$old_daily_plist" ]; then
            "$launchctl_path" bootstrap "$SERVICE_DOMAIN" "$DAILY_PLIST" >/dev/null 2>&1 || true
        fi
        if [ "$rollback_ready" -eq 1 ] && [ "$observer_service_stopped" -eq 1 ] && [ "$observer_was_loaded" -eq 1 ] && [ -n "$old_observer_plist" ]; then
            "$launchctl_path" bootstrap "$SERVICE_DOMAIN" "$OBSERVER_PLIST" >/dev/null 2>&1 || true
        fi
        if [ "$rollback_ready" -eq 0 ]; then
            rm -f "$CLI_PATH"
            retain_transaction=1
            printf '%s\n' 'decisions user deploy: rollback could not prove quiescence or restore every owned artifact; services are stopped and the public command is disabled' >&2
            printf 'decisions user deploy: private rollback backup retained at %s\n' "$transaction_dir" >&2
        fi
    fi
    rm -rf "$temporary"
    rm -f "$temporary_daily_plist" "$temporary_observer_plist"
    [ -z "$old_daily_plist" ] || rm -f "$old_daily_plist"
    [ -z "$old_observer_plist" ] || rm -f "$old_observer_plist"
    [ -z "$old_hooks" ] || rm -f "$old_hooks"
    [ "$retain_transaction" -eq 1 ] || rm -rf "$transaction_dir"
    rmdir "$LOCK_DIR" >/dev/null 2>&1 || true
    exit "$status"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

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
    [ "$(awk 'END { print NR }' "$selected_manifest")" -eq 13 ] \
        || fail "selected Decisions release manifest is not canonical: $selector"
    [ "$(sed -n '1p' "$selected_manifest")" = 'format=2' ] \
        || fail "selected Decisions release manifest format is unsupported: $selector"
    selected_manifest_release=$(sed -n '2s/^release_id=//p' "$selected_manifest")
    selected_version=$(sed -n '3s/^version=//p' "$selected_manifest")
    selected_binary_hash=$(sed -n '4s/^binary_sha256=//p' "$selected_manifest")
    selected_frontend_hash=$(sed -n '5s/^frontend_sha256=//p' "$selected_manifest")
    selected_daily_runner_hash=$(sed -n '6s/^daily_runner_sha256=//p' "$selected_manifest")
    selected_observer_runner_hash=$(sed -n '7s/^observer_runner_sha256=//p' "$selected_manifest")
    selected_daily_plist_hash=$(sed -n '8s/^daily_plist_sha256=//p' "$selected_manifest")
    selected_observer_plist_hash=$(sed -n '9s/^observer_plist_sha256=//p' "$selected_manifest")
    selected_hooks_hash=$(sed -n '10s/^hooks_sha256=//p' "$selected_manifest")
    selected_deployer_hash=$(sed -n '11s/^deployer_sha256=//p' "$selected_manifest")
    selected_uninstaller_hash=$(sed -n '12s/^uninstaller_sha256=//p' "$selected_manifest")
    selected_chancery_hash=$(sed -n '13s/^chancery_sha256=//p' "$selected_manifest")
    printf '%s\n' "$selected_manifest_release" "$selected_binary_hash" "$selected_frontend_hash" \
        "$selected_daily_runner_hash" "$selected_observer_runner_hash" \
        "$selected_daily_plist_hash" "$selected_observer_plist_hash" "$selected_hooks_hash" \
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
        "$selected_release/package/$DAILY_LABEL.plist" \
        "$selected_release/package/$OBSERVER_LABEL.plist" \
        "$selected_release/package/hooks.json"
    do
        [ -f "$owned_file" ] && [ ! -L "$owned_file" ] \
            || fail "selected Decisions release is incomplete: $selector"
    done
    validate_bundle "$selected_release/share/chancery/decisions"
    actual_binary_hash=$(shasum -a 256 "$selected_release/libexec/decisions" | awk '{print $1}')
    actual_frontend_hash=$(shasum -a 256 "$selected_release/bin/decisions" | awk '{print $1}')
    actual_daily_runner_hash=$(shasum -a 256 "$selected_release/bin/decisions-daily-email" | awk '{print $1}')
    actual_observer_runner_hash=$(shasum -a 256 "$selected_release/bin/decisions-observer" | awk '{print $1}')
    actual_daily_plist_hash=$(shasum -a 256 "$selected_release/package/$DAILY_LABEL.plist" | awk '{print $1}')
    actual_observer_plist_hash=$(shasum -a 256 "$selected_release/package/$OBSERVER_LABEL.plist" | awk '{print $1}')
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
    [ "$actual_daily_plist_hash" = "$selected_daily_plist_hash" ] \
        || fail "selected Decisions release daily plist is tampered: $selector"
    [ "$actual_observer_plist_hash" = "$selected_observer_plist_hash" ] \
        || fail "selected Decisions release observer plist is tampered: $selector"
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
        "$actual_daily_plist_hash" "$actual_observer_plist_hash" "$actual_hooks_hash" \
        "$actual_deployer_hash" \
        "$actual_uninstaller_hash" "$actual_chancery_hash" | shasum -a 256 | awk '{print $1}')
    [ "$actual_release_id" = "$selector_id" ] \
        || fail "selected Decisions release content ID does not match: $selector"
}
if [ -n "$old_current" ]; then
    validate_release_selector "$old_current"
    if [ -n "$old_previous" ]; then validate_release_selector "$old_previous"; fi
    [ -z "$old_cli" ] || [ "$old_cli" = "$expected_cli" ] || fail "installed command is not owned by Decisions: $CLI_PATH"
elif [ -n "$old_previous" ] || [ -n "$old_cli" ] || [ -n "$old_provider" ]; then
    fail 'installed selectors have no current Decisions release'
fi
[ -z "$old_provider" ] || [ "$old_provider" = "$expected_provider" ] || fail "provider selector is not owned by Decisions: $PROVIDER_LINK"
if [ -L "$DAILY_PLIST" ]; then fail "LaunchAgent must not be a symbolic link: $DAILY_PLIST"; fi
if [ -e "$DAILY_PLIST" ] && [ ! -f "$DAILY_PLIST" ]; then fail "LaunchAgent path is occupied: $DAILY_PLIST"; fi
if [ -f "$DAILY_PLIST" ]; then
    [ -n "$old_current" ] || fail "LaunchAgent has no owned Decisions release"
    [ "$(plutil -extract Label raw "$DAILY_PLIST" 2>/dev/null)" = "$DAILY_LABEL" ] \
        || fail "LaunchAgent label is not owned by Decisions"
    [ "$(plutil -extract ProgramArguments.1 raw "$DAILY_PLIST" 2>/dev/null)" = "$INSTALL_DIR/current/bin/decisions-daily-email" ] \
        || fail "LaunchAgent runner is not owned by Decisions"
    old_daily_plist=$(mktemp "$INSTALL_DIR/.old-daily-plist.XXXXXX")
    cp -p "$DAILY_PLIST" "$old_daily_plist"
fi
if [ -L "$OBSERVER_PLIST" ]; then fail "LaunchAgent must not be a symbolic link: $OBSERVER_PLIST"; fi
if [ -e "$OBSERVER_PLIST" ] && [ ! -f "$OBSERVER_PLIST" ]; then fail "LaunchAgent path is occupied: $OBSERVER_PLIST"; fi
if [ -f "$OBSERVER_PLIST" ]; then
    [ -n "$old_current" ] || fail "observer LaunchAgent has no owned Decisions release"
    [ "$(plutil -extract Label raw "$OBSERVER_PLIST" 2>/dev/null)" = "$OBSERVER_LABEL" ] \
        || fail "observer LaunchAgent label is not owned by Decisions"
    [ "$(plutil -extract ProgramArguments.1 raw "$OBSERVER_PLIST" 2>/dev/null)" = "$INSTALL_DIR/current/bin/decisions-observer" ] \
        || fail "observer LaunchAgent runner is not owned by Decisions"
    old_observer_plist=$(mktemp "$INSTALL_DIR/.old-observer-plist.XXXXXX")
    cp -p "$OBSERVER_PLIST" "$old_observer_plist"
fi
if [ -L "$HOOKS_PATH" ]; then fail "Codex hooks file must not be a symbolic link: $HOOKS_PATH"; fi
if [ -e "$HOOKS_PATH" ] && [ ! -f "$HOOKS_PATH" ]; then fail "Codex hooks path is occupied: $HOOKS_PATH"; fi
if [ -f "$HOOKS_PATH" ]; then
    [ -n "$old_current" ] || fail "refusing to replace foreign Codex hooks: $HOOKS_PATH"
    cmp -s "$HOOKS_PATH" "$INSTALL_DIR/$old_current/package/hooks.json" \
        || fail "refusing to replace foreign or modified Codex hooks: $HOOKS_PATH"
    old_hooks=$(mktemp "$INSTALL_DIR/.old-hooks.XXXXXX")
    cp -p "$HOOKS_PATH" "$old_hooks"
fi
if "$launchctl_path" print "$DAILY_TARGET" >/dev/null 2>&1; then
    daily_was_loaded=1
    [ -n "$old_daily_plist" ] || fail "loaded Decisions daily label has no owned recoverable plist"
fi
if "$launchctl_path" print "$OBSERVER_TARGET" >/dev/null 2>&1; then
    observer_was_loaded=1
    [ -n "$old_observer_plist" ] || fail "loaded Decisions observer label has no owned recoverable plist"
fi

binary_hash=$(shasum -a 256 "$binary_path" | awk '{print $1}')
frontend_hash=$(shasum -a 256 "$SOURCE_FRONTEND" | awk '{print $1}')
daily_runner_hash=$(shasum -a 256 "$SOURCE_DAILY_RUNNER" | awk '{print $1}')
observer_runner_hash=$(shasum -a 256 "$SOURCE_OBSERVER_RUNNER" | awk '{print $1}')
daily_plist_hash=$(shasum -a 256 "$SOURCE_DAILY_PLIST" | awk '{print $1}')
observer_plist_hash=$(shasum -a 256 "$SOURCE_OBSERVER_PLIST" | awk '{print $1}')
hooks_hash=$(shasum -a 256 "$SOURCE_HOOKS" | awk '{print $1}')
deployer_hash=$(shasum -a 256 "$0" | awk '{print $1}')
uninstaller_hash=$(shasum -a 256 "$SOURCE_UNINSTALLER" | awk '{print $1}')
chancery_hash=$(bundle_hash "$SOURCE_CHANCERY")
release_id=$(printf '%s\n' "$binary_hash" "$frontend_hash" "$daily_runner_hash" \
    "$observer_runner_hash" "$daily_plist_hash" "$observer_plist_hash" "$hooks_hash" \
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
    [ "$(shasum -a 256 "$release/package/$DAILY_LABEL.plist" | awk '{print $1}')" = "$daily_plist_hash" ] || fail "existing release daily plist is tampered: $release_id"
    [ "$(shasum -a 256 "$release/package/$OBSERVER_LABEL.plist" | awk '{print $1}')" = "$observer_plist_hash" ] || fail "existing release observer plist is tampered: $release_id"
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
    install -m 0644 "$SOURCE_DAILY_PLIST" "$temporary/package/$DAILY_LABEL.plist"
    install -m 0644 "$SOURCE_OBSERVER_PLIST" "$temporary/package/$OBSERVER_LABEL.plist"
    install -m 0644 "$SOURCE_HOOKS" "$temporary/package/hooks.json"
    cp -R "$SOURCE_CHANCERY" "$temporary/share/chancery/decisions"
    {
        printf '%s\n' 'format=2'
        printf 'release_id=%s\n' "$release_id"
        printf 'version=%s\n' "$version"
        printf 'binary_sha256=%s\n' "$binary_hash"
        printf 'frontend_sha256=%s\n' "$frontend_hash"
        printf 'daily_runner_sha256=%s\n' "$daily_runner_hash"
        printf 'observer_runner_sha256=%s\n' "$observer_runner_hash"
        printf 'daily_plist_sha256=%s\n' "$daily_plist_hash"
        printf 'observer_plist_sha256=%s\n' "$observer_plist_hash"
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

sed \
    -e "s|__DECISIONS_RUNNER__|$INSTALL_DIR/current/bin/decisions-daily-email|g" \
    -e "s|__DECISIONS_STATE_DIR__|$STATE_DIR|g" \
    -e "s|__DECISIONS_HOME__|$install_home|g" \
    -e "s|__DECISIONS_STDOUT__|$LOG_DIR/daily-email.stdout.log|g" \
    -e "s|__DECISIONS_STDERR__|$LOG_DIR/daily-email.stderr.log|g" \
    "$SOURCE_DAILY_PLIST" >"$temporary_daily_plist"
plutil -lint "$temporary_daily_plist" >/dev/null || fail 'generated daily LaunchAgent is invalid'
sed \
    -e "s|__DECISIONS_OBSERVER_RUNNER__|$INSTALL_DIR/current/bin/decisions-observer|g" \
    -e "s|__DECISIONS_STATE_DIR__|$STATE_DIR|g" \
    -e "s|__DECISIONS_HOME__|$install_home|g" \
    -e "s|__DECISIONS_OBSERVER_STDOUT__|$LOG_DIR/observer.stdout.log|g" \
    -e "s|__DECISIONS_OBSERVER_STDERR__|$LOG_DIR/observer.stderr.log|g" \
    "$SOURCE_OBSERVER_PLIST" >"$temporary_observer_plist"
plutil -lint "$temporary_observer_plist" >/dev/null || fail 'generated observer LaunchAgent is invalid'

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

if [ "$observer_was_loaded" -eq 1 ]; then
    "$launchctl_path" bootout "$OBSERVER_TARGET" >/dev/null \
        || rollback 'unable to stop the owned observer service'
    observer_service_stopped=1
fi
if [ "$daily_was_loaded" -eq 1 ]; then
    "$launchctl_path" bootout "$DAILY_TARGET" >/dev/null \
        || rollback 'unable to stop the owned daily service'
    daily_service_stopped=1
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

install -m 0644 "$temporary_daily_plist" "$DAILY_PLIST"
daily_plist_changed=1
install -m 0644 "$temporary_observer_plist" "$OBSERVER_PLIST"
observer_plist_changed=1
install -m 0600 "$SOURCE_HOOKS" "$HOOKS_PATH"
hooks_changed=1
atomic_symlink "$expected_cli" "$CLI_PATH"
cli_suspended=0

"$launchctl_path" bootstrap "$SERVICE_DOMAIN" "$DAILY_PLIST" >/dev/null 2>&1 \
    || rollback 'launchd rejected the daily service'
new_daily_service_loaded=1
"$launchctl_path" bootstrap "$SERVICE_DOMAIN" "$OBSERVER_PLIST" >/dev/null 2>&1 \
    || rollback 'launchd rejected the observer service'
new_observer_service_loaded=1
committed=1
printf 'installed decisions %s (%s)\n' "$version" "$release_id"
