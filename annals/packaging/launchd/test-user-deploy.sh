#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
temporary=$(mktemp -d "${TMPDIR:-/tmp}/annals-user-deploy-test.XXXXXX")

cleanup() {
    rm -rf "$temporary"
}
trap cleanup EXIT HUP INT TERM

package="$temporary/package"
package_share="$temporary/share/chancery"
home="$temporary/Operator Home"
candidate="$temporary/annals-candidate"
usage_candidate="$temporary/annals-usage-candidate"
candidate_template="$temporary/annals-candidate.template"
usage_candidate_template="$temporary/annals-usage-candidate.template"
nucleus="$temporary/nucleus"
nucleus_socket="$temporary/nucleus.sock"
launchctl="$temporary/launchctl"
launchctl_log="$temporary/launchctl.log"

read_package_version() {
    awk '
        $0 == "[package]" { in_package = 1; next }
        in_package && /^\[/ { exit }
        in_package && /^[[:space:]]*version[[:space:]]*=/ {
            value = $0
            sub(/^[^=]*=[[:space:]]*"/, "", value)
            sub(/"[[:space:]]*$/, "", value)
            print value
            exit
        }
    ' "$1"
}

annals_version=$(read_package_version \
    "$SCRIPT_DIR/../../crates/annals/Cargo.toml")
usage_version=$(read_package_version \
    "$SCRIPT_DIR/../../crates/annals-usage/Cargo.toml")
annals_provider_version=$(awk -F '"' \
    '/"release"[[:space:]]*:/ { print $4; exit }' \
    "$SCRIPT_DIR/../../chancery/annals/provider.json")
usage_provider_version=$(awk -F '"' \
    '/"release"[[:space:]]*:/ { print $4; exit }' \
    "$SCRIPT_DIR/../../chancery/annals-usage/provider.json")
[ -n "$annals_version" ] \
    && [ "$annals_provider_version" = "$annals_version" ] || {
    printf 'test: Annals package version %s does not match provider release %s\n' \
        "$annals_version" "$annals_provider_version" >&2
    exit 1
}
[ -n "$usage_version" ] \
    && [ "$usage_provider_version" = "$usage_version" ] || {
    printf 'test: Annals Usage package version %s does not match provider release %s\n' \
        "$usage_version" "$usage_provider_version" >&2
    exit 1
}
annals_mismatch_version="$annals_version-provider-mismatch"
usage_mismatch_version="$usage_version-provider-mismatch"

mkdir -p "$package" "$package_share" \
    "$home/Library/Application Support/Annals/codex-home"
cp "$SCRIPT_DIR/deploy-user.sh" "$package/deploy-user.sh"
cp "$SCRIPT_DIR/annals-user" "$package/annals-user"
cp "$SCRIPT_DIR/org.annals.inbox.agent.plist" \
    "$package/org.annals.inbox.agent.plist"
cp -R "$SCRIPT_DIR/../../chancery/annals" "$package_share/annals"
cp -R "$SCRIPT_DIR/../../chancery/annals-usage" "$package_share/annals-usage"
chmod 0755 "$package/deploy-user.sh" "$package/annals-user"

cat >"$candidate_template" <<'EOF'
#!/bin/sh
set -eu
config=
while [ "$#" -gt 0 ]; do
    case "$1" in
        --version)
            printf '%s\n' 'annals __ANNALS_VERSION__'
            exit 0
            ;;
        --config)
            config=$2
            shift 2
            ;;
        --quiet|--json)
            shift
            ;;
        *)
            break
            ;;
    esac
done
[ -n "$config" ] || config=${ANNALS_CONFIG:?}
state=$(CDPATH= cd "$(dirname "$config")" && pwd)
if [ -f "$state/expect-maintenance-before-checks" ] \
    && [ ! -f "$state/spool/.maintenance" ]
then
    : >"$state/maintenance-order-error"
fi
command=${1:?}
shift
if [ "$command" = inbox ]; then
    printf 'inbox %s\n' "${1:-}" >>"$state/candidate-commands.log"
else
    printf '%s\n' "$command" >>"$state/candidate-commands.log"
fi
case "$command" in
    environment)
        printf 'config=%s\n' "$config"
        printf 'codex_home=%s\n' "${CODEX_HOME-<unset>}"
        ;;
    init)
        : >"$state/annals.db"
        ;;
    stats)
        [ -f "$state/annals.db" ]
        ;;
    inbox)
        case "${1:-}" in
            status)
                queued=$(find "$state/spool/queued" -mindepth 1 -maxdepth 1 -type d 2>/dev/null \
                    | wc -l | tr -d ' ')
                processing=$(find "$state/spool/processing" -mindepth 1 -maxdepth 1 -type d 2>/dev/null \
                    | wc -l | tr -d ' ')
                locked=false
                paused=false
                maintenance=false
                [ ! -f "$state/spool/.paused" ] || paused=true
                [ ! -f "$state/spool/.maintenance" ] || maintenance=true
                if [ -f "$state/simulate-running-worker" ] \
                    && [ "$maintenance" = true ] \
                    && [ ! -f "$state/active-delivery-finished" ]
                then
                    : >"$state/active-delivery-finished"
                    locked=true
                fi
                printf '{"ok":true,"data":{"locked":%s,"queued":%s,"processing":%s,"paused":%s,"maintenance":%s}}\n' \
                    "$locked" "$queued" "$processing" "$paused" "$maintenance"
                ;;
            run)
                [ -f "$state/spool/.maintenance" ]
                [ -f "$state/spool/.paused" ]
                printf '%s\n' \
                    '{"ok":true,"data":{"stopped_for_maintenance":true}}'
                ;;
            pause)
                : >"$state/spool/.paused"
                ;;
            resume)
                rm -f "$state/spool/.paused"
                ;;
            register)
                ;;
            import-backlog)
                shift
                [ "${1:-}" = --from ]
                from=${2:?}
                [ ! -f "$from/fail-import" ] || exit 1
                imported=$(find "$from/queued" "$from/processing" \
                    -mindepth 1 -maxdepth 1 -type d 2>/dev/null \
                    | wc -l | tr -d ' ')
                sequence=1
                while [ "$sequence" -le "$imported" ]; do
                    id=$(printf 'j%020d' "$sequence")
                    mkdir -p "$state/spool/queued/$id/material"
                    printf '%s\n' imported >"$state/spool/queued/$id/material/source-$sequence.txt"
                    sequence=$((sequence + 1))
                done
                printf '{"ok":true,"data":{"imported":%s}}\n' "$imported"
                ;;
            *)
                exit 1
                ;;
        esac
        ;;
    backup)
        cp "$state/annals.db" "$1"
        ;;
    migrate)
        printf '%s\n' migrated >>"$state/annals.db"
        : >"$state/migrated"
        ;;
    *)
        printf 'unexpected fake Annals command: %s\n' "$command" >&2
        exit 1
        ;;
esac
EOF
sed "s/__ANNALS_VERSION__/$annals_version/g" \
    "$candidate_template" >"$candidate"
chmod 0755 "$candidate"

cat >"$usage_candidate_template" <<'EOF'
#!/bin/sh
set -eu
fail() {
    printf 'fake annals-usage: %s\n' "$*" >&2
    exit 1
}
case "${1:-}" in
    --version)
        printf '%s\n' 'annals-usage __ANNALS_USAGE_VERSION__'
        ;;
    doctor)
        [ "$#" -eq 3 ] || fail 'doctor argument count'
        [ "$2" = --config ] || fail 'doctor omitted --config'
        config=$3
        state=$(CDPATH= cd "$(dirname "$config")" && pwd)
        case "${config##*/}" in
            .usage.toml.*) ;;
            *) fail "doctor used unexpected config $config" ;;
        esac
        configured_nucleus=$(sed -n 's/^nucleus = "\([^"]*\)"$/\1/p' "$config")
        [ -n "$configured_nucleus" ] && [ -x "$configured_nucleus" ] \
            || fail 'doctor observed an unavailable Nucleus executable'
        grep -Fx "nucleus_socket = \"__NUCLEUS_SOCKET__\"" "$config" >/dev/null \
            || fail 'doctor observed the wrong Nucleus socket'
        if grep -Eq '^[[:space:]]*database[[:space:]]*=' "$config"; then
            fail 'doctor observed an obsolete usage database path'
        fi
        [ "${CODEX_HOME-unset}" = unset ] \
            || fail 'doctor inherited an Annals-owned CODEX_HOME'
        [ ! -e "$state/service-loaded" ] || {
            printf '%s\n' 'doctor ran while the old service was loaded' >&2
            exit 1
        }
        current=none
        if [ -L "$state/install/current" ]; then
            current=$(readlink "$state/install/current")
        fi
        printf 'doctor current=%s\n' "$current" >>"$state/usage-doctor.log"
        ;;
    *) exit 1 ;;
esac
EOF
sed \
    -e "s/__ANNALS_USAGE_VERSION__/$usage_version/g" \
    -e "s|__NUCLEUS__|$nucleus|g" \
    -e "s|__NUCLEUS_SOCKET__|$nucleus_socket|g" \
    "$usage_candidate_template" >"$usage_candidate"
chmod 0755 "$usage_candidate"

cat >"$nucleus" <<'EOF'
#!/bin/sh
case "${1:-}" in
    --version) printf '%s\n' 'nucleus test' ;;
    *) exit 1 ;;
esac
EOF
chmod 0755 "$nucleus"
: >"$nucleus_socket"
printf '%s\n' credential-sentinel \
    >"$home/Library/Application Support/Annals/codex-home/auth.json"
printf '%s\n' legacy-config-sentinel \
    >"$home/Library/Application Support/Annals/codex-home/config.toml"
chmod 0600 \
    "$home/Library/Application Support/Annals/codex-home/auth.json" \
    "$home/Library/Application Support/Annals/codex-home/config.toml"

cat >"$launchctl" <<EOF
#!/bin/sh
printf '%s\n' "\$*" >>'$launchctl_log'
exit 99
EOF
chmod 0755 "$launchctl"

deploy() {
    selected_nucleus=${ANNALS_TEST_NUCLEUS:-$nucleus}
    selected_candidate=${ANNALS_TEST_BINARY:-$candidate}
    selected_usage_candidate=${ANNALS_TEST_USAGE_BINARY:-$usage_candidate}
    HOME="$home" "$package/deploy-user.sh" \
        --binary "$selected_candidate" \
        --usage-binary "$selected_usage_candidate" \
        --nucleus "$selected_nucleus" \
        --nucleus-socket "$nucleus_socket" \
        --home "$home" \
        --launchctl "$launchctl" \
        "$@"
}

deploy --no-start >/dev/null
[ ! -e "$launchctl_log" ]

mismatched_candidate="$temporary/annals-mismatched-provider"
sed "s/__ANNALS_VERSION__/$annals_mismatch_version/g" \
    "$candidate_template" \
    >"$mismatched_candidate"
chmod 0755 "$mismatched_candidate"
if (ANNALS_TEST_BINARY="$mismatched_candidate" deploy --no-start) \
    >"$temporary/provider-mismatch.out" 2>"$temporary/provider-mismatch.err"
then
    printf '%s\n' 'deployment unexpectedly accepted an Annals provider/candidate mismatch' >&2
    exit 1
fi
grep -F "provider release $annals_provider_version does not match candidate $annals_mismatch_version" \
    "$temporary/provider-mismatch.err" >/dev/null

mismatched_usage="$temporary/annals-usage-mismatched-provider"
sed \
    -e "s/__ANNALS_USAGE_VERSION__/$usage_mismatch_version/g" \
    -e "s|__NUCLEUS__|$nucleus|g" \
    -e "s|__NUCLEUS_SOCKET__|$nucleus_socket|g" \
    "$usage_candidate_template" \
    >"$mismatched_usage"
chmod 0755 "$mismatched_usage"
if (ANNALS_TEST_USAGE_BINARY="$mismatched_usage" deploy --no-start) \
    >"$temporary/usage-provider-mismatch.out" \
    2>"$temporary/usage-provider-mismatch.err"
then
    printf '%s\n' 'deployment unexpectedly accepted an Annals Usage provider/candidate mismatch' >&2
    exit 1
fi
grep -F "provider release $usage_provider_version does not match candidate $usage_mismatch_version" \
    "$temporary/usage-provider-mismatch.err" >/dev/null

state="$home/Library/Application Support/Annals"
cli="$home/.local/bin/annals"
usage_cli="$home/.local/bin/annals-usage"
plist="$home/Library/LaunchAgents/org.annals.inbox.plist"
chancery_providers="$home/Library/Application Support/Chancery/providers"
annals_provider="$chancery_providers/annals"
usage_provider="$chancery_providers/annals-usage"
[ -L "$cli" ]
[ -L "$usage_cli" ]
[ -L "$state/install/current" ]
[ -f "$state/install/current/manifest.json" ]
[ -x "$state/install/current/libexec/annals" ]
[ -x "$state/install/current/libexec/annals-usage" ]
[ -f "$state/install/current/share/chancery/annals/provider.json" ]
[ -f "$state/install/current/share/chancery/annals-usage/provider.json" ]
[ -L "$annals_provider" ]
[ -L "$usage_provider" ]
[ "$(readlink "$annals_provider")" = \
    "$state/install/current/share/chancery/annals" ]
[ "$(readlink "$usage_provider")" = \
    "$state/install/current/share/chancery/annals-usage" ]
[ -f "$annals_provider/provider.json" ]
[ -f "$usage_provider/provider.json" ]
[ "$(sed -n 's/^  "format": \([0-9][0-9]*\),$/\1/p' \
    "$state/install/current/manifest.json")" -eq 2 ]
[ -f "$state/annals.db" ]
[ -d "$state/spool/queued" ]
[ -d "$state/spool/duplicates" ]
[ -d "$state/spool/skipped" ]
[ "$(cat "$state/codex-home/auth.json")" = credential-sentinel ]
[ "$(stat -f '%Lp' "$state/codex-home/auth.json")" = 600 ]
[ "$(cat "$state/codex-home/config.toml")" = legacy-config-sentinel ]
[ "$(stat -f '%Lp' "$state/codex-home/config.toml")" = 600 ]
[ "$(tail -n 1 "$state/usage-doctor.log")" = 'doctor current=none' ]
grep -Fx 'library = "annals.db"' "$state/config.toml" >/dev/null
grep -Fx 'root = "spool"' "$state/config.toml" >/dev/null
grep -Fx 'minimum_available_bytes = 7_000_000_000' \
    "$state/config.toml" >/dev/null
grep -Fx "nucleus_socket = \"$nucleus_socket\"" "$state/config.toml" >/dev/null
grep -Fx "nucleus = \"$nucleus\"" "$state/usage.toml" >/dev/null
grep -Fx "nucleus_socket = \"$nucleus_socket\"" "$state/usage.toml" >/dev/null
grep -Fx "library = \"$state/annals.db\"" "$state/usage.toml" >/dev/null
grep -Fx "spool = \"$state/spool\"" "$state/usage.toml" >/dev/null
if grep -Eq '^[[:space:]]*database[[:space:]]*=' "$state/usage.toml"; then
    printf '%s\n' 'usage config unexpectedly contains a database path' >&2
    exit 1
fi
[ ! -e "$state/usage.db" ]
[ "$(readlink "$usage_cli")" = "$state/install/current/libexec/annals-usage" ]
HOME="$home" "$usage_cli" --version >/dev/null
default_environment=$(env -u ANNALS_CONFIG -u ANNALS_LIBRARY -u CODEX_HOME \
    HOME="$home" "$cli" environment)
printf '%s\n' "$default_environment" \
    | grep -Fx "config=$state/config.toml" >/dev/null
printf '%s\n' "$default_environment" \
    | grep -Fx 'codex_home=<unset>' >/dev/null
custom_codex_home="$temporary/custom-codex-home"
alternate_config="$temporary/alternate.toml"
: >"$alternate_config"
alternate_environment=$(env -u ANNALS_CONFIG -u ANNALS_LIBRARY \
    HOME="$home" CODEX_HOME="$custom_codex_home" \
    "$cli" --config "$alternate_config" environment)
printf '%s\n' "$alternate_environment" \
    | grep -Fx "config=$alternate_config" >/dev/null
printf '%s\n' "$alternate_environment" \
    | grep -Fx "codex_home=$custom_codex_home" >/dev/null
usage_candidate_hash=$(shasum -a 256 "$usage_candidate" | awk '{print $1}')
grep -Fx "  \"usage_binary_sha256\": \"$usage_candidate_hash\"," \
    "$state/install/current/manifest.json" >/dev/null
printf '%s\n' preserved >"$state/spool/duplicates/preserved"
printf '%s\n' skipped >"$state/spool/skipped/preserved"
: >"$state/spool/.paused"
[ "$(plutil -extract ProgramArguments.0 raw -o - "$plist")" = "$cli" ]
[ "$(plutil -extract ProgramArguments.1 raw -o - "$plist")" = --quiet ]
[ "$(plutil -extract ProgramArguments.2 raw -o - "$plist")" = inbox ]
[ "$(plutil -extract ProgramArguments.3 raw -o - "$plist")" = run ]
if plutil -extract ProgramArguments.4 raw -o - "$plist" >/dev/null 2>&1; then
    printf '%s\n' 'user LaunchAgent contains an extra program argument' >&2
    exit 1
fi
[ "$(plutil -extract WorkingDirectory raw -o - "$plist")" = "$state" ]
[ "$(plutil -extract EnvironmentVariables.HOME raw -o - "$plist")" = "$home" ]
if plutil -extract EnvironmentVariables.CODEX_HOME raw -o - "$plist" >/dev/null 2>&1; then
    printf '%s\n' 'user LaunchAgent unexpectedly owns CODEX_HOME' >&2
    exit 1
fi
if plutil -extract UserName raw -o - "$plist" >/dev/null 2>&1; then
    printf '%s\n' 'user LaunchAgent unexpectedly contains UserName' >&2
    exit 1
fi
HOME="$home" "$cli" stats >/dev/null

first_release=$(readlink "$state/install/current")
ln -s /preserved/provider "$chancery_providers/preserved"
first_candidate_hash=$(shasum -a 256 "$candidate" | awk '{print $1}')
printf '%s\n' 'ambient_setting = true' >>"$state/codex-home/config.toml"
codex_config_with_ambient_setting=$(cat "$state/codex-home/config.toml")
deploy --no-start >/dev/null
[ "$(cat "$state/codex-home/config.toml")" = \
    "$codex_config_with_ambient_setting" ]
# Simulate the configuration written before the Nucleus requester integration.
# Deployment must migrate only the liaison selector and
# preserve the rest of the document.
awk -v codex="$nucleus" '
    /^[[:space:]]*nucleus_socket[[:space:]]*=/ {
        print "codex = \"" codex "\""
        next
    }
    { print }
' "$state/config.toml" >"$state/config.legacy.toml"
printf '%s\n' '# retained operator setting' >>"$state/config.legacy.toml"
mv "$state/config.legacy.toml" "$state/config.toml"
legacy_config_hash=$(shasum -a 256 "$state/config.toml" | awk '{print $1}')
grep -Fx "codex = \"$nucleus\"" "$state/config.toml" >/dev/null
printf '%s\n' legacy-normal >"$state/usage.db"
printf '%s\n' legacy-normal-wal >"$state/usage.db-wal"
printf '%s\n' legacy-normal-shm >"$state/usage.db-shm"
printf '%s\n' '# candidate update' >>"$candidate"
second_candidate_hash=$(shasum -a 256 "$candidate" | awk '{print $1}')
[ "$first_candidate_hash" != "$second_candidate_hash" ]
second_output=$(deploy --no-start)
second_release=$(readlink "$state/install/current")
if [ "$first_release" = "$second_release" ]; then
    printf '%s\n' "$second_output" >&2
    printf 'candidate changed from %s to %s but release stayed %s\n' \
        "$first_candidate_hash" "$second_candidate_hash" "$first_release" >&2
    exit 1
fi
[ "$(tail -n 1 "$state/usage-doctor.log")" = \
    "doctor current=$first_release" ]
[ "$(readlink "$state/install/previous")" = "$first_release" ]
[ -f "$annals_provider/provider.json" ]
[ -f "$usage_provider/provider.json" ]
[ "$(readlink "$chancery_providers/preserved")" = /preserved/provider ]
[ "$(shasum -a 256 "$state/config.toml" | awk '{print $1}')" != "$legacy_config_hash" ]
grep -Fx "nucleus_socket = \"$nucleus_socket\"" "$state/config.toml" >/dev/null
grep -Fx '# retained operator setting' "$state/config.toml" >/dev/null
[ ! -e "$state/usage.db" ]
[ ! -e "$state/usage.db-wal" ]
[ ! -e "$state/usage.db-shm" ]
rollback_snapshot=$(sed -n 's/^  "rollback_snapshot": "\([^"]*\)",$/\1/p' \
    "$state/install/last-update.json")
[ -n "$rollback_snapshot" ]
[ -f "$rollback_snapshot/config.toml" ]
[ -f "$rollback_snapshot/usage.toml" ]
[ -f "$rollback_snapshot/agent.plist" ]
[ -f "$rollback_snapshot/rollback.json" ]
grep -Fx "codex = \"$nucleus\"" "$rollback_snapshot/config.toml" >/dev/null
grep -F "\"release\": \"$first_release\"" \
    "$rollback_snapshot/rollback.json" >/dev/null
config_hash=$(shasum -a 256 "$state/config.toml" | awk '{print $1}')
usage_config_hash=$(shasum -a 256 "$state/usage.toml" | awk '{print $1}')
backup_count=$(find "$state/backups" -type f -maxdepth 1 | wc -l | tr -d ' ')
[ "$backup_count" -eq 1 ]
[ -f "$state/migrated" ]
grep -Fx preserved "$state/spool/duplicates/preserved" >/dev/null
grep -Fx skipped "$state/spool/skipped/preserved" >/dev/null
[ -f "$state/spool/.paused" ]
[ "$(tail -n 6 "$state/candidate-commands.log" | tr '\n' ' ')" = \
    'inbox status backup migrate inbox status stats inbox status ' ]
[ ! -e "$launchctl_log" ]
deploy --no-start >/dev/null
[ "$(readlink "$state/install/current")" = "$second_release" ]
[ "$(readlink "$state/install/previous")" = "$first_release" ]
[ -f "$annals_provider/provider.json" ]
[ -f "$usage_provider/provider.json" ]
[ "$(readlink "$chancery_providers/preserved")" = /preserved/provider ]
[ "$(shasum -a 256 "$state/config.toml" | awk '{print $1}')" = "$config_hash" ]
[ "$(shasum -a 256 "$state/usage.toml" | awk '{print $1}')" = "$usage_config_hash" ]
backup_count=$(find "$state/backups" -type f -maxdepth 1 | wc -l | tr -d ' ')
[ "$backup_count" -eq 1 ]

printf '%s\n' '# tampered' >>"$state/install/current/package/deploy-user.sh"
if deploy --no-start >"$temporary/tampered.out" 2>"$temporary/tampered.err"; then
    printf '%s\n' 'deployment unexpectedly accepted a tampered release' >&2
    exit 1
fi
install -m 0755 "$package/deploy-user.sh" \
    "$state/install/current/package/deploy-user.sh"

printf '%s\n' ' ' >>"$state/install/current/share/chancery/annals/provider.json"
if deploy --no-start >"$temporary/tampered-provider.out" \
    2>"$temporary/tampered-provider.err"
then
    printf '%s\n' 'deployment unexpectedly accepted a tampered Chancery bundle' >&2
    exit 1
fi
install -m 0600 "$package_share/annals/provider.json" \
    "$state/install/current/share/chancery/annals/provider.json"

loaded="$state/service-loaded"
fail_bootstrap="$temporary/fail-next-bootstrap"
fail_kickstart="$temporary/fail-next-kickstart"
kickstart_order_error="$temporary/kickstart-order-error"
cat >"$launchctl" <<EOF
#!/bin/sh
printf '%s\n' "\$*" >>'$launchctl_log'
case "\${1:-}" in
    print)
        [ -f '$loaded' ]
        ;;
    disable|enable)
        ;;
    kickstart)
        [ -f '$state/install/last-update.json' ] \
            || : >'$kickstart_order_error'
        current=\$(readlink '$state/install/current')
        release=\${current#releases/}
        grep -F "\"release_id\": \"\$release\"" \
            '$state/install/last-update.json' >/dev/null \
            || : >'$kickstart_order_error'
        [ ! -e '$state/spool/.maintenance' ] \
            || : >'$kickstart_order_error'
        if [ -f '$fail_kickstart' ]; then
            rm -f '$fail_kickstart'
            exit 1
        fi
        ;;
    bootout)
        rm -f '$loaded'
        ;;
    bootstrap)
        if [ -f '$fail_bootstrap' ]; then
            rm -f '$fail_bootstrap'
            exit 1
        fi
        : >'$loaded'
        ;;
    *)
        exit 1
        ;;
esac
EOF
chmod 0755 "$launchctl"

mkdir -p "$state/spool/queued/successor/material"
printf '%s\n' successor >"$state/spool/queued/successor/material/source.txt"
: >"$state/simulate-running-worker"
: >"$state/expect-maintenance-before-checks"
printf '%s\n' '# launchd update' >>"$candidate"
deploy >/dev/null
running_release=$(readlink "$state/install/current")
[ "$running_release" != "$second_release" ]
[ ! -e "$state/maintenance-order-error" ]
[ -f "$state/active-delivery-finished" ]
[ -f "$state/spool/queued/successor/material/source.txt" ]
[ -f "$loaded" ]
[ ! -e "$state/spool/.maintenance" ]
[ -f "$state/spool/.paused" ]
[ "$(cat "$state/codex-home/auth.json")" = credential-sentinel ]
[ "$(stat -f '%Lp' "$state/codex-home/auth.json")" = 600 ]
[ "$(tail -n 1 "$state/usage-doctor.log")" = \
    "doctor current=$second_release" ]
[ "$(tail -n 8 "$state/candidate-commands.log" | tr '\n' ' ')" = \
    'inbox status inbox status inbox run backup migrate inbox status stats inbox status ' ]

printf '%s\n' '# rejected update' >>"$candidate"
alternate_nucleus="$temporary/alternate-nucleus"
cp "$nucleus" "$alternate_nucleus"
chmod 0755 "$alternate_nucleus"
: >"$fail_bootstrap"
config_before_rejection=$(shasum -a 256 "$state/config.toml" | awk '{print $1}')
usage_config_before_rejection=$(shasum -a 256 "$state/usage.toml" | awk '{print $1}')
library_before_rejection=$(shasum -a 256 "$state/annals.db" | awk '{print $1}')
printf '%s\n' rejected-legacy >"$state/usage.db"
printf '%s\n' rejected-legacy-wal >"$state/usage.db-wal"
printf '%s\n' rejected-legacy-shm >"$state/usage.db-shm"
if ANNALS_TEST_NUCLEUS="$alternate_nucleus" \
    deploy >"$temporary/rejected.out" 2>"$temporary/rejected.err"
then
    printf '%s\n' 'deployment unexpectedly survived a bootstrap failure' >&2
    exit 1
fi
[ "$(readlink "$state/install/current")" = "$running_release" ]
[ "$(readlink "$state/install/previous")" = "$second_release" ]
[ -f "$annals_provider/provider.json" ]
[ -f "$usage_provider/provider.json" ]
[ -f "$loaded" ]
[ ! -e "$state/spool/.maintenance" ]
[ -f "$state/spool/.paused" ]
[ ! -e "$state/install/.update-lock" ]
[ ! -e "$kickstart_order_error" ]
[ "$(shasum -a 256 "$state/config.toml" | awk '{print $1}')" = "$config_before_rejection" ]
[ "$(shasum -a 256 "$state/usage.toml" | awk '{print $1}')" = "$usage_config_before_rejection" ]
[ "$(shasum -a 256 "$state/annals.db" | awk '{print $1}')" = "$library_before_rejection" ]
[ "$(cat "$state/usage.db")" = rejected-legacy ]
[ "$(cat "$state/usage.db-wal")" = rejected-legacy-wal ]
[ "$(cat "$state/usage.db-shm")" = rejected-legacy-shm ]
grep -Fx "nucleus = \"$nucleus\"" "$state/usage.toml" >/dev/null

: >"$fail_kickstart"
deploy >"$temporary/kickstart-warning.out" 2>"$temporary/kickstart-warning.err"
[ "$(readlink "$state/install/current")" != "$running_release" ]
[ "$(readlink "$state/install/previous")" = "$running_release" ]
[ -f "$loaded" ]
[ ! -e "$state/spool/.maintenance" ]
[ -f "$state/spool/.paused" ]
[ ! -e "$state/install/.update-lock" ]
[ ! -e "$kickstart_order_error" ]
[ ! -e "$state/usage.db" ]
[ ! -e "$state/usage.db-wal" ]
[ ! -e "$state/usage.db-shm" ]
grep -F 'warning: unable to wake the installed service' \
    "$temporary/kickstart-warning.err" >/dev/null

printf '%s\n' old-library >"$state/annals.db"
printf '%s\n' legacy-usage >"$state/usage.db"
printf '%s\n' legacy-wal >"$state/usage.db-wal"
printf '%s\n' legacy-shm >"$state/usage.db-shm"
mkdir -p \
    "$state/spool/processing/j00000000000000000090/material" \
    "$state/spool/queued/j00000000000000000091/material"
printf '%s\n' first >"$state/spool/processing/j00000000000000000090/material/first.txt"
printf '%s\n' second >"$state/spool/queued/j00000000000000000091/material/second.txt"
current_before_fresh=$(readlink "$state/install/current")
: >"$state/spool/fail-import"
if deploy --fresh-state >"$temporary/fresh-failure.out" 2>"$temporary/fresh-failure.err"; then
    printf '%s\n' 'fresh deployment unexpectedly survived backlog import failure' >&2
    exit 1
fi
[ "$(cat "$state/annals.db")" = old-library ]
[ "$(readlink "$state/install/current")" = "$current_before_fresh" ]
[ -f "$annals_provider/provider.json" ]
[ -f "$usage_provider/provider.json" ]
[ -f "$state/spool/processing/j00000000000000000090/material/first.txt" ]
[ -f "$state/spool/queued/j00000000000000000091/material/second.txt" ]
[ -f "$state/spool/.paused" ]
[ ! -e "$state/spool/.maintenance" ]
[ -f "$loaded" ]
[ "$(cat "$state/usage.db")" = legacy-usage ]
[ "$(cat "$state/usage.db-wal")" = legacy-wal ]
[ "$(cat "$state/usage.db-shm")" = legacy-shm ]
rm -f "$state/spool/fail-import"
fresh_output=$(deploy --fresh-state)
[ ! -s "$state/annals.db" ]
[ "$(find "$state/spool/queued" -mindepth 1 -maxdepth 1 -type d | wc -l | tr -d ' ')" -eq 3 ]
[ "$(find "$state/spool/processing" -mindepth 1 -maxdepth 1 -type d | wc -l | tr -d ' ')" -eq 0 ]
[ -d "$state/spool/skipped" ]
[ ! -e "$state/spool/.paused" ]
[ ! -e "$state/spool/.maintenance" ]
[ -f "$loaded" ]
grep -F '"fresh_state": true' "$state/install/last-update.json" >/dev/null
grep -F '"imported_backlog": 3' "$state/install/last-update.json" >/dev/null
generation=$(sed -n 's/^  "rollback_generation": "\([^"]*\)",$/\1/p' \
    "$state/install/last-update.json")
[ -n "$generation" ]
[ "$(cat "$state/backups/generations/$generation/annals.db")" = old-library ]
[ ! -e "$state/backups/generations/$generation/usage.db" ]
[ ! -e "$state/backups/generations/$generation/usage.db-wal" ]
[ ! -e "$state/backups/generations/$generation/usage.db-shm" ]
[ ! -e "$state/usage.db" ]
[ ! -e "$state/usage.db-wal" ]
[ ! -e "$state/usage.db-shm" ]
[ -f "$state/backups/generations/$generation/spool/duplicates/preserved" ]
[ -f "$state/backups/generations/$generation/spool/skipped/preserved" ]
[ -f "$state/backups/generations/$generation/spool/processing/j00000000000000000090/material/first.txt" ]
[ -f "$state/backups/generations/$generation/spool/queued/j00000000000000000091/material/second.txt" ]
printf '%s\n' "$fresh_output" | grep -F 'Imported backlog: 3' >/dev/null

rm -f "$annals_provider"
ln -s /foreign/annals-provider "$annals_provider"
if deploy --no-start >"$temporary/foreign-provider.out" \
    2>"$temporary/foreign-provider.err"
then
    printf '%s\n' 'deployment unexpectedly took over a foreign provider selector' >&2
    exit 1
fi
[ "$(readlink "$annals_provider")" = /foreign/annals-provider ]
rm -f "$annals_provider"
ln -s "$state/install/current/share/chancery/annals" "$annals_provider"

printf '%s\n' 'user deploy test passed'
