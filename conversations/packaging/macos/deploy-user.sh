#!/bin/sh

set -eu

PATH=/usr/bin:/bin:/usr/sbin:/sbin
export PATH
umask 077

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
SOURCE_DEPLOYER="$SCRIPT_DIR/deploy-user.sh"
if [ -d "$SCRIPT_DIR/../share/chancery/conversations" ]; then
    SOURCE_CHANCERY="$SCRIPT_DIR/../share/chancery/conversations"
else
    SOURCE_CHANCERY="$SCRIPT_DIR/../../chancery"
fi

binary_path=
install_home=${HOME:-}

usage() {
    cat <<'EOF'
Usage: deploy-user.sh --binary ABSOLUTE_PATH [OPTIONS]

Install or update the current user's macOS Conversations CLI. Conversations
has no daemon, database, authentication, model, or network service of its own.

Options:
  --home ABSOLUTE_PATH  Override the operator home (primarily for tests)
EOF
}

fail() {
    printf 'conversations user deploy: %s\n' "$*" >&2
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
case "$install_home" in /*) ;; *) fail 'home path must be absolute' ;; esac

[ -d "$install_home" ] && [ ! -L "$install_home" ] \
    || fail "operator home is not a regular directory: $install_home"
[ -f "$binary_path" ] && [ ! -L "$binary_path" ] && [ -x "$binary_path" ] \
    || fail "candidate is not an executable regular file: $binary_path"
[ -f "$SOURCE_DEPLOYER" ] && [ ! -L "$SOURCE_DEPLOYER" ] \
    || fail "missing packaged deployer: $SOURCE_DEPLOYER"
for command in awk cp find grep id install mktemp mv readlink shasum sort stat; do
    command -v "$command" >/dev/null 2>&1 \
        || fail "required command not found: $command"
done

operator_uid=$(id -u)
[ "$operator_uid" -ne 0 ] || fail 'run as the Conversations operator, not root'
if home_uid=$(stat -f '%u' "$install_home" 2>/dev/null); then
    :
elif home_uid=$(stat -c '%u' "$install_home" 2>/dev/null); then
    :
else
    fail "unable to inspect home ownership: $install_home"
fi
[ "$home_uid" = "$operator_uid" ] \
    || fail "operator home is not owned by uid $operator_uid: $install_home"

validate_bundle() {
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

validate_release_selector() {
    selector=$1
    printf '%s\n' "$selector" | grep -Eq '^releases/[0-9a-f]{64}$' \
        || fail "invalid release selector: $selector"
}

validate_installed_release() {
    selector=$1
    validate_release_selector "$selector"
    inspected_id=${selector#releases/}
    inspected_path="$INSTALL_DIR/$selector"

    for directory in "$inspected_path" "$inspected_path/bin" \
        "$inspected_path/package" "$inspected_path/share" \
        "$inspected_path/share/chancery"; do
        [ -d "$directory" ] && [ ! -L "$directory" ] \
            || fail "installed release has an invalid directory: $selector"
    done
    [ -f "$inspected_path/bin/conversations" ] \
        && [ ! -L "$inspected_path/bin/conversations" ] \
        && [ -x "$inspected_path/bin/conversations" ] \
        || fail "installed release has an invalid binary: $selector"
    [ -f "$inspected_path/package/deploy-user.sh" ] \
        && [ ! -L "$inspected_path/package/deploy-user.sh" ] \
        && [ -x "$inspected_path/package/deploy-user.sh" ] \
        || fail "installed release has an invalid deployer: $selector"
    [ -f "$inspected_path/manifest.txt" ] \
        && [ ! -L "$inspected_path/manifest.txt" ] \
        || fail "installed release has an invalid manifest: $selector"
    validate_bundle "$inspected_path/share/chancery/conversations"

    inspected_binary_hash=$(shasum -a 256 \
        "$inspected_path/bin/conversations" | awk '{print $1}')
    inspected_deployer_hash=$(shasum -a 256 \
        "$inspected_path/package/deploy-user.sh" | awk '{print $1}')
    inspected_chancery_hash=$(bundle_hash \
        "$inspected_path/share/chancery/conversations")
    inspected_computed_id=$(printf '%s\n' "$inspected_binary_hash" \
        "$inspected_deployer_hash" "$inspected_chancery_hash" \
        | shasum -a 256 | awk '{print $1}')
    [ "$inspected_computed_id" = "$inspected_id" ] \
        || fail "installed release content identity is invalid: $selector"

    inspected_version=$(awk '
        index($0, "version=") == 1 { print substr($0, 9); exit }
    ' "$inspected_path/manifest.txt")
    [ -n "$inspected_version" ] \
        || fail "installed release has no manifest version: $selector"
    inspected_provider_release=$(awk -F '"' \
        '/"release"[[:space:]]*:/ { print $4; exit }' \
        "$inspected_path/share/chancery/conversations/provider.json")
    [ "$inspected_provider_release" = "$inspected_version" ] \
        || fail "installed release provider version is invalid: $selector"
    inspected_manifest_hash=$(shasum -a 256 \
        "$inspected_path/manifest.txt" | awk '{print $1}')
    inspected_expected_manifest_hash=$(
        {
            printf '%s\n' 'format=1'
            printf 'release_id=%s\n' "$inspected_id"
            printf 'version=%s\n' "$inspected_version"
            printf 'binary_sha256=%s\n' "$inspected_binary_hash"
            printf 'deployer_sha256=%s\n' "$inspected_deployer_hash"
            printf 'chancery_sha256=%s\n' "$inspected_chancery_hash"
        } | shasum -a 256 | awk '{print $1}'
    )
    [ "$inspected_manifest_hash" = "$inspected_expected_manifest_hash" ] \
        || fail "installed release manifest is invalid: $selector"
}

validate_bundle "$SOURCE_CHANCERY"
candidate_version=$("$binary_path" --version) \
    || fail 'unable to read the Conversations candidate version'
case "$candidate_version" in
    'conversations '*) version=${candidate_version#conversations } ;;
    *) fail "candidate reported an unexpected version: $candidate_version" ;;
esac
[ -n "$version" ] || fail 'candidate reported an empty version'
provider_release=$(awk -F '"' '/"release"[[:space:]]*:/ { print $4; exit }' \
    "$SOURCE_CHANCERY/provider.json")
[ "$provider_release" = "$version" ] \
    || fail "Chancery provider release $provider_release does not match candidate $version"
"$binary_path" --help >/dev/null || fail 'unable to read candidate help'
sh -n "$SOURCE_DEPLOYER"

STATE_DIR="$install_home/Library/Application Support/Conversations"
INSTALL_DIR="$STATE_DIR/install"
RELEASES_DIR="$INSTALL_DIR/releases"
CURRENT_LINK="$INSTALL_DIR/current"
PREVIOUS_LINK="$INSTALL_DIR/previous"
UPDATE_LOCK="$INSTALL_DIR/.update-lock"
CLI_DIR="$install_home/.local/bin"
CLI_PATH="$CLI_DIR/conversations"
CHANCERY_STATE="$install_home/Library/Application Support/Chancery"
CHANCERY_PROVIDERS="$CHANCERY_STATE/providers"
CHANCERY_LINK="$CHANCERY_PROVIDERS/conversations"
CHANCERY_TARGET="$INSTALL_DIR/current/share/chancery/conversations"

for path in "$STATE_DIR" "$INSTALL_DIR" "$RELEASES_DIR" \
    "$CHANCERY_STATE" "$CHANCERY_PROVIDERS"; do
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
    if [ "$status" -ne 0 ] && [ "$committed" -eq 0 ] \
        && [ "$switched" -eq 1 ]; then
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
            atomic_symlink "$old_provider" "$CHANCERY_LINK"
        else
            rm -f "$CHANCERY_LINK"
        fi
    fi
    [ -z "$temporary_release" ] || rm -rf "$temporary_release"
    [ "$lock_created" -eq 0 ] || rmdir "$UPDATE_LOCK" >/dev/null 2>&1 || true
    exit "$status"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

mkdir "$UPDATE_LOCK" 2>/dev/null \
    || fail "another Conversations deployment is active: $UPDATE_LOCK"
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
if [ -L "$CHANCERY_LINK" ]; then
    old_provider=$(readlink "$CHANCERY_LINK")
elif [ -e "$CHANCERY_LINK" ]; then
    fail "$CHANCERY_LINK exists and is not a symbolic link"
fi
if [ -n "$old_provider" ] && [ "$old_provider" != "$CHANCERY_TARGET" ]; then
    fail "Chancery provider selector is not owned by this installation: $CHANCERY_LINK"
fi
if [ -n "$old_current" ]; then
    validate_installed_release "$old_current"
    [ "$old_cli" = "$INSTALL_DIR/current/bin/conversations" ] \
        || fail "installed command does not select the current release: $CLI_PATH"
    if [ -n "$old_previous" ]; then
        validate_installed_release "$old_previous"
    fi
elif [ -n "$old_cli" ] || [ -n "$old_previous" ] || [ -n "$old_provider" ]; then
    fail 'installed selectors have no current Conversations release'
fi

binary_hash=$(shasum -a 256 "$binary_path" | awk '{print $1}')
deployer_hash=$(shasum -a 256 "$SOURCE_DEPLOYER" | awk '{print $1}')
chancery_hash=$(bundle_hash "$SOURCE_CHANCERY")
release_id=$(printf '%s\n' "$binary_hash" "$deployer_hash" "$chancery_hash" \
    | shasum -a 256 | awk '{print $1}')
release_path="$RELEASES_DIR/$release_id"

if [ -L "$release_path" ]; then
    fail "existing release must not be a symbolic link: $release_id"
elif [ -e "$release_path" ] && [ ! -d "$release_path" ]; then
    fail "existing release is not a directory: $release_id"
fi
if [ -d "$release_path" ]; then
    [ -f "$release_path/bin/conversations" ] \
        && [ ! -L "$release_path/bin/conversations" ] \
        && [ -x "$release_path/bin/conversations" ] \
        || fail "existing release has an invalid binary: $release_id"
    [ -f "$release_path/package/deploy-user.sh" ] \
        && [ ! -L "$release_path/package/deploy-user.sh" ] \
        || fail "existing release has an invalid deployer: $release_id"
    [ -f "$release_path/manifest.txt" ] \
        && [ ! -L "$release_path/manifest.txt" ] \
        || fail "existing release has an invalid manifest: $release_id"
    validate_bundle "$release_path/share/chancery/conversations"
    [ "$(shasum -a 256 "$release_path/bin/conversations" | awk '{print $1}')" \
        = "$binary_hash" ] || fail "existing release binary is tampered: $release_id"
    [ "$(shasum -a 256 "$release_path/package/deploy-user.sh" | awk '{print $1}')" \
        = "$deployer_hash" ] || fail "existing release deployer is tampered: $release_id"
    [ "$(bundle_hash "$release_path/share/chancery/conversations")" \
        = "$chancery_hash" ] || fail "existing release provider is tampered: $release_id"
    grep -Fx "release_id=$release_id" "$release_path/manifest.txt" >/dev/null \
        || fail "existing release manifest is invalid: $release_id"
else
    temporary_release=$(mktemp -d "$RELEASES_DIR/.stage.XXXXXX")
    install -d -m 0755 "$temporary_release/bin" "$temporary_release/package" \
        "$temporary_release/share" "$temporary_release/share/chancery"
    install -m 0755 "$binary_path" "$temporary_release/bin/conversations"
    install -m 0755 "$SOURCE_DEPLOYER" "$temporary_release/package/deploy-user.sh"
    cp -R "$SOURCE_CHANCERY" "$temporary_release/share/chancery/conversations"
    {
        printf '%s\n' 'format=1'
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

switched=1
if [ -n "$old_current" ] && [ "$old_current" != "releases/$release_id" ]; then
    atomic_symlink "$old_current" "$PREVIOUS_LINK"
fi
atomic_symlink "releases/$release_id" "$CURRENT_LINK"
atomic_symlink "$INSTALL_DIR/current/bin/conversations" "$CLI_PATH"
atomic_symlink "$CHANCERY_TARGET" "$CHANCERY_LINK"

HOME="$install_home" "$CLI_PATH" --version >/dev/null \
    || fail 'installed Conversations version check failed'
HOME="$install_home" "$CLI_PATH" --help >/dev/null \
    || fail 'installed Conversations help check failed'
[ -f "$CHANCERY_LINK/provider.json" ] \
    || fail 'installed Conversations Chancery provider is unavailable'

committed=1
printf 'Installed Conversations release %s\n' "$release_id"
printf 'Command: %s\n' "$CLI_PATH"
printf 'State:   %s\n' "$STATE_DIR"
printf 'Chancery provider: %s\n' "$CHANCERY_LINK"
