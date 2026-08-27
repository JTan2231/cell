#!/bin/sh

set -eu

PATH=/usr/bin:/bin:/usr/sbin:/sbin
export PATH

umask 077

SERVICE_LABEL=org.annals.inbox
SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
SOURCE_FRONTEND="$SCRIPT_DIR/annals-user"
SOURCE_PLIST="$SCRIPT_DIR/org.annals.inbox.agent.plist"
SOURCE_UPDATER="$SCRIPT_DIR/deploy-user.sh"

binary_path=
usage_binary_path=
nucleus_path=
nucleus_socket=
install_home=${HOME:-}
launchctl_path=/bin/launchctl
no_start=0
fresh_state=0

usage() {
    cat <<'EOF'
Usage: deploy-user.sh --binary ABSOLUTE_PATH --usage-binary ABSOLUTE_PATH \
  --nucleus ABSOLUTE_PATH --nucleus-socket ABSOLUTE_PATH [OPTIONS]

Install or update the complete user-owned macOS Annals release.

Options:
  --home ABSOLUTE_PATH       Override the operator home (primarily for tests)
  --launchctl ABSOLUTE_PATH  Override launchctl (primarily for tests)
  --no-start                 Do not inspect or change launchd state
  --fresh-state              Replace the library and spool as one rollback generation,
                             import its uncompleted backlog in lane order, and resume processing
EOF
}

fail() {
    printf 'annals user deploy: %s\n' "$*" >&2
    exit 1
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --binary)
            [ "$#" -ge 2 ] || fail '--binary requires a path'
            binary_path=$2
            shift 2
            ;;
        --usage-binary)
            [ "$#" -ge 2 ] || fail '--usage-binary requires a path'
            usage_binary_path=$2
            shift 2
            ;;
        --nucleus)
            [ "$#" -ge 2 ] || fail '--nucleus requires a path'
            nucleus_path=$2
            shift 2
            ;;
        --nucleus-socket)
            [ "$#" -ge 2 ] || fail '--nucleus-socket requires a path'
            nucleus_socket=$2
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
        --no-start)
            no_start=1
            shift
            ;;
        --fresh-state)
            fresh_state=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            fail "unknown argument: $1"
            ;;
    esac
done

[ "$fresh_state" -eq 0 ] || [ "$no_start" -eq 0 ] \
    || fail '--fresh-state requires launchd control; do not combine it with --no-start'

operator_uid=$(id -u)
[ "$operator_uid" -ne 0 ] || fail 'run this deployer as the Annals operator, not root'
operator=$(id -un)

[ -n "$usage_binary_path" ] || fail '--usage-binary is required'
for value_name in binary_path usage_binary_path nucleus_path nucleus_socket install_home launchctl_path; do
    eval "value=\${$value_name}"
    [ -n "$value" ] || fail "--${value_name%_path} is required"
    case "$value" in
        /*) ;;
        *) fail "$value_name must be an absolute path" ;;
    esac
done

[ -d "$install_home" ] && [ ! -L "$install_home" ] \
    || fail "operator home is not a regular directory: $install_home"
[ "$(stat -f '%u' "$install_home")" -eq "$operator_uid" ] \
    || fail "operator home is not owned by $operator"
[ -f "$binary_path" ] && [ ! -L "$binary_path" ] && [ -x "$binary_path" ] \
    || fail "Annals candidate is not an executable regular file: $binary_path"
[ -f "$usage_binary_path" ] && [ ! -L "$usage_binary_path" ] && [ -x "$usage_binary_path" ] \
    || fail "Annals usage candidate is not an executable regular file: $usage_binary_path"
[ -e "$nucleus_path" ] && [ -x "$nucleus_path" ] \
    || fail "Nucleus executable is unavailable: $nucleus_path"
[ "$usage_binary_path" != "$nucleus_path" ] \
    || fail 'the Annals usage candidate and Nucleus executable must differ'
[ -f "$launchctl_path" ] && [ -x "$launchctl_path" ] \
    || fail "launchctl is unavailable: $launchctl_path"
for source in "$SOURCE_FRONTEND" "$SOURCE_PLIST" "$SOURCE_UPDATER"; do
    [ -f "$source" ] && [ ! -L "$source" ] \
        || fail "missing packaged file: $source"
done
for command in awk date grep install mv plutil readlink sed shasum stat; do
    command -v "$command" >/dev/null 2>&1 || fail "required command not found: $command"
done

for config_value in "$nucleus_path" "$nucleus_socket"; do
    value_lines=$(printf '%s\n' "$config_value" | wc -l | tr -d ' ')
    [ "$value_lines" -eq 1 ] \
        || fail 'a Nucleus path contains a newline'
    case "$config_value" in
        *\"*|*\\*) fail 'a Nucleus path contains characters unsupported by config rendering' ;;
    esac
done

STATE_DIR="$install_home/Library/Application Support/Annals"
CONFIG_PATH="$STATE_DIR/config.toml"
USAGE_CONFIG_PATH="$STATE_DIR/usage.toml"
LIBRARY_PATH="$STATE_DIR/annals.db"
USAGE_LIBRARY_PATH="$STATE_DIR/usage.db"
SPOOL_DIR="$STATE_DIR/spool"
INSTALL_DIR="$STATE_DIR/install"
RELEASES_DIR="$INSTALL_DIR/releases"
DEPLOYMENT_BACKUPS_DIR="$STATE_DIR/backups/deployments"
CURRENT_LINK="$INSTALL_DIR/current"
PREVIOUS_LINK="$INSTALL_DIR/previous"
UPDATE_LOCK="$INSTALL_DIR/.update-lock"
MAINTENANCE_MARKER="$SPOOL_DIR/.maintenance"
PAUSED_MARKER="$SPOOL_DIR/.paused"
CLI_DIR="$install_home/.local/bin"
CLI_PATH="$CLI_DIR/annals"
USAGE_CLI_PATH="$CLI_DIR/annals-usage"
AGENT_DIR="$install_home/Library/LaunchAgents"
AGENT_PLIST="$AGENT_DIR/$SERVICE_LABEL.plist"
SERVICE_TARGET="gui/$operator_uid/$SERVICE_LABEL"

temporary_release=
temporary_plist=
temporary_config=
temporary_usage_config=
transaction_dir=
fresh_stage=
generation_dir=
old_current=
old_previous=
old_cli=0
old_usage_cli=0
old_plist=0
old_config=0
old_usage_config=0
was_loaded=0
service_stopped=0
launchd_changed=0
marker_created=0
switched=0
config_changed=0
usage_config_changed=0
committed=0
lock_created=0
fresh_state_switched=0
pause_created=0
imported_backlog=0
backup_path=
rollback_snapshot=
rollback_snapshot_created=0
library_backup_ready=0
library_migration_may_need_rollback=0

atomic_symlink() {
    target=$1
    path=$2
    temporary="$path.tmp.$$"
    rm -f "$temporary"
    ln -s "$target" "$temporary"
    # -h replaces a selector that points at a directory instead of moving the
    # temporary link into that directory.
    mv -fh "$temporary" "$path"
}

move_if_present() {
    source_path=$1
    destination_path=$2
    if [ -e "$source_path" ] || [ -L "$source_path" ]; then
        mv "$source_path" "$destination_path"
    fi
}

restore_fresh_generation() {
    [ "$fresh_state_switched" -eq 1 ] || return 0
    failed_state="$transaction_dir/failed-fresh-state"
    install -d -m 0700 "$failed_state"
    move_if_present "$SPOOL_DIR" "$failed_state/spool"
    for name in annals.db annals.db-wal annals.db-shm \
        usage.db usage.db-wal usage.db-shm
    do
        move_if_present "$STATE_DIR/$name" "$failed_state/$name"
        move_if_present "$generation_dir/$name" "$STATE_DIR/$name"
    done
    move_if_present "$generation_dir/spool" "$SPOOL_DIR"
    fresh_state_switched=0
}

restore_service() {
    [ "$no_start" -eq 0 ] || return 0
    "$launchctl_path" enable "$SERVICE_TARGET" >/dev/null 2>&1 || true
    if [ "$service_stopped" -eq 1 ] || [ "$switched" -eq 1 ]; then
        "$launchctl_path" bootout --wait "$SERVICE_TARGET" >/dev/null 2>&1 || true
        if [ "$was_loaded" -eq 1 ] && [ -f "$AGENT_PLIST" ]; then
            "$launchctl_path" bootstrap "gui/$operator_uid" "$AGENT_PLIST" >/dev/null 2>&1 || true
        fi
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
            if [ "$old_cli" -eq 1 ]; then
                atomic_symlink "$INSTALL_DIR/current/bin/annals" "$CLI_PATH"
            else
                rm -f "$CLI_PATH"
            fi
            if [ "$old_usage_cli" -eq 1 ]; then
                atomic_symlink "$INSTALL_DIR/current/libexec/annals-usage" "$USAGE_CLI_PATH"
            else
                rm -f "$USAGE_CLI_PATH"
            fi
            if [ -n "$old_previous" ]; then
                atomic_symlink "$old_previous" "$PREVIOUS_LINK"
            else
                rm -f "$PREVIOUS_LINK"
            fi
            if [ "$old_plist" -eq 1 ]; then
                install -m 0600 "$transaction_dir/agent.plist" "$AGENT_PLIST"
            else
                rm -f "$AGENT_PLIST"
            fi
        fi
        if [ "$config_changed" -eq 1 ]; then
            if [ "$old_config" -eq 1 ]; then
                install -m 0600 "$transaction_dir/config.toml" "$CONFIG_PATH"
            else
                rm -f "$CONFIG_PATH"
            fi
        fi
        if [ "$usage_config_changed" -eq 1 ]; then
            if [ "$old_usage_config" -eq 1 ]; then
                install -m 0600 "$transaction_dir/usage.toml" "$USAGE_CONFIG_PATH"
            else
                rm -f "$USAGE_CONFIG_PATH"
            fi
        fi
        restore_fresh_generation
        if [ "$library_migration_may_need_rollback" -eq 1 ] \
            && [ "$library_backup_ready" -eq 1 ]
        then
            rm -f "$LIBRARY_PATH-wal" "$LIBRARY_PATH-shm"
            install -m 0600 "$backup_path" "$LIBRARY_PATH"
            library_migration_may_need_rollback=0
        fi
        if [ "$launchd_changed" -eq 1 ] || [ "$service_stopped" -eq 1 ] || [ "$switched" -eq 1 ]; then
            restore_service
        fi
        if [ "$rollback_snapshot_created" -eq 1 ]; then
            rm -rf "$rollback_snapshot"
        fi
    fi
    if [ "$marker_created" -eq 1 ]; then
        rm -f "$MAINTENANCE_MARKER"
    fi
    if [ "$pause_created" -eq 1 ]; then
        rm -f "$PAUSED_MARKER"
    fi
    [ -z "$temporary_release" ] || rm -rf "$temporary_release"
    [ -z "$temporary_plist" ] || rm -f "$temporary_plist"
    [ -z "$temporary_config" ] || rm -f "$temporary_config"
    [ -z "$temporary_usage_config" ] || rm -f "$temporary_usage_config"
    if [ "$status" -ne 0 ] && [ -n "$generation_dir" ]; then
        rm -f \
            "$generation_dir/config.toml" \
            "$generation_dir/usage.toml" \
            "$generation_dir/agent.plist" \
            "$generation_dir/generation.json"
        rmdir "$generation_dir" >/dev/null 2>&1 || true
    fi
    [ -z "$transaction_dir" ] || rm -rf "$transaction_dir"
    if [ "$lock_created" -eq 1 ]; then
        rmdir "$UPDATE_LOCK" >/dev/null 2>&1 || true
    fi
    exit "$status"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

"$binary_path" --version >/dev/null
"$usage_binary_path" --version >/dev/null
sh -n "$SOURCE_FRONTEND"
sh -n "$SOURCE_UPDATER"
plutil -lint "$SOURCE_PLIST" >/dev/null

for path in \
    "$STATE_DIR" \
    "$STATE_DIR/log" \
    "$SPOOL_DIR" \
    "$SPOOL_DIR/incoming" \
    "$SPOOL_DIR/queued" \
    "$SPOOL_DIR/processing" \
    "$SPOOL_DIR/done" \
    "$SPOOL_DIR/duplicates" \
    "$SPOOL_DIR/failed" \
    "$SPOOL_DIR/skipped" \
    "$INSTALL_DIR" \
    "$RELEASES_DIR" \
    "$STATE_DIR/backups" \
    "$DEPLOYMENT_BACKUPS_DIR"
do
    if [ -L "$path" ]; then
        fail "refusing symlink at directory path: $path"
    fi
    install -d -m 0700 "$path"
done

if [ -L "$PAUSED_MARKER" ] \
    || { [ -e "$PAUSED_MARKER" ] && [ ! -f "$PAUSED_MARKER" ]; }
then
    fail "invalid inbox pause marker: $PAUSED_MARKER"
fi
if [ -L "$MAINTENANCE_MARKER" ] \
    || { [ -e "$MAINTENANCE_MARKER" ] && [ ! -f "$MAINTENANCE_MARKER" ]; }
then
    fail "invalid inbox maintenance marker: $MAINTENANCE_MARKER"
fi
for path in "$CLI_DIR" "$AGENT_DIR"; do
    if [ -L "$path" ]; then
        fail "refusing symlink at directory path: $path"
    fi
    install -d -m 0755 "$path"
done

if ! mkdir "$UPDATE_LOCK" 2>/dev/null; then
    fail "another Annals deployment is active: $UPDATE_LOCK"
fi
lock_created=1

# Establish the no-new-claim boundary as soon as this deployment owns the update lock. The
# currently active delivery may finish, but the worker observes maintenance before claiming its
# successor while candidate preparation and validation continue.
if [ "$no_start" -eq 0 ] && [ ! -e "$MAINTENANCE_MARKER" ]; then
    : >"$MAINTENANCE_MARKER"
    marker_created=1
fi

if [ -L "$CONFIG_PATH" ] || { [ -e "$CONFIG_PATH" ] && [ ! -f "$CONFIG_PATH" ]; }; then
    fail "invalid configuration path: $CONFIG_PATH"
fi
temporary_config="$STATE_DIR/.config.toml.$$"
if [ -e "$CONFIG_PATH" ]; then
    if ! awk -v socket="$nucleus_socket" '
        BEGIN {
            in_liaison = 0
            saw_liaison = 0
            selected = 0
        }
        /^\[liaison\][[:space:]]*$/ {
            in_liaison = 1
            saw_liaison = 1
            print
            next
        }
        /^\[/ {
            if (in_liaison && selected == 0) {
                print "nucleus_socket = \"" socket "\""
                selected = 1
            }
            in_liaison = 0
        }
        in_liaison && /^[[:space:]]*(codex|nucleus_socket)[[:space:]]*=/ {
            if (selected == 0) {
                print "nucleus_socket = \"" socket "\""
                selected = 1
            }
            next
        }
        {
            print
        }
        END {
            if (in_liaison && selected == 0) {
                print "nucleus_socket = \"" socket "\""
                selected = 1
            }
            if (!saw_liaison || selected != 1) {
                exit 1
            }
        }
    ' "$CONFIG_PATH" >"$temporary_config"
    then
        fail "unable to select Nucleus in $CONFIG_PATH"
    fi
else
    {
        printf '%s\n' 'library = "annals.db"'
        printf '%s\n' '' '[inbox]' 'root = "spool"' 'settle_seconds = 60'
        printf '%s\n' '' '[liaison]' 'quality = "high"'
        printf 'nucleus_socket = "%s"\n' "$nucleus_socket"
    } >"$temporary_config"
fi
chmod 0600 "$temporary_config"
grep -Fqx "nucleus_socket = \"$nucleus_socket\"" "$temporary_config" \
    || fail "candidate configuration does not select Nucleus: $temporary_config"

if [ -L "$USAGE_CONFIG_PATH" ] \
    || { [ -e "$USAGE_CONFIG_PATH" ] && [ ! -f "$USAGE_CONFIG_PATH" ]; }
then
    fail "invalid usage configuration path: $USAGE_CONFIG_PATH"
fi
temporary_usage_config="$STATE_DIR/.usage.toml.$$"
{
    printf 'nucleus = "%s"\n' "$nucleus_path"
    printf 'nucleus_socket = "%s"\n' "$nucleus_socket"
    printf 'library = "%s"\n' "$LIBRARY_PATH"
    printf 'spool = "%s"\n' "$SPOOL_DIR"
    printf 'database = "%s"\n' "$USAGE_LIBRARY_PATH"
} >"$temporary_usage_config"
chmod 0600 "$temporary_usage_config"

run_with_installation_environment() {
    (
        cd "$STATE_DIR"
        env -i \
            HOME="$install_home" \
            PATH=/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin \
            USER="$operator" \
            LOGNAME="$operator" \
            "$@"
    )
}

run_active_annals() {
    if [ -n "$old_current" ]; then
        run_with_installation_environment "$CLI_PATH" "$@"
    else
        run_with_installation_environment "$binary_path" \
            --config "$temporary_config" "$@"
    fi
}

library_existed=1
if [ ! -e "$LIBRARY_PATH" ]; then
    library_existed=0
    if [ "$fresh_state" -eq 0 ]; then
        run_with_installation_environment "$binary_path" --config "$temporary_config" init >/dev/null
        run_with_installation_environment "$binary_path" --config "$temporary_config" validate >/dev/null
    fi
fi
run_with_installation_environment "$binary_path" --config "$temporary_config" inbox status >/dev/null

binary_hash=$(shasum -a 256 "$binary_path" | awk '{print $1}')
usage_binary_hash=$(shasum -a 256 "$usage_binary_path" | awk '{print $1}')
frontend_hash=$(shasum -a 256 "$SOURCE_FRONTEND" | awk '{print $1}')
plist_hash=$(shasum -a 256 "$SOURCE_PLIST" | awk '{print $1}')
updater_hash=$(shasum -a 256 "$SOURCE_UPDATER" | awk '{print $1}')

temporary_plist="$INSTALL_DIR/.org.annals.inbox.plist.$$"
install -m 0600 "$SOURCE_PLIST" "$temporary_plist"
plutil -remove ProgramArguments.0 "$temporary_plist"
plutil -insert ProgramArguments.0 -string "$CLI_PATH" "$temporary_plist"
plutil -replace WorkingDirectory -string "$STATE_DIR" "$temporary_plist"
plutil -replace EnvironmentVariables.HOME -string "$install_home" "$temporary_plist"
plutil -replace StandardOutPath -string "$STATE_DIR/log/inbox.stdout.log" "$temporary_plist"
plutil -replace StandardErrorPath -string "$STATE_DIR/log/inbox.stderr.log" "$temporary_plist"
plutil -lint "$temporary_plist" >/dev/null
rendered_plist_hash=$(shasum -a 256 "$temporary_plist" | awk '{print $1}')

release_id=$(printf '%s\n' \
    "$binary_hash" "$usage_binary_hash" "$frontend_hash" "$plist_hash" \
    "$updater_hash" "$rendered_plist_hash" \
    | shasum -a 256 | awk '{print $1}')
release_dir="$RELEASES_DIR/$release_id"

if [ ! -e "$release_dir" ]; then
    temporary_release="$RELEASES_DIR/.release.$$"
    install -d -m 0700 \
        "$temporary_release/bin" \
        "$temporary_release/libexec" \
        "$temporary_release/package"
    install -m 0755 "$SOURCE_FRONTEND" "$temporary_release/bin/annals"
    install -m 0755 "$binary_path" "$temporary_release/libexec/annals"
    install -m 0755 "$usage_binary_path" "$temporary_release/libexec/annals-usage"
    install -m 0755 "$SOURCE_UPDATER" "$temporary_release/package/deploy-user.sh"
    install -m 0755 "$SOURCE_FRONTEND" "$temporary_release/package/annals-user"
    install -m 0600 "$SOURCE_PLIST" \
        "$temporary_release/package/org.annals.inbox.agent.plist"

    install -m 0600 "$temporary_plist" \
        "$temporary_release/org.annals.inbox.plist"

    source_revision=unknown
    source_dirty=true
    repository=$(CDPATH='' cd "$SCRIPT_DIR/../.." 2>/dev/null && pwd || true)
    if [ -n "$repository" ] && git -C "$repository" rev-parse --git-dir >/dev/null 2>&1; then
        source_revision=$(git -C "$repository" rev-parse HEAD)
        if [ -z "$(git -C "$repository" status --short)" ]; then
            source_dirty=false
        fi
    fi
    {
        printf '{\n'
        printf '  "format": 2,\n'
        printf '  "release_id": "%s",\n' "$release_id"
        printf '  "binary_sha256": "%s",\n' "$binary_hash"
        printf '  "usage_binary_sha256": "%s",\n' "$usage_binary_hash"
        printf '  "frontend_sha256": "%s",\n' "$frontend_hash"
        printf '  "plist_template_sha256": "%s",\n' "$plist_hash"
        printf '  "rendered_plist_sha256": "%s",\n' "$rendered_plist_hash"
        printf '  "updater_sha256": "%s",\n' "$updater_hash"
        printf '  "source_revision": "%s",\n' "$source_revision"
        printf '  "source_dirty": %s\n' "$source_dirty"
        printf '}\n'
    } >"$temporary_release/manifest.json"
    chmod 0600 "$temporary_release/manifest.json"
    mv "$temporary_release" "$release_dir"
    temporary_release=
else
    [ -d "$release_dir" ] && [ ! -L "$release_dir" ] \
        || fail "invalid existing release path: $release_dir"
    [ "$(shasum -a 256 "$release_dir/libexec/annals" | awk '{print $1}')" = "$binary_hash" ] \
        || fail "existing release payload does not match $release_id"
    [ "$(shasum -a 256 "$release_dir/libexec/annals-usage" | awk '{print $1}')" = "$usage_binary_hash" ] \
        || fail "existing release usage payload does not match $release_id"
    [ "$(shasum -a 256 "$release_dir/bin/annals" | awk '{print $1}')" = "$frontend_hash" ] \
        || fail "existing release frontend does not match $release_id"
    [ "$(shasum -a 256 "$release_dir/package/annals-user" | awk '{print $1}')" = "$frontend_hash" ] \
        || fail "existing release packaged frontend does not match $release_id"
    [ "$(shasum -a 256 "$release_dir/package/deploy-user.sh" | awk '{print $1}')" = "$updater_hash" ] \
        || fail "existing release updater does not match $release_id"
    [ "$(shasum -a 256 "$release_dir/package/org.annals.inbox.agent.plist" | awk '{print $1}')" = "$plist_hash" ] \
        || fail "existing release plist template does not match $release_id"
    [ "$(shasum -a 256 "$release_dir/org.annals.inbox.plist" | awk '{print $1}')" = "$rendered_plist_hash" ] \
        || fail "existing release plist does not match $release_id"
fi

if [ -L "$CURRENT_LINK" ]; then
    old_current=$(readlink "$CURRENT_LINK")
elif [ -e "$CURRENT_LINK" ]; then
    fail "current release selector is not a symlink: $CURRENT_LINK"
fi
if [ -L "$PREVIOUS_LINK" ]; then
    old_previous=$(readlink "$PREVIOUS_LINK")
elif [ -e "$PREVIOUS_LINK" ]; then
    fail "previous release selector is not a symlink: $PREVIOUS_LINK"
fi
if [ -L "$CLI_PATH" ]; then
    [ "$(readlink "$CLI_PATH")" = "$INSTALL_DIR/current/bin/annals" ] \
        || fail "installed command has an unexpected target: $CLI_PATH"
    old_cli=1
elif [ -e "$CLI_PATH" ]; then
    fail "installed command is not a symlink: $CLI_PATH"
fi
if [ -L "$USAGE_CLI_PATH" ]; then
    [ "$(readlink "$USAGE_CLI_PATH")" = "$INSTALL_DIR/current/libexec/annals-usage" ] \
        || fail "installed usage command has an unexpected target: $USAGE_CLI_PATH"
    old_usage_cli=1
elif [ -e "$USAGE_CLI_PATH" ]; then
    fail "installed usage command is not a symlink: $USAGE_CLI_PATH"
fi
if [ -f "$AGENT_PLIST" ] && [ ! -L "$AGENT_PLIST" ]; then
    old_plist=1
elif [ -e "$AGENT_PLIST" ]; then
    fail "invalid LaunchAgent path: $AGENT_PLIST"
fi

transaction_dir="$INSTALL_DIR/transaction.$$"
install -d -m 0700 "$transaction_dir"
if [ "$old_plist" -eq 1 ]; then
    install -m 0600 "$AGENT_PLIST" "$transaction_dir/agent.plist"
fi
if [ -f "$CONFIG_PATH" ]; then
    old_config=1
    install -m 0600 "$CONFIG_PATH" "$transaction_dir/config.toml"
fi
if [ -f "$USAGE_CONFIG_PATH" ]; then
    old_usage_config=1
    install -m 0600 "$USAGE_CONFIG_PATH" "$transaction_dir/usage.toml"
fi
if [ "$fresh_state" -eq 1 ]; then
    fresh_stage="$transaction_dir/fresh-state"
    install -d -m 0700 \
        "$fresh_stage" \
        "$fresh_stage/spool" \
        "$fresh_stage/spool/incoming" \
        "$fresh_stage/spool/queued" \
        "$fresh_stage/spool/processing" \
        "$fresh_stage/spool/done" \
        "$fresh_stage/spool/duplicates" \
        "$fresh_stage/spool/failed" \
        "$fresh_stage/spool/skipped"
    fresh_config="$fresh_stage/config.toml"
    if ! awk '
        BEGIN {
            section = ""
            library = 0
            inbox_root = 0
        }
        /^\[[^]]+\][[:space:]]*$/ {
            section = $0
        }
        section == "" && /^[[:space:]]*library[[:space:]]*=/ {
            print "library = \"annals.db\""
            library++
            next
        }
        section == "[inbox]" && /^[[:space:]]*root[[:space:]]*=/ {
            print "root = \"spool\""
            inbox_root++
            next
        }
        { print }
        END {
            if (library != 1 || inbox_root != 1) {
                exit 1
            }
        }
    ' "$temporary_config" >"$fresh_config"
    then
        fail 'unable to render the fresh-state candidate configuration'
    fi
    chmod 0600 "$fresh_config"
    run_with_installation_environment "$binary_path" \
        --config "$fresh_config" init >/dev/null
    run_with_installation_environment "$binary_path" \
        --config "$fresh_config" validate >/dev/null
    run_with_installation_environment "$binary_path" \
        --config "$fresh_config" --quiet inbox pause
    [ -f "$fresh_stage/spool/.paused" ] && [ ! -L "$fresh_stage/spool/.paused" ] \
        || fail 'fresh inbox did not enter the paused state'
    : >"$fresh_stage/spool/.maintenance"
    run_with_installation_environment "$binary_path" \
        --config "$fresh_config" inbox status >/dev/null
fi

if [ -n "$old_current" ]; then
    run_with_installation_environment "$CLI_PATH" validate >/dev/null
    run_with_installation_environment "$CLI_PATH" inbox status >/dev/null
fi

if [ "$no_start" -eq 0 ]; then
    if "$launchctl_path" print "$SERVICE_TARGET" >/dev/null 2>&1; then
        was_loaded=1
    fi
    "$launchctl_path" disable "$SERVICE_TARGET" >/dev/null 2>&1 || true
    launchd_changed=1

    if [ "$fresh_state" -eq 1 ] && [ ! -e "$PAUSED_MARKER" ]; then
        run_active_annals --quiet inbox pause
        pause_created=1
    fi

    if [ "$fresh_state" -eq 1 ] || [ "$was_loaded" -eq 1 ]; then
        wait_seconds=${ANNALS_UPDATE_WAIT_SECONDS:-3900}
        case "$wait_seconds" in
            ''|*[!0-9]*) fail 'ANNALS_UPDATE_WAIT_SECONDS must be a nonnegative integer' ;;
        esac
        waited=0
        while :; do
            status_json=$(run_active_annals --json inbox status) \
                || fail 'unable to inspect the running inbox'
            if printf '%s\n' "$status_json" | grep -q '"locked":false'; then
                break
            fi
            [ "$waited" -lt "$wait_seconds" ] \
                || fail "inbox did not become idle within $wait_seconds seconds"
            sleep 1
            waited=$((waited + 1))
        done
    fi

    if [ "$fresh_state" -eq 1 ]; then
        run_active_annals --quiet inbox register --settle-seconds 0
    fi

    if [ "$was_loaded" -eq 1 ]; then
        "$launchctl_path" bootout --wait "$SERVICE_TARGET" >/dev/null
        service_stopped=1
    fi
fi

run_with_installation_environment "$usage_binary_path" doctor \
    --config "$temporary_usage_config" >/dev/null \
    || fail 'candidate Annals usage doctor could not verify Nucleus authentication'

if [ "$no_start" -eq 0 ]; then
    smoke_json=$(run_with_installation_environment "$binary_path" \
        --config "$temporary_config" --json inbox run) \
        || fail 'candidate cannot read the quiesced inbox'
    printf '%s\n' "$smoke_json" | grep -q '"stopped_for_maintenance":true' \
        || fail 'candidate did not honor inbox maintenance'
fi

new_current="releases/$release_id"
if [ "$fresh_state" -eq 1 ]; then
    generation_name="pre-fresh-$release_id-$(date -u '+%Y%m%dT%H%M%SZ')-$$"
    generation_dir="$STATE_DIR/backups/generations/$generation_name"
    [ ! -e "$generation_dir" ] \
        || fail "rollback generation already exists: $generation_dir"
    install -d -m 0700 "$STATE_DIR/backups/generations" "$generation_dir"
    for name in annals.db annals.db-wal annals.db-shm \
        usage.db usage.db-wal usage.db-shm
    do
        [ ! -L "$STATE_DIR/$name" ] \
            || fail "refusing symlink at state file: $STATE_DIR/$name"
    done

    fresh_state_switched=1
    for name in annals.db annals.db-wal annals.db-shm \
        usage.db usage.db-wal usage.db-shm
    do
        move_if_present "$STATE_DIR/$name" "$generation_dir/$name"
    done
    mv "$SPOOL_DIR" "$generation_dir/spool"
    mv "$fresh_stage/spool" "$SPOOL_DIR"
    for name in annals.db annals.db-wal annals.db-shm
    do
        move_if_present "$fresh_stage/$name" "$STATE_DIR/$name"
    done
    [ -f "$LIBRARY_PATH" ] \
        || fail 'fresh library disappeared during the state switch'
    if [ "$marker_created" -eq 1 ]; then
        rm -f "$generation_dir/spool/.maintenance"
        marker_created=0
    fi
    if [ "$pause_created" -eq 1 ]; then
        rm -f "$generation_dir/spool/.paused"
        pause_created=0
    fi
    run_with_installation_environment "$binary_path" \
        --config "$temporary_config" validate >/dev/null
    run_with_installation_environment "$binary_path" \
        --config "$temporary_config" inbox status >/dev/null
elif [ "$library_existed" -eq 1 ]; then
    if [ "$old_current" != "$new_current" ]; then
        backup_path="$STATE_DIR/backups/pre-update-$release_id-$$.db"
        run_with_installation_environment "$binary_path" \
            --config "$temporary_config" --quiet backup "$backup_path"
        library_backup_ready=1
        library_migration_may_need_rollback=1
    fi
    run_with_installation_environment "$binary_path" \
        --config "$temporary_config" --quiet migrate
    run_with_installation_environment "$binary_path" \
        --config "$temporary_config" validate >/dev/null
    run_with_installation_environment "$binary_path" \
        --config "$temporary_config" inbox status >/dev/null
fi

switched=1
if [ "$old_current" != "$new_current" ]; then
    if [ -n "$old_current" ]; then
        atomic_symlink "$old_current" "$PREVIOUS_LINK"
    fi
    atomic_symlink "$new_current" "$CURRENT_LINK"
fi
atomic_symlink "$INSTALL_DIR/current/bin/annals" "$CLI_PATH"
atomic_symlink "$INSTALL_DIR/current/libexec/annals-usage" "$USAGE_CLI_PATH"
install -m 0600 "$temporary_config" "$transaction_dir/config.next.toml"
config_changed=1
mv -f "$transaction_dir/config.next.toml" "$CONFIG_PATH"
install -m 0600 "$temporary_usage_config" "$transaction_dir/usage.next.toml"
usage_config_changed=1
mv -f "$transaction_dir/usage.next.toml" "$USAGE_CONFIG_PATH"
install -m 0600 "$release_dir/org.annals.inbox.plist" "$AGENT_PLIST.tmp.$$"
mv -f "$AGENT_PLIST.tmp.$$" "$AGENT_PLIST"

run_with_installation_environment "$CLI_PATH" --version >/dev/null
run_with_installation_environment "$USAGE_CLI_PATH" --version >/dev/null
run_with_installation_environment "$CLI_PATH" validate >/dev/null
run_with_installation_environment "$CLI_PATH" inbox status >/dev/null

if [ "$fresh_state" -eq 1 ]; then
    import_json=$(run_with_installation_environment "$CLI_PATH" \
        --json inbox import-backlog --from "$generation_dir/spool") \
        || fail 'unable to import the archived inbox backlog'
    imported_backlog=$(printf '%s\n' "$import_json" \
        | sed -n 's/.*"imported":\([0-9][0-9]*\).*/\1/p')
    case "$imported_backlog" in
        ''|*[!0-9]*) fail 'candidate returned an invalid backlog import receipt' ;;
    esac
    run_with_installation_environment "$CLI_PATH" validate >/dev/null
    status_json=$(run_with_installation_environment "$CLI_PATH" --json inbox status) \
        || fail 'unable to validate the imported inbox backlog'
    printf '%s\n' "$status_json" | grep -q "\"queued\":$imported_backlog" \
        || fail 'fresh inbox queued count does not match the imported backlog'
    printf '%s\n' "$status_json" | grep -q '"processing":0' \
        || fail 'fresh inbox started work before the cutover committed'
    printf '%s\n' "$status_json" | grep -q '"paused":true' \
        || fail 'fresh inbox lost its pause during backlog import'
    printf '%s\n' "$status_json" | grep -q '"maintenance":true' \
        || fail 'fresh inbox lost maintenance during backlog import'
    run_with_installation_environment "$CLI_PATH" --quiet inbox resume
    status_json=$(run_with_installation_environment "$CLI_PATH" --json inbox status) \
        || fail 'unable to validate the resumed inbox'
    printf '%s\n' "$status_json" | grep -q '"paused":false' \
        || fail 'fresh inbox did not resume'
    printf '%s\n' "$status_json" | grep -q '"maintenance":true' \
        || fail 'maintenance ended before the cutover committed'
fi

if [ "$no_start" -eq 0 ]; then
    "$launchctl_path" enable "$SERVICE_TARGET"
    "$launchctl_path" bootstrap "gui/$operator_uid" "$AGENT_PLIST"
    "$launchctl_path" print "$SERVICE_TARGET" >/dev/null
fi

completed_at=$(date -u '+%Y-%m-%dT%H:%M:%SZ')
if [ -n "$old_current" ] && [ "$old_current" != "$new_current" ]; then
    rollback_name="pre-$release_id-$(date -u '+%Y%m%dT%H%M%SZ')-$$"
    rollback_snapshot="$DEPLOYMENT_BACKUPS_DIR/$rollback_name"
    rollback_stage="$transaction_dir/rollback-snapshot"
    install -d -m 0700 "$rollback_stage"
    if [ "$old_config" -eq 1 ]; then
        install -m 0600 "$transaction_dir/config.toml" "$rollback_stage/config.toml"
    fi
    if [ "$old_usage_config" -eq 1 ]; then
        install -m 0600 "$transaction_dir/usage.toml" "$rollback_stage/usage.toml"
    fi
    if [ "$old_plist" -eq 1 ]; then
        install -m 0600 "$transaction_dir/agent.plist" "$rollback_stage/agent.plist"
    fi
    {
        printf '{\n'
        printf '  "format": 1,\n'
        printf '  "release": "%s",\n' "$old_current"
        printf '  "replacement_release": "%s",\n' "$new_current"
        printf '  "created_at": "%s"\n' "$completed_at"
        printf '}\n'
    } >"$rollback_stage/rollback.json"
    chmod 0600 "$rollback_stage/rollback.json"
    mv "$rollback_stage" "$rollback_snapshot"
    rollback_snapshot_created=1
fi
receipt="$INSTALL_DIR/last-update.json.tmp.$$"
{
    printf '{\n'
    printf '  "release_id": "%s",\n' "$release_id"
    printf '  "previous": "%s",\n' "$old_current"
    if [ "$fresh_state" -eq 1 ]; then
        printf '  "fresh_state": true,\n'
        printf '  "rollback_generation": "%s",\n' "$generation_name"
        printf '  "imported_backlog": %s,\n' "$imported_backlog"
    else
        printf '  "fresh_state": false,\n'
    fi
    if [ -n "$rollback_snapshot" ]; then
        printf '  "rollback_snapshot": "%s",\n' "$rollback_snapshot"
    fi
    printf '  "completed_at": "%s"\n' "$completed_at"
    printf '}\n'
} >"$receipt"
chmod 0600 "$receipt"
mv -f "$receipt" "$INSTALL_DIR/last-update.json"

if [ "$fresh_state" -eq 1 ]; then
    if [ "$old_config" -eq 1 ]; then
        install -m 0600 "$transaction_dir/config.toml" "$generation_dir/config.toml"
    fi
    if [ "$old_usage_config" -eq 1 ]; then
        install -m 0600 "$transaction_dir/usage.toml" "$generation_dir/usage.toml"
    fi
    if [ "$old_plist" -eq 1 ]; then
        install -m 0600 "$transaction_dir/agent.plist" "$generation_dir/agent.plist"
    fi
    {
        printf '{\n'
        printf '  "format": 1,\n'
        printf '  "release": "%s",\n' "$old_current"
        printf '  "replacement_release": "%s",\n' "$new_current"
        printf '  "imported_backlog": %s,\n' "$imported_backlog"
        printf '  "archived_at": "%s"\n' "$completed_at"
        printf '}\n'
    } >"$generation_dir/generation.json"
    chmod 0600 "$generation_dir/generation.json"
fi

committed=1
rm -rf "$transaction_dir"
transaction_dir=

if [ "$no_start" -eq 0 ]; then
    if [ "$fresh_state" -eq 1 ]; then
        rm -f "$MAINTENANCE_MARKER"
    elif [ "$marker_created" -eq 1 ]; then
        rm -f "$MAINTENANCE_MARKER"
        marker_created=0
    fi
    if ! "$launchctl_path" kickstart "$SERVICE_TARGET"; then
        printf '%s\n' \
            'annals user deploy: warning: unable to wake the installed service; launchd will retry on its interval' >&2
    fi
fi

printf '%s\n' 'Annals user installation is deployed and validated.'
printf 'Release: %s\n' "$release_id"
printf 'Command: %s\n' "$CLI_PATH"
printf 'Usage:   %s\n' "$USAGE_CLI_PATH"
printf 'Service: %s\n' "$SERVICE_TARGET"
printf 'State:   %s\n' "$STATE_DIR"
if [ "$fresh_state" -eq 1 ]; then
    printf 'Imported backlog: %s\n' "$imported_backlog"
    printf 'Rollback generation: %s\n' "$generation_dir"
fi
