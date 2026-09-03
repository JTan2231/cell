#!/bin/sh

set -eu

PATH=/usr/bin:/bin:/usr/sbin:/sbin
export PATH
umask 077

install_home=${HOME:-}
launchctl_path=/bin/launchctl
clockwork_path=
fail() {
    printf 'decisions uninstall: %s\n' "$*" >&2
    exit 1
}
while [ "$#" -gt 0 ]; do
    case "$1" in
        --clockwork) [ "$#" -ge 2 ] || fail '--clockwork requires a path'; clockwork_path=$2; shift 2 ;;
        --home) [ "$#" -ge 2 ] || fail '--home requires a path'; install_home=$2; shift 2 ;;
        --launchctl) [ "$#" -ge 2 ] || fail '--launchctl requires a path'; launchctl_path=$2; shift 2 ;;
        *) fail "unknown argument: $1" ;;
    esac
done
[ -n "$clockwork_path" ] || fail '--clockwork is required'
case "$clockwork_path" in /*) ;; *) fail 'clockwork must be absolute' ;; esac
case "$install_home" in /*) ;; *) fail 'home must be absolute' ;; esac
case "$install_home" in *'&'*|*'<'*|*'>'*|*'|'*|*'"'*|*'\'*|*'
'*) fail 'home contains characters unsupported by schedule rendering' ;; esac
case "$launchctl_path" in /*) ;; *) fail 'launchctl must be absolute' ;; esac
[ -d "$install_home" ] && [ ! -L "$install_home" ] || fail 'home is not a regular directory'
operator_uid=$(id -u)
[ "$operator_uid" -ne 0 ] || fail 'run as the Decisions operator, not root'
[ "$(stat -f '%u' "$install_home")" -eq "$operator_uid" ] \
    || fail 'home is not owned by the Decisions operator'
[ -x "$launchctl_path" ] && [ ! -L "$launchctl_path" ] || fail 'launchctl is unavailable'
[ -e "$clockwork_path" ] && [ -x "$clockwork_path" ] || fail 'Clockwork executable is unavailable'

daily_label=org.decisions.daily-email
observer_label=org.decisions.observer
state="$install_home/Library/Application Support/Decisions"
install_dir="$state/install"
current="$install_dir/current"
cli="$install_home/.local/bin/decisions"
provider="$install_home/Library/Application Support/Chancery/providers/decisions"
daily_plist="$install_home/Library/LaunchAgents/$daily_label.plist"
observer_plist="$install_home/Library/LaunchAgents/$observer_label.plist"
hooks="$install_home/.codex/hooks.json"
expected_cli="$install_dir/current/bin/decisions"
expected_provider="$install_dir/current/share/chancery/decisions"
service_domain="gui/$operator_uid"
daily_target="$service_domain/$daily_label"
observer_target="$service_domain/$observer_label"
daily_clockwork_key=decisions/daily-email
observer_clockwork_key=decisions/observer
logs="$install_home/Library/Logs/Decisions"
maintenance="$state/.clockwork-maintenance"
interpreter_hash=$(shasum -a 256 /bin/sh | awk '{print $1}')
update_lock="$install_dir/.update-lock"
lock_created=0
state_created=0
install_dir_created=0
clockwork_inspection=

cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    set +e
    [ -z "$clockwork_inspection" ] || rm -rf "$clockwork_inspection"
    [ "$lock_created" -eq 0 ] || rmdir "$update_lock" >/dev/null 2>&1
    [ "$install_dir_created" -eq 0 ] || rmdir "$install_dir" >/dev/null 2>&1
    [ "$state_created" -eq 0 ] || rmdir "$state" >/dev/null 2>&1
    exit "$status"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

validate_bundle() {
    bundle=$1
    [ -d "$bundle" ] && [ ! -L "$bundle" ] \
        || fail "Chancery provider is not a regular directory: $bundle"
    [ -f "$bundle/provider.json" ] && [ ! -L "$bundle/provider.json" ] \
        || fail "Chancery provider manifest is missing: $bundle"
    if find "$bundle" -type l -print | grep -q .; then
        fail "Chancery provider contains a symbolic link: $bundle"
    fi
    if find "$bundle" ! -type d ! -type f -print | grep -q .; then
        fail "Chancery provider contains a non-file entry: $bundle"
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

maintenance_marker_is_owned() {
    [ -f "$maintenance" ] && [ ! -L "$maintenance" ] \
        && [ "$(stat -f '%u' "$maintenance")" -eq "$operator_uid" ] \
        && [ "$(stat -f '%Lp' "$maintenance")" = 600 ] \
        && [ "$(stat -f '%l' "$maintenance")" -eq 1 ]
}

engage_maintenance() {
    if [ -L "$maintenance" ] \
        || { [ -e "$maintenance" ] && [ ! -f "$maintenance" ]; }
    then
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

render_legacy_plist() {
    legacy_kind=$1
    legacy_template=$2
    legacy_output=$3
    [ -f "$legacy_template" ] && [ ! -L "$legacy_template" ] || return 1
    case "$legacy_kind" in
        daily)
            sed \
                -e "s|__DECISIONS_RUNNER__|$install_dir/current/bin/decisions-daily-email|g" \
                -e "s|__DECISIONS_STATE_DIR__|$state|g" \
                -e "s|__DECISIONS_HOME__|$install_home|g" \
                -e "s|__DECISIONS_STDOUT__|$logs/daily-email.stdout.log|g" \
                -e "s|__DECISIONS_STDERR__|$logs/daily-email.stderr.log|g" \
                "$legacy_template" >"$legacy_output"
            ;;
        observer)
            sed \
                -e "s|__DECISIONS_OBSERVER_RUNNER__|$install_dir/current/bin/decisions-observer|g" \
                -e "s|__DECISIONS_STATE_DIR__|$state|g" \
                -e "s|__DECISIONS_HOME__|$install_home|g" \
                -e "s|__DECISIONS_OBSERVER_STDOUT__|$logs/observer.stdout.log|g" \
                -e "s|__DECISIONS_OBSERVER_STDERR__|$logs/observer.stderr.log|g" \
                "$legacy_template" >"$legacy_output"
            ;;
        *) return 1 ;;
    esac
}

legacy_plist_matches_expected() {
    legacy_candidate=$1
    legacy_expected=$2
    [ -f "$legacy_candidate" ] && [ ! -L "$legacy_candidate" ] \
        && [ "$(stat -f '%u' "$legacy_candidate")" -eq "$operator_uid" ] \
        && [ "$(stat -f '%Lp' "$legacy_candidate")" = 644 ] \
        && [ -f "$legacy_expected" ] && [ ! -L "$legacy_expected" ] \
        && cmp -s "$legacy_expected" "$legacy_candidate"
}

validate_release_selector() {
    selector=$1
    printf '%s\n' "$selector" | grep -Eq '^releases/[0-9a-f]{64}$' \
        || fail "current selector is not owned by Decisions: $selector"
    release="$install_dir/$selector"
    release_id=${selector#releases/}
    [ -d "$release" ] && [ ! -L "$release" ] \
        || fail 'current Decisions release is unavailable'
    manifest="$release/manifest.txt"
    [ -f "$manifest" ] && [ ! -L "$manifest" ] \
        || fail 'current Decisions release has no owned manifest'
    [ "$(awk 'END { print NR }' "$manifest")" -eq 13 ] \
        || fail 'current Decisions release manifest is not canonical'
    manifest_format=$(sed -n '1s/^format=//p' "$manifest")
    case "$manifest_format" in 2|3) ;; *) fail 'current Decisions release manifest format is unsupported' ;; esac
    manifest_release=$(sed -n '2s/^release_id=//p' "$manifest")
    manifest_version=$(sed -n '3s/^version=//p' "$manifest")
    manifest_binary_hash=$(sed -n '4s/^binary_sha256=//p' "$manifest")
    manifest_frontend_hash=$(sed -n '5s/^frontend_sha256=//p' "$manifest")
    manifest_daily_runner_hash=$(sed -n '6s/^daily_runner_sha256=//p' "$manifest")
    manifest_observer_runner_hash=$(sed -n '7s/^observer_runner_sha256=//p' "$manifest")
    if [ "$manifest_format" -eq 2 ]; then
        manifest_daily_schedule_hash=$(sed -n '8s/^daily_plist_sha256=//p' "$manifest")
        manifest_observer_schedule_hash=$(sed -n '9s/^observer_plist_sha256=//p' "$manifest")
    else
        manifest_daily_schedule_hash=$(sed -n '8s/^daily_clockwork_definition_sha256=//p' "$manifest")
        manifest_observer_schedule_hash=$(sed -n '9s/^observer_clockwork_definition_sha256=//p' "$manifest")
    fi
    manifest_hooks_hash=$(sed -n '10s/^hooks_sha256=//p' "$manifest")
    manifest_deployer_hash=$(sed -n '11s/^deployer_sha256=//p' "$manifest")
    manifest_uninstaller_hash=$(sed -n '12s/^uninstaller_sha256=//p' "$manifest")
    manifest_chancery_hash=$(sed -n '13s/^chancery_sha256=//p' "$manifest")
    printf '%s\n' "$manifest_release" "$manifest_binary_hash" "$manifest_frontend_hash" \
        "$manifest_daily_runner_hash" "$manifest_observer_runner_hash" \
        "$manifest_daily_schedule_hash" "$manifest_observer_schedule_hash" "$manifest_hooks_hash" \
        "$manifest_deployer_hash" \
        "$manifest_uninstaller_hash" "$manifest_chancery_hash" \
        | grep -Eqv '^[0-9a-f]{64}$' \
        && fail 'current Decisions release manifest hashes are invalid'
    printf '%s\n' "$manifest_version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+([+-][0-9A-Za-z.-]+)?$' \
        || fail 'current Decisions release version is invalid'
    [ "$manifest_release" = "$release_id" ] \
        || fail 'current Decisions release manifest does not match'
    for owned_file in \
        "$release/libexec/decisions" \
        "$release/bin/decisions" \
        "$release/bin/decisions-daily-email" \
        "$release/bin/decisions-observer" \
        "$release/package/decisions" \
        "$release/package/decisions-daily-email" \
        "$release/package/decisions-observer" \
        "$release/package/deploy-user.sh" \
        "$release/package/uninstall-user.sh" \
        "$release/package/hooks.json"
    do
        [ -f "$owned_file" ] && [ ! -L "$owned_file" ] \
            || fail 'current Decisions release is incomplete'
    done
    if [ "$manifest_format" -eq 2 ]; then
        daily_schedule_file="$release/package/$daily_label.plist"
        observer_schedule_file="$release/package/$observer_label.plist"
    else
        daily_schedule_file="$release/package/decisions-daily-email.clockwork.toml.in"
        observer_schedule_file="$release/package/decisions-observer.clockwork.toml.in"
    fi
    for schedule_file in "$daily_schedule_file" "$observer_schedule_file"; do
        [ -f "$schedule_file" ] && [ ! -L "$schedule_file" ] \
            || fail 'current Decisions release has no owned schedule template'
    done
    validate_bundle "$release/share/chancery/decisions"
    actual_binary_hash=$(shasum -a 256 "$release/libexec/decisions" | awk '{print $1}')
    actual_frontend_hash=$(shasum -a 256 "$release/bin/decisions" | awk '{print $1}')
    actual_daily_runner_hash=$(shasum -a 256 "$release/bin/decisions-daily-email" | awk '{print $1}')
    actual_observer_runner_hash=$(shasum -a 256 "$release/bin/decisions-observer" | awk '{print $1}')
    actual_daily_schedule_hash=$(shasum -a 256 "$daily_schedule_file" | awk '{print $1}')
    actual_observer_schedule_hash=$(shasum -a 256 "$observer_schedule_file" | awk '{print $1}')
    actual_hooks_hash=$(shasum -a 256 "$release/package/hooks.json" | awk '{print $1}')
    actual_deployer_hash=$(shasum -a 256 "$release/package/deploy-user.sh" | awk '{print $1}')
    actual_uninstaller_hash=$(shasum -a 256 "$release/package/uninstall-user.sh" | awk '{print $1}')
    actual_chancery_hash=$(bundle_hash "$release/share/chancery/decisions")
    [ "$actual_binary_hash" = "$manifest_binary_hash" ] \
        || fail 'current Decisions release binary is tampered'
    [ "$actual_frontend_hash" = "$manifest_frontend_hash" ] \
        || fail 'current Decisions release frontend is tampered'
    [ "$(shasum -a 256 "$release/package/decisions" | awk '{print $1}')" = "$manifest_frontend_hash" ] \
        || fail 'current Decisions release packaged frontend is tampered'
    [ "$actual_daily_runner_hash" = "$manifest_daily_runner_hash" ] \
        || fail 'current Decisions release daily runner is tampered'
    [ "$(shasum -a 256 "$release/package/decisions-daily-email" | awk '{print $1}')" = "$manifest_daily_runner_hash" ] \
        || fail 'current Decisions release packaged daily runner is tampered'
    [ "$actual_observer_runner_hash" = "$manifest_observer_runner_hash" ] \
        || fail 'current Decisions release observer runner is tampered'
    [ "$(shasum -a 256 "$release/package/decisions-observer" | awk '{print $1}')" = "$manifest_observer_runner_hash" ] \
        || fail 'current Decisions release packaged observer runner is tampered'
    [ "$actual_daily_schedule_hash" = "$manifest_daily_schedule_hash" ] \
        || fail 'current Decisions release daily schedule template is tampered'
    [ "$actual_observer_schedule_hash" = "$manifest_observer_schedule_hash" ] \
        || fail 'current Decisions release observer schedule template is tampered'
    [ "$actual_hooks_hash" = "$manifest_hooks_hash" ] \
        || fail 'current Decisions release hook definition is tampered'
    [ "$actual_deployer_hash" = "$manifest_deployer_hash" ] \
        || fail 'current Decisions release deployer is tampered'
    [ "$actual_uninstaller_hash" = "$manifest_uninstaller_hash" ] \
        || fail 'current Decisions release uninstaller is tampered'
    [ "$actual_chancery_hash" = "$manifest_chancery_hash" ] \
        || fail 'current Decisions release provider is tampered'
    actual_release_id=$(printf '%s\n' "$actual_binary_hash" "$actual_frontend_hash" \
        "$actual_daily_runner_hash" "$actual_observer_runner_hash" \
        "$actual_daily_schedule_hash" "$actual_observer_schedule_hash" "$actual_hooks_hash" \
        "$actual_deployer_hash" \
        "$actual_uninstaller_hash" "$actual_chancery_hash" | shasum -a 256 | awk '{print $1}')
    [ "$actual_release_id" = "$release_id" ] \
        || fail 'current Decisions release content ID does not match'
}

owned_clockwork_binding_digest() {
    definition_key=$1
    definition_runner=$2
    definition_runner_hash=$3
    definition_name=$4
    owned_clockwork_digest=
    binding_show="$clockwork_inspection/$definition_name-binding.json"
    if HOME="$install_home" "$clockwork_path" --json binding show "$definition_key" \
        >"$binding_show" 2>"$binding_show.stderr"
    then
        binding_compact=$(tr -d '[:space:]' <"$binding_show")
        case "$binding_compact" in
            *'"enabled":true'*) binding_enabled=1 ;;
            *'"enabled":false'*) binding_enabled=0 ;;
            *) fail "Clockwork returned invalid $definition_name binding state" ;;
        esac
        binding_digest=$(printf '%s\n' "$binding_compact" | sed -n \
            's/.*"definition_digest":"\([0-9a-f]\{64\}\)".*/\1/p')
        if [ -z "$binding_digest" ]; then
            [ "$binding_enabled" -eq 0 ] \
                && printf '%s\n' "$binding_compact" | grep -F '"definition_digest":null' >/dev/null \
                || fail "$definition_name Clockwork binding has an invalid definition digest"
            # An unselected disabled tombstone has no executable authority to remove
            # and no definition from which Decisions ownership can be established.
            return 0
        fi
    else
        grep -F '"code":"binding_not_found"' "$binding_show.stderr" >/dev/null \
            || fail "unable to inspect the $definition_name Clockwork binding"
        return 0
    fi

    [ "$manifest_format" = 3 ] \
        || fail "$definition_name Clockwork binding cannot be owned by a legacy release"
    definition_show="$clockwork_inspection/$definition_name-definition.json"
    HOME="$install_home" "$clockwork_path" --json definition show "$binding_digest" \
        >"$definition_show" 2>"$definition_show.stderr" \
        || fail "unable to inspect the selected $definition_name Clockwork definition"
    [ "$(plutil -extract ok raw "$definition_show" 2>/dev/null)" = true ] \
        && [ "$(plutil -extract data.digest raw "$definition_show" 2>/dev/null)" = "$binding_digest" ] \
        && [ "$(plutil -extract data.key raw "$definition_show" 2>/dev/null)" = "$definition_key" ] \
        && [ "$(plutil -extract data.manifest.schema_version raw "$definition_show" 2>/dev/null)" = 1 ] \
        && [ "$(plutil -extract data.manifest.key raw "$definition_show" 2>/dev/null)" = "$definition_key" ] \
        && [ "$(plutil -extract data.manifest.release_id raw "$definition_show" 2>/dev/null)" = "$release_id" ] \
        && [ "$(plutil -extract data.manifest.release_root raw "$definition_show" 2>/dev/null)" = "$release" ] \
        && [ "$(plutil -extract data.manifest.authority raw "$definition_show" 2>/dev/null)" = current-user-background ] \
        && [ "$(plutil -extract data.manifest.overlap raw "$definition_show" 2>/dev/null)" = skip ] \
        && [ "$(plutil -extract data.manifest.cwd raw "$definition_show" 2>/dev/null)" = "$state" ] \
        && [ "$(plutil -extract data.manifest.launch.kind raw "$definition_show" 2>/dev/null)" = interpreted ] \
        && [ "$(plutil -extract data.manifest.launch.interpreter raw "$definition_show" 2>/dev/null)" = /bin/sh ] \
        && [ "$(plutil -extract data.manifest.launch.interpreter_sha256 raw "$definition_show" 2>/dev/null)" = "$interpreter_hash" ] \
        && [ "$(plutil -extract data.manifest.launch.script raw "$definition_show" 2>/dev/null)" = "$definition_runner" ] \
        && [ "$(plutil -extract data.manifest.launch.script_sha256 raw "$definition_show" 2>/dev/null)" = "$definition_runner_hash" ] \
        && [ "$(plutil -extract data.manifest.environment.HOME raw "$definition_show" 2>/dev/null)" = "$install_home" ] \
        || fail "$definition_name Clockwork definition is not owned by the current Decisions release"
    if plutil -extract data.manifest.timeout_seconds raw "$definition_show" >/dev/null 2>&1 \
        || plutil -extract data.manifest.arguments.0 raw "$definition_show" >/dev/null 2>&1; then
        fail "$definition_name Clockwork definition adds unsupported timeout or arguments"
    fi
    environment_keys=$(plutil -extract data.manifest.environment xml1 -o - \
        "$definition_show" 2>/dev/null | awk '/<key>/{count++} END {print count+0}')
    [ "$environment_keys" -eq 1 ] \
        || fail "$definition_name Clockwork definition contains foreign environment entries"
    case "$definition_name" in
        daily)
            [ "$(plutil -extract data.manifest.schedule.kind raw "$definition_show" 2>/dev/null)" = local-calendar ] \
                && [ "$(plutil -extract data.manifest.schedule.hour raw "$definition_show" 2>/dev/null)" = 9 ] \
                && [ "$(plutil -extract data.manifest.schedule.minute raw "$definition_show" 2>/dev/null)" = 0 ] \
                && [ "$(plutil -extract data.manifest.schedule.run_at_load raw "$definition_show" 2>/dev/null)" = false ] \
                && [ "$(plutil -extract data.manifest.output.stdout raw "$definition_show" 2>/dev/null)" = "$logs/daily-email.stdout.log" ] \
                && [ "$(plutil -extract data.manifest.output.stderr raw "$definition_show" 2>/dev/null)" = "$logs/daily-email.stderr.log" ] \
                || fail 'daily Clockwork definition schedule or output is not owned by Decisions'
            ;;
        observer)
            [ "$(plutil -extract data.manifest.schedule.kind raw "$definition_show" 2>/dev/null)" = interval ] \
                && [ "$(plutil -extract data.manifest.schedule.seconds raw "$definition_show" 2>/dev/null)" = 60 ] \
                && [ "$(plutil -extract data.manifest.schedule.run_at_load raw "$definition_show" 2>/dev/null)" = false ] \
                && [ "$(plutil -extract data.manifest.output.stdout raw "$definition_show" 2>/dev/null)" = "$logs/observer.stdout.log" ] \
                && [ "$(plutil -extract data.manifest.output.stderr raw "$definition_show" 2>/dev/null)" = "$logs/observer.stderr.log" ] \
                || fail 'observer Clockwork definition schedule or output is not owned by Decisions'
            ;;
        *) fail 'internal Clockwork definition ownership selector is invalid' ;;
    esac
    owned_clockwork_digest=$binding_digest
}

if [ -L "$state" ] || { [ -e "$state" ] && [ ! -d "$state" ]; }; then
    fail 'Decisions state path is not a regular directory'
fi
if [ ! -d "$state" ]; then
    mkdir -p "$state"
    chmod 0700 "$state"
    state_created=1
fi
if [ -L "$install_dir" ] \
    || { [ -e "$install_dir" ] && [ ! -d "$install_dir" ]; }
then
    fail 'Decisions installation path is not a regular directory'
fi
if [ ! -d "$install_dir" ]; then
    mkdir "$install_dir"
    chmod 0700 "$install_dir"
    install_dir_created=1
fi
trap '' HUP INT TERM
mkdir "$update_lock" 2>/dev/null \
    || fail 'another Decisions deployment or uninstall is active'
lock_created=1
trap 'exit 1' HUP INT TERM

if [ -L "$current" ]; then
    selector=$(readlink "$current")
    validate_release_selector "$selector"
elif [ -e "$current" ]; then
    fail 'current selector is not a symbolic link'
else
    for public_path in "$cli" "$provider" "$daily_plist" "$observer_plist"; do
        [ ! -e "$public_path" ] && [ ! -L "$public_path" ] \
            || fail 'public Decisions state exists without an owned current release'
    done
    "$launchctl_path" print "$daily_target" >/dev/null 2>&1 \
        && fail 'loaded Decisions daily label has no owned current release'
    "$launchctl_path" print "$observer_target" >/dev/null 2>&1 \
        && fail 'loaded Decisions observer label has no owned current release'
    for clockwork_key in decisions/observer decisions/daily-email; do
        if clockwork_show=$(HOME="$install_home" "$clockwork_path" --json \
            binding show "$clockwork_key" 2>&1)
        then
            clockwork_compact=$(printf '%s' "$clockwork_show" | tr -d '[:space:]')
            case "$clockwork_compact" in
                *'"enabled":false'*)
                    case "$clockwork_compact" in
                        *'"definition_digest":null'*) ;;
                        *)
                            fail "selected Clockwork binding has no owned current release: $clockwork_key"
                            ;;
                    esac
                    ;;
                *'"enabled":true'*)
                    fail "enabled Clockwork binding has no owned current release: $clockwork_key"
                    ;;
                *) fail "Clockwork returned invalid binding state for $clockwork_key" ;;
            esac
        else
            printf '%s\n' "$clockwork_show" \
                | grep -F '"code":"binding_not_found"' >/dev/null \
                || fail "unable to inspect Clockwork binding $clockwork_key"
        fi
    done
    printf '%s\n' 'Decisions is not installed; retained maintenance gate, database, releases, and logs'
    exit 0
fi

clockwork_inspection=$(mktemp -d "$install_dir/.clockwork-uninstall.XXXXXX")

for link_and_target in "$cli|$expected_cli" "$provider|$expected_provider"; do
    link=${link_and_target%%|*}
    target=${link_and_target#*|}
    if [ -L "$link" ]; then
        [ "$(readlink "$link")" = "$target" ] \
            || fail "selector is not owned by Decisions: $link"
    elif [ -e "$link" ]; then
        fail "path is not an owned Decisions selector: $link"
    fi
done

owned_daily_plist=0
expected_daily_plist=
if [ -L "$daily_plist" ]; then
    fail 'daily LaunchAgent must not be a symbolic link'
elif [ -f "$daily_plist" ]; then
    [ "$manifest_format" = 2 ] \
        || fail 'legacy daily LaunchAgent is not owned by the current Decisions release'
    expected_daily_plist="$clockwork_inspection/expected-daily.plist"
    render_legacy_plist daily "$release/package/$daily_label.plist" \
        "$expected_daily_plist" \
        || fail 'unable to render the current release legacy daily LaunchAgent'
    legacy_plist_matches_expected "$daily_plist" "$expected_daily_plist" \
        || fail 'legacy daily LaunchAgent bytes, owner, or mode are not owned by Decisions'
    owned_daily_plist=1
elif [ -e "$daily_plist" ]; then
    fail 'daily LaunchAgent path is not a regular file'
fi

owned_observer_plist=0
expected_observer_plist=
if [ -L "$observer_plist" ]; then
    fail 'observer LaunchAgent must not be a symbolic link'
elif [ -f "$observer_plist" ]; then
    [ "$manifest_format" = 2 ] \
        || fail 'legacy observer LaunchAgent is not owned by the current Decisions release'
    expected_observer_plist="$clockwork_inspection/expected-observer.plist"
    render_legacy_plist observer "$release/package/$observer_label.plist" \
        "$expected_observer_plist" \
        || fail 'unable to render the current release legacy observer LaunchAgent'
    legacy_plist_matches_expected "$observer_plist" "$expected_observer_plist" \
        || fail 'legacy observer LaunchAgent bytes, owner, or mode are not owned by Decisions'
    owned_observer_plist=1
elif [ -e "$observer_plist" ]; then
    fail 'observer LaunchAgent path is not a regular file'
fi

owned_hooks=0
if [ -L "$hooks" ]; then
    fail 'Codex hooks file must not be a symbolic link'
elif [ -f "$hooks" ]; then
    cmp -s "$hooks" "$release/package/hooks.json" \
        || fail 'refusing to remove foreign or modified Codex hooks'
    owned_hooks=1
elif [ -e "$hooks" ]; then
    fail 'Codex hooks path is not a regular file'
fi

owned_clockwork_binding_digest "$daily_clockwork_key" \
    "$release/bin/decisions-daily-email" "$manifest_daily_runner_hash" daily
owned_daily_clockwork_digest=$owned_clockwork_digest
owned_clockwork_binding_digest "$observer_clockwork_key" \
    "$release/bin/decisions-observer" "$manifest_observer_runner_hash" observer
owned_observer_clockwork_digest=$owned_clockwork_digest

engage_maintenance \
    || fail 'Decisions maintenance gate is invalid or unavailable'

if [ -n "$owned_observer_clockwork_digest" ]; then
    HOME="$install_home" "$clockwork_path" --json binding disable "$observer_clockwork_key" >/dev/null \
        || fail 'unable to disable the owned Clockwork observer binding'
fi
if [ -n "$owned_daily_clockwork_digest" ]; then
    HOME="$install_home" "$clockwork_path" --json binding disable "$daily_clockwork_key" >/dev/null \
        || fail 'unable to disable the owned Clockwork daily binding'
fi

if "$launchctl_path" print "$observer_target" >/dev/null 2>&1; then
    [ "$owned_observer_plist" -eq 1 ] || fail 'loaded Decisions observer label has no owned recoverable plist'
    legacy_plist_matches_expected "$observer_plist" "$expected_observer_plist" \
        || fail 'legacy observer LaunchAgent changed before stop'
    "$launchctl_path" bootout "$observer_target" >/dev/null \
        || fail 'unable to stop the owned Decisions observer service'
fi
if "$launchctl_path" print "$daily_target" >/dev/null 2>&1; then
    [ "$owned_daily_plist" -eq 1 ] || fail 'loaded Decisions daily label has no owned recoverable plist'
    legacy_plist_matches_expected "$daily_plist" "$expected_daily_plist" \
        || fail 'legacy daily LaunchAgent changed before stop'
    "$launchctl_path" bootout "$daily_target" >/dev/null \
        || fail 'unable to stop the owned Decisions daily service'
fi
if [ "$owned_observer_plist" -eq 1 ]; then
    legacy_plist_matches_expected "$observer_plist" "$expected_observer_plist" \
        || fail 'legacy observer LaunchAgent changed before removal'
    rm -f "$observer_plist"
fi
if [ "$owned_daily_plist" -eq 1 ]; then
    legacy_plist_matches_expected "$daily_plist" "$expected_daily_plist" \
        || fail 'legacy daily LaunchAgent changed before removal'
    rm -f "$daily_plist"
fi
[ "$owned_hooks" -eq 0 ] || rm -f "$hooks"
[ ! -L "$cli" ] || rm -f "$cli"
[ ! -L "$provider" ] || rm -f "$provider"
printf '%s\n' 'uninstalled Decisions schedules, hook, and selectors; retained maintenance gate, Clockwork definitions/history, database, releases, and logs'
