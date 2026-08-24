#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
temporary=$(mktemp -d "${TMPDIR:-/tmp}/annals-user-deploy-test.XXXXXX")

cleanup() {
    rm -rf "$temporary"
}
trap cleanup EXIT HUP INT TERM

package="$temporary/package"
home="$temporary/Operator Home"
candidate="$temporary/annals-candidate"
usage_candidate="$temporary/annals-usage-candidate"
codex="$temporary/codex"
launchctl="$temporary/launchctl"
launchctl_log="$temporary/launchctl.log"
mkdir -p "$package" "$home/Library/Application Support/Annals/codex-home"
cp "$SCRIPT_DIR/deploy-user.sh" "$package/deploy-user.sh"
cp "$SCRIPT_DIR/annals-user" "$package/annals-user"
cp "$SCRIPT_DIR/org.annals.inbox.agent.plist" \
    "$package/org.annals.inbox.agent.plist"
chmod 0755 "$package/deploy-user.sh" "$package/annals-user"

cat >"$candidate" <<'EOF'
#!/bin/sh
set -eu
config=
while [ "$#" -gt 0 ]; do
    case "$1" in
        --version)
            printf '%s\n' 'annals test-candidate'
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
command=${1:?}
shift
if [ "$command" = inbox ]; then
    printf 'inbox %s\n' "${1:-}" >>"$state/candidate-commands.log"
else
    printf '%s\n' "$command" >>"$state/candidate-commands.log"
fi
case "$command" in
    init)
        : >"$state/annals.db"
        ;;
    validate)
        [ -f "$state/annals.db" ]
        ;;
    inbox)
        case "${1:-}" in
            status)
                printf '%s\n' '{"ok":true,"data":{"locked":false}}'
                ;;
            run)
                [ -f "$state/spool/.maintenance" ]
                [ -f "$state/spool/.paused" ]
                printf '%s\n' \
                    '{"ok":true,"data":{"stopped_for_maintenance":true}}'
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
        : >"$state/migrated"
        ;;
    *)
        printf 'unexpected fake Annals command: %s\n' "$command" >&2
        exit 1
        ;;
esac
EOF
chmod 0755 "$candidate"

cat >"$usage_candidate" <<'EOF'
#!/bin/sh
case "${1:-}" in
    --version) printf '%s\n' 'annals-usage test-candidate' ;;
    *) exit 1 ;;
esac
EOF
chmod 0755 "$usage_candidate"

cat >"$codex" <<'EOF'
#!/bin/sh
case "${1:-}" in
    --version) printf '%s\n' 'codex test' ;;
    login) [ "${2:-}" = status ] ;;
    *) exit 1 ;;
esac
EOF
chmod 0755 "$codex"
: >"$home/Library/Application Support/Annals/codex-home/auth.json"

cat >"$launchctl" <<EOF
#!/bin/sh
printf '%s\n' "\$*" >>'$launchctl_log'
exit 99
EOF
chmod 0755 "$launchctl"

deploy() {
    selected_codex=${ANNALS_TEST_CODEX:-$codex}
    HOME="$home" "$package/deploy-user.sh" \
        --binary "$candidate" \
        --usage-binary "$usage_candidate" \
        --codex "$selected_codex" \
        --home "$home" \
        --launchctl "$launchctl" \
        "$@"
}

deploy --no-start >/dev/null
[ ! -e "$launchctl_log" ]

state="$home/Library/Application Support/Annals"
cli="$home/.local/bin/annals"
usage_cli="$home/.local/bin/annals-usage"
plist="$home/Library/LaunchAgents/org.annals.inbox.plist"
[ -L "$cli" ]
[ -L "$usage_cli" ]
[ -L "$state/install/current" ]
[ -f "$state/install/current/manifest.json" ]
[ -x "$state/install/current/libexec/annals" ]
[ -x "$state/install/current/libexec/annals-usage" ]
[ "$(sed -n 's/^  "format": \([0-9][0-9]*\),$/\1/p' \
    "$state/install/current/manifest.json")" -eq 2 ]
[ -f "$state/annals.db" ]
[ -d "$state/spool/queued" ]
[ -d "$state/spool/duplicates" ]
grep -Fx 'library = "annals.db"' "$state/config.toml" >/dev/null
grep -Fx 'root = "spool"' "$state/config.toml" >/dev/null
proxy="$state/install/current/libexec/annals-usage"
grep -Fx "codex = \"$proxy\"" "$state/config.toml" >/dev/null
grep -Fx "codex = \"$codex\"" "$state/usage.toml" >/dev/null
grep -Fx "codex_home = \"$state/codex-home\"" "$state/usage.toml" >/dev/null
grep -Fx "library = \"$state/annals.db\"" "$state/usage.toml" >/dev/null
grep -Fx "spool = \"$state/spool\"" "$state/usage.toml" >/dev/null
grep -Fx "database = \"$state/usage.db\"" "$state/usage.toml" >/dev/null
[ "$(readlink "$usage_cli")" = "$state/install/current/libexec/annals-usage" ]
HOME="$home" "$usage_cli" --version >/dev/null
usage_candidate_hash=$(shasum -a 256 "$usage_candidate" | awk '{print $1}')
grep -Fx "  \"usage_binary_sha256\": \"$usage_candidate_hash\"," \
    "$state/install/current/manifest.json" >/dev/null
printf '%s\n' preserved >"$state/spool/duplicates/preserved"
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
[ "$(plutil -extract EnvironmentVariables.CODEX_HOME raw -o - "$plist")" = "$state/codex-home" ]
if plutil -extract UserName raw -o - "$plist" >/dev/null 2>&1; then
    printf '%s\n' 'user LaunchAgent unexpectedly contains UserName' >&2
    exit 1
fi
HOME="$home" "$cli" validate >/dev/null

first_release=$(readlink "$state/install/current")
first_candidate_hash=$(shasum -a 256 "$candidate" | awk '{print $1}')
# Simulate the configuration written by releases before annals-usage became
# the default Codex proxy. Deployment must migrate only the Codex selector and
# preserve the rest of the document.
awk -v codex="$codex" '
    /^[[:space:]]*codex[[:space:]]*=/ {
        print "codex = \"" codex "\""
        next
    }
    { print }
' "$state/config.toml" >"$state/config.legacy.toml"
printf '%s\n' '# retained operator setting' >>"$state/config.legacy.toml"
mv "$state/config.legacy.toml" "$state/config.toml"
legacy_config_hash=$(shasum -a 256 "$state/config.toml" | awk '{print $1}')
grep -Fx "codex = \"$codex\"" "$state/config.toml" >/dev/null
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
[ "$(readlink "$state/install/previous")" = "$first_release" ]
[ "$(shasum -a 256 "$state/config.toml" | awk '{print $1}')" != "$legacy_config_hash" ]
grep -Fx "codex = \"$proxy\"" "$state/config.toml" >/dev/null
grep -Fx '# retained operator setting' "$state/config.toml" >/dev/null
config_hash=$(shasum -a 256 "$state/config.toml" | awk '{print $1}')
usage_config_hash=$(shasum -a 256 "$state/usage.toml" | awk '{print $1}')
backup_count=$(find "$state/backups" -type f -maxdepth 1 | wc -l | tr -d ' ')
[ "$backup_count" -eq 1 ]
[ -f "$state/migrated" ]
grep -Fx preserved "$state/spool/duplicates/preserved" >/dev/null
[ -f "$state/spool/.paused" ]
[ "$(tail -n 6 "$state/candidate-commands.log" | tr '\n' ' ')" = \
    'backup migrate validate inbox status validate inbox status ' ]
[ ! -e "$launchctl_log" ]
deploy --no-start >/dev/null
[ "$(readlink "$state/install/current")" = "$second_release" ]
[ "$(readlink "$state/install/previous")" = "$first_release" ]
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

loaded="$temporary/service-loaded"
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

printf '%s\n' '# launchd update' >>"$candidate"
deploy >/dev/null
running_release=$(readlink "$state/install/current")
[ "$running_release" != "$second_release" ]
[ -f "$loaded" ]
[ ! -e "$state/spool/.maintenance" ]
[ -f "$state/spool/.paused" ]
[ "$(tail -n 10 "$state/candidate-commands.log" | tr '\n' ' ')" = \
    'inbox status validate inbox status inbox run backup migrate validate inbox status validate inbox status ' ]

printf '%s\n' '# rejected update' >>"$candidate"
alternate_codex="$temporary/alternate-codex"
cp "$codex" "$alternate_codex"
chmod 0755 "$alternate_codex"
: >"$fail_bootstrap"
config_before_rejection=$(shasum -a 256 "$state/config.toml" | awk '{print $1}')
usage_config_before_rejection=$(shasum -a 256 "$state/usage.toml" | awk '{print $1}')
if ANNALS_TEST_CODEX="$alternate_codex" \
    deploy >"$temporary/rejected.out" 2>"$temporary/rejected.err"
then
    printf '%s\n' 'deployment unexpectedly survived a bootstrap failure' >&2
    exit 1
fi
[ "$(readlink "$state/install/current")" = "$running_release" ]
[ "$(readlink "$state/install/previous")" = "$second_release" ]
[ -f "$loaded" ]
[ ! -e "$state/spool/.maintenance" ]
[ -f "$state/spool/.paused" ]
[ ! -e "$state/install/.update-lock" ]
[ ! -e "$kickstart_order_error" ]
[ "$(shasum -a 256 "$state/config.toml" | awk '{print $1}')" = "$config_before_rejection" ]
[ "$(shasum -a 256 "$state/usage.toml" | awk '{print $1}')" = "$usage_config_before_rejection" ]
grep -Fx "codex = \"$codex\"" "$state/usage.toml" >/dev/null

: >"$fail_kickstart"
deploy >"$temporary/kickstart-warning.out" 2>"$temporary/kickstart-warning.err"
[ "$(readlink "$state/install/current")" != "$running_release" ]
[ "$(readlink "$state/install/previous")" = "$running_release" ]
[ -f "$loaded" ]
[ ! -e "$state/spool/.maintenance" ]
[ -f "$state/spool/.paused" ]
[ ! -e "$state/install/.update-lock" ]
[ ! -e "$kickstart_order_error" ]
grep -F 'warning: unable to wake the installed service' \
    "$temporary/kickstart-warning.err" >/dev/null

printf '%s\n' 'user deploy test passed'
