#!/bin/sh

set -eu

PATH=/usr/bin:/bin:/usr/sbin:/sbin
export PATH
umask 077

install_home=${HOME:-}
launchctl_path=/bin/launchctl

fail() {
    printf 'semantics uninstall: %s\n' "$*" >&2
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

label=org.semantics.worker
state="$install_home/Library/Application Support/Semantics"
install_dir="$state/install"
current="$install_dir/current"
cli="$install_home/.local/bin/semantics"
provider="$install_home/Library/Application Support/Chancery/providers/semantics"
plist="$install_home/Library/LaunchAgents/$label.plist"
expected_cli="$install_dir/current/bin/semantics"
expected_provider="$install_dir/current/share/chancery/semantics"
service_domain="gui/$(id -u)"
service_target="$service_domain/$label"
lock_dir="$install_dir/.update-lock"

for directory in "$state" "$install_dir"; do
    [ ! -L "$directory" ] || fail "refusing symbolic-link directory: $directory"
    [ ! -e "$directory" ] || [ -d "$directory" ] || fail "directory path is occupied: $directory"
    [ -d "$directory" ] || install -d -m 0700 "$directory"
done
mkdir "$lock_dir" 2>/dev/null || fail 'a Semantics deployment or uninstall is active'
cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    rmdir "$lock_dir" >/dev/null 2>&1 || true
    exit "$status"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

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
    [ "$(sed -n '1p' "$manifest")" = 'format=1' ] || fail 'current Semantics release manifest format is unsupported'
    manifest_release=$(sed -n '2s/^release_id=//p' "$manifest")
    manifest_version=$(sed -n '3s/^version=//p' "$manifest")
    manifest_binary_hash=$(sed -n '4s/^binary_sha256=//p' "$manifest")
    manifest_frontend_hash=$(sed -n '5s/^frontend_sha256=//p' "$manifest")
    manifest_runner_hash=$(sed -n '6s/^runner_sha256=//p' "$manifest")
    manifest_plist_hash=$(sed -n '7s/^plist_sha256=//p' "$manifest")
    manifest_deployer_hash=$(sed -n '8s/^deployer_sha256=//p' "$manifest")
    manifest_uninstaller_hash=$(sed -n '9s/^uninstaller_sha256=//p' "$manifest")
    manifest_chancery_hash=$(sed -n '10s/^chancery_sha256=//p' "$manifest")
    printf '%s\n' "$manifest_release" "$manifest_binary_hash" "$manifest_frontend_hash" \
        "$manifest_runner_hash" "$manifest_plist_hash" "$manifest_deployer_hash" \
        "$manifest_uninstaller_hash" "$manifest_chancery_hash" \
        | grep -Eqv '^[0-9a-f]{64}$' && fail 'current Semantics release manifest hashes are invalid'
    printf '%s\n' "$manifest_version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+([+-][0-9A-Za-z.-]+)?$' \
        || fail 'current Semantics release version is invalid'
    [ "$manifest_release" = "$release_id" ] || fail 'current Semantics release manifest does not match'
    for owned_file in \
        "$release/libexec/semantics" "$release/bin/semantics" "$release/bin/semantics-worker" \
        "$release/package/semantics" "$release/package/semantics-worker" \
        "$release/package/deploy-user.sh" "$release/package/uninstall-user.sh" \
        "$release/package/$label.plist"
    do
        [ -f "$owned_file" ] && [ ! -L "$owned_file" ] || fail 'current Semantics release is incomplete'
    done
    validate_bundle "$release/share/chancery/semantics"
    actual_binary_hash=$(shasum -a 256 "$release/libexec/semantics" | awk '{print $1}')
    actual_frontend_hash=$(shasum -a 256 "$release/bin/semantics" | awk '{print $1}')
    actual_runner_hash=$(shasum -a 256 "$release/bin/semantics-worker" | awk '{print $1}')
    actual_plist_hash=$(shasum -a 256 "$release/package/$label.plist" | awk '{print $1}')
    actual_deployer_hash=$(shasum -a 256 "$release/package/deploy-user.sh" | awk '{print $1}')
    actual_uninstaller_hash=$(shasum -a 256 "$release/package/uninstall-user.sh" | awk '{print $1}')
    actual_chancery_hash=$(bundle_hash "$release/share/chancery/semantics")
    [ "$actual_binary_hash" = "$manifest_binary_hash" ] || fail 'current Semantics binary is tampered'
    [ "$actual_frontend_hash" = "$manifest_frontend_hash" ] || fail 'current Semantics frontend is tampered'
    [ "$(shasum -a 256 "$release/package/semantics" | awk '{print $1}')" = "$manifest_frontend_hash" ] || fail 'current packaged frontend is tampered'
    [ "$actual_runner_hash" = "$manifest_runner_hash" ] || fail 'current Semantics runner is tampered'
    [ "$(shasum -a 256 "$release/package/semantics-worker" | awk '{print $1}')" = "$manifest_runner_hash" ] || fail 'current packaged runner is tampered'
    [ "$actual_plist_hash" = "$manifest_plist_hash" ] || fail 'current Semantics plist is tampered'
    [ "$actual_deployer_hash" = "$manifest_deployer_hash" ] || fail 'current Semantics deployer is tampered'
    [ "$actual_uninstaller_hash" = "$manifest_uninstaller_hash" ] || fail 'current Semantics uninstaller is tampered'
    [ "$actual_chancery_hash" = "$manifest_chancery_hash" ] || fail 'current Semantics provider is tampered'
    actual_release_id=$(printf '%s\n' "$actual_binary_hash" "$actual_frontend_hash" \
        "$actual_runner_hash" "$actual_plist_hash" "$actual_deployer_hash" \
        "$actual_uninstaller_hash" "$actual_chancery_hash" | shasum -a 256 | awk '{print $1}')
    [ "$actual_release_id" = "$release_id" ] || fail 'current Semantics release content ID does not match'
}

if [ -L "$current" ]; then
    selector=$(readlink "$current")
    validate_release_selector "$selector"
elif [ -e "$current" ]; then
    fail 'current selector is not a symbolic link'
else
    for public_path in "$cli" "$provider" "$plist"; do
        [ ! -e "$public_path" ] && [ ! -L "$public_path" ] \
            || fail 'public Semantics state exists without an owned current release'
    done
    "$launchctl_path" print "$service_target" >/dev/null 2>&1 \
        && fail 'loaded Semantics label has no owned current release'
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
    [ "$(plutil -extract Label raw "$plist" 2>/dev/null)" = "$label" ] \
        || fail 'worker LaunchAgent label is not owned by Semantics'
    [ "$(plutil -extract ProgramArguments.1 raw "$plist" 2>/dev/null)" = "$install_dir/current/bin/semantics-worker" ] \
        || fail 'worker LaunchAgent runner is not owned by Semantics'
    owned_plist=1
elif [ -e "$plist" ]; then
    fail 'worker LaunchAgent path is not a regular file'
fi

if "$launchctl_path" print "$service_target" >/dev/null 2>&1; then
    [ "$owned_plist" -eq 1 ] || fail 'loaded Semantics label has no owned recoverable plist'
    "$launchctl_path" bootout "$service_target" >/dev/null \
        || fail 'unable to stop the owned Semantics worker'
fi
[ "$owned_plist" -eq 0 ] || rm -f "$plist"
[ ! -L "$cli" ] || rm -f "$cli"
[ ! -L "$provider" ] || rm -f "$provider"
printf '%s\n' 'uninstalled Semantics service and selectors; retained database, releases, and logs'
