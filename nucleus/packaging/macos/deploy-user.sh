#!/bin/sh

set -eu

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

binary_version=$("$binary_path" --version) \
    || fail 'unable to read the Nucleus candidate version'
case "$binary_version" in
    'nucleus '*) version=${binary_version#nucleus } ;;
    *) fail "Nucleus candidate reported an unexpected version: $binary_version" ;;
esac
[ -n "$version" ] || fail 'Nucleus candidate reported an empty version'

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
lock_created=0
cleanup() {
    deploy_status=$?
    trap - EXIT HUP INT TERM
    set +e
    [ "$lock_created" -eq 0 ] || rmdir "$update_lock" >/dev/null 2>&1 || true
    exit "$deploy_status"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM
if ! mkdir "$update_lock" 2>/dev/null; then
    fail "another deployment holds $update_lock"
fi
lock_created=1

if [ -n "$codex_home" ]; then
    HOME=$install_home "$binary_path" service install \
        --daemon "$daemon_path" \
        --codex "$codex_path" \
        --codex-home "$codex_home"
else
    HOME=$install_home "$binary_path" service install \
        --daemon "$daemon_path" \
        --codex "$codex_path"
fi
