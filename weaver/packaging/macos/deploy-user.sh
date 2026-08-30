#!/bin/sh

set -eu

PATH=/usr/bin:/bin:/usr/sbin:/sbin
export PATH
umask 077

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
SERVICE_LABEL=org.weaver.worker
SOURCE_DEPLOYER="$SCRIPT_DIR/deploy-user.sh"
if [ -d "$SCRIPT_DIR/../share/chancery/weaver" ]; then
    SOURCE_CHANCERY="$SCRIPT_DIR/../share/chancery/weaver"
else
    SOURCE_CHANCERY="$SCRIPT_DIR/../../chancery"
fi

binary_path=
install_home=${HOME:-}
launchctl_path=/bin/launchctl
wait_seconds=${WEAVER_UPDATE_WAIT_SECONDS:-21600}

usage() {
    cat <<'EOF'
Usage: deploy-user.sh --binary ABSOLUTE_PATH [OPTIONS]

Install or update the current user's macOS Weaver CLI. A healthy user-owned
Nucleus service must already be installed. This deployer also removes the exact
org.weaver.worker LaunchAgent from the superseded prototype, when present.

Options:
  --home ABSOLUTE_PATH       Override the operator home (primarily for tests)
  --launchctl ABSOLUTE_PATH  Override launchctl (primarily for tests)
  --wait-seconds SECONDS     Bound the wait for an active workflow to finish
EOF
}

fail() {
    printf 'weaver user deploy: %s\n' "$*" >&2
    exit 1
}

validate_chancery_bundle() {
    bundle=$1
    [ -d "$bundle" ] && [ ! -L "$bundle" ] \
        || fail "Chancery bundle is not a regular directory: $bundle"
    [ -f "$bundle/provider.json" ] && [ ! -L "$bundle/provider.json" ] \
        || fail "Chancery bundle has no regular provider.json: $bundle"
    if find "$bundle" -type l -print | grep -q .; then
        fail "Chancery bundle contains a symbolic link: $bundle"
    fi
    if find "$bundle" ! -type d ! -type f -print | grep -q .; then
        fail "Chancery bundle contains a non-file entry: $bundle"
    fi
}

chancery_bundle_hash() {
    bundle=$1
    (
        cd "$bundle"
        find . -type f -print | LC_ALL=C sort | while IFS= read -r file; do
            printf 'path=%s\n' "$file"
            shasum -a 256 "$file"
        done
    ) | shasum -a 256 | awk '{print $1}'
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --binary)
            [ "$#" -ge 2 ] || fail '--binary requires a path'
            binary_path=$2
            shift 2
            ;;
        --home)
            [ "$#" -ge 2 ] || fail '--home requires a path'
            install_home=$2
            shift 2
            ;;
        --launchctl)
            [ "$#" -ge 2 ] || fail '--launchctl requires a path'
            launchctl_path=$2
            shift 2
            ;;
        --wait-seconds)
            [ "$#" -ge 2 ] || fail '--wait-seconds requires a value'
            wait_seconds=$2
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *) fail "unknown argument: $1" ;;
    esac
done

[ -n "$binary_path" ] || fail '--binary is required'
[ -n "$install_home" ] || fail '--home is required'
case "$binary_path" in
    /*) ;;
    *) fail 'binary path must be absolute' ;;
esac
case "$install_home" in
    /*) ;;
    *) fail 'install home must be absolute' ;;
esac
case "$launchctl_path" in
    /*) ;;
    *) fail 'launchctl path must be absolute' ;;
esac
case "$wait_seconds" in
    ''|*[!0-9]*) fail 'wait seconds must be a nonnegative integer' ;;
esac

[ -d "$install_home" ] && [ ! -L "$install_home" ] \
    || fail "operator home is not a regular directory: $install_home"
operator_uid=$(id -u)
[ "$operator_uid" -ne 0 ] || fail 'run this deployer as the Weaver operator, not root'
if operator_home_uid=$(stat -f '%u' "$install_home" 2>/dev/null); then
    :
elif operator_home_uid=$(stat -c '%u' "$install_home" 2>/dev/null); then
    :
else
    fail "unable to inspect operator home ownership: $install_home"
fi
[ "$operator_home_uid" = "$operator_uid" ] \
    || fail "operator home is not owned by uid $operator_uid: $install_home"
[ -f "$binary_path" ] && [ ! -L "$binary_path" ] && [ -x "$binary_path" ] \
    || fail "Weaver candidate is not an executable regular file: $binary_path"
[ -f "$launchctl_path" ] && [ ! -L "$launchctl_path" ] && [ -x "$launchctl_path" ] \
    || fail "launchctl is unavailable: $launchctl_path"
[ -f "$SOURCE_DEPLOYER" ] && [ ! -L "$SOURCE_DEPLOYER" ] \
    || fail "missing packaged file: $SOURCE_DEPLOYER"
for command in awk cp find grep id install mktemp mv readlink shasum sort stat; do
    command -v "$command" >/dev/null 2>&1 \
        || fail "required command not found: $command"
done
validate_chancery_bundle "$SOURCE_CHANCERY"

candidate_version=$("$binary_path" --version) \
    || fail 'unable to read the Weaver candidate version'
case "$candidate_version" in
    'weaver '*) version=${candidate_version#weaver } ;;
    *) fail "Weaver candidate reported an unexpected version: $candidate_version" ;;
esac
[ -n "$version" ] || fail 'Weaver candidate reported an empty version'
provider_release=$(awk -F '"' '/"release"[[:space:]]*:/ { print $4; exit }' \
    "$SOURCE_CHANCERY/provider.json")
[ "$provider_release" = "$version" ] \
    || fail "Chancery provider release $provider_release does not match Weaver $version"
"$binary_path" --help >/dev/null \
    || fail 'unable to read the Weaver candidate help'
sh -n "$SOURCE_DEPLOYER"

STATE_DIR="$install_home/Library/Application Support/Weaver"
INSTALL_DIR="$STATE_DIR/install"
RELEASES_DIR="$INSTALL_DIR/releases"
CURRENT_LINK="$INSTALL_DIR/current"
PREVIOUS_LINK="$INSTALL_DIR/previous"
UPDATE_LOCK="$INSTALL_DIR/.update-lock"
MAINTENANCE_MARKER="$STATE_DIR/.maintenance"
CLI_DIR="$install_home/.local/bin"
CLI_PATH="$CLI_DIR/weaver"
AGENT_PLIST="$install_home/Library/LaunchAgents/$SERVICE_LABEL.plist"
CHANCERY_STATE_DIR="$install_home/Library/Application Support/Chancery"
CHANCERY_PROVIDERS_DIR="$CHANCERY_STATE_DIR/providers"
CHANCERY_PROVIDER_LINK="$CHANCERY_PROVIDERS_DIR/weaver"
CHANCERY_PROVIDER_TARGET="$INSTALL_DIR/current/share/chancery/weaver"
SERVICE_DOMAIN="gui/$operator_uid"
SERVICE_TARGET="$SERVICE_DOMAIN/$SERVICE_LABEL"

for path in "$STATE_DIR" "$INSTALL_DIR" "$RELEASES_DIR"; do
    [ ! -L "$path" ] || fail "refusing symbolic-link directory: $path"
    [ ! -e "$path" ] || [ -d "$path" ] \
        || fail "directory path is occupied by a non-directory: $path"
    install -d -m 0700 "$path"
done
for path in "$CHANCERY_STATE_DIR" "$CHANCERY_PROVIDERS_DIR"; do
    [ ! -L "$path" ] || fail "refusing symbolic-link directory: $path"
    [ ! -e "$path" ] || [ -d "$path" ] \
        || fail "directory path is occupied by a non-directory: $path"
    install -d -m 0700 "$path"
done
[ ! -L "$CLI_DIR" ] || fail "refusing symbolic-link directory: $CLI_DIR"
[ ! -e "$CLI_DIR" ] || [ -d "$CLI_DIR" ] \
    || fail "directory path is occupied by a non-directory: $CLI_DIR"
install -d -m 0755 "$CLI_DIR"

HOME="$install_home" WEAVER_STATE_DIR="$STATE_DIR" \
    "$binary_path" doctor >/dev/null \
    || fail 'candidate doctor could not verify Nucleus readiness'

temporary_release=
transaction_dir=
old_current=
old_previous=
old_cli=
old_chancery_provider=
old_plist=0
prototype_plist_removed=0
switched=0
chancery_provider_switched=0
was_loaded=0
launchd_changed=0
maintenance_started=0
committed=0
lock_created=0

atomic_symlink() {
    target=$1
    path=$2
    temporary="$path.tmp.$$"
    rm -f "$temporary"
    ln -s "$target" "$temporary"
    if mv -fh "$temporary" "$path" 2>/dev/null; then
        return 0
    fi
    mv -fT "$temporary" "$path"
}

run_weaver() {
    executable=$1
    shift
    HOME="$install_home" WEAVER_STATE_DIR="$STATE_DIR" \
        "$executable" "$@"
}

end_maintenance() {
    executable=$1
    if [ "$maintenance_started" -eq 1 ]; then
        if ! run_weaver "$executable" maintenance end >/dev/null; then
            return 1
        fi
        maintenance_started=0
    fi
}

cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    set +e
    if [ "$status" -ne 0 ] && [ "$committed" -eq 0 ]; then
        if [ "$switched" -eq 1 ]; then
            if [ -n "$old_current" ]; then
                atomic_symlink "$old_current" "$CURRENT_LINK"
            else
                rm -f "$CURRENT_LINK"
            fi
            if [ -n "$old_previous" ]; then
                atomic_symlink "$old_previous" "$PREVIOUS_LINK"
            else
                rm -f "$PREVIOUS_LINK"
            fi
            if [ -n "$old_cli" ]; then
                atomic_symlink "$old_cli" "$CLI_PATH"
            else
                rm -f "$CLI_PATH"
            fi
        fi
        if [ "$chancery_provider_switched" -eq 1 ]; then
            if [ -n "$old_chancery_provider" ]; then
                atomic_symlink "$old_chancery_provider" "$CHANCERY_PROVIDER_LINK"
            else
                rm -f "$CHANCERY_PROVIDER_LINK"
            fi
        fi
        if [ "$prototype_plist_removed" -eq 1 ] && [ "$old_plist" -eq 1 ]; then
            install -m 0644 "$transaction_dir/$SERVICE_LABEL.plist" "$AGENT_PLIST"
            prototype_plist_removed=0
        fi
        maintenance_end_cli=$binary_path
        if [ -n "$old_current" ] && [ -x "$INSTALL_DIR/$old_current/bin/weaver" ]; then
            maintenance_end_cli="$INSTALL_DIR/$old_current/bin/weaver"
        fi
        if [ "$maintenance_started" -eq 1 ]; then
            run_weaver "$maintenance_end_cli" maintenance end >/dev/null 2>&1 \
                || rm -f "$MAINTENANCE_MARKER"
            maintenance_started=0
        fi
        if [ "$launchd_changed" -eq 1 ]; then
            "$launchctl_path" enable "$SERVICE_TARGET" >/dev/null 2>&1 || true
        fi
        if [ "$was_loaded" -eq 1 ] && [ -f "$AGENT_PLIST" ]; then
            if ! "$launchctl_path" print "$SERVICE_TARGET" >/dev/null 2>&1; then
                "$launchctl_path" bootstrap "$SERVICE_DOMAIN" "$AGENT_PLIST" \
                    >/dev/null 2>&1 || true
            fi
            "$launchctl_path" kickstart "$SERVICE_TARGET" >/dev/null 2>&1 || true
        fi
    fi
    [ -z "$temporary_release" ] || rm -rf "$temporary_release"
    [ -z "$transaction_dir" ] || rm -rf "$transaction_dir"
    [ "$lock_created" -eq 0 ] || rmdir "$UPDATE_LOCK" >/dev/null 2>&1 || true
    exit "$status"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

if ! mkdir "$UPDATE_LOCK" 2>/dev/null; then
    fail "another Weaver deployment is active: $UPDATE_LOCK"
fi
lock_created=1

if [ -L "$MAINTENANCE_MARKER" ] \
    || { [ -e "$MAINTENANCE_MARKER" ] && [ ! -f "$MAINTENANCE_MARKER" ]; }
then
    fail "invalid maintenance marker: $MAINTENANCE_MARKER"
fi
if [ -e "$MAINTENANCE_MARKER" ]; then
    fail "Weaver is already under maintenance: $MAINTENANCE_MARKER"
fi
if [ -L "$AGENT_PLIST" ] \
    || { [ -e "$AGENT_PLIST" ] && [ ! -f "$AGENT_PLIST" ]; }
then
    fail "invalid prototype LaunchAgent path: $AGENT_PLIST"
fi
if [ -f "$AGENT_PLIST" ]; then
    old_plist=1
fi
if [ -L "$CURRENT_LINK" ]; then
    old_current=$(readlink "$CURRENT_LINK")
elif [ -e "$CURRENT_LINK" ]; then
    fail "$CURRENT_LINK must be a symbolic link"
fi
if [ -L "$PREVIOUS_LINK" ]; then
    old_previous=$(readlink "$PREVIOUS_LINK")
elif [ -e "$PREVIOUS_LINK" ]; then
    fail "$PREVIOUS_LINK must be a symbolic link"
fi
if [ -L "$CLI_PATH" ]; then
    old_cli=$(readlink "$CLI_PATH")
elif [ -e "$CLI_PATH" ]; then
    fail "$CLI_PATH exists and is not a symbolic link"
fi
if [ -L "$CHANCERY_PROVIDER_LINK" ]; then
    old_chancery_provider=$(readlink "$CHANCERY_PROVIDER_LINK")
elif [ -e "$CHANCERY_PROVIDER_LINK" ]; then
    fail "$CHANCERY_PROVIDER_LINK exists and is not a symbolic link"
fi
if [ -n "$old_chancery_provider" ] \
    && [ "$old_chancery_provider" != "$CHANCERY_PROVIDER_TARGET" ]
then
    fail "Chancery provider selector is not owned by this Weaver installation: $CHANCERY_PROVIDER_LINK"
fi
if [ -n "$old_current" ]; then
    case "$old_current" in
        releases/*) ;;
        *) fail "current release selector is invalid: $old_current" ;;
    esac
    old_binary="$INSTALL_DIR/$old_current/bin/weaver"
    [ -f "$old_binary" ] && [ ! -L "$old_binary" ] && [ -x "$old_binary" ] \
        || fail "current release contains no valid Weaver binary: $old_current"
    [ "$old_cli" = "$INSTALL_DIR/current/bin/weaver" ] \
        || fail "installed Weaver command does not select the current release: $CLI_PATH"
elif [ -n "$old_cli" ]; then
    fail "installed Weaver command has no current release: $CLI_PATH"
fi

transaction_dir=$(mktemp -d "$INSTALL_DIR/.transaction.XXXXXX")
if [ "$old_plist" -eq 1 ]; then
    install -m 0644 "$AGENT_PLIST" "$transaction_dir/$SERVICE_LABEL.plist"
fi

binary_hash=$(shasum -a 256 "$binary_path" | awk '{print $1}')
deployer_hash=$(shasum -a 256 "$SOURCE_DEPLOYER" | awk '{print $1}')
chancery_hash=$(chancery_bundle_hash "$SOURCE_CHANCERY")
release_id=$(printf '%s\n' "$binary_hash" "$deployer_hash" "$chancery_hash" \
    | shasum -a 256 | awk '{print $1}')
release_path="$RELEASES_DIR/$release_id"

if [ -L "$release_path" ]; then
    fail "existing release must not be a symbolic link: $release_id"
elif [ -e "$release_path" ] && [ ! -d "$release_path" ]; then
    fail "existing release is not a directory: $release_id"
fi
if [ -d "$release_path" ]; then
    for shipped_file in \
        "$release_path/bin/weaver" \
        "$release_path/package/deploy-user.sh"
    do
        [ -f "$shipped_file" ] && [ ! -L "$shipped_file" ] && [ -x "$shipped_file" ] \
            || fail "existing release contains an invalid executable: $shipped_file"
    done
    [ -f "$release_path/manifest.txt" ] && [ ! -L "$release_path/manifest.txt" ] \
        || fail "existing release contains an invalid manifest: $release_id"
    validate_chancery_bundle "$release_path/share/chancery/weaver"
    [ "$(shasum -a 256 "$release_path/bin/weaver" | awk '{print $1}')" = \
        "$binary_hash" ] \
        || fail "existing release binary hash is invalid: $release_id"
    [ "$(shasum -a 256 "$release_path/package/deploy-user.sh" | awk '{print $1}')" = \
        "$deployer_hash" ] \
        || fail "existing release deployer hash is invalid: $release_id"
    [ "$(chancery_bundle_hash "$release_path/share/chancery/weaver")" = \
        "$chancery_hash" ] \
        || fail "existing release Chancery bundle is invalid: $release_id"
    grep -Fx 'format=3' "$release_path/manifest.txt" >/dev/null \
        || fail "existing release manifest is invalid: $release_id"
    grep -Fx "release_id=$release_id" "$release_path/manifest.txt" >/dev/null \
        || fail "existing release manifest is invalid: $release_id"
    grep -Fx "binary_sha256=$binary_hash" "$release_path/manifest.txt" >/dev/null \
        || fail "existing release manifest is invalid: $release_id"
    grep -Fx "deployer_sha256=$deployer_hash" "$release_path/manifest.txt" >/dev/null \
        || fail "existing release manifest is invalid: $release_id"
    grep -Fx "chancery_sha256=$chancery_hash" "$release_path/manifest.txt" >/dev/null \
        || fail "existing release manifest is invalid: $release_id"
    grep -Fx "version=$version" "$release_path/manifest.txt" >/dev/null \
        || fail "existing release manifest is invalid: $release_id"
else
    temporary_release=$(mktemp -d "$RELEASES_DIR/.stage.XXXXXX")
    install -d -m 0755 "$temporary_release/bin" "$temporary_release/package" \
        "$temporary_release/share" "$temporary_release/share/chancery"
    install -m 0755 "$binary_path" "$temporary_release/bin/weaver"
    install -m 0755 "$SOURCE_DEPLOYER" \
        "$temporary_release/package/deploy-user.sh"
    cp -R "$SOURCE_CHANCERY" "$temporary_release/share/chancery/weaver"
    {
        printf '%s\n' 'format=3'
        printf 'release_id=%s\n' "$release_id"
        printf 'version=%s\n' "$version"
        printf 'binary_sha256=%s\n' "$binary_hash"
        printf 'deployer_sha256=%s\n' "$deployer_hash"
        printf 'chancery_sha256=%s\n' "$chancery_hash"
    } >"$temporary_release/manifest.txt"
    chmod 0444 "$temporary_release/manifest.txt"
    mv "$temporary_release" "$release_path"
    temporary_release=
fi

if "$launchctl_path" print "$SERVICE_TARGET" >/dev/null 2>&1; then
    was_loaded=1
    [ "$old_plist" -eq 1 ] \
        || fail "loaded prototype service has no recoverable plist at $AGENT_PLIST"
fi

maintenance_cli=$binary_path
if [ -n "$old_current" ]; then
    maintenance_cli="$INSTALL_DIR/$old_current/bin/weaver"
fi
maintenance_started=1
run_weaver "$maintenance_cli" maintenance begin --wait-seconds "$wait_seconds" \
    >/dev/null \
    || fail "active workflow did not settle within $wait_seconds seconds"

launchd_changed=1
"$launchctl_path" disable "$SERVICE_TARGET" >/dev/null 2>&1 || true
if [ "$was_loaded" -eq 1 ]; then
    "$launchctl_path" bootout --wait "$SERVICE_TARGET" >/dev/null \
        || fail "unable to stop the prototype $SERVICE_LABEL service"
fi
if [ "$old_plist" -eq 1 ]; then
    prototype_plist_removed=1
    rm -f "$AGENT_PLIST"
fi

switched=1
if [ -n "$old_current" ] && [ "$old_current" != "releases/$release_id" ]; then
    atomic_symlink "$old_current" "$PREVIOUS_LINK"
fi
atomic_symlink "releases/$release_id" "$CURRENT_LINK"
atomic_symlink "$INSTALL_DIR/current/bin/weaver" "$CLI_PATH"
chancery_provider_switched=1
atomic_symlink "$CHANCERY_PROVIDER_TARGET" "$CHANCERY_PROVIDER_LINK"

run_weaver "$CLI_PATH" --version >/dev/null
[ -f "$CHANCERY_PROVIDER_LINK/provider.json" ] \
    || fail 'installed Weaver Chancery provider is unavailable'
run_weaver "$CLI_PATH" doctor >/dev/null \
    || fail 'installed Weaver could not verify Nucleus readiness'
run_weaver "$CLI_PATH" worker run >/dev/null \
    || fail 'installed Weaver did not stop cleanly for maintenance'
if "$launchctl_path" print "$SERVICE_TARGET" >/dev/null 2>&1; then
    fail "prototype $SERVICE_LABEL service remained loaded after removal"
fi
"$launchctl_path" enable "$SERVICE_TARGET" >/dev/null 2>&1 || true

committed=1
end_maintenance "$CLI_PATH" \
    || fail "installation committed but maintenance could not end; run WEAVER_STATE_DIR='$STATE_DIR' '$CLI_PATH' maintenance end"

printf 'Installed Weaver release %s\n' "$release_id"
printf 'Command: %s\n' "$CLI_PATH"
printf 'State:   %s\n' "$STATE_DIR"
printf 'Chancery provider: %s\n' "$CHANCERY_PROVIDER_LINK"
if [ "$old_plist" -eq 1 ] || [ "$was_loaded" -eq 1 ]; then
    printf 'Removed prototype service: %s\n' "$SERVICE_TARGET"
fi
