#!/bin/sh

set -eu

PATH=/usr/bin:/bin:/usr/sbin:/sbin
export PATH
umask 077

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
SOURCE_DEPLOYER="$SCRIPT_DIR/deploy-user.sh"
SOURCE_FRONTEND="$SCRIPT_DIR/email"
if [ -d "$SCRIPT_DIR/../share/chancery/email" ]; then
    SOURCE_CHANCERY="$SCRIPT_DIR/../share/chancery/email"
else
    SOURCE_CHANCERY="$SCRIPT_DIR/../../chancery"
fi

binary_path=
install_home=${HOME:-}

usage() {
    cat <<'EOF'
Usage: deploy-user.sh --binary ABSOLUTE_PATH [OPTIONS]

Install or update the current user's macOS Email CLI. Email has no daemon,
configuration, database, or Nucleus dependency.

Options:
  --home ABSOLUTE_PATH  Override the operator home (primarily for tests)
EOF
}

fail() {
    printf 'email user deploy: %s\n' "$*" >&2
    exit 1
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
        -h|--help)
            usage
            exit 0
            ;;
        *) fail "unknown argument: $1" ;;
    esac
done

[ -n "$binary_path" ] || fail '--binary is required'
[ -n "$install_home" ] || fail '--home is required'
case "$binary_path" in /*) ;; *) fail 'binary path must be absolute' ;; esac
case "$install_home" in /*) ;; *) fail 'install home must be absolute' ;; esac

[ -d "$install_home" ] && [ ! -L "$install_home" ] \
    || fail "operator home is not a regular directory: $install_home"
[ -f "$binary_path" ] && [ ! -L "$binary_path" ] && [ -x "$binary_path" ] \
    || fail "Email candidate is not an executable regular file: $binary_path"
[ -f "$SOURCE_DEPLOYER" ] && [ ! -L "$SOURCE_DEPLOYER" ] \
    || fail "missing packaged file: $SOURCE_DEPLOYER"
[ -f "$SOURCE_FRONTEND" ] && [ ! -L "$SOURCE_FRONTEND" ] \
    || fail "missing packaged file: $SOURCE_FRONTEND"
[ -x /bin/zsh ] || fail '/bin/zsh is unavailable'
for command in awk cat cp find grep id install mktemp mv readlink shasum sort stat; do
    command -v "$command" >/dev/null 2>&1 \
        || fail "required command not found: $command"
done

operator_uid=$(id -u)
[ "$operator_uid" -ne 0 ] || fail 'run this deployer as the Email operator, not root'
if operator_home_uid=$(stat -f '%u' "$install_home" 2>/dev/null); then
    :
elif operator_home_uid=$(stat -c '%u' "$install_home" 2>/dev/null); then
    :
else
    fail "unable to inspect operator home ownership: $install_home"
fi
[ "$operator_home_uid" = "$operator_uid" ] \
    || fail "operator home is not owned by uid $operator_uid: $install_home"

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
validate_chancery_bundle "$SOURCE_CHANCERY"

candidate_version=$("$binary_path" --version) \
    || fail 'unable to read the Email candidate version'
case "$candidate_version" in
    'email '*) version=${candidate_version#email } ;;
    *) fail "Email candidate reported an unexpected version: $candidate_version" ;;
esac
[ -n "$version" ] || fail 'Email candidate reported an empty version'
provider_release=$(awk -F '"' '/"release"[[:space:]]*:/ { print $4; exit }' \
    "$SOURCE_CHANCERY/provider.json")
[ "$provider_release" = "$version" ] \
    || fail "Chancery provider release $provider_release does not match candidate $version"
"$binary_path" --help >/dev/null \
    || fail 'unable to read the Email candidate help'
sh -n "$SOURCE_DEPLOYER"
/bin/zsh -n "$SOURCE_FRONTEND"

STATE_DIR="$install_home/Library/Application Support/Email"
INSTALL_DIR="$STATE_DIR/install"
RELEASES_DIR="$INSTALL_DIR/releases"
CURRENT_LINK="$INSTALL_DIR/current"
PREVIOUS_LINK="$INSTALL_DIR/previous"
UPDATE_LOCK="$INSTALL_DIR/.update-lock"
CLI_DIR="$install_home/.local/bin"
CLI_PATH="$CLI_DIR/email"
CHANCERY_STATE_DIR="$install_home/Library/Application Support/Chancery"
CHANCERY_PROVIDERS_DIR="$CHANCERY_STATE_DIR/providers"
CHANCERY_PROVIDER_LINK="$CHANCERY_PROVIDERS_DIR/email"
CHANCERY_PROVIDER_TARGET="$INSTALL_DIR/current/share/chancery/email"

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

temporary_release=
old_current=
old_previous=
old_cli=
old_provider=
switched=0
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

cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    set +e
    if [ "$status" -ne 0 ] && [ "$committed" -eq 0 ] && [ "$switched" -eq 1 ]; then
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
        if [ -n "$old_provider" ]; then
            atomic_symlink "$old_provider" "$CHANCERY_PROVIDER_LINK"
        else
            rm -f "$CHANCERY_PROVIDER_LINK"
        fi
    fi
    [ -z "$temporary_release" ] || rm -rf "$temporary_release"
    [ "$lock_created" -eq 0 ] || rmdir "$UPDATE_LOCK" >/dev/null 2>&1 || true
    exit "$status"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

mkdir "$UPDATE_LOCK" 2>/dev/null \
    || fail "another Email deployment is active: $UPDATE_LOCK"
lock_created=1

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
    old_provider=$(readlink "$CHANCERY_PROVIDER_LINK")
elif [ -e "$CHANCERY_PROVIDER_LINK" ]; then
    fail "$CHANCERY_PROVIDER_LINK exists and is not a symbolic link"
fi
if [ -n "$old_current" ]; then
    case "$old_current" in releases/*) ;; *) fail "current selector is invalid: $old_current" ;; esac
    [ "$old_cli" = "$INSTALL_DIR/current/bin/email" ] \
        || fail "installed command does not select the current release: $CLI_PATH"
elif [ -n "$old_cli" ] || [ -n "$old_previous" ]; then
    fail 'installed selectors have no current Email release'
fi
if [ -n "$old_provider" ] && [ "$old_provider" != "$CHANCERY_PROVIDER_TARGET" ]; then
    fail "provider selector is not owned by this Email installation: $CHANCERY_PROVIDER_LINK"
fi

hash_tree() {
    tree=$1
    if find "$tree" ! -type d ! -type f -print | grep -q .; then
        fail "provider bundle contains a non-regular object: $tree"
    fi
    (
        cd "$tree"
        find . -type f -print | LC_ALL=C sort | while IFS= read -r relative; do
            printf '%s  %s\n' \
                "$(shasum -a 256 "$relative" | awk '{print $1}')" "$relative"
        done
    ) | shasum -a 256 | awk '{print $1}'
}

payload_hash=$(shasum -a 256 "$binary_path" | awk '{print $1}')
frontend_hash=$(shasum -a 256 "$SOURCE_FRONTEND" | awk '{print $1}')
deployer_hash=$(shasum -a 256 "$SOURCE_DEPLOYER" | awk '{print $1}')
provider_hash=$(hash_tree "$SOURCE_CHANCERY")
release_id=$(printf '%s\n' \
    "$payload_hash" "$frontend_hash" "$deployer_hash" "$provider_hash" \
    | shasum -a 256 | awk '{print $1}')
release_path="$RELEASES_DIR/$release_id"

write_manifest() {
    printf '%s\n' 'format=1'
    printf 'release_id=%s\n' "$release_id"
    printf 'version=%s\n' "$version"
    printf 'payload_sha256=%s\n' "$payload_hash"
    printf 'frontend_sha256=%s\n' "$frontend_hash"
    printf 'deployer_sha256=%s\n' "$deployer_hash"
    printf 'provider_sha256=%s\n' "$provider_hash"
}

if [ -L "$release_path" ]; then
    fail "existing release must not be a symbolic link: $release_id"
elif [ -e "$release_path" ] && [ ! -d "$release_path" ]; then
    fail "existing release is not a directory: $release_id"
fi
if [ -d "$release_path" ]; then
    [ -x "$release_path/bin/email" ] && [ ! -L "$release_path/bin/email" ] \
        || fail "existing release contains no valid Email wrapper: $release_id"
    [ -x "$release_path/libexec/email" ] && [ ! -L "$release_path/libexec/email" ] \
        || fail "existing release contains no valid Email payload: $release_id"
    [ -x "$release_path/package/email" ] && [ ! -L "$release_path/package/email" ] \
        || fail "existing release contains no valid packaged wrapper: $release_id"
    [ -x "$release_path/package/deploy-user.sh" ] \
        && [ ! -L "$release_path/package/deploy-user.sh" ] \
        || fail "existing release contains no valid deployer: $release_id"
    [ -f "$release_path/manifest.txt" ] && [ ! -L "$release_path/manifest.txt" ] \
        || fail "existing release contains no valid manifest: $release_id"
    [ "$(cat "$release_path/manifest.txt")" = "$(write_manifest)" ] \
        || fail "existing release manifest is invalid: $release_id"
    [ "$(shasum -a 256 "$release_path/libexec/email" | awk '{print $1}')" = "$payload_hash" ] \
        || fail "existing release payload hash is invalid: $release_id"
    [ "$(shasum -a 256 "$release_path/bin/email" | awk '{print $1}')" = "$frontend_hash" ] \
        || fail "existing release wrapper hash is invalid: $release_id"
    [ "$(shasum -a 256 "$release_path/package/email" | awk '{print $1}')" = "$frontend_hash" ] \
        || fail "existing release packaged wrapper hash is invalid: $release_id"
    [ "$(shasum -a 256 "$release_path/package/deploy-user.sh" | awk '{print $1}')" = "$deployer_hash" ] \
        || fail "existing release deployer hash is invalid: $release_id"
    [ -d "$release_path/share/chancery/email" ] \
        && [ ! -L "$release_path/share/chancery/email" ] \
        || fail "existing release contains no valid provider bundle: $release_id"
    [ "$(hash_tree "$release_path/share/chancery/email")" = "$provider_hash" ] \
        || fail "existing release provider hash is invalid: $release_id"
else
    temporary_release=$(mktemp -d "$RELEASES_DIR/.stage.XXXXXX")
    install -d -m 0755 \
        "$temporary_release/bin" \
        "$temporary_release/libexec" \
        "$temporary_release/package" \
        "$temporary_release/share/chancery"
    install -m 0755 "$SOURCE_FRONTEND" "$temporary_release/bin/email"
    install -m 0755 "$binary_path" "$temporary_release/libexec/email"
    install -m 0755 "$SOURCE_DEPLOYER" "$temporary_release/package/deploy-user.sh"
    install -m 0755 "$SOURCE_FRONTEND" "$temporary_release/package/email"
    cp -R "$SOURCE_CHANCERY" "$temporary_release/share/chancery/email"
    find "$temporary_release/share/chancery/email" -type d -exec chmod 0755 {} \;
    find "$temporary_release/share/chancery/email" -type f -exec chmod 0444 {} \;
    write_manifest >"$temporary_release/manifest.txt"
    chmod 0444 "$temporary_release/manifest.txt"
    mv "$temporary_release" "$release_path"
    temporary_release=
fi

switched=1
if [ -n "$old_current" ] && [ "$old_current" != "releases/$release_id" ]; then
    atomic_symlink "$old_current" "$PREVIOUS_LINK"
fi
atomic_symlink "releases/$release_id" "$CURRENT_LINK"
atomic_symlink "$INSTALL_DIR/current/bin/email" "$CLI_PATH"
atomic_symlink "$CHANCERY_PROVIDER_TARGET" "$CHANCERY_PROVIDER_LINK"

HOME="$install_home" "$CLI_PATH" --version >/dev/null \
    || fail 'installed Email version check failed'
HOME="$install_home" "$CLI_PATH" --help >/dev/null \
    || fail 'installed Email help check failed'

committed=1
printf 'Installed Email release %s\n' "$release_id"
printf 'Command:  %s\n' "$CLI_PATH"
printf 'Provider: %s\n' "$CHANCERY_PROVIDER_LINK"
