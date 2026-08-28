#!/bin/sh

set -eu

PATH=/usr/bin:/bin:/usr/sbin:/sbin
export PATH
umask 077

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
SOURCE_FRONTEND="$SCRIPT_DIR/todo"
SOURCE_DEPLOYER="$SCRIPT_DIR/deploy-user.sh"

binary_path=
install_home=${HOME:-}

usage() {
    cat <<'EOF'
Usage: deploy-user.sh --binary ABSOLUTE_PATH [OPTIONS]

Install or update the user-owned macOS Todo CLI. A healthy user-owned Nucleus
service must already be installed.

Options:
  --home ABSOLUTE_PATH  Override the operator home (primarily for tests)
EOF
}

fail() {
    printf 'todo user deploy: %s\n' "$*" >&2
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
case "$binary_path" in
    /*) ;;
    *) fail 'binary_path must be an absolute path' ;;
esac
case "$install_home" in
    /*) ;;
    *) fail 'install_home must be an absolute path' ;;
esac

[ -d "$install_home" ] && [ ! -L "$install_home" ] \
    || fail "operator home is not a regular directory: $install_home"
[ -f "$binary_path" ] && [ ! -L "$binary_path" ] && [ -x "$binary_path" ] \
    || fail "Todo candidate is not an executable regular file: $binary_path"
nucleus_cli="$install_home/.local/bin/nucleus"
[ -f "$nucleus_cli" ] && [ ! -L "$nucleus_cli" ] && [ -x "$nucleus_cli" ] \
    || fail "Nucleus CLI is unavailable: $nucleus_cli"
for source in "$SOURCE_FRONTEND" "$SOURCE_DEPLOYER"; do
    [ -f "$source" ] && [ ! -L "$source" ] \
        || fail "missing packaged file: $source"
done
for command in awk grep install mktemp mv readlink shasum; do
    command -v "$command" >/dev/null 2>&1 \
        || fail "required command not found: $command"
done

STATE_DIR="$install_home/Library/Application Support/Todo"
CONFIG_PATH="$STATE_DIR/config.toml"
DATABASE_PATH="$STATE_DIR/todo.db"
INSTALL_DIR="$STATE_DIR/install"
RELEASES_DIR="$INSTALL_DIR/releases"
CURRENT_LINK="$INSTALL_DIR/current"
PREVIOUS_LINK="$INSTALL_DIR/previous"
UPDATE_LOCK="$INSTALL_DIR/.update-lock"
CLI_DIR="$install_home/.local/bin"
CLI_PATH="$CLI_DIR/todo"

temporary_release=
temporary_config=
transaction_dir=
old_current=
old_previous=
old_cli=
old_config=0
switched=0
config_changed=0
committed=0
lock_created=0
database_was_absent=0

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
        if [ "$config_changed" -eq 1 ]; then
            if [ "$old_config" -eq 1 ]; then
                install -m 0600 "$transaction_dir/config.toml" "$CONFIG_PATH"
            else
                rm -f "$CONFIG_PATH"
            fi
        fi
    fi
    [ -z "$temporary_release" ] || rm -rf "$temporary_release"
    [ -z "$temporary_config" ] || rm -f "$temporary_config"
    [ -z "$transaction_dir" ] || rm -rf "$transaction_dir"
    [ "$lock_created" -eq 0 ] || rmdir "$UPDATE_LOCK" >/dev/null 2>&1 || true
    exit "$status"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

"$binary_path" --version >/dev/null
nucleus_health=$(HOME="$install_home" "$nucleus_cli" --compact health) \
    || fail 'could not read the user-owned Nucleus service health'
case "$nucleus_health" in
    *'"supportedProtocolVersions":[1]'*|*'"supportedProtocolVersions":[1,'*) ;;
    *) fail 'Nucleus is not ready for Todo (protocol v1 is not supported)' ;;
esac
for required_health_field in \
    '"status":"ok"' \
    '"acceptingJobs":true' \
    '"harness":{"harness":"codex"' \
    '"exact-model"' \
    '"reasoning-effort"' \
    '"workspace-read-only"' \
    '"builtin-local-execution"' \
    '"builtin-web-search"' \
    '"dynamic-client-tools"' \
    '"developer-instructions"' \
    '"experimental-raw-events"' \
    '"persistent-file-authentication"' \
    '"authenticated":true'
do
    printf '%s\n' "$nucleus_health" | grep -F "$required_health_field" >/dev/null \
        || fail "Nucleus is not ready for Todo (missing $required_health_field)"
done
sh -n "$SOURCE_FRONTEND"
sh -n "$SOURCE_DEPLOYER"

install -d -m 0700 "$STATE_DIR" "$INSTALL_DIR" "$RELEASES_DIR"
install -d -m 0755 "$CLI_DIR"
if ! mkdir "$UPDATE_LOCK" 2>/dev/null; then
    fail "another update holds $UPDATE_LOCK"
fi
lock_created=1
if [ -L "$DATABASE_PATH" ]; then
    fail "$DATABASE_PATH must not be a symbolic link"
elif [ -e "$DATABASE_PATH" ] && [ ! -f "$DATABASE_PATH" ]; then
    fail "$DATABASE_PATH must be a regular file"
elif [ ! -e "$DATABASE_PATH" ]; then
    database_was_absent=1
fi

transaction_dir=$(mktemp -d "$INSTALL_DIR/.transaction.XXXXXX")
if [ -f "$CONFIG_PATH" ]; then
    install -m 0600 "$CONFIG_PATH" "$transaction_dir/config.toml"
    old_config=1
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

binary_hash=$(shasum -a 256 "$binary_path" | awk '{print $1}')
frontend_hash=$(shasum -a 256 "$SOURCE_FRONTEND" | awk '{print $1}')
deployer_hash=$(shasum -a 256 "$SOURCE_DEPLOYER" | awk '{print $1}')
release_id=$(printf '%s\n' "$binary_hash" "$frontend_hash" "$deployer_hash" \
    | shasum -a 256 | awk '{print $1}')
release_path="$RELEASES_DIR/$release_id"

if [ -L "$release_path" ]; then
    fail "existing release must not be a symbolic link: $release_id"
elif [ -e "$release_path" ] && [ ! -d "$release_path" ]; then
    fail "existing release is not a directory: $release_id"
fi
if [ -d "$release_path" ]; then
    for shipped_file in \
        "$release_path/bin/todo" \
        "$release_path/libexec/todo" \
        "$release_path/package/todo" \
        "$release_path/package/deploy-user.sh"
    do
        [ -f "$shipped_file" ] && [ ! -L "$shipped_file" ] && [ -x "$shipped_file" ] \
            || fail "existing release contains an invalid executable: $shipped_file"
    done
    [ -f "$release_path/manifest.txt" ] && [ ! -L "$release_path/manifest.txt" ] \
        || fail "existing release contains an invalid manifest: $release_id"
    [ "$(shasum -a 256 "$release_path/bin/todo" | awk '{print $1}')" = \
        "$frontend_hash" ] \
        || fail "existing release frontend hash is invalid: $release_id"
    [ "$(shasum -a 256 "$release_path/package/todo" | awk '{print $1}')" = \
        "$frontend_hash" ] \
        || fail "existing release packaged frontend hash is invalid: $release_id"
    [ "$(shasum -a 256 "$release_path/libexec/todo" | awk '{print $1}')" = \
        "$binary_hash" ] \
        || fail "existing release binary hash is invalid: $release_id"
    [ "$(shasum -a 256 "$release_path/package/deploy-user.sh" | awk '{print $1}')" = \
        "$deployer_hash" ] \
        || fail "existing release deployer hash is invalid: $release_id"
    grep -Fx 'format=1' "$release_path/manifest.txt" >/dev/null \
        || fail "existing release manifest is invalid: $release_id"
    grep -Fx "release_id=$release_id" "$release_path/manifest.txt" >/dev/null \
        || fail "existing release manifest is invalid: $release_id"
    grep -Fx "binary_sha256=$binary_hash" "$release_path/manifest.txt" >/dev/null \
        || fail "existing release is invalid: $release_id"
    grep -Fx "frontend_sha256=$frontend_hash" "$release_path/manifest.txt" >/dev/null \
        || fail "existing release is invalid: $release_id"
    grep -Fx "deployer_sha256=$deployer_hash" "$release_path/manifest.txt" >/dev/null \
        || fail "existing release is invalid: $release_id"
else
    temporary_release=$(mktemp -d "$RELEASES_DIR/.stage.XXXXXX")
    install -d -m 0755 \
        "$temporary_release/bin" \
        "$temporary_release/libexec" \
        "$temporary_release/package"
    install -m 0755 "$SOURCE_FRONTEND" "$temporary_release/bin/todo"
    install -m 0755 "$binary_path" "$temporary_release/libexec/todo"
    install -m 0755 "$SOURCE_FRONTEND" "$temporary_release/package/todo"
    install -m 0755 "$SOURCE_DEPLOYER" \
        "$temporary_release/package/deploy-user.sh"
    {
        printf '%s\n' 'format=1'
        printf 'release_id=%s\n' "$release_id"
        printf 'binary_sha256=%s\n' "$binary_hash"
        printf 'frontend_sha256=%s\n' "$frontend_hash"
        printf 'deployer_sha256=%s\n' "$deployer_hash"
    } >"$temporary_release/manifest.txt"
    chmod 0444 "$temporary_release/manifest.txt"
    mv "$temporary_release" "$release_path"
    temporary_release=
fi

temporary_config=$(mktemp "$STATE_DIR/.config.XXXXXX")
{
    printf '%s\n' 'database = "todo.db"'
    printf '\n%s\n' '[liaison]'
    printf '%s\n' 'quality = "high"'
} >"$temporary_config"
chmod 0600 "$temporary_config"
mv "$temporary_config" "$CONFIG_PATH"
temporary_config=
config_changed=1

switched=1
if [ -n "$old_current" ] && [ "$old_current" != "releases/$release_id" ]; then
    atomic_symlink "$old_current" "$PREVIOUS_LINK"
fi
atomic_symlink "releases/$release_id" "$CURRENT_LINK"
atomic_symlink "$INSTALL_DIR/current/bin/todo" "$CLI_PATH"

if [ "$database_was_absent" -eq 1 ]; then
    TODO_STATE_DIR="$STATE_DIR" HOME="$install_home" \
        "$CLI_PATH" --config "$CONFIG_PATH" init >/dev/null
fi
TODO_STATE_DIR="$STATE_DIR" HOME="$install_home" \
    "$CLI_PATH" --config "$CONFIG_PATH" --json list --limit 1 >/dev/null

committed=1
printf 'Installed Todo release %s\n' "$release_id"
