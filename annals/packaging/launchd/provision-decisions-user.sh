#!/bin/sh

set -eu

PATH=/usr/bin:/bin:/usr/sbin:/sbin
export PATH

umask 077

CLOCKWORK_KEY=annals/decisions-inbox
SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
PROVISIONER_SELF="$SCRIPT_DIR/$(basename "$0")"
CONFIG_TEMPLATE=
DEFINITION_TEMPLATE=

release_root=
nucleus_socket=
clockwork_path=
install_home=${HOME:-}
keep_maintenance=0

usage() {
    cat <<'EOF'
Usage: provision-decisions-user.sh --release-root ABSOLUTE_CONTENT_RELEASE \
  --nucleus-socket ABSOLUTE_PATH --clockwork ABSOLUTE_PATH [OPTIONS]

Provision or update the current user's dedicated Annals decisions library and
its annals/decisions-inbox Clockwork binding.

Options:
  --home ABSOLUTE_PATH  Override the operator home (primarily for tests)
  --keep-maintenance    Leave the provisioner-owned decisions gate engaged
EOF
}

fail() {
    printf 'annals decisions provision: %s\n' "$*" >&2
    exit 1
}

absolute_path() {
    case "$1" in
        /*) return 0 ;;
        *) return 1 ;;
    esac
}

renderable_value() {
    case "$1" in
        *'&'*|*'|'*|*'"'*|*'\'*) return 1 ;;
    esac
    if printf '%s' "$1" | LC_ALL=C grep '[[:cntrl:]]' >/dev/null 2>&1; then
        return 1
    fi
    return 0
}

validate_private_file() {
    private_path=$1
    private_description=$2
    [ -f "$private_path" ] && [ ! -L "$private_path" ] \
        && [ "$(stat -f '%u' "$private_path")" -eq "$operator_uid" ] \
        && [ "$(stat -f '%Lp' "$private_path")" = 600 ] \
        && [ "$(stat -f '%l' "$private_path")" -eq 1 ] \
        || fail "invalid $private_description: $private_path"
}

validate_optional_private_file() {
    private_path=$1
    private_description=$2
    if [ -e "$private_path" ] || [ -L "$private_path" ]; then
        validate_private_file "$private_path" "$private_description"
    fi
}

ensure_private_file() {
    private_path=$1
    private_description=$2
    if [ -e "$private_path" ] || [ -L "$private_path" ]; then
        validate_private_file "$private_path" "$private_description"
        return
    fi
    (set -C; : >"$private_path") 2>/dev/null \
        || fail "unable to create $private_description: $private_path"
    chmod 0600 "$private_path" \
        || fail "unable to protect $private_description: $private_path"
    validate_private_file "$private_path" "$private_description"
}

validate_decisions_mutable_files() {
    validate_private_file "$CONFIG_PATH" 'decisions config'
    validate_private_file "$LIBRARY_PATH" 'decisions library'
    validate_private_file \
        "$SPOOL_DIR/.decision-feed-library.json" \
        'decisions spool identity'
    validate_optional_private_file "$LIBRARY_PATH-wal" 'decisions library WAL'
    validate_optional_private_file "$LIBRARY_PATH-shm" 'decisions library shared memory'
    validate_optional_private_file "$LIBRARY_PATH-journal" 'decisions library journal'
    validate_optional_private_file "$SPOOL_DIR/.queue.json" 'decisions queue index'
    validate_optional_private_file "$SPOOL_DIR/.queue.json.tmp" 'decisions queue temporary index'
    validate_optional_private_file "$SPOOL_DIR/.run.lock" 'decisions run lock'
    validate_optional_private_file "$SPOOL_DIR/.control.lock" 'decisions control lock'
    validate_optional_private_file "$SPOOL_DIR/.paused" 'decisions pause marker'
    validate_optional_private_file "$MAINTENANCE_MARKER" 'decisions maintenance marker'
    validate_optional_private_file \
        "$SPOOL_DIR/.decision-feed-library.json.tmp" \
        'decisions spool identity temporary file'
    validate_optional_private_file "$HOLD_RECEIPT" 'decisions maintenance receipt'
}

prepare_output_files() {
    ensure_private_file "$STDOUT_LOG" 'decisions stdout log'
    ensure_private_file "$STDERR_LOG" 'decisions stderr log'
    [ "$(stat -f '%d:%i' "$STDOUT_LOG")" != \
        "$(stat -f '%d:%i' "$STDERR_LOG")" ] \
        || fail 'decisions stdout and stderr logs must be distinct files'
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

# Validate the complete format-four content release used by the established
# Annals installer. The decisions binding reuses only its immutable payload and
# runner; it never follows the mutable `current` selector.
validate_release() {
    checked_root=$1
    absolute_path "$checked_root" \
        || fail "release root must be absolute: $checked_root"
    [ -d "$checked_root" ] && [ ! -L "$checked_root" ] \
        || fail "Annals release is unavailable: $checked_root"
    checked_canonical=$(CDPATH='' cd "$checked_root" && pwd -P) \
        || fail "unable to resolve Annals release: $checked_root"
    [ "$checked_canonical" = "$checked_root" ] \
        || fail "Annals release contains a symbolic selector: $checked_root"
    checked_release_id=${checked_root##*/}
    [ "${#checked_release_id}" -eq 64 ] \
        || fail "Annals release has an invalid content identity: $checked_root"
    case "$checked_release_id" in
        *[!0-9a-f]*) fail "Annals release has an invalid content identity: $checked_root" ;;
    esac

    checked_manifest="$checked_root/manifest.json"
    checked_runner="$checked_root/bin/annals-inbox"
    checked_template="$checked_root/package/annals-inbox.clockwork.toml.in"
    for checked_file in \
        "$checked_manifest" \
        "$checked_root/libexec/annals" \
        "$checked_root/libexec/annals-usage" \
        "$checked_root/bin/annals" \
        "$checked_runner" \
        "$checked_root/package/annals-user" \
        "$checked_root/package/annals-inbox" \
        "$checked_root/package/deploy-user.sh" \
        "$checked_template" \
        "$checked_root/package/annals-decisions.toml.in" \
        "$checked_root/package/annals-decisions-inbox.clockwork.toml.in" \
        "$checked_root/package/provision-decisions-user.sh" \
        "$checked_root/package/org.annals.inbox.agent.plist"
    do
        [ -f "$checked_file" ] && [ ! -L "$checked_file" ] \
            || fail "Annals release has an invalid file: $checked_file"
    done
    [ "$(awk 'END { print NR }' "$checked_manifest")" -eq 18 ] \
        || fail "Annals release manifest is not canonical: $checked_manifest"

    checked_format=$(sed -n 's/^  "format": \([0-9][0-9]*\),$/\1/p' \
        "$checked_manifest")
    checked_manifest_release=$(sed -n \
        's/^  "release_id": "\([0-9a-f]\{64\}\)",$/\1/p' "$checked_manifest")
    checked_binary_hash=$(sed -n \
        's/^  "binary_sha256": "\([0-9a-f]\{64\}\)",$/\1/p' "$checked_manifest")
    checked_usage_hash=$(sed -n \
        's/^  "usage_binary_sha256": "\([0-9a-f]\{64\}\)",$/\1/p' "$checked_manifest")
    checked_frontend_hash=$(sed -n \
        's/^  "frontend_sha256": "\([0-9a-f]\{64\}\)",$/\1/p' "$checked_manifest")
    checked_runner_hash=$(sed -n \
        's/^  "runner_sha256": "\([0-9a-f]\{64\}\)",$/\1/p' "$checked_manifest")
    checked_template_hash=$(sed -n \
        's/^  "clockwork_template_sha256": "\([0-9a-f]\{64\}\)",$/\1/p' "$checked_manifest")
    checked_decisions_config_hash=$(sed -n \
        's/^  "decisions_config_template_sha256": "\([0-9a-f]\{64\}\)",$/\1/p' "$checked_manifest")
    checked_decisions_template_hash=$(sed -n \
        's/^  "decisions_clockwork_template_sha256": "\([0-9a-f]\{64\}\)",$/\1/p' "$checked_manifest")
    checked_decisions_provisioner_hash=$(sed -n \
        's/^  "decisions_provisioner_sha256": "\([0-9a-f]\{64\}\)",$/\1/p' "$checked_manifest")
    checked_plist_hash=$(sed -n \
        's/^  "legacy_agent_plist_sha256": "\([0-9a-f]\{64\}\)",$/\1/p' "$checked_manifest")
    checked_updater_hash=$(sed -n \
        's/^  "updater_sha256": "\([0-9a-f]\{64\}\)",$/\1/p' "$checked_manifest")
    checked_chancery_annals_hash=$(sed -n \
        's/^  "chancery_annals_sha256": "\([0-9a-f]\{64\}\)",$/\1/p' "$checked_manifest")
    checked_chancery_usage_hash=$(sed -n \
        's/^  "chancery_usage_sha256": "\([0-9a-f]\{64\}\)",$/\1/p' "$checked_manifest")
    [ "$checked_format" = 4 ] \
        && [ "$checked_manifest_release" = "$checked_release_id" ] \
        || fail "Annals release has no exact format-four identity: $checked_root"
    for checked_hash in \
        "$checked_binary_hash" "$checked_usage_hash" "$checked_frontend_hash" \
        "$checked_runner_hash" "$checked_template_hash" \
        "$checked_decisions_config_hash" "$checked_decisions_template_hash" \
        "$checked_decisions_provisioner_hash" "$checked_plist_hash" \
        "$checked_updater_hash" "$checked_chancery_annals_hash" \
        "$checked_chancery_usage_hash"
    do
        [ "${#checked_hash}" -eq 64 ] \
            || fail "Annals release has an invalid hash: $checked_root"
        case "$checked_hash" in
            *[!0-9a-f]*) fail "Annals release has an invalid hash: $checked_root" ;;
        esac
    done

    validate_chancery_bundle "$checked_root/share/chancery/annals"
    validate_chancery_bundle "$checked_root/share/chancery/annals-usage"
    actual_binary_hash=$(shasum -a 256 "$checked_root/libexec/annals" | awk '{print $1}')
    actual_usage_hash=$(shasum -a 256 "$checked_root/libexec/annals-usage" | awk '{print $1}')
    actual_frontend_hash=$(shasum -a 256 "$checked_root/bin/annals" | awk '{print $1}')
    actual_runner_hash=$(shasum -a 256 "$checked_runner" | awk '{print $1}')
    actual_template_hash=$(shasum -a 256 "$checked_template" | awk '{print $1}')
    actual_decisions_config_hash=$(shasum -a 256 \
        "$checked_root/package/annals-decisions.toml.in" | awk '{print $1}')
    actual_decisions_template_hash=$(shasum -a 256 \
        "$checked_root/package/annals-decisions-inbox.clockwork.toml.in" \
        | awk '{print $1}')
    actual_decisions_provisioner_hash=$(shasum -a 256 \
        "$checked_root/package/provision-decisions-user.sh" | awk '{print $1}')
    actual_plist_hash=$(shasum -a 256 \
        "$checked_root/package/org.annals.inbox.agent.plist" | awk '{print $1}')
    actual_updater_hash=$(shasum -a 256 \
        "$checked_root/package/deploy-user.sh" | awk '{print $1}')
    actual_chancery_annals_hash=$(chancery_bundle_hash \
        "$checked_root/share/chancery/annals")
    actual_chancery_usage_hash=$(chancery_bundle_hash \
        "$checked_root/share/chancery/annals-usage")
    [ "$actual_binary_hash" = "$checked_binary_hash" ] \
        && [ "$actual_usage_hash" = "$checked_usage_hash" ] \
        && [ "$actual_frontend_hash" = "$checked_frontend_hash" ] \
        && [ "$actual_runner_hash" = "$checked_runner_hash" ] \
        && [ "$actual_template_hash" = "$checked_template_hash" ] \
        && [ "$actual_decisions_config_hash" = "$checked_decisions_config_hash" ] \
        && [ "$actual_decisions_template_hash" = "$checked_decisions_template_hash" ] \
        && [ "$actual_decisions_provisioner_hash" = "$checked_decisions_provisioner_hash" ] \
        && [ "$actual_plist_hash" = "$checked_plist_hash" ] \
        && [ "$actual_updater_hash" = "$checked_updater_hash" ] \
        && [ "$actual_chancery_annals_hash" = "$checked_chancery_annals_hash" ] \
        && [ "$actual_chancery_usage_hash" = "$checked_chancery_usage_hash" ] \
        && [ "$(shasum -a 256 "$checked_root/package/annals-user" | awk '{print $1}')" = "$checked_frontend_hash" ] \
        && [ "$(shasum -a 256 "$checked_root/package/annals-inbox" | awk '{print $1}')" = "$checked_runner_hash" ] \
        || fail "Annals release content changed: $checked_root"
    actual_release_id=$(printf '%s\n' \
        "$actual_binary_hash" "$actual_usage_hash" "$actual_frontend_hash" \
        "$actual_runner_hash" "$actual_template_hash" \
        "$actual_decisions_config_hash" "$actual_decisions_template_hash" \
        "$actual_decisions_provisioner_hash" "$actual_plist_hash" \
        "$actual_updater_hash" "$actual_chancery_annals_hash" \
        "$actual_chancery_usage_hash" | shasum -a 256 | awk '{print $1}')
    [ "$actual_release_id" = "$checked_release_id" ] \
        || fail "Annals release content identity changed: $checked_root"

    validated_release_id=$checked_release_id
    validated_runner_hash=$checked_runner_hash
    validated_provisioner_hash=$checked_decisions_provisioner_hash
}

xml_top_level_key_count() {
    plutil -extract "$1" xml1 -o - "$2" 2>/dev/null | awk '
        /<dict>/ { depth++; next }
        /<\/dict>/ { depth--; next }
        depth == 1 && /<key>.*<\/key>/ { count++ }
        END { print count + 0 }
    '
}

render_config() {
    rendered_state=$1
    rendered_library_id=$2
    rendered_socket=$3
    rendered_destination=$4
    sed \
        -e "s|__ANNALS_DECISIONS_STATE__|$rendered_state|g" \
        -e "s|__ANNALS_DECISIONS_LIBRARY_ID__|$rendered_library_id|g" \
        -e "s|__NUCLEUS_SOCKET__|$rendered_socket|g" \
        "$CONFIG_TEMPLATE" >"$rendered_destination"
    chmod 0600 "$rendered_destination"
}

render_definition() {
    rendered_destination=$1
    sed \
        -e "s|__RELEASE_ID__|$release_id|g" \
        -e "s|__RELEASE_ROOT__|$release_root|g" \
        -e "s|__INTERPRETER_SHA256__|$interpreter_hash|g" \
        -e "s|__RUNNER_SHA256__|$runner_hash|g" \
        -e "s|__ANNALS_DECISIONS_STATE__|$STATE_DIR|g" \
        -e "s|__ANNALS_DECISIONS_LOGS__|$LOG_DIR|g" \
        -e "s|__ANNALS_HOME__|$install_home|g" \
        -e "s|__ANNALS_USER__|$operator|g" \
        "$DEFINITION_TEMPLATE" >"$rendered_destination"
    chmod 0600 "$rendered_destination"
}

run_annals() {
    /usr/bin/env -i \
        HOME="$install_home" USER="$operator" LOGNAME="$operator" PATH="$PATH" \
        "$@"
}

inspect_binding() {
    observed_present=0
    observed_enabled=0
    observed_digest=
    if HOME="$install_home" "$clockwork_path" --json binding show \
        "$CLOCKWORK_KEY" >"$transaction_dir/binding.json" \
        2>"$transaction_dir/binding.stderr"
    then
        observed_present=1
        [ "$(plutil -extract ok raw "$transaction_dir/binding.json" 2>/dev/null)" = true ] \
            && [ "$(plutil -extract data.key raw "$transaction_dir/binding.json" 2>/dev/null)" = "$CLOCKWORK_KEY" ] \
            || return 1
        observed_enabled_raw=$(plutil -extract data.enabled raw \
            "$transaction_dir/binding.json" 2>/dev/null) || return 1
        case "$observed_enabled_raw" in
            true) observed_enabled=1 ;;
            false) observed_enabled=0 ;;
            *) return 1 ;;
        esac
        observed_compact=$(tr -d '[:space:]' <"$transaction_dir/binding.json")
        case "$observed_compact" in
            *'"definition_digest":null'*) ;;
            *'"definition_digest":"'*)
                observed_digest=$(printf '%s\n' "$observed_compact" | sed -n \
                    's/.*"definition_digest":"\([0-9a-f]\{64\}\)".*/\1/p')
                [ -n "$observed_digest" ] || return 1
                ;;
            *) return 1 ;;
        esac
        [ "$observed_enabled" -eq 0 ] || [ -n "$observed_digest" ] || return 1
        return 0
    fi
    tr -d '[:space:]' <"$transaction_dir/binding.stderr" \
        | grep -F '"code":"binding_not_found"' >/dev/null 2>&1
}

prove_selected_definition() {
    proved_digest=$1
    HOME="$install_home" "$clockwork_path" --json definition show "$proved_digest" \
        >"$transaction_dir/prior-definition.json" \
        2>"$transaction_dir/prior-definition.stderr" \
        || fail 'unable to inspect the selected decisions definition'
    proved="$transaction_dir/prior-definition.json"
    proved_root=$(plutil -extract data.manifest.release_root raw "$proved" 2>/dev/null) \
        || fail 'selected decisions definition has no release root'
    proved_release=$(plutil -extract data.manifest.release_id raw "$proved" 2>/dev/null) \
        || fail 'selected decisions definition has no release identity'
    validate_release "$proved_root"
    proved_runner_hash=$validated_runner_hash
    [ "$validated_release_id" = "$proved_release" ] \
        || fail 'selected decisions definition release identity changed'
    [ "$(plutil -extract ok raw "$proved" 2>/dev/null)" = true ] \
        && [ "$(plutil -extract data.digest raw "$proved" 2>/dev/null)" = "$proved_digest" ] \
        && [ "$(plutil -extract data.key raw "$proved" 2>/dev/null)" = "$CLOCKWORK_KEY" ] \
        && [ "$(plutil -extract data.manifest.schema_version raw "$proved" 2>/dev/null)" = 1 ] \
        && [ "$(plutil -extract data.manifest.key raw "$proved" 2>/dev/null)" = "$CLOCKWORK_KEY" ] \
        && [ "$(plutil -extract data.manifest.authority raw "$proved" 2>/dev/null)" = current-user-background ] \
        && [ "$(plutil -extract data.manifest.overlap raw "$proved" 2>/dev/null)" = skip ] \
        && [ "$(plutil -extract data.manifest.cwd raw "$proved" 2>/dev/null)" = "$STATE_DIR" ] \
        && [ "$(plutil -extract data.manifest.schedule.kind raw "$proved" 2>/dev/null)" = interval ] \
        && [ "$(plutil -extract data.manifest.schedule.seconds raw "$proved" 2>/dev/null)" = 300 ] \
        && [ "$(plutil -extract data.manifest.schedule.run_at_load raw "$proved" 2>/dev/null)" = true ] \
        && [ "$(plutil -extract data.manifest.launch.kind raw "$proved" 2>/dev/null)" = interpreted ] \
        && [ "$(plutil -extract data.manifest.launch.interpreter raw "$proved" 2>/dev/null)" = /bin/sh ] \
        && [ "$(plutil -extract data.manifest.launch.interpreter_sha256 raw "$proved" 2>/dev/null)" = "$interpreter_hash" ] \
        && [ "$(plutil -extract data.manifest.launch.script raw "$proved" 2>/dev/null)" = "$proved_root/bin/annals-inbox" ] \
        && [ "$(plutil -extract data.manifest.launch.script_sha256 raw "$proved" 2>/dev/null)" = "$proved_runner_hash" ] \
        && [ "$(plutil -extract data.manifest.environment.HOME raw "$proved" 2>/dev/null)" = "$install_home" ] \
        && [ "$(plutil -extract data.manifest.environment.USER raw "$proved" 2>/dev/null)" = "$operator" ] \
        && [ "$(plutil -extract data.manifest.environment.LOGNAME raw "$proved" 2>/dev/null)" = "$operator" ] \
        && [ "$(plutil -extract data.manifest.environment.ANNALS_CONFIG raw "$proved" 2>/dev/null)" = "$CONFIG_PATH" ] \
        && [ "$(plutil -extract data.manifest.output.stdout raw "$proved" 2>/dev/null)" = "$LOG_DIR/inbox.stdout.log" ] \
        && [ "$(plutil -extract data.manifest.output.stderr raw "$proved" 2>/dev/null)" = "$LOG_DIR/inbox.stderr.log" ] \
        || fail 'selected annals/decisions-inbox definition is foreign or changed'
    [ "$(xml_top_level_key_count data.manifest "$proved")" -eq 12 ] \
        && [ "$(xml_top_level_key_count data.manifest.schedule "$proved")" -eq 3 ] \
        && [ "$(xml_top_level_key_count data.manifest.launch "$proved")" -eq 5 ] \
        && [ "$(xml_top_level_key_count data.manifest.environment "$proved")" -eq 4 ] \
        && [ "$(xml_top_level_key_count data.manifest.output "$proved")" -eq 2 ] \
        || fail 'selected annals/decisions-inbox definition has foreign fields'
    if plutil -extract data.manifest.timeout_seconds raw "$proved" >/dev/null 2>&1 \
        || plutil -extract data.manifest.arguments.0 raw "$proved" >/dev/null 2>&1
    then
        fail 'selected annals/decisions-inbox definition adds a timeout or argument'
    fi
    prior_binary="$proved_root/libexec/annals"
}

binding_matches_prior() {
    [ "$observed_present" -eq "$prior_present" ] \
        && [ "$observed_enabled" -eq "$prior_enabled" ] \
        && [ "$observed_digest" = "$prior_digest" ]
}

selection_is_attributable() {
    [ -z "$observed_digest" ] \
        || [ "$observed_digest" = "$candidate_digest" ] \
        || { [ -n "$prior_digest" ] && [ "$observed_digest" = "$prior_digest" ]; }
}

restore_schedule() {
    inspect_binding || return 1
    selection_is_attributable || return 1
    if [ "$prior_present" -eq 0 ]; then
        # Clockwork itself can restore true absence from its transition journal.
        # Its public disable operation cannot erase a post-switch selection.
        [ "$observed_present" -eq 0 ] && return 0
        return 1
    fi
    if [ "$prior_enabled" -eq 1 ]; then
        if [ "$observed_enabled" -eq 1 ] \
            && [ "$observed_digest" = "$prior_digest" ]
        then
            return 0
        fi
        if [ "$observed_enabled" -eq 1 ]; then
            HOME="$install_home" "$clockwork_path" --json binding disable \
                "$CLOCKWORK_KEY" >/dev/null 2>&1 || return 1
        fi
        HOME="$install_home" "$clockwork_path" --json binding switch \
            "$CLOCKWORK_KEY" "$prior_digest" >/dev/null 2>&1 || return 1
    elif [ -n "$prior_digest" ]; then
        HOME="$install_home" "$clockwork_path" --json binding disable \
            "$CLOCKWORK_KEY" --select "$prior_digest" >/dev/null 2>&1 || return 1
    else
        # A failed Clockwork transition must have restored the nullable
        # selection itself; the public API deliberately cannot clear it.
        [ -z "$observed_digest" ] || return 1
        if [ "$observed_enabled" -eq 1 ]; then
            HOME="$install_home" "$clockwork_path" --json binding disable \
                "$CLOCKWORK_KEY" >/dev/null 2>&1 || return 1
        fi
    fi
    inspect_binding && binding_matches_prior
}

restore_file() {
    captured=$1
    destination=$2
    existed=$3
    if [ "$existed" -eq 1 ]; then
        install -m 0600 "$captured" "$destination"
    else
        rm -f "$destination"
    fi
}

cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    set +e
    rollback_ok=1
    retain_transaction=0
    if [ "$status" -ne 0 ] && [ "$committed" -eq 0 ]; then
        if [ "$schedule_touched" -eq 1 ]; then
            restore_schedule || rollback_ok=0
        fi
        if [ "$rollback_ok" -eq 1 ]; then
            if [ "$config_changed" -eq 1 ]; then
                restore_file "$transaction_dir/config.before" "$CONFIG_PATH" \
                    "$config_existed" || rollback_ok=0
            fi
            if [ "$library_may_need_restore" -eq 1 ]; then
                rm -f "$LIBRARY_PATH-wal" "$LIBRARY_PATH-shm"
                install -m 0600 "$library_backup" "$LIBRARY_PATH" \
                    || rollback_ok=0
            fi
            if [ "$state_published" -eq 1 ] && [ "$new_state" -eq 1 ]; then
                mv "$STATE_DIR" "$transaction_dir/failed-new-state" \
                    || rollback_ok=0
                state_published=0
            elif [ "$hold_changed" -eq 1 ]; then
                restore_file "$transaction_dir/hold.before" "$HOLD_RECEIPT" \
                    "$hold_existed" || rollback_ok=0
            fi
            if [ "$marker_created" -eq 1 ] && [ "$state_published" -eq 1 ]; then
                rm -f "$MAINTENANCE_MARKER" || rollback_ok=0
            fi
        fi
        if [ "$rollback_ok" -eq 0 ]; then
            retain_transaction=1
            if [ "$state_published" -eq 1 ] && [ -d "$SPOOL_DIR" ]; then
                : >"$MAINTENANCE_MARKER" || true
            fi
            if inspect_binding && selection_is_attributable \
                && [ "$observed_enabled" -eq 1 ] \
                && [ "$observed_digest" = "$candidate_digest" ]
            then
                HOME="$install_home" "$clockwork_path" --json binding disable \
                    "$CLOCKWORK_KEY" >/dev/null 2>&1 || true
            fi
            printf '%s\n' \
                'annals decisions provision: exact rollback could not be proved; decisions maintenance remains engaged and only an attributable candidate was disabled' >&2
            printf 'annals decisions provision: recovery transaction retained at %s\n' \
                "$transaction_dir" >&2
        fi
    fi
    if [ "$retain_transaction" -eq 0 ] && [ -n "$transaction_dir" ]; then
        rm -rf "$transaction_dir"
    fi
    if [ "$lock_created" -eq 1 ]; then
        rmdir "$UPDATE_LOCK" >/dev/null 2>&1 || true
    fi
    exit "$status"
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --release-root)
            [ "$#" -ge 2 ] || fail '--release-root requires a path'
            release_root=$2
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
        --keep-maintenance)
            keep_maintenance=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            usage >&2
            fail "unknown argument: $1"
            ;;
    esac
done

[ "$(uname -s)" = Darwin ] || fail 'the decisions provisioner supports macOS only'
[ -n "$release_root" ] || fail '--release-root is required'
[ -n "$nucleus_socket" ] || fail '--nucleus-socket is required'
[ -n "$clockwork_path" ] || fail '--clockwork is required'
[ -n "$install_home" ] || fail 'HOME or --home is required'
for required_path in "$release_root" "$nucleus_socket" "$clockwork_path" "$install_home"; do
    absolute_path "$required_path" || fail "path must be absolute: $required_path"
    renderable_value "$required_path" \
        || fail "path cannot be represented safely: $required_path"
done
[ -f "$clockwork_path" ] && [ -x "$clockwork_path" ] \
    || fail "Clockwork executable is unavailable: $clockwork_path"
[ -d "$install_home" ] && [ ! -L "$install_home" ] \
    || fail "operator home is unavailable: $install_home"
[ "$(CDPATH='' cd "$install_home" && pwd -P)" = "$install_home" ] \
    || fail "operator home contains a symbolic selector: $install_home"
operator=$(id -un)
operator_uid=$(id -u)
[ "$operator_uid" -ne 0 ] || fail 'the decisions provisioner must not run as root'
[ "$(stat -f '%u' "$install_home")" -eq "$operator_uid" ] \
    || fail "operator does not own home: $install_home"
renderable_value "$operator" || fail 'operator name cannot be represented safely'
[ -f "$PROVISIONER_SELF" ] && [ ! -L "$PROVISIONER_SELF" ] \
    || fail "invalid decisions provisioner image: $PROVISIONER_SELF"
for command in awk basename cmp cp date find grep id install mkdir mv plutil sed shasum sort stat tr uname; do
    command -v "$command" >/dev/null 2>&1 \
        || fail "required command not found: $command"
done

validate_release "$release_root"
release_id=$validated_release_id
runner_hash=$validated_runner_hash
[ "$(shasum -a 256 "$PROVISIONER_SELF" | awk '{print $1}')" = \
    "$validated_provisioner_hash" ] \
    || fail 'invoked decisions provisioner does not belong to the selected release'
CONFIG_TEMPLATE="$release_root/package/annals-decisions.toml.in"
DEFINITION_TEMPLATE="$release_root/package/annals-decisions-inbox.clockwork.toml.in"
interpreter_hash=$(shasum -a 256 /bin/sh | awk '{print $1}')
ANNALS_BASE="$install_home/Library/Application Support/Annals"
INSTALL_DIR="$ANNALS_BASE/install"
UPDATE_LOCK="$INSTALL_DIR/.update-lock"
STATE_DIR="$ANNALS_BASE/decisions"
CONFIG_PATH="$STATE_DIR/config.toml"
LIBRARY_PATH="$STATE_DIR/annals.db"
SPOOL_DIR="$STATE_DIR/spool"
LOG_DIR="$STATE_DIR/log"
STDOUT_LOG="$LOG_DIR/inbox.stdout.log"
STDERR_LOG="$LOG_DIR/inbox.stderr.log"
BACKUPS_DIR="$STATE_DIR/backups"
MAINTENANCE_MARKER="$SPOOL_DIR/.maintenance"
HOLD_RECEIPT="$STATE_DIR/.provision-maintenance.json"

for rendered_value in "$STATE_DIR" "$CONFIG_PATH" "$LOG_DIR"; do
    renderable_value "$rendered_value" \
        || fail "installation path cannot be represented safely: $rendered_value"
done
if [ -e "$ANNALS_BASE" ]; then
    [ -d "$ANNALS_BASE" ] && [ ! -L "$ANNALS_BASE" ] \
        || fail "invalid Annals state root: $ANNALS_BASE"
else
    install -d -m 0700 "$ANNALS_BASE"
fi
if [ -e "$INSTALL_DIR" ]; then
    [ -d "$INSTALL_DIR" ] && [ ! -L "$INSTALL_DIR" ] \
        || fail "invalid Annals install root: $INSTALL_DIR"
else
    install -d -m 0700 "$INSTALL_DIR"
fi
mkdir "$UPDATE_LOCK" 2>/dev/null \
    || fail "another Annals deployment holds the update lock: $UPDATE_LOCK"
lock_created=1
transaction_dir="$INSTALL_DIR/.decisions-transaction.$$"
install -d -m 0700 "$transaction_dir"

committed=0
schedule_touched=0
candidate_digest=
prior_present=0
prior_enabled=0
prior_digest=
prior_binary=
new_state=0
state_published=0
marker_created=0
marker_owned=0
hold_existed=0
hold_changed=0
config_existed=0
config_changed=0
library_may_need_restore=0
library_backup=
retain_transaction=0
trap cleanup EXIT HUP INT TERM

inspect_binding || fail 'unable to inspect the decisions Clockwork binding'
prior_present=$observed_present
prior_enabled=$observed_enabled
prior_digest=$observed_digest
if [ -n "$prior_digest" ]; then
    prove_selected_definition "$prior_digest"
fi

if [ -e "$STATE_DIR" ]; then
    [ -d "$STATE_DIR" ] && [ ! -L "$STATE_DIR" ] \
        || fail "invalid decisions state root: $STATE_DIR"
    for state_path in "$CONFIG_PATH" "$LIBRARY_PATH" "$SPOOL_DIR" "$LOG_DIR" "$BACKUPS_DIR"; do
        [ -e "$state_path" ] && [ ! -L "$state_path" ] \
            || fail "incomplete or symbolic decisions state: $state_path"
    done
    [ -f "$CONFIG_PATH" ] && [ -f "$LIBRARY_PATH" ] \
        && [ -d "$SPOOL_DIR" ] && [ -d "$LOG_DIR" ] && [ -d "$BACKUPS_DIR" ] \
        || fail "incomplete decisions state: $STATE_DIR"
    state_published=1
    [ "$(stat -f '%u' "$STATE_DIR")" -eq "$operator_uid" ] \
        || fail 'decisions state ownership is invalid'
    for private_dir in "$STATE_DIR" "$SPOOL_DIR" "$LOG_DIR" "$BACKUPS_DIR"; do
        [ "$(stat -f '%u' "$private_dir")" -eq "$operator_uid" ] \
            && [ "$(stat -f '%Lp' "$private_dir")" = 700 ] \
            || fail "decisions directory ownership or mode is invalid: $private_dir"
    done
    validate_decisions_mutable_files
    prepare_output_files
    [ "$(grep -Fxc "library = \"$LIBRARY_PATH\"" "$CONFIG_PATH")" -eq 1 ] \
        && [ "$(grep -Fxc "root = \"$SPOOL_DIR\"" "$CONFIG_PATH")" -eq 1 ] \
        && [ "$(grep -Fxc '[decision_feed]' "$CONFIG_PATH")" -eq 1 ] \
        || fail 'decisions config does not select the exact dedicated state'
    library_id=$(sed -n \
        's/^expected_library_id = "\([0-9a-f]\{32\}\)"$/\1/p' "$CONFIG_PATH")
    [ "${#library_id}" -eq 32 ] \
        || fail 'decisions config has no exact persistent library identity'
    watermark=$(run_annals "$release_root/libexec/annals" \
        --config "$CONFIG_PATH" --json decision-feed watermark) \
        || fail 'candidate cannot verify the decisions library identity'
    printf '%s\n' "$watermark" >"$transaction_dir/watermark.json"
    [ "$(plutil -extract ok raw "$transaction_dir/watermark.json" 2>/dev/null)" = true ] \
        && [ "$(plutil -extract data.library_id raw "$transaction_dir/watermark.json" 2>/dev/null)" = "$library_id" ] \
        || fail 'candidate returned the wrong decisions library identity'
    config_existed=1
    install -m 0600 "$CONFIG_PATH" "$transaction_dir/config.before"
    if [ -e "$HOLD_RECEIPT" ]; then
        validate_private_file "$HOLD_RECEIPT" 'decisions maintenance receipt'
        validate_private_file "$MAINTENANCE_MARKER" \
            'decisions maintenance receipt gate'
        [ "$(plutil -extract version raw "$HOLD_RECEIPT" 2>/dev/null)" = 1 ] \
            && [ "$(plutil -extract key raw "$HOLD_RECEIPT" 2>/dev/null)" = "$CLOCKWORK_KEY" ] \
            && [ "$(plutil -extract library_id raw "$HOLD_RECEIPT" 2>/dev/null)" = "$library_id" ] \
            && [ "$(plutil -extract definition_digest raw "$HOLD_RECEIPT" 2>/dev/null)" = "$prior_digest" ] \
            || fail 'decisions maintenance receipt does not match current owned state'
        hold_existed=1
        marker_owned=1
        install -m 0600 "$HOLD_RECEIPT" "$transaction_dir/hold.before"
    elif [ -e "$MAINTENANCE_MARKER" ]; then
        validate_private_file "$MAINTENANCE_MARKER" \
            'decisions maintenance marker'
    fi
else
    [ "$prior_present" -eq 0 ] \
        || fail 'decisions binding exists without its dedicated state'
    new_state=1
    stage="$transaction_dir/decisions-state"
    install -d -m 0700 "$stage" "$stage/log" "$stage/backups"
    initialized=$(run_annals "$release_root/libexec/annals" \
        --library "$stage/annals.db" --json init --kind decisions) \
        || fail 'unable to initialize the decisions library'
    printf '%s\n' "$initialized" >"$transaction_dir/init.json"
    library_id=$(plutil -extract data.library_id raw \
        "$transaction_dir/init.json" 2>/dev/null) \
        || fail 'Annals init returned no persistent library identity'
    [ "$(plutil -extract ok raw "$transaction_dir/init.json" 2>/dev/null)" = true ] \
        && [ "$(plutil -extract data.kind raw "$transaction_dir/init.json" 2>/dev/null)" = decisions ] \
        && [ "${#library_id}" -eq 32 ] \
        || fail 'Annals init returned an invalid persistent library identity'
    case "$library_id" in *[!0-9a-f]*) fail 'Annals init returned an invalid persistent library identity' ;; esac
    render_config "$stage" "$library_id" "$nucleus_socket" "$stage/config.toml"
    initial_run=$(run_annals "$release_root/libexec/annals" \
        --config "$stage/config.toml" --json inbox run) \
        || fail 'unable to bind the fresh decisions spool'
    printf '%s\n' "$initial_run" >"$transaction_dir/initial-run.json"
    [ "$(plutil -extract ok raw "$transaction_dir/initial-run.json" 2>/dev/null)" = true ] \
        || fail 'fresh decisions spool binding returned an invalid result'
    [ -f "$stage/spool/.decision-feed-library.json" ] \
        && [ ! -L "$stage/spool/.decision-feed-library.json" ] \
        || fail 'fresh decisions spool has no persistent library binding'
    render_config "$STATE_DIR" "$library_id" "$nucleus_socket" "$stage/config.toml"
    : >"$stage/spool/.maintenance"
    marker_created=1
    marker_owned=1
    mv "$stage" "$STATE_DIR"
    state_published=1
    validate_decisions_mutable_files
    prepare_output_files
fi

if [ ! -e "$MAINTENANCE_MARKER" ]; then
    : >"$MAINTENANCE_MARKER"
    marker_created=1
    marker_owned=1
fi
validate_private_file "$MAINTENANCE_MARKER" 'decisions maintenance gate'

definition="$transaction_dir/annals-decisions-inbox.clockwork.toml"
render_definition "$definition"
registered=$(HOME="$install_home" "$clockwork_path" --json \
    definition register "$definition") \
    || fail 'Clockwork rejected the candidate decisions definition'
printf '%s\n' "$registered" >"$transaction_dir/registered.json"
candidate_digest=$(plutil -extract data.digest raw \
    "$transaction_dir/registered.json" 2>/dev/null) \
    || fail 'Clockwork returned no candidate decisions definition digest'
[ "$(plutil -extract ok raw "$transaction_dir/registered.json" 2>/dev/null)" = true ] \
    && [ "$(plutil -extract data.key raw "$transaction_dir/registered.json" 2>/dev/null)" = "$CLOCKWORK_KEY" ] \
    && [ "${#candidate_digest}" -eq 64 ] \
    || fail 'Clockwork returned an invalid candidate decisions definition'
case "$candidate_digest" in *[!0-9a-f]*) fail 'Clockwork returned an invalid candidate decisions definition' ;; esac

inspect_binding || fail 'unable to re-inspect the decisions binding before handoff'
binding_matches_prior || fail 'decisions binding changed after its ownership check'
if [ "$prior_enabled" -eq 1 ]; then
    schedule_touched=1
    HOME="$install_home" "$clockwork_path" --json binding disable \
        "$CLOCKWORK_KEY" >/dev/null \
        || fail 'unable to disable and drain the decisions binding'
fi

wait_seconds=${ANNALS_DECISIONS_UPDATE_WAIT_SECONDS:-3900}
case "$wait_seconds" in
    ''|*[!0-9]*) fail 'ANNALS_DECISIONS_UPDATE_WAIT_SECONDS must be a nonnegative integer' ;;
esac
waited=0
while :; do
    status_json=$(run_annals "$release_root/libexec/annals" \
        --config "$CONFIG_PATH" --json inbox status) \
        || fail 'unable to inspect the decisions inbox'
    printf '%s\n' "$status_json" >"$transaction_dir/status.json"
    if [ "$(plutil -extract data.locked raw "$transaction_dir/status.json" 2>/dev/null)" = false ]; then
        break
    fi
    [ "$waited" -lt "$wait_seconds" ] \
        || fail "decisions inbox did not become idle within $wait_seconds seconds"
    sleep 1
    waited=$((waited + 1))
done

if [ "$new_state" -eq 0 ]; then
    backup_stamp=$(date -u '+%Y%m%dT%H%M%SZ')
    library_backup="$BACKUPS_DIR/pre-provision-$release_id-$backup_stamp-$$.db"
    backup_binary=$release_root/libexec/annals
    if [ -n "$prior_binary" ]; then
        backup_binary=$prior_binary
    fi
    run_annals "$backup_binary" --config "$CONFIG_PATH" --quiet backup \
        "$library_backup" \
        || fail 'unable to capture a consistent decisions-library backup'
    library_may_need_restore=1
    run_annals "$release_root/libexec/annals" --config "$CONFIG_PATH" --quiet migrate \
        || fail 'unable to migrate the decisions library'
    next_config="$transaction_dir/config.next"
    render_config "$STATE_DIR" "$library_id" "$nucleus_socket" "$next_config"
    config_changed=1
    mv "$next_config" "$CONFIG_PATH"
fi

smoke=$(run_annals "$release_root/libexec/annals" \
    --config "$CONFIG_PATH" --json inbox run) \
    || fail 'candidate cannot verify the gated decisions inbox'
printf '%s\n' "$smoke" >"$transaction_dir/smoke.json"
[ "$(plutil -extract ok raw "$transaction_dir/smoke.json" 2>/dev/null)" = true ] \
    && [ "$(plutil -extract data.stopped_for_maintenance raw "$transaction_dir/smoke.json" 2>/dev/null)" = true ] \
    || fail 'candidate did not honor decisions maintenance'
watermark=$(run_annals "$release_root/libexec/annals" \
    --config "$CONFIG_PATH" --json decision-feed watermark) \
    || fail 'candidate cannot read the decisions feed'
printf '%s\n' "$watermark" >"$transaction_dir/final-watermark.json"
[ "$(plutil -extract ok raw "$transaction_dir/final-watermark.json" 2>/dev/null)" = true ] \
    && [ "$(plutil -extract data.library_id raw "$transaction_dir/final-watermark.json" 2>/dev/null)" = "$library_id" ] \
    || fail 'candidate decisions feed returned the wrong library identity'

# Record ownership before the switch even for the default release path. If the
# process is interrupted after Clockwork commits but before gate removal, the
# next invocation can prove and release this exact hold instead of mistaking it
# for an operator-owned maintenance gate.
if [ "$marker_owned" -eq 1 ]; then
    hold_next="$transaction_dir/hold.next"
    {
        printf '{\n'
        printf '  "version": 1,\n'
        printf '  "key": "%s",\n' "$CLOCKWORK_KEY"
        printf '  "library_id": "%s",\n' "$library_id"
        printf '  "definition_digest": "%s"\n' "$candidate_digest"
        printf '}\n'
    } >"$hold_next"
    chmod 0600 "$hold_next"
    hold_changed=1
    mv "$hold_next" "$HOLD_RECEIPT"
fi

inspect_binding || fail 'unable to inspect the disabled decisions binding before cutover'
if [ "$prior_enabled" -eq 1 ]; then
    [ "$observed_present" -eq 1 ] && [ "$observed_enabled" -eq 0 ] \
        && [ "$observed_digest" = "$prior_digest" ] \
        || fail 'decisions binding changed while it was disabled'
else
    binding_matches_prior || fail 'inactive decisions binding changed before cutover'
fi
schedule_touched=1
if ! HOME="$install_home" "$clockwork_path" --json binding switch \
    "$CLOCKWORK_KEY" "$candidate_digest" >"$transaction_dir/switched.json"
then
    fail 'Clockwork rejected the decisions binding switch'
fi
[ "$(plutil -extract ok raw "$transaction_dir/switched.json" 2>/dev/null)" = true ] \
    && [ "$(plutil -extract data.key raw "$transaction_dir/switched.json" 2>/dev/null)" = "$CLOCKWORK_KEY" ] \
    && [ "$(plutil -extract data.definition_digest raw "$transaction_dir/switched.json" 2>/dev/null)" = "$candidate_digest" ] \
    && [ "$(plutil -extract data.enabled raw "$transaction_dir/switched.json" 2>/dev/null)" = true ] \
    || fail 'Clockwork returned an invalid decisions binding state'

# The coherent successful Clockwork transition is the commit boundary. Every
# product-domain check was completed behind maintenance before this point.
committed=1
library_may_need_restore=0

if [ "$keep_maintenance" -eq 0 ] && [ "$marker_owned" -eq 1 ]; then
    rm -f "$HOLD_RECEIPT"
    rm -f "$MAINTENANCE_MARKER"
fi
if [ -e "$MAINTENANCE_MARKER" ]; then
    maintenance_json=true
else
    maintenance_json=false
fi

completed_at=$(date -u '+%Y-%m-%dT%H:%M:%SZ')
receipt_tmp="$transaction_dir/last-provision.json"
{
    printf '{\n'
    printf '  "contract_version": 1,\n'
    printf '  "config": "%s",\n' "$CONFIG_PATH"
    printf '  "library_id": "%s",\n' "$library_id"
    printf '  "clockwork_key": "%s",\n' "$CLOCKWORK_KEY"
    printf '  "clockwork_definition": "%s",\n' "$candidate_digest"
    printf '  "selected": true,\n'
    printf '  "enabled": true,\n'
    printf '  "maintenance": %s,\n' "$maintenance_json"
    printf '  "release_id": "%s",\n' "$release_id"
    printf '  "previous_present": %s,\n' \
        "$( [ "$prior_present" -eq 1 ] && printf true || printf false )"
    printf '  "previous_enabled": %s,\n' \
        "$( [ "$prior_enabled" -eq 1 ] && printf true || printf false )"
    if [ -n "$prior_digest" ]; then
        printf '  "previous_definition": "%s",\n' "$prior_digest"
    else
        printf '  "previous_definition": null,\n'
    fi
    printf '  "completed_at": "%s"\n' "$completed_at"
    printf '}\n'
} >"$receipt_tmp"
chmod 0600 "$receipt_tmp"
install -m 0600 "$receipt_tmp" "$BACKUPS_DIR/last-provision.json.tmp.$$"
mv "$BACKUPS_DIR/last-provision.json.tmp.$$" "$BACKUPS_DIR/last-provision.json"

printf '{"ok":true,"data":{'
printf '"contract_version":1,'
printf '"config":"%s",' "$CONFIG_PATH"
printf '"library_id":"%s",' "$library_id"
printf '"clockwork_key":"%s",' "$CLOCKWORK_KEY"
printf '"clockwork_definition":"%s",' "$candidate_digest"
printf '"selected":true,"enabled":true,'
printf '"maintenance":%s,' "$maintenance_json"
printf '"release_id":"%s"' "$release_id"
printf '}}\n'
