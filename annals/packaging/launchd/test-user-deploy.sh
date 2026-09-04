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
clockwork="$temporary/clockwork"
clockwork_loaded="$temporary/clockwork-loaded"
fail_clockwork_switch="$temporary/fail-next-clockwork-switch"
fail_bootout="$temporary/fail-next-bootout"

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
cp "$SCRIPT_DIR/annals-inbox" "$package/annals-inbox"
cp "$SCRIPT_DIR/annals-inbox.clockwork.toml.in" \
    "$package/annals-inbox.clockwork.toml.in"
cp "$SCRIPT_DIR/annals-decisions.toml.in" \
    "$package/annals-decisions.toml.in"
cp "$SCRIPT_DIR/annals-decisions-inbox.clockwork.toml.in" \
    "$package/annals-decisions-inbox.clockwork.toml.in"
cp "$SCRIPT_DIR/provision-decisions-user.sh" \
    "$package/provision-decisions-user.sh"
cp "$SCRIPT_DIR/org.annals.inbox.agent.plist" \
    "$package/org.annals.inbox.agent.plist"
cp -R "$SCRIPT_DIR/../../chancery/annals" "$package_share/annals"
cp -R "$SCRIPT_DIR/../../chancery/annals-usage" "$package_share/annals-usage"
chmod 0755 "$package/deploy-user.sh" "$package/annals-user" \
    "$package/annals-inbox" "$package/provision-decisions-user.sh"

cat >"$candidate_template" <<'EOF'
#!/bin/sh
set -eu
config=
reject_unmigrated_schema_four=__REJECT_UNMIGRATED_SCHEMA_FOUR__
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
                if [ "$reject_unmigrated_schema_four" -eq 1 ] \
                    && [ -f "$state/schema-4" ] \
                    && [ ! -f "$state/migrated" ]
                then
                    exit 5
                fi
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
                if [ "$reject_unmigrated_schema_four" -eq 1 ] \
                    && [ -f "$state/schema-4" ] \
                    && [ ! -f "$state/migrated" ]
                then
                    exit 5
                fi
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
        if [ "$reject_unmigrated_schema_four" -eq 1 ] \
            && [ -f "$state/schema-4" ] \
            && [ ! -f "$state/migrated" ]
        then
            exit 5
        fi
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
sed \
    -e "s/__ANNALS_VERSION__/$annals_version/g" \
    -e 's/__REJECT_UNMIGRATED_SCHEMA_FOUR__/0/g' \
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

cat >"$clockwork" <<EOF
#!/bin/sh
set -eu
printf '%s\n' "\$*" >>'$temporary/clockwork.log'
[ "\${1:-}" = --json ] && shift
root="\${HOME:?}/Library/Application Support/Clockwork/test"
binding="\$root/annals.inbox"
mkdir -p "\$root" "$clockwork_loaded"
command=\${1:-}; shift || true
case "\$command:\${1:-}" in
    definition:register)
        shift
        digest=\$(shasum -a 256 "\$1" | awk '{print \$1}')
        cp "\$1" "\$root/definition.\$digest.toml"
        printf '{"ok":true,"data":{"digest":"%s"}}\n' "\$digest"
        ;;
    definition:show)
        shift
        selected_digest=\$1
        selected_definition="\$root/definition.\$selected_digest.toml"
        [ -f "\$selected_definition" ] || exit 1
        selected_release_id=\$(sed -n 's/^release_id = "\(.*\)"$/\1/p' \
            "\$selected_definition")
        selected_release_root=\$(sed -n 's/^release_root = "\(.*\)"$/\1/p' \
            "\$selected_definition")
        selected_cwd=\$(sed -n 's/^cwd = "\(.*\)"$/\1/p' "\$selected_definition")
        selected_seconds=\$(sed -n 's/^seconds = \([0-9][0-9]*\)$/\1/p' \
            "\$selected_definition")
        selected_run_at_load=\$(sed -n 's/^run_at_load = \(.*\)$/\1/p' \
            "\$selected_definition")
        selected_interpreter_hash=\$(sed -n \
            's/^interpreter_sha256 = "\(.*\)"$/\1/p' "\$selected_definition")
        selected_script=\$(sed -n 's/^script = "\(.*\)"$/\1/p' \
            "\$selected_definition")
        selected_script_hash=\$(sed -n 's/^script_sha256 = "\(.*\)"$/\1/p' \
            "\$selected_definition")
        selected_home=\$(sed -n 's/^HOME = "\(.*\)"$/\1/p' "\$selected_definition")
        selected_user=\$(sed -n 's/^USER = "\(.*\)"$/\1/p' "\$selected_definition")
        selected_logname=\$(sed -n 's/^LOGNAME = "\(.*\)"$/\1/p' \
            "\$selected_definition")
        selected_config=\$(sed -n 's/^ANNALS_CONFIG = "\(.*\)"$/\1/p' \
            "\$selected_definition")
        selected_stdout=\$(sed -n 's/^stdout = "\(.*\)"$/\1/p' \
            "\$selected_definition")
        selected_stderr=\$(sed -n 's/^stderr = "\(.*\)"$/\1/p' \
            "\$selected_definition")
        printf '{"ok":true,"data":{"digest":"%s","key":"annals/inbox","registered_at":1,"manifest":{"schema_version":1,"key":"annals/inbox","release_id":"%s","release_root":"%s","authority":"current-user-background","overlap":"skip","arguments":[],"cwd":"%s","schedule":{"kind":"interval","seconds":%s,"run_at_load":%s},"launch":{"kind":"interpreted","interpreter":"/bin/sh","interpreter_sha256":"%s","script":"%s","script_sha256":"%s"},"environment":{"HOME":"%s","USER":"%s","LOGNAME":"%s","ANNALS_CONFIG":"%s"},"output":{"stdout":"%s","stderr":"%s"}}}}\n' \
            "\$selected_digest" "\$selected_release_id" "\$selected_release_root" \
            "\$selected_cwd" "\$selected_seconds" "\$selected_run_at_load" \
            "\$selected_interpreter_hash" "\$selected_script" \
            "\$selected_script_hash" "\$selected_home" "\$selected_user" \
            "\$selected_logname" "\$selected_config" "\$selected_stdout" \
            "\$selected_stderr"
        ;;
    binding:show)
        shift
        if [ ! -f "\$binding" ]; then
            printf '%s\n' '{"ok":false,"error":{"code":"binding_not_found","message":"absent"}}' >&2
            exit 1
        fi
        enabled=\$(sed -n '1p' "\$binding")
        digest=\$(sed -n '2p' "\$binding")
        if [ -n "\$digest" ]; then digest_json="\"\$digest\""; else digest_json=null; fi
        printf '{"ok":true,"data":{"key":"annals/inbox","definition_digest":%s,"enabled":%s,"updated_at":1}}\n' \
            "\$digest_json" "\$enabled"
        ;;
    binding:disable)
        shift
        key=\$1
        shift
        digest=
        [ ! -f "\$binding" ] || digest=\$(sed -n '2p' "\$binding")
        if [ "\${1:-}" = --select ]; then
            digest=\${2:?}
        fi
        printf 'false\n%s\n' "\$digest" >"\$binding"
        rm -f "$clockwork_loaded/org.clockwork.annals.inbox"
        if [ -n "\$digest" ]; then digest_json="\"\$digest\""; else digest_json=null; fi
        printf '{"ok":true,"data":{"key":"%s","definition_digest":%s,"enabled":false}}\n' \
            "\$key" "\$digest_json"
        ;;
    binding:switch)
        shift
        key=\$1; digest=\$2
        if [ -f "$fail_clockwork_switch" ]; then
            rm -f "$fail_clockwork_switch"
            exit 1
        fi
        printf 'true\n%s\n' "\$digest" >"\$binding"
        : >"$clockwork_loaded/org.clockwork.annals.inbox"
        printf '{"ok":true,"data":{"key":"%s","definition_digest":"%s","enabled":true}}\n' "\$key" "\$digest"
        ;;
    *) exit 1 ;;
esac
EOF
chmod 0755 "$clockwork"

deploy() {
    selected_nucleus=${ANNALS_TEST_NUCLEUS:-$nucleus}
    selected_candidate=${ANNALS_TEST_BINARY:-$candidate}
    selected_usage_candidate=${ANNALS_TEST_USAGE_BINARY:-$usage_candidate}
    HOME="$home" "$package/deploy-user.sh" \
        --binary "$selected_candidate" \
        --usage-binary "$selected_usage_candidate" \
        --nucleus "$selected_nucleus" \
        --nucleus-socket "$nucleus_socket" \
        --clockwork "$clockwork" \
        --home "$home" \
        --launchctl "$launchctl" \
        "$@"
}

deploy --no-start >/dev/null
[ ! -e "$launchctl_log" ]

mismatched_candidate="$temporary/annals-mismatched-provider"
sed \
    -e "s/__ANNALS_VERSION__/$annals_mismatch_version/g" \
    -e 's/__REJECT_UNMIGRATED_SCHEMA_FOUR__/0/g' \
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
[ -x "$state/install/current/bin/annals-inbox" ]
grep -Fx 'umask 077' "$state/install/current/bin/annals-inbox" >/dev/null
[ -x "$state/install/current/package/annals-inbox" ]
[ -f "$state/install/current/package/annals-inbox.clockwork.toml.in" ]
[ -f "$state/install/current/package/annals-decisions.toml.in" ]
[ -f "$state/install/current/package/annals-decisions-inbox.clockwork.toml.in" ]
[ -x "$state/install/current/package/provision-decisions-user.sh" ]
[ -f "$state/install/current/package/org.annals.inbox.agent.plist" ]
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
    "$state/install/current/manifest.json")" -eq 4 ]
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
legacy_agent_plist_hash=$(shasum -a 256 \
    "$SCRIPT_DIR/org.annals.inbox.agent.plist" | awk '{print $1}')
grep -Fx "  \"legacy_agent_plist_sha256\": \"$legacy_agent_plist_hash\"," \
    "$state/install/current/manifest.json" >/dev/null
decisions_config_hash=$(shasum -a 256 \
    "$SCRIPT_DIR/annals-decisions.toml.in" | awk '{print $1}')
decisions_definition_hash=$(shasum -a 256 \
    "$SCRIPT_DIR/annals-decisions-inbox.clockwork.toml.in" | awk '{print $1}')
decisions_provisioner_hash=$(shasum -a 256 \
    "$SCRIPT_DIR/provision-decisions-user.sh" | awk '{print $1}')
grep -Fx "  \"decisions_config_template_sha256\": \"$decisions_config_hash\"," \
    "$state/install/current/manifest.json" >/dev/null
grep -Fx "  \"decisions_clockwork_template_sha256\": \"$decisions_definition_hash\"," \
    "$state/install/current/manifest.json" >/dev/null
grep -Fx "  \"decisions_provisioner_sha256\": \"$decisions_provisioner_hash\"," \
    "$state/install/current/manifest.json" >/dev/null
printf '%s\n' preserved >"$state/spool/duplicates/preserved"
printf '%s\n' skipped >"$state/spool/skipped/preserved"
: >"$state/spool/.paused"
definition=$(find "$home/Library/Application Support/Clockwork/test" \
    -type f -name 'definition.*.toml' -exec grep -l \
    "release_root = \"$state/install/$(readlink "$state/install/current")\"" {} \; \
    | head -1)
[ -n "$definition" ]
grep -Fx 'key = "annals/inbox"' "$definition" >/dev/null
grep -Fx 'seconds = 300' "$definition" >/dev/null
grep -Fx 'run_at_load = true' "$definition" >/dev/null
grep -Fx 'overlap = "skip"' "$definition" >/dev/null
! grep -F 'timeout_seconds' "$definition" >/dev/null
grep -Fx "cwd = \"$state\"" "$definition" >/dev/null
grep -Fx "script = \"$state/install/$(readlink "$state/install/current")/bin/annals-inbox\"" \
    "$definition" >/dev/null
grep -Fx "HOME = \"$home\"" "$definition" >/dev/null
grep -Fx "ANNALS_CONFIG = \"$state/config.toml\"" "$definition" >/dev/null
grep -Fx "stdout = \"$state/log/inbox.stdout.log\"" "$definition" >/dev/null
grep -Fx "stderr = \"$state/log/inbox.stderr.log\"" "$definition" >/dev/null
! grep -E 'CODEX_HOME|TOKEN|SECRET|CREDENTIAL' "$definition" >/dev/null
[ ! -e "$plist" ]
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
# Model the real schema-four-to-five boundary: the old installed binary can
# inspect the existing library, while the candidate refuses to open it until
# its later guarded migration has completed.
: >"$state/schema-4"
sed \
    -e "s/__ANNALS_VERSION__/$annals_version/g" \
    -e 's/__REJECT_UNMIGRATED_SCHEMA_FOUR__/1/g' \
    "$candidate_template" >"$candidate"
chmod 0755 "$candidate"
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
[ -f "$rollback_snapshot/schedule.txt" ]
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

printf '%s\n' '# tampered' \
    >>"$state/install/current/package/annals-decisions.toml.in"
if deploy --no-start >"$temporary/tampered-decisions.out" \
    2>"$temporary/tampered-decisions.err"
then
    printf '%s\n' 'deployment unexpectedly accepted a tampered decisions template' >&2
    exit 1
fi
install -m 0600 "$package/annals-decisions.toml.in" \
    "$state/install/current/package/annals-decisions.toml.in"

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
fail_bootstrap="$fail_clockwork_switch"
cat >"$launchctl" <<EOF
#!/bin/sh
printf '%s\n' "\$*" >>'$launchctl_log'
case "\${1:-}" in
    print)
        [ -f '$loaded' ]
        ;;
    disable|enable)
        ;;
    bootout)
        if [ -f '$fail_bootout' ]; then
            rm -f '$fail_bootout'
            exit 1
        fi
        rm -f '$loaded'
        ;;
    bootstrap)
        [ ! -f '$loaded' ] || exit 1
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

# Refuse a foreign file at the migration-only legacy LaunchAgent path before
# touching either scheduler.
cat >"$plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict><key>Label</key><string>org.foreign.inbox</string><key>ProgramArguments</key><array><string>/bin/false</string></array></dict></plist>
EOF
if deploy >"$temporary/foreign-plist.out" 2>"$temporary/foreign-plist.err"; then
    printf '%s\n' 'deployment unexpectedly replaced a foreign LaunchAgent' >&2
    exit 1
fi
grep -F 'org.foreign.inbox' "$plist" >/dev/null
rm -f "$plist"

# Model the one-time migration from Annals' owned direct LaunchAgent. The
# deployer must quiesce and remove it before enabling Clockwork.
cp "$SCRIPT_DIR/org.annals.inbox.agent.plist" "$plist"
plutil -remove ProgramArguments.0 "$plist"
plutil -insert ProgramArguments.0 -string "$cli" "$plist"
plutil -replace WorkingDirectory -string "$state" "$plist"
plutil -replace EnvironmentVariables.HOME -string "$home" "$plist"
plutil -replace StandardOutPath -string "$state/log/inbox.stdout.log" "$plist"
plutil -replace StandardErrorPath -string "$state/log/inbox.stderr.log" "$plist"
chmod 0600 "$plist"

# Matching the executable tuple is insufficient ownership: any additional
# launchd behavior makes the legacy file foreign and it must remain untouched.
install -m 0600 "$plist" "$temporary/owned-agent.plist"
plutil -insert KeepAlive -bool true "$plist"
if deploy >"$temporary/extra-plist-key.out" 2>"$temporary/extra-plist-key.err"; then
    printf '%s\n' 'deployment unexpectedly removed a LaunchAgent with extra behavior' >&2
    exit 1
fi
plutil -extract KeepAlive raw -o - "$plist" >/dev/null
install -m 0600 "$temporary/owned-agent.plist" "$plist"
: >"$loaded"

# If bootout reports a transition failure while the legacy service remains
# loaded, rollback must not try to bootstrap the already loaded label.
: >"$fail_bootout"
if deploy >"$temporary/legacy-bootout-failure.out" \
    2>"$temporary/legacy-bootout-failure.err"
then
    printf '%s\n' 'deployment unexpectedly survived a legacy bootout failure' >&2
    exit 1
fi
[ -f "$loaded" ]
[ -f "$plist" ]
[ ! -e "$state/spool/.maintenance" ]
[ -L "$cli" ]
[ ! -e "$clockwork_loaded/org.clockwork.annals.inbox" ]

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
[ ! -f "$loaded" ]
[ ! -e "$plist" ]
[ -f "$clockwork_loaded/org.clockwork.annals.inbox" ]
[ ! -e "$state/spool/.maintenance" ]
[ -f "$state/spool/.paused" ]
[ "$(cat "$state/codex-home/auth.json")" = credential-sentinel ]
[ "$(stat -f '%Lp' "$state/codex-home/auth.json")" = 600 ]
[ "$(tail -n 1 "$state/usage-doctor.log")" = \
    "doctor current=$second_release" ]
[ "$(tail -n 8 "$state/candidate-commands.log" | tr '\n' ' ')" = \
    'inbox status inbox status backup migrate inbox status inbox run stats inbox status ' ]

# A same-key selected digest is not ownership. Even while disabled, a
# definition that is not the exact current Annals release must remain
# untouched and must block the deployment before any binding mutation.
clockwork_binding="$home/Library/Application Support/Clockwork/test/annals.inbox"
owned_definition_digest=$(sed -n '2p' "$clockwork_binding")
[ -n "$owned_definition_digest" ]
binding_mutations_before=$(grep -Ec '^--json binding (disable|switch) ' \
    "$temporary/clockwork.log")
clockwork_store="$home/Library/Application Support/Clockwork/test"
sed 's/^seconds = 300$/seconds = 301/' \
    "$clockwork_store/definition.$owned_definition_digest.toml" \
    >"$clockwork_store/definition.bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.toml"
printf '%s\n%s\n' false \
    bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb \
    >"$clockwork_binding"
rm -f "$clockwork_loaded/org.clockwork.annals.inbox"
if deploy >"$temporary/foreign-clockwork.out" \
    2>"$temporary/foreign-clockwork.err"
then
    printf '%s\n' 'deployment unexpectedly replaced a foreign Clockwork definition' >&2
    exit 1
fi
grep -F 'does not select the exact current Annals release definition' \
    "$temporary/foreign-clockwork.err" >/dev/null
binding_mutations_after=$(grep -Ec '^--json binding (disable|switch) ' \
    "$temporary/clockwork.log")
[ "$binding_mutations_after" -eq "$binding_mutations_before" ]
[ "$(sed -n '1p' "$clockwork_binding")" = false ]
[ "$(sed -n '2p' "$clockwork_binding")" = \
    bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb ]
[ "$(readlink "$state/install/current")" = "$running_release" ]
[ ! -e "$state/spool/.maintenance" ]
printf 'true\n%s\n' "$owned_definition_digest" >"$clockwork_binding"
: >"$clockwork_loaded/org.clockwork.annals.inbox"

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
    printf '%s\n' 'deployment unexpectedly survived a Clockwork switch failure' >&2
    exit 1
fi
[ "$(readlink "$state/install/current")" = "$running_release" ]
[ "$(readlink "$state/install/previous")" = "$second_release" ]
[ -f "$annals_provider/provider.json" ]
[ -f "$usage_provider/provider.json" ]
[ -f "$clockwork_loaded/org.clockwork.annals.inbox" ]
[ ! -e "$state/spool/.maintenance" ]
[ -f "$state/spool/.paused" ]
[ ! -e "$state/install/.update-lock" ]
[ "$(shasum -a 256 "$state/config.toml" | awk '{print $1}')" = "$config_before_rejection" ]
[ "$(shasum -a 256 "$state/usage.toml" | awk '{print $1}')" = "$usage_config_before_rejection" ]
[ "$(shasum -a 256 "$state/annals.db" | awk '{print $1}')" = "$library_before_rejection" ]
[ "$(cat "$state/usage.db")" = rejected-legacy ]
[ "$(cat "$state/usage.db-wal")" = rejected-legacy-wal ]
[ "$(cat "$state/usage.db-shm")" = rejected-legacy-shm ]
grep -Fx "nucleus = \"$nucleus\"" "$state/usage.toml" >/dev/null

# A disabled prior binding stays disabled during rollback; restoring an old
# inactive digest must not transiently enable its schedule.
HOME="$home" "$clockwork" --json binding disable annals/inbox >/dev/null
disabled_switches_before=$(grep -c '^--json binding switch ' "$temporary/clockwork.log")
: >"$fail_bootstrap"
if deploy >"$temporary/disabled-rollback.out" 2>"$temporary/disabled-rollback.err"; then
    printf '%s\n' 'failed deployment unexpectedly enabled a disabled prior binding' >&2
    exit 1
fi
disabled_switches_after=$(grep -c '^--json binding switch ' "$temporary/clockwork.log")
[ "$disabled_switches_after" -eq "$((disabled_switches_before + 1))" ]
[ "$(sed -n '1p' "$home/Library/Application Support/Clockwork/test/annals.inbox")" = false ]
[ ! -f "$clockwork_loaded/org.clockwork.annals.inbox" ]
[ "$(readlink "$state/install/current")" = "$running_release" ]

deploy >"$temporary/next-update.out" 2>"$temporary/next-update.err"
[ "$(readlink "$state/install/current")" != "$running_release" ]
[ "$(readlink "$state/install/previous")" = "$running_release" ]
[ -f "$clockwork_loaded/org.clockwork.annals.inbox" ]
[ ! -e "$state/spool/.maintenance" ]
[ -f "$state/spool/.paused" ]
[ ! -e "$state/install/.update-lock" ]
[ ! -e "$state/usage.db" ]
[ ! -e "$state/usage.db-wal" ]
[ ! -e "$state/usage.db-shm" ]

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
[ -f "$clockwork_loaded/org.clockwork.annals.inbox" ]
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
[ -f "$clockwork_loaded/org.clockwork.annals.inbox" ]
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
