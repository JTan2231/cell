#!/bin/sh

set -eu

PATH=/usr/bin:/bin:/usr/sbin:/sbin
export PATH

umask 077

SERVICE_LABEL=org.annals.inbox
CLOCKWORK_KEY=annals/inbox
SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
SOURCE_FRONTEND="$SCRIPT_DIR/annals-user"
SOURCE_RUNNER="$SCRIPT_DIR/annals-inbox"
SOURCE_DEFINITION="$SCRIPT_DIR/annals-inbox.clockwork.toml.in"
SOURCE_LEGACY_AGENT_PLIST="$SCRIPT_DIR/org.annals.inbox.agent.plist"
SOURCE_UPDATER="$SCRIPT_DIR/deploy-user.sh"
if [ -d "$SCRIPT_DIR/../share/chancery/annals" ] \
    && [ -d "$SCRIPT_DIR/../share/chancery/annals-usage" ]
then
    SOURCE_CHANCERY_ANNALS="$SCRIPT_DIR/../share/chancery/annals"
    SOURCE_CHANCERY_USAGE="$SCRIPT_DIR/../share/chancery/annals-usage"
else
    SOURCE_CHANCERY_ANNALS="$SCRIPT_DIR/../../chancery/annals"
    SOURCE_CHANCERY_USAGE="$SCRIPT_DIR/../../chancery/annals-usage"
fi

binary_path=
usage_binary_path=
nucleus_path=
nucleus_socket=
clockwork_path=
install_home=${HOME:-}
launchctl_path=/bin/launchctl
no_start=0
fresh_state=0
migration_clockwork_handoff=0

usage() {
    cat <<'EOF'
Usage: deploy-user.sh --binary ABSOLUTE_PATH --usage-binary ABSOLUTE_PATH \
  --nucleus ABSOLUTE_PATH --nucleus-socket ABSOLUTE_PATH \
  --clockwork ABSOLUTE_PATH [OPTIONS]

Install or update the complete user-owned macOS Annals release.

Options:
  --home ABSOLUTE_PATH       Override the operator home (primarily for tests)
  --launchctl ABSOLUTE_PATH  Override launchctl (primarily for tests)
  --no-start                 Do not inspect or change launchd state
  --fresh-state              Replace the library and spool as one rollback generation,
                             import its uncompleted backlog in lane order, and resume processing

Option used only by migrate-to-user.sh:
  --migration-clockwork-handoff
                             Return the definition without registering or selecting it
EOF
}

fail() {
    printf 'annals user deploy: %s\n' "$*" >&2
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

# Compare the complete rendered document so extra launchd behavior is foreign,
# even when its label and executable tuple look like the former Annals job.
legacy_agent_plist_matches_expected() {
    candidate=$1
    [ -f "$candidate" ] && [ ! -L "$candidate" ] \
        && [ "$(stat -f '%u' "$candidate")" -eq "$operator_uid" ] \
        && [ "$(stat -f '%Lp' "$candidate")" = 600 ] \
        && [ -f "$SOURCE_LEGACY_AGENT_PLIST" ] \
        && [ ! -L "$SOURCE_LEGACY_AGENT_PLIST" ] \
        || return 1

    expected_dir=$(mktemp -d "$INSTALL_DIR/.legacy-agent-plist.XXXXXX") \
        || return 1
    expected_plist="$expected_dir/$SERVICE_LABEL.plist"
    if install -m 0600 "$SOURCE_LEGACY_AGENT_PLIST" "$expected_plist" \
        && plutil -remove ProgramArguments.0 "$expected_plist" \
        && plutil -insert ProgramArguments.0 -string "$CLI_PATH" "$expected_plist" \
        && plutil -replace WorkingDirectory -string "$STATE_DIR" "$expected_plist" \
        && plutil -replace EnvironmentVariables.HOME -string "$install_home" \
            "$expected_plist" \
        && plutil -replace StandardOutPath \
            -string "$STATE_DIR/log/inbox.stdout.log" "$expected_plist" \
        && plutil -replace StandardErrorPath \
            -string "$STATE_DIR/log/inbox.stderr.log" "$expected_plist" \
        && cmp -s "$expected_plist" "$candidate"
    then
        matched=0
    else
        matched=1
    fi
    rm -f "$expected_plist" >/dev/null 2>&1 || true
    rmdir "$expected_dir" >/dev/null 2>&1 || true
    return "$matched"
}

render_clockwork_definition() {
    definition_release_id=$1
    definition_release_root=$2
    definition_runner_hash=$3
    definition_template=$4
    definition_destination=$5

    sed \
        -e "s|__RELEASE_ID__|$definition_release_id|g" \
        -e "s|__RELEASE_ROOT__|$definition_release_root|g" \
        -e "s|__ANNALS_STATE__|$STATE_DIR|g" \
        -e "s|__ANNALS_HOME__|$install_home|g" \
        -e "s|__ANNALS_LOGS__|$STATE_DIR/log|g" \
        -e "s|__ANNALS_USER__|$operator|g" \
        -e "s|__INTERPRETER_SHA256__|$interpreter_hash|g" \
        -e "s|__RUNNER_SHA256__|$definition_runner_hash|g" \
        "$definition_template" >"$definition_destination"
    chmod 0600 "$definition_destination"
}

# A same-key digest is not ownership. Validate the complete current release,
# then compare every executable-definition field exposed by Clockwork.
prove_current_release_definition() {
    owned_selector=$1
    owned_digest=$2
    case "$owned_selector" in
        releases/*) owned_release_id=${owned_selector#releases/} ;;
        *) fail "current selector is not an Annals release: $owned_selector" ;;
    esac
    [ "$owned_selector" = "releases/$owned_release_id" ] \
        && [ "${#owned_release_id}" -eq 64 ] \
        || fail "current selector has an invalid Annals release identity: $owned_selector"
    case "$owned_release_id" in
        *[!0-9a-f]*) fail "current selector has an invalid Annals release identity: $owned_selector" ;;
    esac

    owned_release_root="$INSTALL_DIR/$owned_selector"
    owned_manifest="$owned_release_root/manifest.json"
    owned_template="$owned_release_root/package/annals-inbox.clockwork.toml.in"
    owned_runner="$owned_release_root/bin/annals-inbox"
    [ -d "$owned_release_root" ] && [ ! -L "$owned_release_root" ] \
        || fail "current Annals release is unavailable: $owned_release_root"
    for owned_file in \
        "$owned_manifest" \
        "$owned_release_root/libexec/annals" \
        "$owned_release_root/libexec/annals-usage" \
        "$owned_release_root/bin/annals" \
        "$owned_runner" \
        "$owned_release_root/package/annals-user" \
        "$owned_release_root/package/annals-inbox" \
        "$owned_release_root/package/deploy-user.sh" \
        "$owned_template" \
        "$owned_release_root/package/org.annals.inbox.agent.plist"
    do
        [ -f "$owned_file" ] && [ ! -L "$owned_file" ] \
            || fail "current Annals release has an invalid file: $owned_file"
    done
    [ "$(awk 'END { print NR }' "$owned_manifest")" -eq 15 ] \
        || fail "current Annals release manifest is not canonical: $owned_manifest"

    owned_format=$(sed -n 's/^  "format": \([0-9][0-9]*\),$/\1/p' \
        "$owned_manifest")
    owned_manifest_release=$(sed -n \
        's/^  "release_id": "\([0-9a-f]\{64\}\)",$/\1/p' "$owned_manifest")
    owned_binary_hash=$(sed -n \
        's/^  "binary_sha256": "\([0-9a-f]\{64\}\)",$/\1/p' "$owned_manifest")
    owned_usage_binary_hash=$(sed -n \
        's/^  "usage_binary_sha256": "\([0-9a-f]\{64\}\)",$/\1/p' \
        "$owned_manifest")
    owned_frontend_hash=$(sed -n \
        's/^  "frontend_sha256": "\([0-9a-f]\{64\}\)",$/\1/p' "$owned_manifest")
    owned_runner_hash=$(sed -n \
        's/^  "runner_sha256": "\([0-9a-f]\{64\}\)",$/\1/p' "$owned_manifest")
    owned_template_hash=$(sed -n \
        's/^  "clockwork_template_sha256": "\([0-9a-f]\{64\}\)",$/\1/p' \
        "$owned_manifest")
    owned_legacy_plist_hash=$(sed -n \
        's/^  "legacy_agent_plist_sha256": "\([0-9a-f]\{64\}\)",$/\1/p' \
        "$owned_manifest")
    owned_updater_hash=$(sed -n \
        's/^  "updater_sha256": "\([0-9a-f]\{64\}\)",$/\1/p' "$owned_manifest")
    owned_chancery_annals_hash=$(sed -n \
        's/^  "chancery_annals_sha256": "\([0-9a-f]\{64\}\)",$/\1/p' \
        "$owned_manifest")
    owned_chancery_usage_hash=$(sed -n \
        's/^  "chancery_usage_sha256": "\([0-9a-f]\{64\}\)",$/\1/p' \
        "$owned_manifest")
    [ "$owned_format" = 3 ] \
        && [ "$owned_manifest_release" = "$owned_release_id" ] \
        || fail "current release has no exact Annals Clockwork identity: $owned_release_root"
    for owned_hash in \
        "$owned_binary_hash" "$owned_usage_binary_hash" "$owned_frontend_hash" \
        "$owned_runner_hash" "$owned_template_hash" "$owned_legacy_plist_hash" \
        "$owned_updater_hash" "$owned_chancery_annals_hash" \
        "$owned_chancery_usage_hash"
    do
        [ "${#owned_hash}" -eq 64 ] || fail 'current Annals release has an invalid hash'
        case "$owned_hash" in
            *[!0-9a-f]*) fail 'current Annals release has an invalid hash' ;;
        esac
    done

    validate_chancery_bundle "$owned_release_root/share/chancery/annals"
    validate_chancery_bundle "$owned_release_root/share/chancery/annals-usage"
    actual_owned_binary_hash=$(shasum -a 256 \
        "$owned_release_root/libexec/annals" | awk '{print $1}')
    actual_owned_usage_binary_hash=$(shasum -a 256 \
        "$owned_release_root/libexec/annals-usage" | awk '{print $1}')
    actual_owned_frontend_hash=$(shasum -a 256 \
        "$owned_release_root/bin/annals" | awk '{print $1}')
    actual_owned_runner_hash=$(shasum -a 256 "$owned_runner" | awk '{print $1}')
    actual_owned_template_hash=$(shasum -a 256 "$owned_template" | awk '{print $1}')
    actual_owned_legacy_plist_hash=$(shasum -a 256 \
        "$owned_release_root/package/org.annals.inbox.agent.plist" | awk '{print $1}')
    actual_owned_updater_hash=$(shasum -a 256 \
        "$owned_release_root/package/deploy-user.sh" | awk '{print $1}')
    actual_owned_chancery_annals_hash=$(chancery_bundle_hash \
        "$owned_release_root/share/chancery/annals")
    actual_owned_chancery_usage_hash=$(chancery_bundle_hash \
        "$owned_release_root/share/chancery/annals-usage")
    [ "$actual_owned_binary_hash" = "$owned_binary_hash" ] \
        && [ "$actual_owned_usage_binary_hash" = "$owned_usage_binary_hash" ] \
        && [ "$actual_owned_frontend_hash" = "$owned_frontend_hash" ] \
        && [ "$actual_owned_runner_hash" = "$owned_runner_hash" ] \
        && [ "$actual_owned_template_hash" = "$owned_template_hash" ] \
        && [ "$actual_owned_legacy_plist_hash" = "$owned_legacy_plist_hash" ] \
        && [ "$actual_owned_updater_hash" = "$owned_updater_hash" ] \
        && [ "$actual_owned_chancery_annals_hash" = "$owned_chancery_annals_hash" ] \
        && [ "$actual_owned_chancery_usage_hash" = "$owned_chancery_usage_hash" ] \
        && [ "$(shasum -a 256 "$owned_release_root/package/annals-user" | awk '{print $1}')" = "$owned_frontend_hash" ] \
        && [ "$(shasum -a 256 "$owned_release_root/package/annals-inbox" | awk '{print $1}')" = "$owned_runner_hash" ] \
        || fail "current Annals release content changed: $owned_release_root"
    actual_owned_release_id=$(printf '%s\n' \
        "$actual_owned_binary_hash" "$actual_owned_usage_binary_hash" \
        "$actual_owned_frontend_hash" "$actual_owned_runner_hash" \
        "$actual_owned_template_hash" "$actual_owned_legacy_plist_hash" \
        "$actual_owned_updater_hash" "$actual_owned_chancery_annals_hash" \
        "$actual_owned_chancery_usage_hash" \
        | shasum -a 256 | awk '{print $1}')
    [ "$actual_owned_release_id" = "$owned_release_id" ] \
        || fail "current Annals release content identity changed: $owned_release_root"

    owned_definition="$transaction_dir/current-binding-definition.json"
    HOME="$install_home" "$clockwork_path" --json definition show "$owned_digest" \
        >"$owned_definition" 2>"$owned_definition.stderr" \
        || fail 'unable to inspect the selected Clockwork definition'
    [ "$(plutil -extract ok raw "$owned_definition" 2>/dev/null)" = true ] \
        && [ "$(plutil -extract data.digest raw "$owned_definition" 2>/dev/null)" = "$owned_digest" ] \
        && [ "$(plutil -extract data.key raw "$owned_definition" 2>/dev/null)" = "$CLOCKWORK_KEY" ] \
        && [ "$(plutil -extract data.manifest.schema_version raw "$owned_definition" 2>/dev/null)" = 1 ] \
        && [ "$(plutil -extract data.manifest.key raw "$owned_definition" 2>/dev/null)" = "$CLOCKWORK_KEY" ] \
        && [ "$(plutil -extract data.manifest.release_id raw "$owned_definition" 2>/dev/null)" = "$owned_release_id" ] \
        && [ "$(plutil -extract data.manifest.release_root raw "$owned_definition" 2>/dev/null)" = "$owned_release_root" ] \
        && [ "$(plutil -extract data.manifest.authority raw "$owned_definition" 2>/dev/null)" = current-user-background ] \
        && [ "$(plutil -extract data.manifest.overlap raw "$owned_definition" 2>/dev/null)" = skip ] \
        && [ "$(plutil -extract data.manifest.cwd raw "$owned_definition" 2>/dev/null)" = "$STATE_DIR" ] \
        && [ "$(plutil -extract data.manifest.schedule.kind raw "$owned_definition" 2>/dev/null)" = interval ] \
        && [ "$(plutil -extract data.manifest.schedule.seconds raw "$owned_definition" 2>/dev/null)" = 300 ] \
        && [ "$(plutil -extract data.manifest.schedule.run_at_load raw "$owned_definition" 2>/dev/null)" = true ] \
        && [ "$(plutil -extract data.manifest.launch.kind raw "$owned_definition" 2>/dev/null)" = interpreted ] \
        && [ "$(plutil -extract data.manifest.launch.interpreter raw "$owned_definition" 2>/dev/null)" = /bin/sh ] \
        && [ "$(plutil -extract data.manifest.launch.interpreter_sha256 raw "$owned_definition" 2>/dev/null)" = "$interpreter_hash" ] \
        && [ "$(plutil -extract data.manifest.launch.script raw "$owned_definition" 2>/dev/null)" = "$owned_runner" ] \
        && [ "$(plutil -extract data.manifest.launch.script_sha256 raw "$owned_definition" 2>/dev/null)" = "$owned_runner_hash" ] \
        && [ "$(plutil -extract data.manifest.environment.HOME raw "$owned_definition" 2>/dev/null)" = "$install_home" ] \
        && [ "$(plutil -extract data.manifest.environment.USER raw "$owned_definition" 2>/dev/null)" = "$operator" ] \
        && [ "$(plutil -extract data.manifest.environment.LOGNAME raw "$owned_definition" 2>/dev/null)" = "$operator" ] \
        && [ "$(plutil -extract data.manifest.environment.ANNALS_CONFIG raw "$owned_definition" 2>/dev/null)" = "$CONFIG_PATH" ] \
        && [ "$(plutil -extract data.manifest.output.stdout raw "$owned_definition" 2>/dev/null)" = "$STATE_DIR/log/inbox.stdout.log" ] \
        && [ "$(plutil -extract data.manifest.output.stderr raw "$owned_definition" 2>/dev/null)" = "$STATE_DIR/log/inbox.stderr.log" ] \
        || fail 'annals/inbox does not select the exact current Annals release definition'
    if plutil -extract data.manifest.timeout_seconds raw "$owned_definition" \
        >/dev/null 2>&1 \
        || plutil -extract data.manifest.arguments.0 raw "$owned_definition" \
            >/dev/null 2>&1
    then
        fail 'selected annals/inbox definition adds a timeout or argument'
    fi
    owned_environment_keys=$(plutil -extract data.manifest.environment xml1 -o - \
        "$owned_definition" 2>/dev/null | awk '/<key>/{count++} END {print count+0}')
    [ "$owned_environment_keys" -eq 4 ] \
        || fail 'selected annals/inbox definition has foreign environment entries'
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
        --clockwork)
            [ "$#" -ge 2 ] || fail '--clockwork requires a path'
            clockwork_path=$2
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
        --migration-clockwork-handoff)
            migration_clockwork_handoff=1
            no_start=1
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

[ "$migration_clockwork_handoff" -eq 0 ] || [ "$fresh_state" -eq 1 ] \
    || fail '--migration-clockwork-handoff requires --fresh-state'
[ "$fresh_state" -eq 0 ] || [ "$no_start" -eq 0 ] \
    || [ "$migration_clockwork_handoff" -eq 1 ] \
    || fail '--fresh-state requires launchd control; do not combine it with --no-start'

operator_uid=$(id -u)
[ "$operator_uid" -ne 0 ] || fail 'run this deployer as the Annals operator, not root'
operator=$(id -un)

[ -n "$usage_binary_path" ] || fail '--usage-binary is required'
for value_name in binary_path usage_binary_path nucleus_path nucleus_socket clockwork_path install_home launchctl_path; do
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
[ -e "$clockwork_path" ] && [ -x "$clockwork_path" ] \
    || fail "Clockwork executable is unavailable: $clockwork_path"
[ "$usage_binary_path" != "$nucleus_path" ] \
    || fail 'the Annals usage candidate and Nucleus executable must differ'
[ -f "$launchctl_path" ] && [ -x "$launchctl_path" ] \
    || fail "launchctl is unavailable: $launchctl_path"
for source in \
    "$SOURCE_FRONTEND" \
    "$SOURCE_RUNNER" \
    "$SOURCE_DEFINITION" \
    "$SOURCE_LEGACY_AGENT_PLIST" \
    "$SOURCE_UPDATER"
do
    [ -f "$source" ] && [ ! -L "$source" ] \
        || fail "missing packaged file: $source"
done
for command in awk cmp cp date find grep install mktemp mv plutil readlink sed shasum sort stat; do
    command -v "$command" >/dev/null 2>&1 || fail "required command not found: $command"
done
validate_chancery_bundle "$SOURCE_CHANCERY_ANNALS"
validate_chancery_bundle "$SOURCE_CHANCERY_USAGE"

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
SPOOL_DIR="$STATE_DIR/spool"
INSTALL_DIR="$STATE_DIR/install"
RELEASES_DIR="$INSTALL_DIR/releases"
CLOCKWORK_HANDOFF="$INSTALL_DIR/.migration-annals-inbox.clockwork.toml"
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
CHANCERY_STATE_DIR="$install_home/Library/Application Support/Chancery"
CHANCERY_PROVIDERS_DIR="$CHANCERY_STATE_DIR/providers"
CHANCERY_ANNALS_LINK="$CHANCERY_PROVIDERS_DIR/annals"
CHANCERY_USAGE_LINK="$CHANCERY_PROVIDERS_DIR/annals-usage"
CHANCERY_ANNALS_TARGET="$INSTALL_DIR/current/share/chancery/annals"
CHANCERY_USAGE_TARGET="$INSTALL_DIR/current/share/chancery/annals-usage"
SERVICE_TARGET="gui/$operator_uid/$SERVICE_LABEL"

temporary_release=
temporary_definition=
temporary_config=
temporary_usage_config=
transaction_dir=
fresh_stage=
generation_dir=
old_current=
old_previous=
old_chancery_annals=
old_chancery_usage=
old_cli=0
old_usage_cli=0
old_plist=0
old_config=0
old_usage_config=0
was_loaded=0
service_stopped=0
launchd_changed=0
legacy_plist_removed=0
prior_clockwork_enabled=0
prior_clockwork_digest=
prior_clockwork_expected_digest=
clockwork_disabled=0
clockwork_switched=0
candidate_definition_digest=
observed_clockwork_present=0
observed_clockwork_enabled=0
observed_clockwork_digest=
clockwork_handoff_created=0
marker_created=0
switched=0
chancery_providers_switched=0
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
legacy_usage_state_staged=0
retain_transaction=0

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

stage_legacy_usage_state() {
    legacy_stage="$transaction_dir/legacy-usage-state"
    for name in usage.db usage.db-wal usage.db-shm; do
        path="$STATE_DIR/$name"
        [ ! -L "$path" ] || fail "refusing symlink at legacy usage file: $path"
        if [ -e "$path" ]; then
            [ -f "$path" ] || fail "legacy usage path is not a regular file: $path"
            if [ "$legacy_usage_state_staged" -eq 0 ]; then
                install -d -m 0700 "$legacy_stage"
                legacy_usage_state_staged=1
            fi
            mv "$path" "$legacy_stage/$name"
        fi
    done
}

restore_legacy_usage_state() {
    [ "$legacy_usage_state_staged" -eq 1 ] || return 0
    for name in usage.db usage.db-wal usage.db-shm; do
        move_if_present "$transaction_dir/legacy-usage-state/$name" "$STATE_DIR/$name" \
            || return 1
    done
    legacy_usage_state_staged=0
}

restore_fresh_generation() {
    [ "$fresh_state_switched" -eq 1 ] || return 0
    failed_state="$transaction_dir/failed-fresh-state"
    install -d -m 0700 "$failed_state" || return 1
    move_if_present "$SPOOL_DIR" "$failed_state/spool" || return 1
    for name in annals.db annals.db-wal annals.db-shm
    do
        move_if_present "$STATE_DIR/$name" "$failed_state/$name" || return 1
        move_if_present "$generation_dir/$name" "$STATE_DIR/$name" || return 1
    done
    move_if_present "$generation_dir/spool" "$SPOOL_DIR" || return 1
    fresh_state_switched=0
}

inspect_clockwork_binding() {
    observed_clockwork_present=0
    observed_clockwork_enabled=0
    observed_clockwork_digest=
    if observed_clockwork_show=$(HOME="$install_home" "$clockwork_path" --json \
        binding show "$CLOCKWORK_KEY" 2>"$transaction_dir/clockwork-show.stderr")
    then
        observed_clockwork_present=1
        observed_clockwork_compact=$(printf '%s' "$observed_clockwork_show" \
            | tr -d '[:space:]')
        case "$observed_clockwork_compact" in
            *'"key":"annals/inbox"'*) ;;
            *) return 1 ;;
        esac
        case "$observed_clockwork_compact" in
            *'"definition_digest":null'*) ;;
            *'"definition_digest":"'*)
                observed_clockwork_digest=$(printf '%s\n' \
                    "$observed_clockwork_compact" | sed -n \
                    's/.*"definition_digest":"\([0-9a-f]\{64\}\)".*/\1/p')
                [ -n "$observed_clockwork_digest" ] || return 1
                ;;
            *) return 1 ;;
        esac
        case "$observed_clockwork_compact" in
            *'"enabled":true'*) observed_clockwork_enabled=1 ;;
            *'"enabled":false'*) observed_clockwork_enabled=0 ;;
            *) return 1 ;;
        esac
        [ "$observed_clockwork_enabled" -eq 0 ] \
            || [ -n "$observed_clockwork_digest" ] \
            || return 1
        return 0
    fi
    grep -F '"code":"binding_not_found"' \
        "$transaction_dir/clockwork-show.stderr" >/dev/null 2>&1
}

clockwork_selection_is_known() {
    [ -z "$observed_clockwork_digest" ] && return 0
    [ "$observed_clockwork_digest" = "$candidate_definition_digest" ] \
        && return 0
    [ -n "$prior_clockwork_expected_digest" ] \
        && [ "$observed_clockwork_digest" = "$prior_clockwork_expected_digest" ]
}

restore_schedule() {
    [ "$no_start" -eq 0 ] || return 0
    inspect_clockwork_binding || return 1
    clockwork_selection_is_known || return 1
    if [ "$prior_clockwork_enabled" -eq 1 ]; then
        if [ "$observed_clockwork_enabled" -eq 1 ] \
            && [ "$observed_clockwork_digest" = "$prior_clockwork_digest" ]
        then
            return 0
        fi
        if [ "$observed_clockwork_enabled" -eq 1 ]; then
            HOME="$install_home" "$clockwork_path" --json binding disable \
                "$CLOCKWORK_KEY" >/dev/null 2>&1 || return 1
        fi
        HOME="$install_home" "$clockwork_path" --json binding switch \
            "$CLOCKWORK_KEY" "$prior_clockwork_digest" >/dev/null 2>&1 || return 1
        return 0
    fi

    if [ -n "$prior_clockwork_digest" ] \
        && [ "$observed_clockwork_digest" != "$prior_clockwork_digest" ]
    then
        HOME="$install_home" "$clockwork_path" --json binding disable \
            "$CLOCKWORK_KEY" --select "$prior_clockwork_digest" \
            >/dev/null 2>&1 || return 1
    elif [ "$observed_clockwork_enabled" -eq 1 ]; then
        HOME="$install_home" "$clockwork_path" --json binding disable \
            "$CLOCKWORK_KEY" >/dev/null 2>&1 || return 1
    fi
    if [ "$was_loaded" -eq 1 ] && [ -f "$AGENT_PLIST" ]; then
        legacy_agent_plist_matches_expected "$AGENT_PLIST" || return 1
        "$launchctl_path" enable "$SERVICE_TARGET" >/dev/null 2>&1 || return 1
        if ! "$launchctl_path" print "$SERVICE_TARGET" >/dev/null 2>&1; then
            "$launchctl_path" bootstrap "gui/$operator_uid" "$AGENT_PLIST" \
                >/dev/null 2>&1 || return 1
        fi
    fi
}

cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    set +e
    rollback_ready=1
    if [ "$status" -ne 0 ] && [ "$committed" -eq 0 ]; then
        if [ "$clockwork_handoff_created" -eq 1 ]; then
            rm -f "$CLOCKWORK_HANDOFF" || rollback_ready=0
        fi
        if [ "$switched" -eq 1 ]; then
            if [ -n "$old_current" ]; then
                atomic_symlink "$old_current" "$CURRENT_LINK" || rollback_ready=0
            else
                rm -f "$CURRENT_LINK" || rollback_ready=0
            fi
            if [ "$old_cli" -eq 1 ]; then
                atomic_symlink "$INSTALL_DIR/current/bin/annals" "$CLI_PATH" \
                    || rollback_ready=0
            else
                rm -f "$CLI_PATH" || rollback_ready=0
            fi
            if [ "$old_usage_cli" -eq 1 ]; then
                atomic_symlink "$INSTALL_DIR/current/libexec/annals-usage" "$USAGE_CLI_PATH" \
                    || rollback_ready=0
            else
                rm -f "$USAGE_CLI_PATH" || rollback_ready=0
            fi
            if [ -n "$old_previous" ]; then
                atomic_symlink "$old_previous" "$PREVIOUS_LINK" || rollback_ready=0
            else
                rm -f "$PREVIOUS_LINK" || rollback_ready=0
            fi
        fi
        if [ "$legacy_plist_removed" -eq 1 ]; then
            if [ "$old_plist" -eq 1 ]; then
                if [ -e "$AGENT_PLIST" ] || [ -L "$AGENT_PLIST" ] \
                    || ! legacy_agent_plist_matches_expected \
                        "$transaction_dir/agent.plist"
                then
                    rollback_ready=0
                else
                    install -m 0600 "$transaction_dir/agent.plist" "$AGENT_PLIST" \
                        || rollback_ready=0
                fi
            else
                rollback_ready=0
            fi
        fi
        if [ "$chancery_providers_switched" -eq 1 ]; then
            if [ -n "$old_chancery_annals" ]; then
                atomic_symlink "$old_chancery_annals" "$CHANCERY_ANNALS_LINK" \
                    || rollback_ready=0
            else
                rm -f "$CHANCERY_ANNALS_LINK" || rollback_ready=0
            fi
            if [ -n "$old_chancery_usage" ]; then
                atomic_symlink "$old_chancery_usage" "$CHANCERY_USAGE_LINK" \
                    || rollback_ready=0
            else
                rm -f "$CHANCERY_USAGE_LINK" || rollback_ready=0
            fi
        fi
        if [ "$config_changed" -eq 1 ]; then
            if [ "$old_config" -eq 1 ]; then
                install -m 0600 "$transaction_dir/config.toml" "$CONFIG_PATH" \
                    || rollback_ready=0
            else
                rm -f "$CONFIG_PATH" || rollback_ready=0
            fi
        fi
        if [ "$usage_config_changed" -eq 1 ]; then
            if [ "$old_usage_config" -eq 1 ]; then
                install -m 0600 "$transaction_dir/usage.toml" "$USAGE_CONFIG_PATH" \
                    || rollback_ready=0
            else
                rm -f "$USAGE_CONFIG_PATH" || rollback_ready=0
            fi
        fi
        restore_fresh_generation || rollback_ready=0
        restore_legacy_usage_state || rollback_ready=0
        if [ "$library_migration_may_need_rollback" -eq 1 ] \
            && [ "$library_backup_ready" -eq 1 ]
        then
            if rm -f "$LIBRARY_PATH-wal" "$LIBRARY_PATH-shm" \
                && install -m 0600 "$backup_path" "$LIBRARY_PATH"
            then
                library_migration_may_need_rollback=0
            else
                rollback_ready=0
            fi
        fi
        if [ "$rollback_ready" -eq 1 ] \
            && { [ "$clockwork_disabled" -eq 1 ] || [ "$clockwork_switched" -eq 1 ] \
                || [ "$launchd_changed" -eq 1 ] || [ "$service_stopped" -eq 1 ]; }
        then
            restore_schedule || rollback_ready=0
        fi
        if [ "$rollback_ready" -eq 0 ]; then
            if inspect_clockwork_binding && clockwork_selection_is_known; then
                if [ "$observed_clockwork_enabled" -eq 1 ]; then
                    HOME="$install_home" "$clockwork_path" --json binding disable \
                        "$CLOCKWORK_KEY" >/dev/null 2>&1 || true
                fi
            else
                printf '%s\n' \
                    'annals user deploy: non-owned or uninspectable Clockwork binding was left untouched' \
                    >&2
            fi
            "$launchctl_path" bootout --wait "$SERVICE_TARGET" >/dev/null 2>&1 || true
            if [ -e "$AGENT_PLIST" ] || [ -L "$AGENT_PLIST" ]; then
                if legacy_agent_plist_matches_expected "$AGENT_PLIST"; then
                    rm -f "$AGENT_PLIST"
                else
                    printf 'annals user deploy: non-owned LaunchAgent was left untouched at %s\n' \
                        "$AGENT_PLIST" >&2
                fi
            fi
            rm -f "$CLI_PATH" "$USAGE_CLI_PATH" \
                "$CHANCERY_ANNALS_LINK" "$CHANCERY_USAGE_LINK" \
                "$CURRENT_LINK" "$PREVIOUS_LINK"
            retain_transaction=1
            marker_created=0
            printf '%s\n' 'annals user deploy: rollback could not restore one exclusive scheduler; inbox admission remains maintenance-gated, only attributable scheduler cleanup was attempted, and public selectors were removed' >&2
            printf 'annals user deploy: private rollback transaction retained at %s\n' \
                "$transaction_dir" >&2
        fi
        if [ "$rollback_snapshot_created" -eq 1 ]; then
            rm -rf "$rollback_snapshot"
        fi
    fi
    if [ "$marker_created" -eq 1 ] && [ "$retain_transaction" -eq 0 ]; then
        rm -f "$MAINTENANCE_MARKER"
    fi
    if [ "$pause_created" -eq 1 ]; then
        rm -f "$PAUSED_MARKER"
    fi
    [ -z "$temporary_release" ] || rm -rf "$temporary_release"
    [ -z "$temporary_definition" ] || rm -f "$temporary_definition"
    [ -z "$temporary_config" ] || rm -f "$temporary_config"
    [ -z "$temporary_usage_config" ] || rm -f "$temporary_usage_config"
    if [ "$status" -ne 0 ] && [ -n "$generation_dir" ] \
        && [ "$retain_transaction" -eq 0 ]
    then
        rm -f \
            "$generation_dir/config.toml" \
            "$generation_dir/usage.toml" \
            "$generation_dir/agent.plist" \
            "$generation_dir/schedule.txt" \
            "$generation_dir/generation.json"
        rmdir "$generation_dir" >/dev/null 2>&1 || true
    fi
    if [ "$retain_transaction" -eq 0 ]; then
        [ -z "$transaction_dir" ] || rm -rf "$transaction_dir"
    fi
    if [ "$lock_created" -eq 1 ]; then
        rmdir "$UPDATE_LOCK" >/dev/null 2>&1 || true
    fi
    exit "$status"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

candidate_version=$("$binary_path" --version) \
    || fail 'unable to read the Annals candidate version'
case "$candidate_version" in
    'annals '*) annals_version=${candidate_version#annals } ;;
    *) fail "Annals candidate reported an unexpected version: $candidate_version" ;;
esac
usage_candidate_version=$("$usage_binary_path" --version) \
    || fail 'unable to read the Annals Usage candidate version'
case "$usage_candidate_version" in
    'annals-usage '*) usage_version=${usage_candidate_version#annals-usage } ;;
    *) fail "Annals Usage candidate reported an unexpected version: $usage_candidate_version" ;;
esac
annals_provider_release=$(awk -F '"' \
    '/"release"[[:space:]]*:/ { print $4; exit }' \
    "$SOURCE_CHANCERY_ANNALS/provider.json")
usage_provider_release=$(awk -F '"' \
    '/"release"[[:space:]]*:/ { print $4; exit }' \
    "$SOURCE_CHANCERY_USAGE/provider.json")
[ "$annals_provider_release" = "$annals_version" ] \
    || fail "Annals provider release $annals_provider_release does not match candidate $annals_version"
[ "$usage_provider_release" = "$usage_version" ] \
    || fail "Annals Usage provider release $usage_provider_release does not match candidate $usage_version"
sh -n "$SOURCE_FRONTEND"
sh -n "$SOURCE_RUNNER"
sh -n "$SOURCE_UPDATER"

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

for path in "$CHANCERY_STATE_DIR" "$CHANCERY_PROVIDERS_DIR"; do
    [ ! -L "$path" ] || fail "refusing symlink at directory path: $path"
    [ ! -e "$path" ] || [ -d "$path" ] \
        || fail "Chancery registry path is not a directory: $path"
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
if [ "$migration_clockwork_handoff" -eq 1 ] && [ ! -f "$MAINTENANCE_MARKER" ]; then
    fail '--migration-clockwork-handoff requires outer migration maintenance'
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

if [ "$migration_clockwork_handoff" -eq 1 ] \
    && { [ -e "$CLOCKWORK_HANDOFF" ] || [ -L "$CLOCKWORK_HANDOFF" ]; }
then
    fail "migration Clockwork handoff already exists: $CLOCKWORK_HANDOFF"
fi

# Establish the no-new-claim boundary as soon as this deployment owns the update lock. The
# currently active delivery may finish, but the worker observes maintenance before claiming its
# successor while candidate preparation and checks continue.
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
        printf '%s\n' '' '[inbox]' 'root = "spool"' 'settle_seconds = 60' \
            'minimum_available_bytes = 7_000_000_000'
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
    fi
fi
run_with_installation_environment "$binary_path" --config "$temporary_config" inbox status >/dev/null

binary_hash=$(shasum -a 256 "$binary_path" | awk '{print $1}')
usage_binary_hash=$(shasum -a 256 "$usage_binary_path" | awk '{print $1}')
frontend_hash=$(shasum -a 256 "$SOURCE_FRONTEND" | awk '{print $1}')
runner_hash=$(shasum -a 256 "$SOURCE_RUNNER" | awk '{print $1}')
definition_template_hash=$(shasum -a 256 "$SOURCE_DEFINITION" | awk '{print $1}')
legacy_agent_plist_hash=$(shasum -a 256 "$SOURCE_LEGACY_AGENT_PLIST" | awk '{print $1}')
updater_hash=$(shasum -a 256 "$SOURCE_UPDATER" | awk '{print $1}')
chancery_annals_hash=$(chancery_bundle_hash "$SOURCE_CHANCERY_ANNALS")
chancery_usage_hash=$(chancery_bundle_hash "$SOURCE_CHANCERY_USAGE")

release_id=$(printf '%s\n' \
    "$binary_hash" "$usage_binary_hash" "$frontend_hash" "$runner_hash" \
    "$definition_template_hash" "$legacy_agent_plist_hash" "$updater_hash" \
    "$chancery_annals_hash" "$chancery_usage_hash" \
    | shasum -a 256 | awk '{print $1}')
release_dir="$RELEASES_DIR/$release_id"

if [ ! -e "$release_dir" ]; then
    temporary_release="$RELEASES_DIR/.release.$$"
    install -d -m 0700 \
        "$temporary_release/bin" \
        "$temporary_release/libexec" \
        "$temporary_release/package" \
        "$temporary_release/share/chancery"
    install -m 0755 "$SOURCE_FRONTEND" "$temporary_release/bin/annals"
    install -m 0755 "$SOURCE_RUNNER" "$temporary_release/bin/annals-inbox"
    install -m 0755 "$binary_path" "$temporary_release/libexec/annals"
    install -m 0755 "$usage_binary_path" "$temporary_release/libexec/annals-usage"
    install -m 0755 "$SOURCE_UPDATER" "$temporary_release/package/deploy-user.sh"
    install -m 0755 "$SOURCE_FRONTEND" "$temporary_release/package/annals-user"
    install -m 0755 "$SOURCE_RUNNER" "$temporary_release/package/annals-inbox"
    install -m 0600 "$SOURCE_DEFINITION" \
        "$temporary_release/package/annals-inbox.clockwork.toml.in"
    install -m 0600 "$SOURCE_LEGACY_AGENT_PLIST" \
        "$temporary_release/package/org.annals.inbox.agent.plist"
    cp -R "$SOURCE_CHANCERY_ANNALS" \
        "$temporary_release/share/chancery/annals"
    cp -R "$SOURCE_CHANCERY_USAGE" \
        "$temporary_release/share/chancery/annals-usage"

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
        printf '  "format": 3,\n'
        printf '  "release_id": "%s",\n' "$release_id"
        printf '  "binary_sha256": "%s",\n' "$binary_hash"
        printf '  "usage_binary_sha256": "%s",\n' "$usage_binary_hash"
        printf '  "frontend_sha256": "%s",\n' "$frontend_hash"
        printf '  "runner_sha256": "%s",\n' "$runner_hash"
        printf '  "clockwork_template_sha256": "%s",\n' "$definition_template_hash"
        printf '  "legacy_agent_plist_sha256": "%s",\n' "$legacy_agent_plist_hash"
        printf '  "updater_sha256": "%s",\n' "$updater_hash"
        printf '  "chancery_annals_sha256": "%s",\n' "$chancery_annals_hash"
        printf '  "chancery_usage_sha256": "%s",\n' "$chancery_usage_hash"
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
    [ "$(shasum -a 256 "$release_dir/bin/annals-inbox" | awk '{print $1}')" = "$runner_hash" ] \
        || fail "existing release inbox runner does not match $release_id"
    [ "$(shasum -a 256 "$release_dir/package/annals-user" | awk '{print $1}')" = "$frontend_hash" ] \
        || fail "existing release packaged frontend does not match $release_id"
    [ "$(shasum -a 256 "$release_dir/package/annals-inbox" | awk '{print $1}')" = "$runner_hash" ] \
        || fail "existing release packaged inbox runner does not match $release_id"
    [ "$(shasum -a 256 "$release_dir/package/deploy-user.sh" | awk '{print $1}')" = "$updater_hash" ] \
        || fail "existing release updater does not match $release_id"
    [ "$(shasum -a 256 "$release_dir/package/annals-inbox.clockwork.toml.in" | awk '{print $1}')" = "$definition_template_hash" ] \
        || fail "existing release Clockwork template does not match $release_id"
    [ "$(shasum -a 256 "$release_dir/package/org.annals.inbox.agent.plist" | awk '{print $1}')" = "$legacy_agent_plist_hash" ] \
        || fail "existing release legacy LaunchAgent template does not match $release_id"
    validate_chancery_bundle "$release_dir/share/chancery/annals"
    validate_chancery_bundle "$release_dir/share/chancery/annals-usage"
    [ "$(chancery_bundle_hash "$release_dir/share/chancery/annals")" = \
        "$chancery_annals_hash" ] \
        || fail "existing release Annals Chancery bundle does not match $release_id"
    [ "$(chancery_bundle_hash "$release_dir/share/chancery/annals-usage")" = \
        "$chancery_usage_hash" ] \
        || fail "existing release Annals Usage Chancery bundle does not match $release_id"
fi

# Re-verify the installed bytes even for a newly copied release. Input paths can
# change between hashing and staging; the directory name is trustworthy only
# after the release-local artifacts match the content identity inputs.
[ "$(shasum -a 256 "$release_dir/libexec/annals" | awk '{print $1}')" = "$binary_hash" ] \
    || fail "release payload does not match $release_id"
[ "$(shasum -a 256 "$release_dir/libexec/annals-usage" | awk '{print $1}')" = "$usage_binary_hash" ] \
    || fail "release usage payload does not match $release_id"
[ "$(shasum -a 256 "$release_dir/bin/annals" | awk '{print $1}')" = "$frontend_hash" ] \
    || fail "release frontend does not match $release_id"
[ "$(shasum -a 256 "$release_dir/bin/annals-inbox" | awk '{print $1}')" = "$runner_hash" ] \
    || fail "release inbox runner does not match $release_id"
[ "$(shasum -a 256 "$release_dir/package/annals-user" | awk '{print $1}')" = "$frontend_hash" ] \
    || fail "release packaged frontend does not match $release_id"
[ "$(shasum -a 256 "$release_dir/package/annals-inbox" | awk '{print $1}')" = "$runner_hash" ] \
    || fail "release packaged inbox runner does not match $release_id"
[ "$(shasum -a 256 "$release_dir/package/deploy-user.sh" | awk '{print $1}')" = "$updater_hash" ] \
    || fail "release updater does not match $release_id"
[ "$(shasum -a 256 "$release_dir/package/annals-inbox.clockwork.toml.in" | awk '{print $1}')" = "$definition_template_hash" ] \
    || fail "release Clockwork template does not match $release_id"
[ "$(shasum -a 256 "$release_dir/package/org.annals.inbox.agent.plist" | awk '{print $1}')" = "$legacy_agent_plist_hash" ] \
    || fail "release legacy LaunchAgent template does not match $release_id"
[ "$(chancery_bundle_hash "$release_dir/share/chancery/annals")" = "$chancery_annals_hash" ] \
    || fail "release Annals Chancery bundle does not match $release_id"
[ "$(chancery_bundle_hash "$release_dir/share/chancery/annals-usage")" = "$chancery_usage_hash" ] \
    || fail "release Annals Usage Chancery bundle does not match $release_id"

for rendered_value in "$release_dir" "$STATE_DIR" "$install_home" "$STATE_DIR/log" "$operator"; do
    case "$rendered_value" in
        *'&'*|*'|'*|*'"'*|*'\'*|*'
'*) fail 'an installation path or user name cannot be represented in the Clockwork template' ;;
    esac
done
temporary_definition="$INSTALL_DIR/.annals-inbox.clockwork.$$"
interpreter_hash=$(shasum -a 256 /bin/sh | awk '{print $1}')
render_clockwork_definition \
    "$release_id" "$release_dir" "$runner_hash" \
    "$release_dir/package/annals-inbox.clockwork.toml.in" \
    "$temporary_definition"
if [ "$migration_clockwork_handoff" -eq 0 ]; then
    definition_output=$(HOME="$install_home" "$clockwork_path" --json \
        definition register "$temporary_definition") \
        || fail 'Clockwork rejected the candidate inbox definition'
    definition_compact=$(printf '%s' "$definition_output" | tr -d '[:space:]')
    candidate_definition_digest=$(printf '%s\n' "$definition_compact" | sed -n \
        's/.*"digest":"\([0-9a-f]\{64\}\)".*/\1/p')
    [ -n "$candidate_definition_digest" ] \
        || fail 'Clockwork returned no candidate definition digest'
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
    legacy_agent_plist_matches_expected "$AGENT_PLIST" \
        || fail "legacy LaunchAgent is not the exact Annals-owned plist: $AGENT_PLIST"
    old_plist=1
elif [ -e "$AGENT_PLIST" ]; then
    fail "invalid LaunchAgent path: $AGENT_PLIST"
fi
if [ -L "$CHANCERY_ANNALS_LINK" ]; then
    old_chancery_annals=$(readlink "$CHANCERY_ANNALS_LINK")
elif [ -e "$CHANCERY_ANNALS_LINK" ]; then
    fail "installed Annals Chancery provider is not a symlink: $CHANCERY_ANNALS_LINK"
fi
if [ -n "$old_chancery_annals" ] \
    && [ "$old_chancery_annals" != "$CHANCERY_ANNALS_TARGET" ]
then
    fail "Chancery provider selector is not owned by this Annals installation: $CHANCERY_ANNALS_LINK"
fi
if [ -L "$CHANCERY_USAGE_LINK" ]; then
    old_chancery_usage=$(readlink "$CHANCERY_USAGE_LINK")
elif [ -e "$CHANCERY_USAGE_LINK" ]; then
    fail "installed Annals Usage Chancery provider is not a symlink: $CHANCERY_USAGE_LINK"
fi
if [ -n "$old_chancery_usage" ] \
    && [ "$old_chancery_usage" != "$CHANCERY_USAGE_TARGET" ]
then
    fail "Chancery provider selector is not owned by this Annals installation: $CHANCERY_USAGE_LINK"
fi

transaction_dir="$INSTALL_DIR/transaction.$$"
install -d -m 0700 "$transaction_dir"
if [ "$old_plist" -eq 1 ]; then
    install -m 0600 "$AGENT_PLIST" "$transaction_dir/agent.plist"
    legacy_agent_plist_matches_expected "$transaction_dir/agent.plist" \
        || fail 'captured legacy LaunchAgent changed during inspection'
fi
if [ -f "$CONFIG_PATH" ]; then
    old_config=1
    install -m 0600 "$CONFIG_PATH" "$transaction_dir/config.toml"
fi
if [ -f "$USAGE_CONFIG_PATH" ]; then
    old_usage_config=1
    install -m 0600 "$USAGE_CONFIG_PATH" "$transaction_dir/usage.toml"
fi
inspect_clockwork_binding \
    || fail 'unable to inspect the Clockwork binding'
prior_clockwork_enabled=$observed_clockwork_enabled
prior_clockwork_digest=$observed_clockwork_digest
if [ -n "$prior_clockwork_digest" ]; then
    [ -n "$old_current" ] \
        || fail 'annals/inbox selects a definition without a current Annals release'
    prove_current_release_definition "$old_current" "$prior_clockwork_digest"
    prior_clockwork_expected_digest=$prior_clockwork_digest
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
        --config "$fresh_config" --quiet inbox pause
    [ -f "$fresh_stage/spool/.paused" ] && [ ! -L "$fresh_stage/spool/.paused" ] \
        || fail 'fresh inbox did not enter the paused state'
    : >"$fresh_stage/spool/.maintenance"
    run_with_installation_environment "$binary_path" \
        --config "$fresh_config" inbox status >/dev/null
fi

if [ -n "$old_current" ]; then
    run_with_installation_environment "$CLI_PATH" inbox status >/dev/null
fi

if [ "$no_start" -eq 0 ]; then
    inspect_clockwork_binding \
        || fail 'unable to re-inspect the Clockwork binding before disabling it'
    [ "$observed_clockwork_enabled" -eq "$prior_clockwork_enabled" ] \
        && [ "$observed_clockwork_digest" = "$prior_clockwork_digest" ] \
        || fail 'annals/inbox changed after its ownership check'
    if "$launchctl_path" print "$SERVICE_TARGET" >/dev/null 2>&1; then
        was_loaded=1
        [ "$old_plist" -eq 1 ] \
            || fail 'loaded Annals label has no owned recoverable LaunchAgent'
    fi
    [ "$prior_clockwork_enabled" -eq 0 ] || [ "$was_loaded" -eq 0 ] \
        || fail 'Clockwork and the legacy Annals LaunchAgent are both active'
    # Treat the transition as changed before calling Clockwork because an
    # error may still mean Clockwork failed closed with the binding disabled.
    clockwork_disabled=1
    HOME="$install_home" "$clockwork_path" --json binding disable \
        "$CLOCKWORK_KEY" >/dev/null \
        || fail 'unable to disable the Clockwork inbox binding'
    "$launchctl_path" disable "$SERVICE_TARGET" >/dev/null 2>&1 || true
    launchd_changed=1

    if [ "$fresh_state" -eq 1 ] && [ ! -e "$PAUSED_MARKER" ]; then
        run_active_annals --quiet inbox pause
        pause_created=1
    fi

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

    if [ "$fresh_state" -eq 1 ]; then
        run_active_annals --quiet inbox register --settle-seconds 0
    fi

    if [ "$was_loaded" -eq 1 ]; then
        # A failing bootout can still have stopped the service; make rollback
        # prove restoration instead of assuming no transition occurred.
        service_stopped=1
        "$launchctl_path" bootout --wait "$SERVICE_TARGET" >/dev/null
    fi
    if [ "$old_plist" -eq 1 ]; then
        legacy_agent_plist_matches_expected "$AGENT_PLIST" \
            || fail 'legacy LaunchAgent changed before removal'
        rm -f "$AGENT_PLIST"
        legacy_plist_removed=1
    fi
fi

run_with_installation_environment "$usage_binary_path" doctor \
    --config "$temporary_usage_config" >/dev/null \
    || fail 'candidate Annals usage doctor could not verify Nucleus authentication'

# The live-only companion does not own a database. Retain an obsolete ledger only inside the
# deployment transaction so a pre-commit rollback can still restore the prior release intact.
stage_legacy_usage_state

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
    for name in annals.db annals.db-wal annals.db-shm
    do
        [ ! -L "$STATE_DIR/$name" ] \
            || fail "refusing symlink at state file: $STATE_DIR/$name"
    done

    fresh_state_switched=1
    for name in annals.db annals.db-wal annals.db-shm
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
chancery_providers_switched=1
atomic_symlink "$CHANCERY_ANNALS_TARGET" "$CHANCERY_ANNALS_LINK"
atomic_symlink "$CHANCERY_USAGE_TARGET" "$CHANCERY_USAGE_LINK"
install -m 0600 "$temporary_config" "$transaction_dir/config.next.toml"
config_changed=1
mv -f "$transaction_dir/config.next.toml" "$CONFIG_PATH"
install -m 0600 "$temporary_usage_config" "$transaction_dir/usage.next.toml"
usage_config_changed=1
mv -f "$transaction_dir/usage.next.toml" "$USAGE_CONFIG_PATH"

run_with_installation_environment "$CLI_PATH" --version >/dev/null
run_with_installation_environment "$USAGE_CLI_PATH" --version >/dev/null
run_with_installation_environment "$CLI_PATH" stats >/dev/null
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
    status_json=$(run_with_installation_environment "$CLI_PATH" --json inbox status) \
        || fail 'unable to inspect the imported inbox backlog'
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
        || fail 'unable to inspect the resumed inbox'
    printf '%s\n' "$status_json" | grep -q '"paused":false' \
        || fail 'fresh inbox did not resume'
    printf '%s\n' "$status_json" | grep -q '"maintenance":true' \
        || fail 'maintenance ended before the cutover committed'
fi

if [ "$no_start" -eq 0 ]; then
    inspect_clockwork_binding \
        || fail 'unable to inspect the disabled Clockwork binding before cutover'
    [ "$observed_clockwork_enabled" -eq 0 ] \
        && [ "$observed_clockwork_digest" = "$prior_clockwork_digest" ] \
        || fail 'annals/inbox changed before the candidate binding switch'
    HOME="$install_home" "$clockwork_path" --json binding switch \
        "$CLOCKWORK_KEY" "$candidate_definition_digest" >/dev/null \
        || fail 'Clockwork rejected the inbox binding switch'
    clockwork_switched=1
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
        printf 'enabled=%s\n' "$prior_clockwork_enabled"
        printf 'definition=%s\n' "$prior_clockwork_digest"
        printf 'legacy_launchagent_loaded=%s\n' "$was_loaded"
    } >"$rollback_stage/schedule.txt"
    chmod 0600 "$rollback_stage/schedule.txt"
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
    if [ "$migration_clockwork_handoff" -eq 1 ]; then
        printf '  "clockwork_definition": null,\n'
    else
        printf '  "clockwork_definition": "%s",\n' "$candidate_definition_digest"
    fi
    printf '  "previous_clockwork_definition": "%s",\n' "$prior_clockwork_digest"
    printf '  "previous_clockwork_enabled": %s,\n' \
        "$( [ "$prior_clockwork_enabled" -eq 1 ] && printf true || printf false )"
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
        printf 'enabled=%s\n' "$prior_clockwork_enabled"
        printf 'definition=%s\n' "$prior_clockwork_digest"
        printf 'legacy_launchagent_loaded=%s\n' "$was_loaded"
    } >"$generation_dir/schedule.txt"
    chmod 0600 "$generation_dir/schedule.txt"
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

if [ "$migration_clockwork_handoff" -eq 1 ]; then
    clockwork_handoff_created=1
    mv "$temporary_definition" "$CLOCKWORK_HANDOFF"
    temporary_definition=
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
fi

printf '%s\n' 'Annals user installation is deployed and verified.'
printf 'Release: %s\n' "$release_id"
printf 'Command: %s\n' "$CLI_PATH"
printf 'Usage:   %s\n' "$USAGE_CLI_PATH"
if [ "$migration_clockwork_handoff" -eq 1 ]; then
    printf 'Clockwork handoff: %s\n' "$CLOCKWORK_HANDOFF"
else
    printf 'Clockwork binding: %s (%s)\n' "$CLOCKWORK_KEY" "$candidate_definition_digest"
fi
printf 'State:   %s\n' "$STATE_DIR"
if [ "$fresh_state" -eq 1 ]; then
    printf 'Imported backlog: %s\n' "$imported_backlog"
    printf 'Rollback generation: %s\n' "$generation_dir"
fi
