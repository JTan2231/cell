#!/bin/sh

set -eu

PATH=/usr/bin:/bin:/usr/sbin:/sbin
export PATH

install_home=${HOME:-}
launchctl_path=/bin/launchctl
fail() {
    printf 'decisions uninstall: %s\n' "$*" >&2
    exit 1
}
while [ "$#" -gt 0 ]; do
    case "$1" in
        --home) [ "$#" -ge 2 ] || fail '--home requires a path'; install_home=$2; shift 2 ;;
        --launchctl) [ "$#" -ge 2 ] || fail '--launchctl requires a path'; launchctl_path=$2; shift 2 ;;
        *) fail "unknown argument: $1" ;;
    esac
done
case "$install_home" in /*) ;; *) fail 'home must be absolute' ;; esac
case "$install_home" in *'|'*|*'
'*) fail 'home contains unsupported characters' ;; esac
case "$launchctl_path" in /*) ;; *) fail 'launchctl must be absolute' ;; esac
[ -d "$install_home" ] && [ ! -L "$install_home" ] || fail 'home is not a regular directory'
[ -x "$launchctl_path" ] && [ ! -L "$launchctl_path" ] || fail 'launchctl is unavailable'

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
service_domain="gui/$(id -u)"
daily_target="$service_domain/$daily_label"
observer_target="$service_domain/$observer_label"

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
    [ "$(sed -n '1p' "$manifest")" = 'format=2' ] \
        || fail 'current Decisions release manifest format is unsupported'
    manifest_release=$(sed -n '2s/^release_id=//p' "$manifest")
    manifest_version=$(sed -n '3s/^version=//p' "$manifest")
    manifest_binary_hash=$(sed -n '4s/^binary_sha256=//p' "$manifest")
    manifest_frontend_hash=$(sed -n '5s/^frontend_sha256=//p' "$manifest")
    manifest_daily_runner_hash=$(sed -n '6s/^daily_runner_sha256=//p' "$manifest")
    manifest_observer_runner_hash=$(sed -n '7s/^observer_runner_sha256=//p' "$manifest")
    manifest_daily_plist_hash=$(sed -n '8s/^daily_plist_sha256=//p' "$manifest")
    manifest_observer_plist_hash=$(sed -n '9s/^observer_plist_sha256=//p' "$manifest")
    manifest_hooks_hash=$(sed -n '10s/^hooks_sha256=//p' "$manifest")
    manifest_deployer_hash=$(sed -n '11s/^deployer_sha256=//p' "$manifest")
    manifest_uninstaller_hash=$(sed -n '12s/^uninstaller_sha256=//p' "$manifest")
    manifest_chancery_hash=$(sed -n '13s/^chancery_sha256=//p' "$manifest")
    printf '%s\n' "$manifest_release" "$manifest_binary_hash" "$manifest_frontend_hash" \
        "$manifest_daily_runner_hash" "$manifest_observer_runner_hash" \
        "$manifest_daily_plist_hash" "$manifest_observer_plist_hash" "$manifest_hooks_hash" \
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
        "$release/package/$daily_label.plist" \
        "$release/package/$observer_label.plist" \
        "$release/package/hooks.json"
    do
        [ -f "$owned_file" ] && [ ! -L "$owned_file" ] \
            || fail 'current Decisions release is incomplete'
    done
    validate_bundle "$release/share/chancery/decisions"
    actual_binary_hash=$(shasum -a 256 "$release/libexec/decisions" | awk '{print $1}')
    actual_frontend_hash=$(shasum -a 256 "$release/bin/decisions" | awk '{print $1}')
    actual_daily_runner_hash=$(shasum -a 256 "$release/bin/decisions-daily-email" | awk '{print $1}')
    actual_observer_runner_hash=$(shasum -a 256 "$release/bin/decisions-observer" | awk '{print $1}')
    actual_daily_plist_hash=$(shasum -a 256 "$release/package/$daily_label.plist" | awk '{print $1}')
    actual_observer_plist_hash=$(shasum -a 256 "$release/package/$observer_label.plist" | awk '{print $1}')
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
    [ "$actual_daily_plist_hash" = "$manifest_daily_plist_hash" ] \
        || fail 'current Decisions release daily plist is tampered'
    [ "$actual_observer_plist_hash" = "$manifest_observer_plist_hash" ] \
        || fail 'current Decisions release observer plist is tampered'
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
        "$actual_daily_plist_hash" "$actual_observer_plist_hash" "$actual_hooks_hash" \
        "$actual_deployer_hash" \
        "$actual_uninstaller_hash" "$actual_chancery_hash" | shasum -a 256 | awk '{print $1}')
    [ "$actual_release_id" = "$release_id" ] \
        || fail 'current Decisions release content ID does not match'
}

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
    printf '%s\n' 'Decisions is not installed; retained database, releases, and logs'
    exit 0
fi

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
if [ -L "$daily_plist" ]; then
    fail 'daily LaunchAgent must not be a symbolic link'
elif [ -f "$daily_plist" ]; then
    [ "$(plutil -extract Label raw "$daily_plist" 2>/dev/null)" = "$daily_label" ] \
        || fail 'daily LaunchAgent label is not owned by Decisions'
    [ "$(plutil -extract ProgramArguments.1 raw "$daily_plist" 2>/dev/null)" = "$install_dir/current/bin/decisions-daily-email" ] \
        || fail 'daily LaunchAgent runner is not owned by Decisions'
    owned_daily_plist=1
elif [ -e "$daily_plist" ]; then
    fail 'daily LaunchAgent path is not a regular file'
fi

owned_observer_plist=0
if [ -L "$observer_plist" ]; then
    fail 'observer LaunchAgent must not be a symbolic link'
elif [ -f "$observer_plist" ]; then
    [ "$(plutil -extract Label raw "$observer_plist" 2>/dev/null)" = "$observer_label" ] \
        || fail 'observer LaunchAgent label is not owned by Decisions'
    [ "$(plutil -extract ProgramArguments.1 raw "$observer_plist" 2>/dev/null)" = "$install_dir/current/bin/decisions-observer" ] \
        || fail 'observer LaunchAgent runner is not owned by Decisions'
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

if "$launchctl_path" print "$observer_target" >/dev/null 2>&1; then
    [ "$owned_observer_plist" -eq 1 ] || fail 'loaded Decisions observer label has no owned recoverable plist'
    "$launchctl_path" bootout "$observer_target" >/dev/null \
        || fail 'unable to stop the owned Decisions observer service'
fi
if "$launchctl_path" print "$daily_target" >/dev/null 2>&1; then
    [ "$owned_daily_plist" -eq 1 ] || fail 'loaded Decisions daily label has no owned recoverable plist'
    "$launchctl_path" bootout "$daily_target" >/dev/null \
        || fail 'unable to stop the owned Decisions daily service'
fi
[ "$owned_observer_plist" -eq 0 ] || rm -f "$observer_plist"
[ "$owned_daily_plist" -eq 0 ] || rm -f "$daily_plist"
[ "$owned_hooks" -eq 0 ] || rm -f "$hooks"
[ ! -L "$cli" ] || rm -f "$cli"
[ ! -L "$provider" ] || rm -f "$provider"
printf '%s\n' 'uninstalled Decisions services, hook, and selectors; retained database, releases, and logs'
