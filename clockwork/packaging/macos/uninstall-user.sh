#!/bin/sh

set -eu

PATH=/usr/bin:/bin:/usr/sbin:/sbin
export PATH
umask 077

install_home=${HOME:-}

usage() {
    cat <<'EOF'
Usage: uninstall-user.sh [OPTIONS]

Detach Clockwork's owned public command and Chancery selectors after all
Clockwork product bindings have been disabled. Content-addressed releases,
runtime state, history, locks, and logs are retained.

Options:
  --home ABSOLUTE_PATH  Override the operator home (primarily for tests)
EOF
}

fail() {
    printf 'clockwork user uninstall: %s\n' "$*" >&2
    exit 1
}

while [ "$#" -gt 0 ]; do
    case "$1" in
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

[ -n "$install_home" ] || fail '--home is required'
case "$install_home" in /*) ;; *) fail 'home path must be absolute' ;; esac
[ -d "$install_home" ] && [ ! -L "$install_home" ] \
    || fail "operator home is not a regular directory: $install_home"
for command in chmod grep id ln mkdir mv readlink rm sed stat; do
    command -v "$command" >/dev/null 2>&1 \
        || fail "required command not found: $command"
done
[ -x /usr/bin/shlock ] \
    || fail 'required command not found: /usr/bin/shlock'

operator_uid=$(id -u)
[ "$operator_uid" -ne 0 ] || fail 'run as the Clockwork operator, not root'
if home_uid=$(stat -f '%u' "$install_home" 2>/dev/null); then
    :
elif home_uid=$(stat -c '%u' "$install_home" 2>/dev/null); then
    :
else
    fail "unable to inspect home ownership: $install_home"
fi
[ "$home_uid" = "$operator_uid" ] \
    || fail "operator home is not owned by uid $operator_uid: $install_home"

state_dir="$install_home/Library/Application Support/Clockwork"
install_dir="$state_dir/install"
current_link="$install_dir/current"
previous_link="$install_dir/previous"
cli_path="$install_home/.local/bin/clockwork"
chancery_link="$install_home/Library/Application Support/Chancery/providers/clockwork"
expected_cli="$install_dir/current/bin/clockwork"
expected_chancery="$install_dir/current/share/chancery/clockwork"
launch_agents="$install_home/Library/LaunchAgents"
locks_dir="$state_dir/locks"
update_lock="$state_dir/.update-lock"

lock_created=0
cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    set +e
    if [ "$lock_created" -eq 1 ] && [ -f "$update_lock" ] \
        && [ ! -L "$update_lock" ] \
        && [ "$(sed -n '1p' "$update_lock" 2>/dev/null || true)" = "$$" ]; then
        rm -f "$update_lock" >/dev/null 2>&1 || true
    fi
    exit "$status"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

[ ! -L "$state_dir" ] \
    || fail "state path is a symbolic link: $state_dir"
if [ ! -d "$state_dir" ]; then
    [ ! -e "$state_dir" ] \
        || fail "state path is not a regular directory: $state_dir"
    mkdir -p "$state_dir"
    chmod 0700 "$state_dir"
fi
if [ -e "$install_dir" ] || [ -L "$install_dir" ]; then
    [ -d "$install_dir" ] && [ ! -L "$install_dir" ] \
        || fail "installation path is not a regular directory: $install_dir"
fi
acquire_update_lock() {
    /usr/bin/shlock -p "$$" -f "$update_lock" \
        || fail "another Clockwork installation operation is active, or its lock is not safely recoverable: $update_lock"
}

acquire_update_lock
lock_created=1

if [ -d "$locks_dir" ] && [ ! -L "$locks_dir" ]; then
    for transition in "$locks_dir"/*.transition.json; do
        [ -e "$transition" ] || [ -L "$transition" ] || continue
        fail "recover the pending Clockwork binding transition before uninstall: $transition"
    done
elif [ -e "$locks_dir" ] || [ -L "$locks_dir" ]; then
    fail "Clockwork locks path is not a regular directory: $locks_dir"
fi

if [ -d "$launch_agents" ] && [ ! -L "$launch_agents" ]; then
    for plist in "$launch_agents"/org.clockwork.*.plist; do
        [ -e "$plist" ] || [ -L "$plist" ] || continue
        fail "disable the remaining Clockwork binding before uninstall: $plist"
    done
elif [ -e "$launch_agents" ] || [ -L "$launch_agents" ]; then
    fail "LaunchAgents path is not a regular directory: $launch_agents"
fi

current_target=
previous_target=
for selector in "$current_link" "$previous_link"; do
    if [ -L "$selector" ]; then
        selector_target=$(readlink "$selector")
        printf '%s\n' "$selector_target" \
            | grep -Eq '^releases/[0-9a-f]{64}$' \
            || fail "selector is not owned by Clockwork: $selector"
        release_path="$install_dir/$selector_target"
        [ -d "$release_path" ] && [ ! -L "$release_path" ] \
            || fail "selector target is not a retained Clockwork release: $selector"
        [ -f "$release_path/manifest.txt" ] \
            && [ ! -L "$release_path/manifest.txt" ] \
            || fail "selector target has no regular manifest: $selector"
        [ "$(sed -n '2p' "$release_path/manifest.txt")" = 'product=clockwork' ] \
            || fail "selector target has foreign ownership: $selector"
        if [ "$selector" = "$current_link" ]; then
            current_target=$selector_target
        else
            previous_target=$selector_target
        fi
    elif [ -e "$selector" ]; then
        fail "owned selector path is not a symbolic link: $selector"
    fi
done

cli_target=
if [ -L "$cli_path" ]; then
    cli_target=$(readlink "$cli_path")
    [ "$cli_target" = "$expected_cli" ] \
        || fail "command selector is not owned by Clockwork: $cli_path"
elif [ -e "$cli_path" ]; then
    fail "command path is not a symbolic link: $cli_path"
fi

chancery_target=
if [ -L "$chancery_link" ]; then
    chancery_target=$(readlink "$chancery_link")
    [ "$chancery_target" = "$expected_chancery" ] \
        || fail "provider selector is not owned by Clockwork: $chancery_link"
elif [ -e "$chancery_link" ]; then
    fail "provider path is not a symbolic link: $chancery_link"
fi

if [ ! -L "$current_link" ] \
    && { [ -L "$cli_path" ] || [ -L "$chancery_link" ] || [ -L "$previous_link" ]; }; then
    fail 'Clockwork public selectors exist without a current release'
fi

detach_failed=0
for selector in "$cli_path" "$chancery_link" "$current_link" "$previous_link"; do
    rm -f "$selector" || detach_failed=1
done
if [ "$detach_failed" -ne 0 ]; then
    restore_failed=0
    restore_selector() {
        path=$1
        target=$2
        if [ -n "$target" ]; then
            if [ ! -e "$path" ] && [ ! -L "$path" ]; then
                ln -s "$target" "$path" || return 1
            fi
            [ -L "$path" ] && [ "$(readlink "$path")" = "$target" ] \
                || return 1
        else
            [ ! -e "$path" ] && [ ! -L "$path" ] || return 1
        fi
    }
    restore_selector "$current_link" "$current_target" || restore_failed=1
    restore_selector "$previous_link" "$previous_target" || restore_failed=1
    restore_selector "$cli_path" "$cli_target" || restore_failed=1
    restore_selector "$chancery_link" "$chancery_target" || restore_failed=1
    if [ "$restore_failed" -eq 0 ]; then
        fail 'selector detachment failed; the prior selector view was restored'
    fi
    fail 'selector detachment failed and the prior selector view could not be restored'
fi

printf '%s\n' 'Detached Clockwork command and provider selectors'
printf 'Retained releases: %s/releases\n' "$install_dir"
printf 'Retained runtime state: %s\n' "$state_dir"
printf 'Retained broker logs: %s/Library/Logs/Clockwork\n' "$install_home"
