#!/bin/sh

set -eu

umask 077

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
SOURCE_DEPLOYER="$SCRIPT_DIR/deploy-user.sh"
if [ -d "$SCRIPT_DIR/../share/chancery/nucleus" ]; then
    SOURCE_CHANCERY="$SCRIPT_DIR/../share/chancery/nucleus"
else
    SOURCE_CHANCERY="$SCRIPT_DIR/../../chancery"
fi

binary_path=
daemon_path=
codex_path=
codex_home=
install_home=${HOME:-}

usage() {
    cat <<'EOF'
Usage: deploy-user.sh --binary ABSOLUTE_PATH --daemon ABSOLUTE_PATH \
  --codex ABSOLUTE_PATH [OPTIONS]

Install or update the current user's macOS Nucleus service.

Options:
  --codex-home ABSOLUTE_PATH  Import signed-in auth into Nucleus-owned state
  --home ABSOLUTE_PATH        Override the operator home (primarily for tests)
EOF
}

fail() {
    printf 'nucleus user deploy: %s\n' "$*" >&2
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

while [ "$#" -gt 0 ]; do
    case "$1" in
        --binary)
            [ "$#" -ge 2 ] || fail '--binary requires a path'
            binary_path=$2
            shift 2
            ;;
        --daemon)
            [ "$#" -ge 2 ] || fail '--daemon requires a path'
            daemon_path=$2
            shift 2
            ;;
        --codex)
            [ "$#" -ge 2 ] || fail '--codex requires a path'
            codex_path=$2
            shift 2
            ;;
        --codex-home)
            [ "$#" -ge 2 ] || fail '--codex-home requires a path'
            codex_home=$2
            shift 2
            ;;
        --home)
            [ "$#" -ge 2 ] || fail '--home requires a path'
            install_home=$2
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *) fail "unknown argument: $1" ;;
    esac
done

PATH="/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:/opt/homebrew/sbin:/usr/local/bin:$install_home/.local/bin:$install_home/.cargo/bin"
export PATH

require_absolute() {
    option=$1
    value=$2
    [ -n "$value" ] || fail "$option is required"
    case "$value" in
        /*) ;;
        *) fail "$option must be an absolute path" ;;
    esac
}

require_absolute --binary "$binary_path"
require_absolute --daemon "$daemon_path"
require_absolute --codex "$codex_path"
require_absolute --home "$install_home"
if [ -n "$codex_home" ]; then
    require_absolute --codex-home "$codex_home"
fi

[ -d "$install_home" ] && [ ! -L "$install_home" ] \
    || fail "operator home is not a regular directory: $install_home"
operator_uid=$(id -u)
[ "$operator_uid" -ne 0 ] \
    || fail 'run this deployer as the Nucleus operator, not root'
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
    || fail "Nucleus candidate is not an executable regular file: $binary_path"
[ -f "$daemon_path" ] && [ ! -L "$daemon_path" ] && [ -x "$daemon_path" ] \
    || fail "Nucleus daemon candidate is not an executable regular file: $daemon_path"
[ -f "$codex_path" ] && [ -x "$codex_path" ] \
    || fail "Codex executable is unavailable: $codex_path"
if [ -n "$codex_home" ]; then
    [ -d "$codex_home" ] \
        || fail "Codex home is not a directory: $codex_home"
fi
for source in "$SOURCE_DEPLOYER"; do
    [ -f "$source" ] && [ ! -L "$source" ] \
        || fail "missing packaged file: $source"
done
for command in awk cp find grep install mktemp mv readlink shasum sort; do
    command -v "$command" >/dev/null 2>&1 \
        || fail "required command not found: $command"
done
validate_chancery_bundle "$SOURCE_CHANCERY"

binary_version=$("$binary_path" --version) \
    || fail 'unable to read the Nucleus candidate version'
case "$binary_version" in
    'nucleus '*) version=${binary_version#nucleus } ;;
    *) fail "Nucleus candidate reported an unexpected version: $binary_version" ;;
esac
[ -n "$version" ] || fail 'Nucleus candidate reported an empty version'
provider_release=$(awk -F '"' '/"release"[[:space:]]*:/ { print $4; exit }' \
    "$SOURCE_CHANCERY/provider.json")
[ "$provider_release" = "$version" ] \
    || fail "Nucleus provider release $provider_release does not match candidate $version"

daemon_version=$("$daemon_path" --version) \
    || fail 'unable to read the Nucleus daemon candidate version'
[ "$daemon_version" = "nucleusd $version" ] \
    || fail "Nucleus candidate versions do not match: $binary_version; $daemon_version"
"$codex_path" --version >/dev/null \
    || fail 'unable to run the Codex executable'

state_dir="$install_home/Library/Application Support/Nucleus"
update_lock="$state_dir/.deploy-lock"
[ ! -L "$state_dir" ] \
    || fail "Nucleus state directory must not be a symbolic link: $state_dir"
[ ! -e "$state_dir" ] || [ -d "$state_dir" ] \
    || fail "Nucleus state path is not a directory: $state_dir"
install -d -m 0700 "$state_dir"
install_dir="$state_dir/install"
releases_dir="$install_dir/releases"
current_link="$install_dir/current"
previous_link="$install_dir/previous"
chancery_state_dir="$install_home/Library/Application Support/Chancery"
chancery_providers_dir="$chancery_state_dir/providers"
chancery_provider_link="$chancery_providers_dir/nucleus"
chancery_provider_target="$install_dir/current/share/chancery/nucleus"
for path in "$install_dir" "$releases_dir" "$chancery_state_dir" \
    "$chancery_providers_dir"
do
    [ ! -L "$path" ] || fail "refusing symbolic-link directory: $path"
    [ ! -e "$path" ] || [ -d "$path" ] \
        || fail "directory path is occupied by a non-directory: $path"
    install -d -m 0700 "$path"
done

temporary_release=
old_current=
old_previous=
old_chancery_provider=
switched=0
committed=0
lock_created=0
cleanup() {
    deploy_status=$?
    trap - EXIT HUP INT TERM
    set +e
    if [ "$deploy_status" -ne 0 ] && [ "$committed" -eq 0 ] \
        && [ "$switched" -eq 1 ]
    then
        if [ -n "$old_current" ]; then
            atomic_symlink "$old_current" "$current_link"
        else
            rm -f "$current_link"
        fi
        if [ -n "$old_previous" ]; then
            atomic_symlink "$old_previous" "$previous_link"
        else
            rm -f "$previous_link"
        fi
        if [ -n "$old_chancery_provider" ]; then
            atomic_symlink "$old_chancery_provider" "$chancery_provider_link"
        else
            rm -f "$chancery_provider_link"
        fi
    fi
    [ -z "$temporary_release" ] || rm -rf "$temporary_release"
    [ "$lock_created" -eq 0 ] || rmdir "$update_lock" >/dev/null 2>&1 || true
    exit "$deploy_status"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM
if ! mkdir "$update_lock" 2>/dev/null; then
    fail "another deployment holds $update_lock"
fi
lock_created=1

if [ -L "$current_link" ]; then
    old_current=$(readlink "$current_link")
elif [ -e "$current_link" ]; then
    fail "$current_link must be a symbolic link"
fi
if [ -L "$previous_link" ]; then
    old_previous=$(readlink "$previous_link")
elif [ -e "$previous_link" ]; then
    fail "$previous_link must be a symbolic link"
fi
if [ -L "$chancery_provider_link" ]; then
    old_chancery_provider=$(readlink "$chancery_provider_link")
elif [ -e "$chancery_provider_link" ]; then
    fail "$chancery_provider_link exists and is not a symbolic link"
fi
if [ -n "$old_chancery_provider" ] \
    && [ "$old_chancery_provider" != "$chancery_provider_target" ]
then
    fail "Chancery provider selector is not owned by this Nucleus installation: $chancery_provider_link"
fi

binary_hash=$(shasum -a 256 "$binary_path" | awk '{print $1}')
daemon_hash=$(shasum -a 256 "$daemon_path" | awk '{print $1}')
deployer_hash=$(shasum -a 256 "$SOURCE_DEPLOYER" | awk '{print $1}')
chancery_hash=$(chancery_bundle_hash "$SOURCE_CHANCERY")
release_id=$(printf '%s\n' \
    "$binary_hash" "$daemon_hash" "$deployer_hash" "$chancery_hash" \
    | shasum -a 256 | awk '{print $1}')
release_path="$releases_dir/$release_id"

if [ -L "$release_path" ]; then
    fail "existing release must not be a symbolic link: $release_id"
elif [ -e "$release_path" ] && [ ! -d "$release_path" ]; then
    fail "existing release is not a directory: $release_id"
fi
if [ -d "$release_path" ]; then
    for executable in \
        "$release_path/bin/nucleus" \
        "$release_path/libexec/nucleusd" \
        "$release_path/package/deploy-user.sh"
    do
        [ -f "$executable" ] && [ ! -L "$executable" ] && [ -x "$executable" ] \
            || fail "existing release contains an invalid executable: $executable"
    done
    [ "$(shasum -a 256 "$release_path/bin/nucleus" | awk '{print $1}')" = \
        "$binary_hash" ] \
        || fail "existing release CLI hash is invalid: $release_id"
    [ "$(shasum -a 256 "$release_path/libexec/nucleusd" | awk '{print $1}')" = \
        "$daemon_hash" ] \
        || fail "existing release daemon hash is invalid: $release_id"
    [ "$(shasum -a 256 "$release_path/package/deploy-user.sh" | awk '{print $1}')" = \
        "$deployer_hash" ] \
        || fail "existing release deployer hash is invalid: $release_id"
    validate_chancery_bundle "$release_path/share/chancery/nucleus"
    [ "$(chancery_bundle_hash "$release_path/share/chancery/nucleus")" = \
        "$chancery_hash" ] \
        || fail "existing release Chancery bundle is invalid: $release_id"
    grep -Fx "release_id=$release_id" "$release_path/manifest.txt" >/dev/null \
        || fail "existing release manifest is invalid: $release_id"
else
    temporary_release=$(mktemp -d "$releases_dir/.stage.XXXXXX")
    install -d -m 0700 \
        "$temporary_release/bin" \
        "$temporary_release/libexec" \
        "$temporary_release/package" \
        "$temporary_release/share/chancery"
    install -m 0755 "$binary_path" "$temporary_release/bin/nucleus"
    install -m 0755 "$daemon_path" "$temporary_release/libexec/nucleusd"
    install -m 0755 "$SOURCE_DEPLOYER" \
        "$temporary_release/package/deploy-user.sh"
    cp -R "$SOURCE_CHANCERY" "$temporary_release/share/chancery/nucleus"
    {
        printf '%s\n' 'format=1'
        printf 'release_id=%s\n' "$release_id"
        printf 'version=%s\n' "$version"
        printf 'binary_sha256=%s\n' "$binary_hash"
        printf 'daemon_sha256=%s\n' "$daemon_hash"
        printf 'deployer_sha256=%s\n' "$deployer_hash"
        printf 'chancery_bundle_sha256=%s\n' "$chancery_hash"
    } >"$temporary_release/manifest.txt"
    chmod 0400 "$temporary_release/manifest.txt"
    mv "$temporary_release" "$release_path"
    temporary_release=
fi

switched=1
if [ -n "$old_current" ] && [ "$old_current" != "releases/$release_id" ]; then
    atomic_symlink "$old_current" "$previous_link"
fi
atomic_symlink "releases/$release_id" "$current_link"
atomic_symlink "$chancery_provider_target" "$chancery_provider_link"

service_status=0
if [ -n "$codex_home" ]; then
    if HOME=$install_home "$current_link/bin/nucleus" service install \
        --daemon "$current_link/libexec/nucleusd" \
        --codex "$codex_path" \
        --codex-home "$codex_home"
    then
        :
    else
        service_status=$?
    fi
else
    if HOME=$install_home "$current_link/bin/nucleus" service install \
        --daemon "$current_link/libexec/nucleusd" \
        --codex "$codex_path"
    then
        :
    else
        service_status=$?
    fi
fi

if [ "$service_status" -ne 0 ]; then
    installed_cli="$install_home/.local/bin/nucleus"
    installed_daemon="$install_home/.local/libexec/nucleusd"
    if [ -f "$installed_cli" ] && [ ! -L "$installed_cli" ] \
        && [ -f "$installed_daemon" ] && [ ! -L "$installed_daemon" ] \
        && [ "$(shasum -a 256 "$installed_cli" | awk '{print $1}')" = \
            "$binary_hash" ] \
        && [ "$(shasum -a 256 "$installed_daemon" | awk '{print $1}')" = \
            "$daemon_hash" ]
    then
        # The inner installer deliberately keeps candidate binaries when a
        # database-schema cutover makes binary rollback unsafe. Keep their
        # matching packaged release and provider contract selected too.
        committed=1
        printf '%s\n' \
            'nucleus user deploy: candidate binaries remain installed; preserving their matching packaged release and Chancery provider' >&2
    fi
    exit "$service_status"
fi

committed=1
printf 'Nucleus packaged release: %s\n' "$release_id"
printf 'Chancery provider: %s\n' "$chancery_provider_link"
