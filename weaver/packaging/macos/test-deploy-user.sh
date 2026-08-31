#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
temporary=$(mktemp -d "${TMPDIR:-/tmp}/weaver-user-deploy-test.XXXXXX")

cleanup() {
    rm -rf "$temporary"
}
trap cleanup EXIT HUP INT TERM

package="$temporary/package"
home="$temporary/Operator Home"
candidate="$temporary/weaver-candidate"
candidate_template="$temporary/weaver-candidate.template"
launchctl="$temporary/launchctl"
launchctl_log="$temporary/launchctl.log"
launchctl_state="$temporary/launchctl.loaded"
launchctl_fail_bootout="$temporary/launchctl.fail-bootout"

package_version=$(awk '
    $0 == "[package]" { in_package = 1; next }
    in_package && /^\[/ { exit }
    in_package && /^[[:space:]]*version[[:space:]]*=/ {
        value = $0
        sub(/^[^=]*=[[:space:]]*"/, "", value)
        sub(/"[[:space:]]*$/, "", value)
        print value
        exit
    }
' "$SCRIPT_DIR/../../crates/weaver/Cargo.toml")
provider_version=$(awk -F '"' '/"release"[[:space:]]*:/ { print $4; exit }' \
    "$SCRIPT_DIR/../../chancery/provider.json")
[ -n "$package_version" ] && [ "$provider_version" = "$package_version" ] || {
    printf 'test: package version %s does not match provider release %s\n' \
        "$package_version" "$provider_version" >&2
    exit 1
}
mismatch_version="$package_version-provider-mismatch"

mkdir -p "$package" "$home"
cp "$SCRIPT_DIR/deploy-user.sh" "$package/deploy-user.sh"
chmod 0755 "$package/deploy-user.sh"
mkdir -p "$temporary/share/chancery"
cp -R "$SCRIPT_DIR/../../chancery" "$temporary/share/chancery/weaver"

cat >"$candidate_template" <<'EOF'
#!/bin/sh
set -eu

case "${1:-}" in
    --version)
        printf '%s\n' 'weaver __WEAVER_VERSION__'
        exit 0
        ;;
    --help)
        printf '%s\n' 'fake Weaver help'
        exit 0
        ;;
esac

state=${WEAVER_STATE_DIR:?}
mkdir -p "$state"
printf '%s\n' "$*" >>"$state/commands.log"
case "${1:-}" in
    doctor)
        [ ! -f "$state/fail-doctor" ]
        ;;
    maintenance)
        case "${2:-}" in
            begin)
                [ "${3:-}" = --wait-seconds ]
                case "${4:-}" in
                    ''|*[!0-9]*) exit 64 ;;
                esac
                : >"$state/.maintenance"
                chmod 0600 "$state/.maintenance"
                [ ! -f "$state/fail-maintenance" ]
                ;;
            end)
                [ ! -f "$state/fail-maintenance-end" ]
                rm -f "$state/.maintenance"
                ;;
            *) exit 64 ;;
        esac
        ;;
    worker)
        [ "${2:-}" = run ]
        [ -f "$state/.maintenance" ]
        [ ! -f "$state/fail-worker" ]
        ;;
    *) exit 64 ;;
esac
EOF
sed "s/__WEAVER_VERSION__/$package_version/g" \
    "$candidate_template" >"$candidate"
chmod 0755 "$candidate"

uid=$(id -u)
cat >"$launchctl" <<EOF
#!/bin/sh
set -eu
printf '%s\n' "\$*" >>'$launchctl_log'
case "\${1:-}" in
    print)
        [ "\${2:-}" = "gui/$uid/org.weaver.worker" ]
        [ -f '$launchctl_state' ]
        ;;
    disable|enable)
        [ "\${2:-}" = "gui/$uid/org.weaver.worker" ]
        ;;
    bootout)
        [ "\${2:-}" = --wait ]
        [ "\${3:-}" = "gui/$uid/org.weaver.worker" ]
        rm -f '$launchctl_state'
        if [ -f '$launchctl_fail_bootout' ]; then
            rm -f '$launchctl_fail_bootout'
            exit 1
        fi
        ;;
    bootstrap)
        [ "\${2:-}" = "gui/$uid" ]
        [ "\${3:-}" = "$home/Library/LaunchAgents/org.weaver.worker.plist" ]
        [ -f "\$3" ]
        : >'$launchctl_state'
        ;;
    kickstart)
        [ "\${2:-}" = "gui/$uid/org.weaver.worker" ]
        ;;
    *) exit 64 ;;
esac
EOF
chmod 0755 "$launchctl"

chancery_providers="$home/Library/Application Support/Chancery/providers"
weaver_provider="$chancery_providers/weaver"
preserved_provider="$chancery_providers/preserved"
mkdir -p "$chancery_providers"
ln -s /preserved/provider "$preserved_provider"

deploy() {
    selected_candidate=$1
    shift
    HOME="$home" "$package/deploy-user.sh" \
        --binary "$selected_candidate" \
        --home "$home" \
        --launchctl "$launchctl" \
        --wait-seconds 0 \
        "$@"
}

mismatched_candidate="$temporary/weaver-mismatched-provider"
sed "s/__WEAVER_VERSION__/$mismatch_version/g" \
    "$candidate_template" >"$mismatched_candidate"
chmod 0755 "$mismatched_candidate"
if deploy "$mismatched_candidate" >"$temporary/provider-mismatch.out" \
    2>"$temporary/provider-mismatch.err"
then
    printf '%s\n' 'deployment unexpectedly accepted a provider/candidate mismatch' >&2
    exit 1
fi
grep -F "Chancery provider release $provider_version does not match Weaver $mismatch_version" \
    "$temporary/provider-mismatch.err" >/dev/null

deploy "$candidate" >"$temporary/first.out"
state="$home/Library/Application Support/Weaver"
cli="$home/.local/bin/weaver"
agent_plist="$home/Library/LaunchAgents/org.weaver.worker.plist"
[ -L "$cli" ]
[ -L "$state/install/current" ]
[ ! -e "$state/install/previous" ]
[ -x "$state/install/current/bin/weaver" ]
[ -x "$state/install/current/package/deploy-user.sh" ]
[ ! -e "$state/install/current/package/org.weaver.worker.plist" ]
[ -f "$state/install/current/manifest.txt" ]
[ -f "$state/install/current/share/chancery/weaver/provider.json" ]
[ -L "$weaver_provider" ]
[ "$(readlink "$weaver_provider")" = \
    "$state/install/current/share/chancery/weaver" ]
[ "$(readlink "$preserved_provider")" = /preserved/provider ]
[ ! -e "$agent_plist" ]
[ ! -e "$launchctl_state" ]
[ ! -e "$state/.maintenance" ]
grep -Fx 'format=3' "$state/install/current/manifest.txt" >/dev/null
grep -Fx "version=$package_version" \
    "$state/install/current/manifest.txt" >/dev/null
grep -E '^chancery_sha256=[0-9a-f]{64}$' \
    "$state/install/current/manifest.txt" >/dev/null
grep -F "Chancery provider: $weaver_provider" "$temporary/first.out" >/dev/null
grep -Fx 'doctor' "$state/commands.log" >/dev/null
grep -Fx 'maintenance begin --wait-seconds 0' "$state/commands.log" >/dev/null
grep -Fx 'worker run' "$state/commands.log" >/dev/null
grep -Fx 'maintenance end' "$state/commands.log" >/dev/null
HOME="$home" WEAVER_STATE_DIR="$state" "$cli" --version >/dev/null

first_release=$(readlink "$state/install/current")
first_provider_release=$(CDPATH='' cd "$weaver_provider" && pwd -P)
expected_first_provider=$(CDPATH='' \
    cd "$state/install/$first_release/share/chancery/weaver" && pwd -P)
[ "$first_provider_release" = "$expected_first_provider" ]
deploy "$candidate" >/dev/null
[ "$(readlink "$state/install/current")" = "$first_release" ]
[ ! -e "$state/install/previous" ]
[ ! -e "$agent_plist" ]
[ ! -e "$launchctl_state" ]
[ ! -e "$state/.maintenance" ]
[ "$(readlink "$weaver_provider")" = \
    "$state/install/current/share/chancery/weaver" ]
[ "$(readlink "$preserved_provider")" = /preserved/provider ]

# A provider selector owned by another installation must never be taken over.
rm "$weaver_provider"
ln -s /foreign/weaver-provider "$weaver_provider"
if deploy "$candidate" >"$temporary/foreign-provider.out" \
    2>"$temporary/foreign-provider.err"
then
    printf '%s\n' 'deployment unexpectedly replaced a foreign provider selector' >&2
    exit 1
fi
grep -F 'Chancery provider selector is not owned by this Weaver installation' \
    "$temporary/foreign-provider.err" >/dev/null
[ "$(readlink "$weaver_provider")" = /foreign/weaver-provider ]
[ "$(readlink "$preserved_provider")" = /preserved/provider ]
[ ! -e "$state/install/.update-lock" ]
rm "$weaver_provider"
ln -s "$state/install/current/share/chancery/weaver" "$weaver_provider"

# Give the candidate a new content identity so rollback crosses releases.
printf '%s\n' '# candidate update' >>"$candidate"

# Simulate the exact installed prototype and prove that even a bootout which
# reports failure after unloading is restored before deployment returns.
mkdir -p "$(dirname "$agent_plist")"
printf '%s\n' 'prototype-plist' >"$agent_plist"
: >"$launchctl_state"
: >"$launchctl_fail_bootout"
if deploy "$candidate" >"$temporary/bootout.out" 2>"$temporary/bootout.err"; then
    printf '%s\n' 'deployment unexpectedly ignored a failed prototype bootout' >&2
    exit 1
fi
[ "$(cat "$agent_plist")" = prototype-plist ]
[ -f "$launchctl_state" ]
[ ! -e "$state/.maintenance" ]
[ ! -e "$state/install/.update-lock" ]
[ "$(CDPATH='' cd "$weaver_provider" && pwd -P)" = "$first_provider_release" ]
[ "$(readlink "$preserved_provider")" = /preserved/provider ]

# Fail after the prototype was stopped and its plist removed. The old
# selector, exact plist, and loaded service must all return.
: >"$state/fail-worker"
if deploy "$candidate" >"$temporary/worker.out" 2>"$temporary/worker.err"; then
    printf '%s\n' 'deployment unexpectedly accepted a failed installed worker smoke' >&2
    exit 1
fi
rm -f "$state/fail-worker"
[ "$(readlink "$state/install/current")" = "$first_release" ]
[ ! -e "$state/install/previous" ]
[ "$(cat "$agent_plist")" = prototype-plist ]
[ -f "$launchctl_state" ]
[ ! -e "$state/.maintenance" ]
[ ! -e "$state/install/.update-lock" ]
[ "$(CDPATH='' cd "$weaver_provider" && pwd -P)" = "$first_provider_release" ]
[ "$(readlink "$preserved_provider")" = /preserved/provider ]

# A successful migration removes only the exact prototype service and plist.
deploy "$candidate" >"$temporary/migration.out"
second_release=$(readlink "$state/install/current")
[ "$second_release" != "$first_release" ]
[ "$(readlink "$state/install/previous")" = "$first_release" ]
expected_second_provider=$(CDPATH='' \
    cd "$state/install/$second_release/share/chancery/weaver" && pwd -P)
[ "$(CDPATH='' cd "$weaver_provider" && pwd -P)" = \
    "$expected_second_provider" ]
[ "$(readlink "$preserved_provider")" = /preserved/provider ]
[ ! -e "$agent_plist" ]
[ ! -e "$launchctl_state" ]
[ ! -e "$state/.maintenance" ]
grep -F "Removed prototype service: gui/$uid/org.weaver.worker" \
    "$temporary/migration.out" >/dev/null
grep -Fx "bootout --wait gui/$uid/org.weaver.worker" \
    "$launchctl_log" >/dev/null

: >"$state/fail-doctor"
if deploy "$candidate" >"$temporary/doctor.out" 2>"$temporary/doctor.err"; then
    printf '%s\n' 'deployment unexpectedly accepted a failed doctor' >&2
    exit 1
fi
rm -f "$state/fail-doctor"
[ "$(readlink "$state/install/current")" = "$second_release" ]
[ "$(readlink "$state/install/previous")" = "$first_release" ]
[ ! -e "$agent_plist" ]
[ ! -e "$launchctl_state" ]

printf '%s\n' '# tampered payload' >>"$state/install/current/bin/weaver"
if deploy "$candidate" >"$temporary/tampered.out" 2>"$temporary/tampered.err"; then
    printf '%s\n' 'deployment unexpectedly accepted a tampered release' >&2
    exit 1
fi
[ "$(readlink "$state/install/current")" = "$second_release" ]
[ "$(readlink "$state/install/previous")" = "$first_release" ]
[ ! -e "$state/install/.update-lock" ]
install -m 0755 "$candidate" "$state/install/current/bin/weaver"

printf '%s\n' ' ' >>"$state/install/current/share/chancery/weaver/provider.json"
if deploy "$candidate" >"$temporary/tampered-provider.out" \
    2>"$temporary/tampered-provider.err"
then
    printf '%s\n' 'deployment unexpectedly accepted a tampered Chancery bundle' >&2
    exit 1
fi
grep -F 'existing release Chancery bundle is invalid' \
    "$temporary/tampered-provider.err" >/dev/null
[ "$(readlink "$state/install/current")" = "$second_release" ]
[ "$(readlink "$state/install/previous")" = "$first_release" ]
[ "$(readlink "$weaver_provider")" = \
    "$state/install/current/share/chancery/weaver" ]
[ "$(readlink "$preserved_provider")" = /preserved/provider ]
[ ! -e "$state/install/.update-lock" ]
install -m 0600 "$temporary/share/chancery/weaver/provider.json" \
    "$state/install/current/share/chancery/weaver/provider.json"

: >"$state/fail-maintenance"
if deploy "$candidate" >"$temporary/maintenance.out" \
    2>"$temporary/maintenance.err"
then
    printf '%s\n' 'deployment unexpectedly ignored maintenance failure' >&2
    exit 1
fi
rm -f "$state/fail-maintenance"
[ "$(readlink "$state/install/current")" = "$second_release" ]
[ "$(readlink "$state/install/previous")" = "$first_release" ]
[ ! -e "$state/.maintenance" ]
[ ! -e "$state/install/.update-lock" ]

: >"$state/fail-maintenance-end"
if deploy "$candidate" >"$temporary/maintenance-end.out" \
    2>"$temporary/maintenance-end.err"
then
    printf '%s\n' 'deployment unexpectedly hid a post-commit maintenance failure' >&2
    exit 1
fi
grep -F 'installation committed but maintenance could not end' \
    "$temporary/maintenance-end.err" >/dev/null
[ "$(readlink "$state/install/current")" = "$second_release" ]
[ -f "$state/.maintenance" ]
[ ! -e "$state/install/.update-lock" ]
[ ! -e "$agent_plist" ]
[ ! -e "$launchctl_state" ]
rm -f "$state/fail-maintenance-end"
HOME="$home" WEAVER_STATE_DIR="$state" "$cli" maintenance end >/dev/null
[ ! -e "$state/.maintenance" ]

if "$package/deploy-user.sh" \
    --binary relative/weaver \
    --home "$home" \
    --launchctl "$launchctl" \
    >"$temporary/relative.out" 2>"$temporary/relative.err"
then
    printf '%s\n' 'deployment unexpectedly accepted a relative binary path' >&2
    exit 1
fi
grep -F 'binary path must be absolute' "$temporary/relative.err" >/dev/null

ln -s "$candidate" "$temporary/weaver-symlink"
if deploy "$temporary/weaver-symlink" \
    >"$temporary/symlink.out" 2>"$temporary/symlink.err"
then
    printf '%s\n' 'deployment unexpectedly accepted a symlink candidate' >&2
    exit 1
fi
grep -F 'not an executable regular file' "$temporary/symlink.err" >/dev/null

mkdir "$state/install/.update-lock"
if deploy "$candidate" >"$temporary/locked.out" 2>"$temporary/locked.err"; then
    printf '%s\n' 'deployment unexpectedly ignored the update lock' >&2
    exit 1
fi
grep -F 'another Weaver deployment is active' "$temporary/locked.err" >/dev/null
rmdir "$state/install/.update-lock"

printf '%s\n' 'deploy test passed'
