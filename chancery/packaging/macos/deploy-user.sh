#!/bin/sh

set -eu

PATH=/usr/bin:/bin:/usr/sbin:/sbin
export PATH
umask 077

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
SOURCE_DEPLOYER="$SCRIPT_DIR/deploy-user.sh"
if [ -d "$SCRIPT_DIR/../../provider" ] && [ ! -L "$SCRIPT_DIR/../../provider" ]; then
    SOURCE_PROVIDER=$(CDPATH='' cd "$SCRIPT_DIR/../../provider" && pwd)
elif [ -d "$SCRIPT_DIR/../share/chancery" ] \
    && [ ! -L "$SCRIPT_DIR/../share/chancery" ]; then
    SOURCE_PROVIDER=$(CDPATH='' cd "$SCRIPT_DIR/../share/chancery" && pwd)
else
    printf '%s\n' 'chancery user deploy: Chancery provider bundle is missing' >&2
    exit 1
fi

binary_path=
install_home=${HOME:-}

usage() {
    cat <<'EOF'
Usage: deploy-user.sh --binary ABSOLUTE_PATH [OPTIONS]

Install or update the current user's Chancery CLI. Chancery has no service,
database, authentication, model, network, or Nucleus dependency.

Options:
  --home ABSOLUTE_PATH  Override the operator home (primarily for tests)
EOF
}

fail() {
    printf 'chancery user deploy: %s\n' "$*" >&2
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
    || fail "Chancery candidate is not an executable regular file: $binary_path"
[ -f "$SOURCE_DEPLOYER" ] && [ ! -L "$SOURCE_DEPLOYER" ] \
    || fail "missing packaged file: $SOURCE_DEPLOYER"
for command in awk cat cp find grep id install mktemp mv readlink sed shasum sort stat; do
    command -v "$command" >/dev/null 2>&1 \
        || fail "required command not found: $command"
done
[ -x /usr/bin/shlock ] || fail 'required command not found: /usr/bin/shlock'

operator_uid=$(id -u)
[ "$operator_uid" -ne 0 ] || fail 'run this deployer as the Chancery operator, not root'
if operator_home_uid=$(stat -f '%u' "$install_home" 2>/dev/null); then
    :
elif operator_home_uid=$(stat -c '%u' "$install_home" 2>/dev/null); then
    :
else
    fail "unable to inspect operator home ownership: $install_home"
fi
[ "$operator_home_uid" = "$operator_uid" ] \
    || fail "operator home is not owned by uid $operator_uid: $install_home"

candidate_version=$("$binary_path" --version) \
    || fail 'unable to read the Chancery candidate version'
case "$candidate_version" in
    'chancery '*) version=${candidate_version#chancery } ;;
    *) fail "Chancery candidate reported an unexpected version: $candidate_version" ;;
esac
[ -n "$version" ] || fail 'Chancery candidate reported an empty version'
provider_release=$(awk -F '"' '/"release"[[:space:]]*:/ { print $4; exit }' \
    "$SOURCE_PROVIDER/provider.json")
[ "$provider_release" = "$version" ] \
    || fail "Chancery provider release $provider_release does not match candidate $version"
"$binary_path" --help >/dev/null \
    || fail 'unable to read the Chancery candidate help'
"$binary_path" validate "$SOURCE_PROVIDER" >/dev/null \
    || fail 'candidate rejected the Chancery provider bundle'
sh -n "$SOURCE_DEPLOYER"

STATE_DIR="$install_home/Library/Application Support/Chancery"
PROVIDERS_DIR="$STATE_DIR/providers"
INSTALL_DIR="$STATE_DIR/install"
RELEASES_DIR="$INSTALL_DIR/releases"
CURRENT_LINK="$INSTALL_DIR/current"
PREVIOUS_LINK="$INSTALL_DIR/previous"
UPDATE_LOCK="$INSTALL_DIR/.update-lock"
CATALOG_LOCK="$STATE_DIR/.catalog-update-lock"
CLI_DIR="$install_home/.local/bin"
CLI_PATH="$CLI_DIR/chancery"
PROVIDER_LINK="$PROVIDERS_DIR/chancery"

for path in "$STATE_DIR" "$PROVIDERS_DIR" "$INSTALL_DIR" "$RELEASES_DIR"; do
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
catalog_lock_created=0

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

release_catalog_lock() {
    if [ "$catalog_lock_created" -eq 1 ] && [ -f "$CATALOG_LOCK" ] \
        && [ ! -L "$CATALOG_LOCK" ] \
        && [ "$(sed -n '1p' "$CATALOG_LOCK" 2>/dev/null || true)" = "$$" ]
    then
        rm -f "$CATALOG_LOCK" >/dev/null 2>&1 || true
    fi
}

acquire_catalog_lock() {
    [ ! -L "$CATALOG_LOCK" ] \
        || fail "Chancery catalog writer lock is a symbolic link: $CATALOG_LOCK"
    if [ -e "$CATALOG_LOCK" ] && [ ! -f "$CATALOG_LOCK" ]; then
        fail "Chancery catalog writer lock is not safely recoverable: $CATALOG_LOCK"
    fi
    catalog_lock_created=1
    /usr/bin/shlock -p "$$" -f "$CATALOG_LOCK" \
        || fail "another Chancery catalog writer is active: $CATALOG_LOCK"
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
            atomic_symlink "$old_provider" "$PROVIDER_LINK"
        else
            rm -f "$PROVIDER_LINK"
        fi
    fi
    [ -z "$temporary_release" ] || rm -rf "$temporary_release"
    release_catalog_lock
    [ "$lock_created" -eq 0 ] || rmdir "$UPDATE_LOCK" >/dev/null 2>&1 || true
    exit "$status"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

mkdir "$UPDATE_LOCK" 2>/dev/null \
    || fail "another Chancery deployment is active: $UPDATE_LOCK"
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
if [ -L "$PROVIDER_LINK" ]; then
    old_provider=$(readlink "$PROVIDER_LINK")
elif [ -e "$PROVIDER_LINK" ]; then
    fail "$PROVIDER_LINK exists and is not a symbolic link"
fi
if [ -n "$old_current" ]; then
    case "$old_current" in releases/*) ;; *) fail "current selector is invalid: $old_current" ;; esac
    [ "$old_cli" = "$INSTALL_DIR/current/bin/chancery" ] \
        || fail "installed command does not select the current release: $CLI_PATH"
elif [ -n "$old_cli" ]; then
    fail "installed command has no current release: $CLI_PATH"
fi
expected_provider="$INSTALL_DIR/current/share/chancery"
if [ -n "$old_provider" ] && [ "$old_provider" != "$expected_provider" ]; then
    fail "provider selector is not owned by this Chancery installation: $PROVIDER_LINK"
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

binary_hash=$(shasum -a 256 "$binary_path" | awk '{print $1}')
deployer_hash=$(shasum -a 256 "$SOURCE_DEPLOYER" | awk '{print $1}')
provider_hash=$(hash_tree "$SOURCE_PROVIDER")
release_id=$(printf '%s\n' "$binary_hash" "$deployer_hash" "$provider_hash" \
    | shasum -a 256 | awk '{print $1}')
release_path="$RELEASES_DIR/$release_id"

write_manifest() {
    printf '%s\n' 'format=1'
    printf 'release_id=%s\n' "$release_id"
    printf 'version=%s\n' "$version"
    printf 'binary_sha256=%s\n' "$binary_hash"
    printf 'deployer_sha256=%s\n' "$deployer_hash"
    printf 'provider_sha256=%s\n' "$provider_hash"
}

if [ -L "$release_path" ]; then
    fail "existing release must not be a symbolic link: $release_id"
elif [ -e "$release_path" ] && [ ! -d "$release_path" ]; then
    fail "existing release is not a directory: $release_id"
fi
if [ -d "$release_path" ]; then
    [ -f "$release_path/bin/chancery" ] \
        && [ ! -L "$release_path/bin/chancery" ] \
        && [ -x "$release_path/bin/chancery" ] \
        || fail "existing release contains no valid Chancery binary: $release_id"
    [ -f "$release_path/package/deploy-user.sh" ] \
        && [ ! -L "$release_path/package/deploy-user.sh" ] \
        && [ -x "$release_path/package/deploy-user.sh" ] \
        || fail "existing release contains no valid deployer: $release_id"
    [ -f "$release_path/manifest.txt" ] && [ ! -L "$release_path/manifest.txt" ] \
        || fail "existing release contains no valid manifest: $release_id"
    [ "$(cat "$release_path/manifest.txt")" = "$(write_manifest)" ] \
        || fail "existing release manifest is invalid: $release_id"
    [ "$(shasum -a 256 "$release_path/bin/chancery" | awk '{print $1}')" = "$binary_hash" ] \
        || fail "existing release binary hash is invalid: $release_id"
    [ "$(shasum -a 256 "$release_path/package/deploy-user.sh" | awk '{print $1}')" = "$deployer_hash" ] \
        || fail "existing release deployer hash is invalid: $release_id"
    [ -d "$release_path/share/chancery" ] \
        && [ ! -L "$release_path/share/chancery" ] \
        || fail "existing release contains no valid provider bundle: $release_id"
    [ "$(hash_tree "$release_path/share/chancery")" = "$provider_hash" ] \
        || fail "existing release provider hash is invalid: $release_id"
else
    temporary_release=$(mktemp -d "$RELEASES_DIR/.stage.XXXXXX")
    install -d -m 0755 \
        "$temporary_release/bin" \
        "$temporary_release/package" \
        "$temporary_release/share"
    install -m 0755 "$binary_path" "$temporary_release/bin/chancery"
    install -m 0755 "$SOURCE_DEPLOYER" "$temporary_release/package/deploy-user.sh"
    cp -R "$SOURCE_PROVIDER" "$temporary_release/share/chancery"
    find "$temporary_release/share/chancery" -type d -exec chmod 0755 {} \;
    find "$temporary_release/share/chancery" -type f -exec chmod 0444 {} \;
    write_manifest >"$temporary_release/manifest.txt"
    chmod 0444 "$temporary_release/manifest.txt"
    mv "$temporary_release" "$release_path"
    temporary_release=
fi

acquire_catalog_lock
switched=1
if [ -n "$old_current" ] && [ "$old_current" != "releases/$release_id" ]; then
    atomic_symlink "$old_current" "$PREVIOUS_LINK"
fi
atomic_symlink "releases/$release_id" "$CURRENT_LINK"
atomic_symlink "$INSTALL_DIR/current/bin/chancery" "$CLI_PATH"
atomic_symlink "$expected_provider" "$PROVIDER_LINK"

HOME="$install_home" "$CLI_PATH" --version >/dev/null \
    || fail 'installed Chancery version check failed'
HOME="$install_home" "$CLI_PATH" --help >/dev/null \
    || fail 'installed Chancery help check failed'
HOME="$install_home" "$CLI_PATH" validate "$PROVIDER_LINK" >/dev/null \
    || fail 'installed Chancery provider validation failed'

committed=1
printf 'Installed Chancery release %s\n' "$release_id"
printf 'Command:  %s\n' "$CLI_PATH"
printf 'Registry: %s\n' "$PROVIDERS_DIR"
