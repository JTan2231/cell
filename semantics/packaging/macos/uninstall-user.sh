#!/bin/sh

set -eu

PATH=/usr/bin:/bin:/usr/sbin:/sbin
export PATH
umask 077

install_home=${HOME:-}
launchctl_path=/bin/launchctl
clockwork_path=

fail() {
    printf 'semantics uninstall: %s\n' "$*" >&2
    exit 1
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --home) [ "$#" -ge 2 ] || fail '--home requires a path'; install_home=$2; shift 2 ;;
        --launchctl) [ "$#" -ge 2 ] || fail '--launchctl requires a path'; launchctl_path=$2; shift 2 ;;
        --clockwork) [ "$#" -ge 2 ] || fail '--clockwork requires a path'; clockwork_path=$2; shift 2 ;;
        *) fail "unknown argument: $1" ;;
    esac
done
case "$install_home" in /*) ;; *) fail 'home must be absolute' ;; esac
case "$install_home" in *'|'*|*'
'*) fail 'home contains unsupported characters' ;; esac
case "$launchctl_path" in /*) ;; *) fail 'launchctl must be absolute' ;; esac
[ -n "$clockwork_path" ] || fail '--clockwork is required'
case "$clockwork_path" in /*) ;; *) fail 'clockwork must be absolute' ;; esac
[ -d "$install_home" ] && [ ! -L "$install_home" ] || fail 'home is not a regular directory'
operator_uid=$(id -u)
[ "$operator_uid" -ne 0 ] || fail 'run as the Semantics operator, not root'
[ "$(stat -f '%u' "$install_home")" -eq "$operator_uid" ] \
    || fail 'home is not owned by the Semantics operator'
[ -x "$launchctl_path" ] && [ ! -L "$launchctl_path" ] || fail 'launchctl is unavailable'
[ -e "$clockwork_path" ] && [ -x "$clockwork_path" ] || fail 'Clockwork is unavailable'

label=org.semantics.worker
clockwork_key=semantics/worker
state="$install_home/Library/Application Support/Semantics"
maintenance="$state/.clockwork-maintenance"
install_dir="$state/install"
logs="$install_home/Library/Logs/Semantics"
current="$install_dir/current"
cli="$install_home/.local/bin/semantics"
provider="$install_home/Library/Application Support/Chancery/providers/semantics"
plist="$install_home/Library/LaunchAgents/$label.plist"
expected_cli="$install_dir/current/bin/semantics"
expected_provider="$install_dir/current/share/chancery/semantics"
service_domain="gui/$operator_uid"
service_target="$service_domain/$label"
clockwork_label=org.clockwork.semantics.worker
clockwork_target="$service_domain/$clockwork_label"
clockwork_plist="$install_home/Library/LaunchAgents/$clockwork_label.plist"
lock_dir="$install_dir/.update-lock"

for directory in "$state" "$install_dir"; do
    [ ! -L "$directory" ] || fail "refusing symbolic-link directory: $directory"
    [ ! -e "$directory" ] || [ -d "$directory" ] || fail "directory path is occupied: $directory"
    [ -d "$directory" ] || install -d -m 0700 "$directory"
done
# Defer catchable termination across the atomic mkdir until cleanup owns the
# newly acquired directory lock.
trap '' HUP INT TERM
mkdir "$lock_dir" 2>/dev/null || fail 'a Semantics deployment or uninstall is active'
clockwork_inspection=
cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    [ -z "$clockwork_inspection" ] || rm -rf "$clockwork_inspection"
    rmdir "$lock_dir" >/dev/null 2>&1 || true
    exit "$status"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

clockwork_inspection=$(mktemp -d "$install_dir/.clockwork-inspection.XXXXXX") \
    || fail 'unable to create the Clockwork inspection directory'

maintenance_marker_is_owned() {
    [ -f "$maintenance" ] && [ ! -L "$maintenance" ] \
        && [ "$(stat -f '%u' "$maintenance")" -eq "$operator_uid" ] \
        && [ "$(stat -f '%Lp' "$maintenance")" = 600 ] \
        && [ "$(stat -f '%l' "$maintenance")" -eq 1 ]
}

engage_maintenance() {
    if [ -L "$maintenance" ] || { [ -e "$maintenance" ] && [ ! -f "$maintenance" ]; }; then
        return 1
    fi
    if [ -e "$maintenance" ]; then
        maintenance_marker_is_owned
        return
    fi
    (set -C; : >"$maintenance") || return 1
    chmod 0600 "$maintenance" || return 1
    maintenance_marker_is_owned
}

validate_bundle() {
    bundle=$1
    [ -d "$bundle" ] && [ ! -L "$bundle" ] \
        || fail "Chancery provider is not a regular directory: $bundle"
    [ "$(find "$bundle" -type f | awk 'END { print NR }')" -eq 7 ] \
        || fail 'current Semantics provider has unexpected files'
    if find "$bundle" -type l -print | grep -q .; then fail 'current Semantics provider contains a symbolic link'; fi
    if find "$bundle" ! -type d ! -type f -print | grep -q .; then fail 'current Semantics provider contains a non-file entry'; fi
    for relative in \
        provider.json \
        entries/repository-explore.json entries/project-operate.json entries/develop-change.json \
        manuals/repository-explore.md manuals/project-operate.md manuals/develop-change.md
    do
        [ -f "$bundle/$relative" ] && [ ! -L "$bundle/$relative" ] \
            || fail "current Semantics provider is incomplete: $relative"
    done
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
        || fail "current selector is not owned by Semantics: $selector"
    release="$install_dir/$selector"
    release_id=${selector#releases/}
    manifest="$release/manifest.txt"
    [ -d "$release" ] && [ ! -L "$release" ] || fail 'current Semantics release is unavailable'
    [ -f "$manifest" ] && [ ! -L "$manifest" ] || fail 'current Semantics release has no owned manifest'
    [ "$(awk 'END { print NR }' "$manifest")" -eq 10 ] || fail 'current Semantics release manifest is not canonical'
    manifest_format=$(sed -n '1s/^format=//p' "$manifest")
    case "$manifest_format" in 1|2) ;; *) fail 'current Semantics release manifest format is unsupported' ;; esac
    manifest_release=$(sed -n '2s/^release_id=//p' "$manifest")
    manifest_version=$(sed -n '3s/^version=//p' "$manifest")
    manifest_binary_hash=$(sed -n '4s/^binary_sha256=//p' "$manifest")
    manifest_frontend_hash=$(sed -n '5s/^frontend_sha256=//p' "$manifest")
    manifest_runner_hash=$(sed -n '6s/^runner_sha256=//p' "$manifest")
    if [ "$manifest_format" -eq 1 ]; then
        manifest_schedule_hash=$(sed -n '7s/^plist_sha256=//p' "$manifest")
    else
        manifest_schedule_hash=$(sed -n '7s/^clockwork_template_sha256=//p' "$manifest")
    fi
    manifest_deployer_hash=$(sed -n '8s/^deployer_sha256=//p' "$manifest")
    manifest_uninstaller_hash=$(sed -n '9s/^uninstaller_sha256=//p' "$manifest")
    manifest_chancery_hash=$(sed -n '10s/^chancery_sha256=//p' "$manifest")
    printf '%s\n' "$manifest_release" "$manifest_binary_hash" "$manifest_frontend_hash" \
        "$manifest_runner_hash" "$manifest_schedule_hash" "$manifest_deployer_hash" \
        "$manifest_uninstaller_hash" "$manifest_chancery_hash" \
        | grep -Eqv '^[0-9a-f]{64}$' && fail 'current Semantics release manifest hashes are invalid'
    printf '%s\n' "$manifest_version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+([+-][0-9A-Za-z.-]+)?$' \
        || fail 'current Semantics release version is invalid'
    [ "$manifest_release" = "$release_id" ] || fail 'current Semantics release manifest does not match'
    for owned_file in \
        "$release/libexec/semantics" "$release/bin/semantics" "$release/bin/semantics-worker" \
        "$release/package/semantics" "$release/package/semantics-worker" \
        "$release/package/deploy-user.sh" "$release/package/uninstall-user.sh"
    do
        [ -f "$owned_file" ] && [ ! -L "$owned_file" ] || fail 'current Semantics release is incomplete'
    done
    if [ "$manifest_format" -eq 1 ]; then
        schedule_file="$release/package/$label.plist"
    else
        schedule_file="$release/package/semantics-worker.clockwork.toml.in"
    fi
    [ -f "$schedule_file" ] && [ ! -L "$schedule_file" ] || fail 'current Semantics release has no schedule template'
    validate_bundle "$release/share/chancery/semantics"
    actual_binary_hash=$(shasum -a 256 "$release/libexec/semantics" | awk '{print $1}')
    actual_frontend_hash=$(shasum -a 256 "$release/bin/semantics" | awk '{print $1}')
    actual_runner_hash=$(shasum -a 256 "$release/bin/semantics-worker" | awk '{print $1}')
    actual_schedule_hash=$(shasum -a 256 "$schedule_file" | awk '{print $1}')
    actual_deployer_hash=$(shasum -a 256 "$release/package/deploy-user.sh" | awk '{print $1}')
    actual_uninstaller_hash=$(shasum -a 256 "$release/package/uninstall-user.sh" | awk '{print $1}')
    actual_chancery_hash=$(bundle_hash "$release/share/chancery/semantics")
    [ "$actual_binary_hash" = "$manifest_binary_hash" ] || fail 'current Semantics binary is tampered'
    [ "$actual_frontend_hash" = "$manifest_frontend_hash" ] || fail 'current Semantics frontend is tampered'
    [ "$(shasum -a 256 "$release/package/semantics" | awk '{print $1}')" = "$manifest_frontend_hash" ] || fail 'current packaged frontend is tampered'
    [ "$actual_runner_hash" = "$manifest_runner_hash" ] || fail 'current Semantics runner is tampered'
    [ "$(shasum -a 256 "$release/package/semantics-worker" | awk '{print $1}')" = "$manifest_runner_hash" ] || fail 'current packaged runner is tampered'
    [ "$actual_schedule_hash" = "$manifest_schedule_hash" ] || fail 'current Semantics schedule template is tampered'
    [ "$actual_deployer_hash" = "$manifest_deployer_hash" ] || fail 'current Semantics deployer is tampered'
    [ "$actual_uninstaller_hash" = "$manifest_uninstaller_hash" ] || fail 'current Semantics uninstaller is tampered'
    [ "$actual_chancery_hash" = "$manifest_chancery_hash" ] || fail 'current Semantics provider is tampered'
    actual_release_id=$(printf '%s\n' "$actual_binary_hash" "$actual_frontend_hash" \
        "$actual_runner_hash" "$actual_schedule_hash" "$actual_deployer_hash" \
        "$actual_uninstaller_hash" "$actual_chancery_hash" | shasum -a 256 | awk '{print $1}')
    [ "$actual_release_id" = "$release_id" ] || fail 'current Semantics release content ID does not match'
}

inspect_owned_clockwork_binding() {
    owned_clockwork_digest=
    binding_show="$clockwork_inspection/binding.json"
    if HOME="$install_home" "$clockwork_path" --json binding show "$clockwork_key" \
        >"$binding_show" 2>"$binding_show.stderr"
    then
        binding_compact=$(tr -d '[:space:]' <"$binding_show")
        case "$binding_compact" in
            *'"enabled":true'*) binding_enabled=1 ;;
            *'"enabled":false'*) binding_enabled=0 ;;
            *) fail 'Clockwork returned invalid Semantics binding state' ;;
        esac
        binding_digest=$(printf '%s\n' "$binding_compact" | sed -n \
            's/.*"definition_digest":"\([0-9a-f]\{64\}\)".*/\1/p')
        if [ -z "$binding_digest" ]; then
            [ "$binding_enabled" -eq 0 ] \
                && printf '%s\n' "$binding_compact" | grep -F '"definition_digest":null' >/dev/null \
                || fail 'Semantics Clockwork binding has an invalid definition digest'
            return
        fi
    else
        grep -F '"code":"binding_not_found"' "$binding_show.stderr" >/dev/null \
            || fail 'unable to inspect the Semantics Clockwork binding'
        return
    fi

    [ -n "${current_clockwork_release:-}" ] \
        || fail 'selected Clockwork binding has no current Semantics release'
    [ "$current_release_format" -eq 2 ] \
        || fail 'selected Clockwork binding cannot be owned by a legacy Semantics release'
    interpreter_hash=$(shasum -a 256 /bin/sh | awk '{print $1}')
    definition_show="$clockwork_inspection/definition.json"
    HOME="$install_home" "$clockwork_path" --json definition show "$binding_digest" \
        >"$definition_show" 2>"$definition_show.stderr" \
        || fail 'unable to inspect the selected Semantics Clockwork definition'
    [ "$(/usr/bin/plutil -extract ok raw "$definition_show" 2>/dev/null)" = true ] \
        && [ "$(/usr/bin/plutil -extract data.digest raw "$definition_show" 2>/dev/null)" = "$binding_digest" ] \
        && [ "$(/usr/bin/plutil -extract data.key raw "$definition_show" 2>/dev/null)" = "$clockwork_key" ] \
        && [ "$(/usr/bin/plutil -extract data.manifest.schema_version raw "$definition_show" 2>/dev/null)" = 1 ] \
        && [ "$(/usr/bin/plutil -extract data.manifest.key raw "$definition_show" 2>/dev/null)" = "$clockwork_key" ] \
        && [ "$(/usr/bin/plutil -extract data.manifest.release_id raw "$definition_show" 2>/dev/null)" = "$current_clockwork_release_id" ] \
        && [ "$(/usr/bin/plutil -extract data.manifest.release_root raw "$definition_show" 2>/dev/null)" = "$current_clockwork_release" ] \
        && [ "$(/usr/bin/plutil -extract data.manifest.authority raw "$definition_show" 2>/dev/null)" = current-user-background ] \
        && [ "$(/usr/bin/plutil -extract data.manifest.overlap raw "$definition_show" 2>/dev/null)" = skip ] \
        && [ "$(/usr/bin/plutil -extract data.manifest.cwd raw "$definition_show" 2>/dev/null)" = "$state" ] \
        && [ "$(/usr/bin/plutil -extract data.manifest.schedule.kind raw "$definition_show" 2>/dev/null)" = interval ] \
        && [ "$(/usr/bin/plutil -extract data.manifest.schedule.seconds raw "$definition_show" 2>/dev/null)" = 60 ] \
        && [ "$(/usr/bin/plutil -extract data.manifest.schedule.run_at_load raw "$definition_show" 2>/dev/null)" = false ] \
        && [ "$(/usr/bin/plutil -extract data.manifest.launch.kind raw "$definition_show" 2>/dev/null)" = interpreted ] \
        && [ "$(/usr/bin/plutil -extract data.manifest.launch.interpreter raw "$definition_show" 2>/dev/null)" = /bin/sh ] \
        && [ "$(/usr/bin/plutil -extract data.manifest.launch.interpreter_sha256 raw "$definition_show" 2>/dev/null)" = "$interpreter_hash" ] \
        && [ "$(/usr/bin/plutil -extract data.manifest.launch.script raw "$definition_show" 2>/dev/null)" = "$current_clockwork_release/bin/semantics-worker" ] \
        && [ "$(/usr/bin/plutil -extract data.manifest.launch.script_sha256 raw "$definition_show" 2>/dev/null)" = "$current_clockwork_runner_hash" ] \
        && [ "$(/usr/bin/plutil -extract data.manifest.environment.HOME raw "$definition_show" 2>/dev/null)" = "$install_home" ] \
        && [ "$(/usr/bin/plutil -extract data.manifest.output.stdout raw "$definition_show" 2>/dev/null)" = "$logs/worker.stdout.log" ] \
        && [ "$(/usr/bin/plutil -extract data.manifest.output.stderr raw "$definition_show" 2>/dev/null)" = "$logs/worker.stderr.log" ] \
        || fail 'selected Clockwork definition is not owned by the current Semantics release'
    if /usr/bin/plutil -extract data.manifest.timeout_seconds raw "$definition_show" >/dev/null 2>&1 \
        || /usr/bin/plutil -extract data.manifest.arguments.0 raw "$definition_show" >/dev/null 2>&1; then
        fail 'selected Clockwork definition adds unsupported timeout or arguments'
    fi
    environment_keys=$(/usr/bin/plutil -extract data.manifest.environment xml1 -o - \
        "$definition_show" 2>/dev/null | awk '/<key>/{count++} END {print count+0}')
    [ "$environment_keys" -eq 1 ] \
        || fail 'selected Clockwork definition contains foreign environment entries'
    owned_clockwork_digest=$binding_digest
}

if [ -L "$current" ]; then
    selector=$(readlink "$current")
    validate_release_selector "$selector"
    current_release_format=$manifest_format
    current_schedule_template=$schedule_file
    current_clockwork_release=$release
    current_clockwork_release_id=$release_id
    current_clockwork_runner_hash=$manifest_runner_hash
elif [ -e "$current" ]; then
    fail 'current selector is not a symbolic link'
else
    for public_path in "$cli" "$provider" "$plist"; do
        [ ! -e "$public_path" ] && [ ! -L "$public_path" ] \
            || fail 'public Semantics state exists without an owned current release'
    done
    "$launchctl_path" print "$service_target" >/dev/null 2>&1 \
        && fail 'loaded Semantics label has no owned current release'
fi

inspect_owned_clockwork_binding
if [ -z "$owned_clockwork_digest" ]; then
    [ ! -e "$clockwork_plist" ] && [ ! -L "$clockwork_plist" ] \
        || fail 'unselected Semantics binding has a Clockwork LaunchAgent'
    "$launchctl_path" print "$clockwork_target" >/dev/null 2>&1 \
        && fail 'unselected Semantics binding has a loaded Clockwork label'
fi
if [ ! -L "$current" ]; then
    printf '%s\n' 'Semantics is not installed; retained database, releases, and logs'
    exit 0
fi

for link_and_target in "$cli|$expected_cli" "$provider|$expected_provider"; do
    link=${link_and_target%%|*}
    target=${link_and_target#*|}
    if [ -L "$link" ]; then
        [ "$(readlink "$link")" = "$target" ] || fail "selector is not owned by Semantics: $link"
    elif [ -e "$link" ]; then
        fail "path is not an owned Semantics selector: $link"
    fi
done

owned_plist=0
if [ -L "$plist" ]; then
    fail 'worker LaunchAgent must not be a symbolic link'
elif [ -f "$plist" ]; then
    [ "$current_release_format" -eq 1 ] \
        || fail 'legacy LaunchAgent is not owned by the current Semantics release'
    [ "$(stat -f '%u' "$plist")" -eq "$operator_uid" ] \
        || fail 'legacy LaunchAgent is not owned by the Semantics operator'
    [ "$(stat -f '%Lp' "$plist")" = 644 ] \
        || fail 'legacy LaunchAgent permissions are not owned by Semantics'
    expected_legacy_plist_hash=$(sed \
        -e "s|__SEMANTICS_WORKER_RUNNER__|$install_dir/current/bin/semantics-worker|g" \
        -e "s|__SEMANTICS_STATE_DIR__|$state|g" \
        -e "s|__SEMANTICS_HOME__|$install_home|g" \
        -e "s|__SEMANTICS_WORKER_STDOUT__|$install_home/Library/Logs/Semantics/worker.stdout.log|g" \
        -e "s|__SEMANTICS_WORKER_STDERR__|$install_home/Library/Logs/Semantics/worker.stderr.log|g" \
        "$current_schedule_template" | shasum -a 256 | awk '{print $1}')
    [ "$(shasum -a 256 "$plist" | awk '{print $1}')" = \
        "$expected_legacy_plist_hash" ] \
        || fail 'legacy LaunchAgent bytes do not match the current Semantics release'
    owned_plist=1
elif [ -e "$plist" ]; then
    fail 'worker LaunchAgent path is not a regular file'
fi

engage_maintenance \
    || fail 'Semantics maintenance gate is invalid or unavailable'
if "$launchctl_path" print "$service_target" >/dev/null 2>&1; then
    [ "$owned_plist" -eq 1 ] || fail 'loaded Semantics label has no owned recoverable plist'
    "$launchctl_path" bootout "$service_target" >/dev/null \
        || fail 'unable to stop the owned Semantics worker'
fi
if [ -n "$owned_clockwork_digest" ]; then
    HOME="$install_home" "$clockwork_path" --json binding disable \
        "$clockwork_key" >/dev/null \
        || fail 'unable to disable the owned Semantics Clockwork binding'
fi
[ "$owned_plist" -eq 0 ] || rm -f "$plist"
[ ! -L "$cli" ] || rm -f "$cli"
[ ! -L "$provider" ] || rm -f "$provider"
printf '%s\n' 'uninstalled Semantics schedule and selectors; retained database, releases, and logs'
