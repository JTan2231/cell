#!/bin/sh

set -eu

PATH=/usr/bin:/bin:/usr/sbin:/sbin
export PATH
umask 077

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
LABEL=org.semantics.worker
SOURCE_FRONTEND="$SCRIPT_DIR/semantics"
SOURCE_RUNNER="$SCRIPT_DIR/semantics-worker"
SOURCE_PLIST="$SCRIPT_DIR/$LABEL.plist"
SOURCE_UNINSTALLER="$SCRIPT_DIR/uninstall-user.sh"
if [ -d "$SCRIPT_DIR/../share/chancery/semantics" ]; then
    SOURCE_CHANCERY="$SCRIPT_DIR/../share/chancery/semantics"
else
    SOURCE_CHANCERY="$SCRIPT_DIR/../../chancery"
fi

binary_path=
install_home=${HOME:-}
launchctl_path=/bin/launchctl

fail() {
    printf 'semantics user deploy: %s\n' "$*" >&2
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
[ -f "$binary_path" ] && [ ! -L "$binary_path" ] && [ -x "$binary_path" ] \
    || fail 'candidate is not an executable regular file'
[ -x "$launchctl_path" ] && [ ! -L "$launchctl_path" ] || fail 'launchctl is unavailable'
[ -x /usr/sbin/lsof ] || fail 'lsof is unavailable'
[ -x /usr/bin/perl ] || fail 'perl is unavailable'
for source in "$SOURCE_FRONTEND" "$SOURCE_RUNNER" "$SOURCE_PLIST" "$SOURCE_UNINSTALLER"; do
    [ -f "$source" ] && [ ! -L "$source" ] || fail "missing packaged file: $source"
done

validate_bundle() {
    bundle=$1
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
    grep -Eq '"schema_version"[[:space:]]*:[[:space:]]*2' "$bundle/provider.json" \
        || fail 'Chancery provider schema is not version 2'
    grep -Eq '"id"[[:space:]]*:[[:space:]]*"semantics"' "$bundle/provider.json" \
        || fail 'Chancery provider ID is not semantics'
    for entry_id in semantics.repository.explore semantics.project.operate semantics.develop.change; do
        grep -R -F -q "\"id\": \"$entry_id\"" "$bundle/entries" \
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

validate_bundle "$SOURCE_CHANCERY"
candidate_version=$("$binary_path" --version) || fail 'unable to read candidate version'
case "$candidate_version" in
    'semantics '*) version=${candidate_version#semantics } ;;
    *) fail "unexpected candidate version: $candidate_version" ;;
esac
provider_version=$(awk -F '"' '/"release"[[:space:]]*:/ { print $4; exit }' "$SOURCE_CHANCERY/provider.json")
[ "$provider_version" = "$version" ] \
    || fail "provider release $provider_version does not match candidate $version"

STATE_DIR="$install_home/Library/Application Support/Semantics"
INSTALL_DIR="$STATE_DIR/install"
RELEASES_DIR="$INSTALL_DIR/releases"
CURRENT_LINK="$INSTALL_DIR/current"
PREVIOUS_LINK="$INSTALL_DIR/previous"
LOCK_DIR="$INSTALL_DIR/.update-lock"
CLI_DIR="$install_home/.local/bin"
CLI_PATH="$CLI_DIR/semantics"
AGENT_DIR="$install_home/Library/LaunchAgents"
PLIST_PATH="$AGENT_DIR/$LABEL.plist"
LOG_DIR="$install_home/Library/Logs/Semantics"
DATABASE_PATH="$STATE_DIR/semantics.db"
PROVIDERS_DIR="$install_home/Library/Application Support/Chancery/providers"
PROVIDER_LINK="$PROVIDERS_DIR/semantics"
SERVICE_DOMAIN="gui/$(id -u)"
SERVICE_TARGET="$SERVICE_DOMAIN/$LABEL"
EXPECTED_CLI="$INSTALL_DIR/current/bin/semantics"
EXPECTED_PROVIDER="$INSTALL_DIR/current/share/chancery/semantics"

for directory in "$STATE_DIR" "$INSTALL_DIR" "$RELEASES_DIR" "$LOG_DIR"; do
    [ ! -L "$directory" ] || fail "refusing symbolic-link directory: $directory"
    [ ! -e "$directory" ] || [ -d "$directory" ] || fail "directory path is occupied: $directory"
    [ -d "$directory" ] || install -d -m 0700 "$directory"
done
for directory in "$CLI_DIR" "$AGENT_DIR" "$PROVIDERS_DIR"; do
    [ ! -L "$directory" ] || fail "refusing symbolic-link directory: $directory"
    [ ! -e "$directory" ] || [ -d "$directory" ] || fail "directory path is occupied: $directory"
    [ -d "$directory" ] || install -d -m 0755 "$directory"
done
mkdir "$LOCK_DIR" 2>/dev/null || fail 'another Semantics deployment is active'

temporary=$(mktemp -d "$INSTALL_DIR/.candidate.XXXXXX")
temporary_plist=$(mktemp "$INSTALL_DIR/.worker-plist.XXXXXX")
transaction_dir=$(mktemp -d "$INSTALL_DIR/.transaction.XXXXXX")
worker_lock_ready="$INSTALL_DIR/.worker-lock-ready.$$"
worker_lock_stop="$INSTALL_DIR/.worker-lock-stop.$$"
old_current=
old_previous=
old_cli=
old_provider=
old_plist=
release=
release_created=0
switched=0
committed=0
service_was_loaded=0
service_stopped=0
new_service_loaded=0
plist_changed=0
cli_suspended=0
database_touched=0
database_was_absent=0
retain_transaction=0
worker_lock_pid=

release_worker_lock() {
    [ -n "$worker_lock_pid" ] || return 0
    : >"$worker_lock_stop" || return 1
    if wait "$worker_lock_pid"; then lock_status=0; else lock_status=$?; fi
    worker_lock_pid=
    rm -f "$worker_lock_ready" "$worker_lock_stop"
    return "$lock_status"
}

cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    set +e
    rollback_ready=1
    if [ "$status" -ne 0 ] && [ "$committed" -eq 0 ]; then
        rollback_service_loaded=0
        if [ "$new_service_loaded" -eq 1 ]; then
            rollback_service_loaded=1
        elif [ "$plist_changed" -eq 1 ] \
            && "$launchctl_path" print "$SERVICE_TARGET" >/dev/null 2>&1; then
            rollback_service_loaded=1
        fi
        if [ "$rollback_service_loaded" -eq 1 ]; then
            "$launchctl_path" bootout "$SERVICE_TARGET" >/dev/null 2>&1 \
                || rollback_ready=0
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
        if [ "$plist_changed" -eq 1 ] && [ "$rollback_ready" -eq 1 ]; then
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
        if [ "$rollback_ready" -eq 1 ] && [ "$service_stopped" -eq 1 ] \
            && [ "$service_was_loaded" -eq 1 ] && [ -n "$old_plist" ]; then
            "$launchctl_path" bootstrap "$SERVICE_DOMAIN" "$PLIST_PATH" >/dev/null 2>&1 \
                || rollback_ready=0
        fi
        if [ "$rollback_ready" -eq 1 ] && ! release_worker_lock; then
            rollback_ready=0
        fi
        if [ "$rollback_ready" -eq 0 ]; then
            # Fail closed while the deployment still holds the worker flock.
            # A loaded but unquiescent launchd job then has no executable
            # current runner path, even after the flock holder exits.
            rm -f "$CLI_PATH" "$PROVIDER_LINK" "$CURRENT_LINK" "$PREVIOUS_LINK" "$PLIST_PATH"
            retain_transaction=1
            printf '%s\n' 'semantics user deploy: rollback could not prove service/database quiescence or restore every owned artifact; current runner, plist, and public selectors are disabled' >&2
            printf 'semantics user deploy: private rollback backup retained at %s\n' "$transaction_dir" >&2
            release_worker_lock >/dev/null 2>&1 || true
        elif [ "$release_created" -eq 1 ] && [ -n "$release" ]; then
            rm -rf "$release"
        fi
    fi
    rm -rf "$temporary"
    rm -f "$temporary_plist"
    release_worker_lock >/dev/null 2>&1 || true
    rm -f "$worker_lock_ready" "$worker_lock_stop"
    [ -z "$old_plist" ] || rm -f "$old_plist"
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
    [ "$(sed -n '1p' "$selected_manifest")" = 'format=1' ] \
        || fail "selected Semantics release manifest format is unsupported: $selector"
    selected_manifest_id=$(sed -n '2s/^release_id=//p' "$selected_manifest")
    selected_version=$(sed -n '3s/^version=//p' "$selected_manifest")
    selected_binary_hash=$(sed -n '4s/^binary_sha256=//p' "$selected_manifest")
    selected_frontend_hash=$(sed -n '5s/^frontend_sha256=//p' "$selected_manifest")
    selected_runner_hash=$(sed -n '6s/^runner_sha256=//p' "$selected_manifest")
    selected_plist_hash=$(sed -n '7s/^plist_sha256=//p' "$selected_manifest")
    selected_deployer_hash=$(sed -n '8s/^deployer_sha256=//p' "$selected_manifest")
    selected_uninstaller_hash=$(sed -n '9s/^uninstaller_sha256=//p' "$selected_manifest")
    selected_chancery_hash=$(sed -n '10s/^chancery_sha256=//p' "$selected_manifest")
    printf '%s\n' "$selected_manifest_id" "$selected_binary_hash" "$selected_frontend_hash" \
        "$selected_runner_hash" "$selected_plist_hash" "$selected_deployer_hash" \
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
        "$selected_release/package/uninstall-user.sh" \
        "$selected_release/package/$LABEL.plist"
    do
        [ -f "$owned_file" ] && [ ! -L "$owned_file" ] \
            || fail "selected Semantics release is incomplete: $selector"
    done
    validate_bundle "$selected_release/share/chancery/semantics"
    actual_binary_hash=$(shasum -a 256 "$selected_release/libexec/semantics" | awk '{print $1}')
    actual_frontend_hash=$(shasum -a 256 "$selected_release/bin/semantics" | awk '{print $1}')
    actual_runner_hash=$(shasum -a 256 "$selected_release/bin/semantics-worker" | awk '{print $1}')
    actual_plist_hash=$(shasum -a 256 "$selected_release/package/$LABEL.plist" | awk '{print $1}')
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
    [ "$actual_plist_hash" = "$selected_plist_hash" ] || fail "selected Semantics plist is tampered: $selector"
    [ "$actual_deployer_hash" = "$selected_deployer_hash" ] || fail "selected Semantics deployer is tampered: $selector"
    [ "$actual_uninstaller_hash" = "$selected_uninstaller_hash" ] || fail "selected Semantics uninstaller is tampered: $selector"
    [ "$actual_chancery_hash" = "$selected_chancery_hash" ] || fail "selected Semantics provider is tampered: $selector"
    actual_release_id=$(printf '%s\n' "$actual_binary_hash" "$actual_frontend_hash" \
        "$actual_runner_hash" "$actual_plist_hash" "$actual_deployer_hash" \
        "$actual_uninstaller_hash" "$actual_chancery_hash" | shasum -a 256 | awk '{print $1}')
    [ "$actual_release_id" = "$selected_id" ] \
        || fail "selected Semantics release content ID does not match: $selector"
}

if [ -n "$old_current" ]; then
    validate_release_selector "$old_current"
    [ -z "$old_previous" ] || validate_release_selector "$old_previous"
    [ -z "$old_cli" ] || [ "$old_cli" = "$EXPECTED_CLI" ] \
        || fail "installed command is not owned by Semantics: $CLI_PATH"
elif [ -n "$old_previous" ] || [ -n "$old_cli" ] || [ -n "$old_provider" ]; then
    fail 'installed selectors have no current Semantics release'
fi
[ -z "$old_provider" ] || [ "$old_provider" = "$EXPECTED_PROVIDER" ] \
    || fail "provider selector is not owned by Semantics: $PROVIDER_LINK"

if [ -L "$PLIST_PATH" ]; then fail "LaunchAgent must not be a symbolic link: $PLIST_PATH"; fi
if [ -e "$PLIST_PATH" ] && [ ! -f "$PLIST_PATH" ]; then fail "LaunchAgent path is occupied: $PLIST_PATH"; fi
if [ -f "$PLIST_PATH" ]; then
    [ -n "$old_current" ] || fail 'LaunchAgent has no owned Semantics release'
    [ "$(plutil -extract Label raw "$PLIST_PATH" 2>/dev/null)" = "$LABEL" ] \
        || fail 'LaunchAgent label is not owned by Semantics'
    [ "$(plutil -extract ProgramArguments.1 raw "$PLIST_PATH" 2>/dev/null)" = "$INSTALL_DIR/current/bin/semantics-worker" ] \
        || fail 'LaunchAgent runner is not owned by Semantics'
    old_plist=$(mktemp "$INSTALL_DIR/.old-worker-plist.XXXXXX")
    cp -p "$PLIST_PATH" "$old_plist"
    cp -p "$PLIST_PATH" "$transaction_dir/prior-worker.plist"
fi
if "$launchctl_path" print "$SERVICE_TARGET" >/dev/null 2>&1; then
    service_was_loaded=1
    [ -n "$old_plist" ] || fail 'loaded Semantics label has no owned recoverable plist'
fi

binary_hash=$(shasum -a 256 "$binary_path" | awk '{print $1}')
frontend_hash=$(shasum -a 256 "$SOURCE_FRONTEND" | awk '{print $1}')
runner_hash=$(shasum -a 256 "$SOURCE_RUNNER" | awk '{print $1}')
plist_hash=$(shasum -a 256 "$SOURCE_PLIST" | awk '{print $1}')
deployer_hash=$(shasum -a 256 "$0" | awk '{print $1}')
uninstaller_hash=$(shasum -a 256 "$SOURCE_UNINSTALLER" | awk '{print $1}')
chancery_hash=$(bundle_hash "$SOURCE_CHANCERY")
release_id=$(printf '%s\n' "$binary_hash" "$frontend_hash" "$runner_hash" "$plist_hash" \
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
    [ "$(shasum -a 256 "$release/package/$LABEL.plist" | awk '{print $1}')" = "$plist_hash" ] || fail 'existing release plist is tampered'
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
    install -m 0644 "$SOURCE_PLIST" "$temporary/package/$LABEL.plist"
    cp -R "$SOURCE_CHANCERY" "$temporary/share/chancery/semantics"
    {
        printf '%s\n' 'format=1'
        printf 'release_id=%s\n' "$release_id"
        printf 'version=%s\n' "$version"
        printf 'binary_sha256=%s\n' "$binary_hash"
        printf 'frontend_sha256=%s\n' "$frontend_hash"
        printf 'runner_sha256=%s\n' "$runner_hash"
        printf 'plist_sha256=%s\n' "$plist_hash"
        printf 'deployer_sha256=%s\n' "$deployer_hash"
        printf 'uninstaller_sha256=%s\n' "$uninstaller_hash"
        printf 'chancery_sha256=%s\n' "$chancery_hash"
    } >"$temporary/manifest.txt"
    chmod 0444 "$temporary/manifest.txt"
    chmod -R go-w "$temporary"
    mv "$temporary" "$release"
    release_created=1
    temporary=$(mktemp -d "$INSTALL_DIR/.candidate.XXXXXX")
    validate_release_selector "releases/$release_id"
fi

sed \
    -e "s|__SEMANTICS_WORKER_RUNNER__|$INSTALL_DIR/current/bin/semantics-worker|g" \
    -e "s|__SEMANTICS_STATE_DIR__|$STATE_DIR|g" \
    -e "s|__SEMANTICS_HOME__|$install_home|g" \
    -e "s|__SEMANTICS_WORKER_STDOUT__|$LOG_DIR/worker.stdout.log|g" \
    -e "s|__SEMANTICS_WORKER_STDERR__|$LOG_DIR/worker.stderr.log|g" \
    "$SOURCE_PLIST" >"$temporary_plist"
plutil -lint "$temporary_plist" >/dev/null || fail 'generated worker LaunchAgent is invalid'

if [ -x /opt/homebrew/bin/codex ]; then
    codex_path=/opt/homebrew/bin/codex
elif [ -x "$install_home/.local/bin/codex" ]; then
    codex_path="$install_home/.local/bin/codex"
else
    fail 'Codex executable is unavailable'
fi
[ -x "$install_home/.local/bin/decisions" ] || fail 'Decisions executable is unavailable'

if [ "$service_was_loaded" -eq 1 ]; then
    "$launchctl_path" bootout "$SERVICE_TARGET" >/dev/null \
        || fail 'unable to stop the owned worker service'
    service_stopped=1
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
} >"$transaction_dir/prior-install.txt"
chmod 0600 "$transaction_dir/prior-install.txt"

if [ -L "$DATABASE_PATH" ]; then
    fail 'database must not be a symbolic link'
elif [ -e "$DATABASE_PATH" ] && [ ! -f "$DATABASE_PATH" ]; then
    fail 'database must be a regular file'
elif [ ! -e "$DATABASE_PATH" ]; then
    database_was_absent=1
fi
for suffix in wal shm journal; do
    sidecar="$DATABASE_PATH-$suffix"
    if [ -L "$sidecar" ]; then
        fail "database sidecar must not be a symbolic link: $sidecar"
    elif [ -e "$sidecar" ] && [ ! -f "$sidecar" ]; then
        fail "database sidecar must be a regular file: $sidecar"
    elif [ -f "$sidecar" ] && [ "$database_was_absent" -eq 1 ]; then
        fail "database sidecar exists without its database: $sidecar"
    fi
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

switched=1
if [ -n "$old_current" ] && [ "$old_current" != "releases/$release_id" ]; then
    atomic_symlink "$old_current" "$PREVIOUS_LINK"
fi
atomic_symlink "releases/$release_id" "$CURRENT_LINK"
atomic_symlink "$EXPECTED_PROVIDER" "$PROVIDER_LINK"

database_touched=1
doctor_output=$(/usr/bin/env -i \
    HOME="$install_home" \
    PATH=/usr/bin:/bin:/usr/sbin:/sbin \
    CONVERSATIONS_CODEX="$codex_path" \
    SEMANTICS_DECISIONS="$install_home/.local/bin/decisions" \
    "$release/libexec/semantics" --database "$DATABASE_PATH" --json doctor) \
    || fail 'candidate doctor failed'
doctor_compact=$(printf '%s' "$doctor_output" | tr -d '[:space:]')
case "$doctor_compact" in *'"ok":true'*) ;; *) fail 'candidate doctor did not report ok' ;; esac
for check_name in database participation_markers decisions_lifecycle conversations_exact_cwd nucleus_reconciliation; do
    case "$doctor_compact" in
        *"\"name\":\"$check_name\",\"ok\":true"*) ;;
        *) fail "candidate doctor did not prove $check_name" ;;
    esac
done
case "$doctor_compact" in *'"detail":"schema1at'*) ;; *) fail 'candidate doctor did not prove schema version 1' ;; esac

install -m 0644 "$temporary_plist" "$PLIST_PATH"
plist_changed=1
atomic_symlink "$EXPECTED_CLI" "$CLI_PATH"
cli_suspended=0
"$launchctl_path" bootstrap "$SERVICE_DOMAIN" "$PLIST_PATH" >/dev/null 2>&1 \
    || fail 'launchd rejected the worker service'
new_service_loaded=1
"$launchctl_path" print "$SERVICE_TARGET" >/dev/null 2>&1 \
    || fail 'launchd did not report the worker service loaded'
release_worker_lock || fail 'unable to release the Semantics worker lock'

committed=1
printf 'installed semantics %s (%s)\n' "$version" "$release_id"
