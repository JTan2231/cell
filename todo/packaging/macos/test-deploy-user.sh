#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
temporary=$(mktemp -d "${TMPDIR:-/tmp}/todo-deploy-test.XXXXXX")
cleanup() {
    rm -rf "$temporary"
}
trap cleanup EXIT HUP INT TERM

package="$temporary/package"
package_share="$temporary/share/chancery"
home="$temporary/Operator Home"
candidate="$temporary/todo-candidate"
candidate_template="$temporary/todo-candidate.template"
launchctl="$temporary/launchctl"
launchctl_log="$temporary/launchctl.log"
launchctl_state="$temporary/launchctl.loaded"
launchctl_fail_bootstrap="$temporary/launchctl.fail-bootstrap"

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
' "$SCRIPT_DIR/../../crates/todo/Cargo.toml")
provider_version=$(awk -F '"' '/"release"[[:space:]]*:/ { print $4; exit }' \
    "$SCRIPT_DIR/../../chancery/provider.json")
[ -n "$package_version" ] && [ "$provider_version" = "$package_version" ] || {
    printf 'test: package version %s does not match provider release %s\n' \
        "$package_version" "$provider_version" >&2
    exit 1
}
mismatch_version="$package_version-provider-mismatch"

mkdir -p "$package" "$package_share" "$home"
cp "$SCRIPT_DIR/deploy-user.sh" "$package/deploy-user.sh"
cp "$SCRIPT_DIR/todo" "$package/todo"
cp "$SCRIPT_DIR/todo-daily-email" "$package/todo-daily-email"
cp "$SCRIPT_DIR/org.todo.daily-email.plist" "$package/org.todo.daily-email.plist"
cp -R "$SCRIPT_DIR/../../chancery" "$package_share/todo"
chmod 0755 "$package/deploy-user.sh" "$package/todo" "$package/todo-daily-email"

cat >"$candidate_template" <<'EOF'
#!/bin/sh
set -eu
config=
json=0
while [ "$#" -gt 0 ]; do
    case "$1" in
        --version)
            printf '%s\n' 'todo __TODO_VERSION__'
            exit 0
            ;;
        --config)
            config=$2
            shift 2
            ;;
        --config=*)
            config=${1#*=}
            shift
            ;;
        --json)
            json=1
            shift
            ;;
        --quiet|-v|--verbose)
            shift
            ;;
        *) break ;;
    esac
done
[ -n "$config" ] || config=${TODO_CONFIG:?}
state=$(CDPATH='' cd "$(dirname "$config")" && pwd)
command=${1:?}
shift
printf '%s\n' "$command" >>"$state/commands.log"
case "$command" in
    init)
        printf '%s\n' 'v2' >"$state/todo.db"
        ;;
    migrate)
        [ "${1:-}" = --backup ]
        backup=${2:?}
        case "$(cat "$state/todo.db")" in
            v1)
                [ ! -e "$backup" ]
                cp "$state/todo.db" "$backup"
                printf '%s\n' 'v2' >"$state/todo.db"
                ;;
            v2) ;;
            *) exit 1 ;;
        esac
        ;;
    list)
        [ -f "$state/todo.db" ]
        [ ! -f "$state/fail-list-after-migrate" ]
        [ "$json" -eq 1 ]
        [ "${1:-}" = --limit ]
        [ "${2:-}" = 1 ]
        printf '%s\n' '{"ok":true,"data":[]}'
        ;;
    email)
        [ "${1:-}" = send ]
        [ "${2:-}" = --scheduled ]
        env | sort >"$state/email-environment.log"
        printf '%s\n' "$@" >"$state/email-arguments.log"
        ;;
    *) exit 1 ;;
esac
EOF
sed "s/__TODO_VERSION__/$package_version/g" \
    "$candidate_template" >"$candidate"
chmod 0755 "$candidate"

mkdir -p "$home/.local/bin"
cat >"$home/.local/bin/nucleus" <<'EOF'
#!/bin/sh
[ "${1:-}" = --compact ] || exit 1
[ "${2:-}" = health ] || exit 1
printf '%s\n' '{"version":1,"status":"ok","daemonVersion":"test","acceptingJobs":true,"checkedAt":"2026-08-27T00:00:00Z","supportedProtocolVersions":[1],"harness":{"harness":"codex","harnessVersion":"0.146.0","adapterVersion":"test"},"capabilities":["exact-model","reasoning-effort","workspace-read-only","builtin-local-execution","builtin-web-search","dynamic-client-tools","developer-instructions","experimental-raw-events","persistent-file-authentication"],"authentication":{"codexHome":"/tmp/codex-home","configured":true,"authenticated":true}}'
EOF
chmod 0755 "$home/.local/bin/nucleus"

uid=$(id -u)
cat >"$launchctl" <<EOF
#!/bin/sh
set -eu
printf '%s\n' "\$*" >>"$launchctl_log"
case "\${1:-}" in
    print)
        [ "\${2:-}" = "gui/$uid/org.todo.daily-email" ]
        [ -f "$launchctl_state" ]
        ;;
    bootout)
        [ "\${2:-}" = "gui/$uid/org.todo.daily-email" ]
        rm -f "$launchctl_state"
        ;;
    bootstrap)
        [ "\${2:-}" = "gui/$uid" ]
        [ "\${3:-}" = "$home/Library/LaunchAgents/org.todo.daily-email.plist" ]
        plutil -lint "\$3" >/dev/null
        if [ -f "$launchctl_fail_bootstrap" ]; then
            rm -f "$launchctl_fail_bootstrap"
            exit 1
        fi
        : >"$launchctl_state"
        ;;
    *) exit 1 ;;
esac
EOF
chmod 0755 "$launchctl"

deploy() {
    selected_candidate=$1
    shift
    HOME="$home" "$package/deploy-user.sh" \
        --binary "$selected_candidate" \
        --home "$home" \
        --launchctl "$launchctl" \
        "$@"
}

if deploy "$candidate" >"$temporary/missing-email.out" \
    2>"$temporary/missing-email.err"
then
    printf '%s\n' 'fresh deployment unexpectedly accepted missing email config' >&2
    exit 1
fi
[ ! -e "$launchctl_state" ]

deploy "$candidate" \
    --email-to j.tan2231@gmail.com \
    --email-from 'Todo <todo@joeytan.dev>' >/dev/null
state="$home/Library/Application Support/Todo"
cli="$home/.local/bin/todo"
agent_plist="$home/Library/LaunchAgents/org.todo.daily-email.plist"
chancery_providers="$home/Library/Application Support/Chancery/providers"
todo_provider="$chancery_providers/todo"
[ -L "$cli" ]
[ -L "$state/install/current" ]
[ ! -e "$state/install/previous" ]
[ -x "$state/install/current/bin/todo" ]
[ -x "$state/install/current/bin/todo-daily-email" ]
[ -x "$state/install/current/libexec/todo" ]
[ -x "$state/install/current/package/todo" ]
[ -x "$state/install/current/package/todo-daily-email" ]
[ -f "$state/install/current/package/deploy-user.sh" ]
[ -f "$state/install/current/package/org.todo.daily-email.plist" ]
[ -f "$state/install/current/manifest.txt" ]
[ -f "$state/install/current/share/chancery/todo/provider.json" ]
[ -L "$todo_provider" ]
[ "$(readlink "$todo_provider")" = \
    "$state/install/current/share/chancery/todo" ]
[ -f "$todo_provider/provider.json" ]
[ -f "$state/todo.db" ]
[ -f "$agent_plist" ]
[ -f "$launchctl_state" ]
[ -d "$home/Library/Logs/Todo" ]
grep -Fx 'database = "todo.db"' "$state/config.toml" >/dev/null
grep -Fx '[liaison]' "$state/config.toml" >/dev/null
! grep -F 'codex =' "$state/config.toml" >/dev/null
grep -Fx 'quality = "high"' "$state/config.toml" >/dev/null
grep -Fx '[email]' "$state/config.toml" >/dev/null
grep -Fx 'to = "j.tan2231@gmail.com"' "$state/config.toml" >/dev/null
grep -Fx 'from = "Todo <todo@joeytan.dev>"' "$state/config.toml" >/dev/null
plutil -lint "$agent_plist" >/dev/null
[ "$(plutil -extract Label raw "$agent_plist")" = org.todo.daily-email ]
[ "$(plutil -extract ProgramArguments.0 raw "$agent_plist")" = /bin/zsh ]
[ "$(plutil -extract ProgramArguments.1 raw "$agent_plist")" = \
    "$state/install/current/bin/todo-daily-email" ]
[ "$(plutil -extract ProgramArguments raw "$agent_plist")" -eq 2 ]
! grep -F '__TODO_' "$agent_plist" >/dev/null
[ "$(plutil -extract StartCalendarInterval.Hour raw "$agent_plist")" -eq 9 ]
[ "$(plutil -extract StartCalendarInterval.Minute raw "$agent_plist")" -eq 0 ]
[ "$(plutil -extract EnvironmentVariables.HOME raw "$agent_plist")" = "$home" ]
! plutil -extract RunAtLoad raw "$agent_plist" >/dev/null 2>&1
! grep -F 'RESEND_API_KEY' "$agent_plist" >/dev/null
! grep -F 'scheduled_at' "$agent_plist" >/dev/null
[ "$(tail -n 2 "$state/commands.log" | tr '\n' ' ')" = 'init list ' ]
HOME="$home" "$cli" --json list --limit 1 >/dev/null

mismatched_candidate="$temporary/todo-mismatched-provider"
sed "s/__TODO_VERSION__/$mismatch_version/g" \
    "$candidate_template" >"$mismatched_candidate"
chmod 0755 "$mismatched_candidate"
if deploy "$mismatched_candidate" >"$temporary/provider-mismatch.out" \
    2>"$temporary/provider-mismatch.err"
then
    printf '%s\n' 'deployment unexpectedly accepted a provider/candidate mismatch' >&2
    exit 1
fi
grep -F "provider release $provider_version does not match candidate $mismatch_version" \
    "$temporary/provider-mismatch.err" >/dev/null

cat >"$home/.zshrc" <<'EOF'
setopt XTRACE
export RESEND_API_KEY='resend-test-secret'
export OTHER_CREDENTIAL='must-not-leak'
EOF
HOME="$home" "$state/install/current/bin/todo-daily-email" \
    >"$temporary/email-runner.out" 2>"$temporary/email-runner.err"
grep -Fx 'RESEND_API_KEY=resend-test-secret' "$state/email-environment.log" >/dev/null
grep -Fx "HOME=$home" "$state/email-environment.log" >/dev/null
grep -Fx 'PATH=/usr/bin:/bin:/usr/sbin:/sbin' "$state/email-environment.log" >/dev/null
grep -Fx "TODO_CONFIG=$state/config.toml" "$state/email-environment.log" >/dev/null
! grep -F 'OTHER_CREDENTIAL=' "$state/email-environment.log" >/dev/null
[ "$(tr '\n' ' ' <"$state/email-arguments.log")" = 'send --scheduled ' ]
! grep -F 'resend-test-secret' "$agent_plist" "$launchctl_log" \
    "$state/email-arguments.log" "$temporary/email-runner.out" \
    "$temporary/email-runner.err" >/dev/null

first_release=$(readlink "$state/install/current")
ln -s /preserved/provider "$chancery_providers/preserved"
printf '%s\n' '# preserve-email-config-on-update' >>"$state/config.toml"
HOME="$home" "$state/install/current/package/deploy-user.sh" \
    --binary "$state/install/current/libexec/todo" \
    --home "$home" \
    --launchctl "$launchctl" >/dev/null
[ "$(readlink "$state/install/current")" = "$first_release" ]
[ ! -e "$state/install/previous" ]
[ "$(readlink "$chancery_providers/preserved")" = /preserved/provider ]
[ "$(grep -c '^init$' "$state/commands.log")" -eq 1 ]
grep -Fx '# preserve-email-config-on-update' "$state/config.toml" >/dev/null
[ -f "$launchctl_state" ]

printf '%s\n' '# second release' >>"$candidate"
deploy "$candidate" >/dev/null
second_release=$(readlink "$state/install/current")
[ "$second_release" != "$first_release" ]
[ "$(readlink "$state/install/previous")" = "$first_release" ]
[ "$(readlink "$todo_provider")" = \
    "$state/install/current/share/chancery/todo" ]
[ -f "$todo_provider/provider.json" ]
[ "$(readlink "$chancery_providers/preserved")" = /preserved/provider ]
[ "$(grep -c '^init$' "$state/commands.log")" -eq 1 ]
grep -Fx '# preserve-email-config-on-update' "$state/config.toml" >/dev/null
[ -f "$launchctl_state" ]

nucleus_cli="$home/.local/bin/nucleus"
cp "$nucleus_cli" "$temporary/nucleus-healthy"
cat >"$nucleus_cli" <<'EOF'
#!/bin/sh
printf '%s\n' '{"version":1,"status":"degraded","acceptingJobs":false,"supportedProtocolVersions":[1],"authentication":{"authenticated":false}}'
EOF
chmod 0755 "$nucleus_cli"
if deploy "$candidate" >"$temporary/degraded.out" 2>"$temporary/degraded.err"; then
    printf '%s\n' 'deployment unexpectedly accepted a degraded Nucleus service' >&2
    exit 1
fi
[ "$(readlink "$state/install/current")" = "$second_release" ]
[ "$(readlink "$state/install/previous")" = "$first_release" ]
[ -f "$todo_provider/provider.json" ]
[ -f "$launchctl_state" ]
install -m 0755 "$temporary/nucleus-healthy" "$nucleus_cli"

printf '%s\n' '# tampered payload' >>"$state/install/current/libexec/todo"
if deploy "$candidate" >"$temporary/tampered.out" 2>"$temporary/tampered.err"; then
    printf '%s\n' 'deployment unexpectedly accepted a tampered release' >&2
    exit 1
fi
[ "$(readlink "$state/install/current")" = "$second_release" ]
[ "$(readlink "$state/install/previous")" = "$first_release" ]
[ -f "$todo_provider/provider.json" ]
[ ! -e "$state/install/.update-lock" ]
[ -f "$launchctl_state" ]
install -m 0755 "$candidate" "$state/install/current/libexec/todo"

printf '%s\n' '<!-- rollback-sentinel -->' >>"$agent_plist"
plutil -lint "$agent_plist" >/dev/null
: >"$launchctl_fail_bootstrap"
if deploy "$candidate" >"$temporary/bootstrap.out" 2>"$temporary/bootstrap.err"; then
    printf '%s\n' 'deployment unexpectedly accepted a failed service bootstrap' >&2
    exit 1
fi
[ "$(readlink "$state/install/current")" = "$second_release" ]
[ "$(readlink "$state/install/previous")" = "$first_release" ]
[ -f "$todo_provider/provider.json" ]
[ -f "$launchctl_state" ]
grep -Fx '<!-- rollback-sentinel -->' "$agent_plist" >/dev/null

printf '%s\n' 'v1' >"$state/todo.db"
: >"$state/fail-list-after-migrate"
if deploy "$candidate" >"$temporary/migration-rollback.out" \
    2>"$temporary/migration-rollback.err"
then
    printf '%s\n' 'deployment unexpectedly kept a failed database migration' >&2
    exit 1
fi
[ "$(cat "$state/todo.db")" = v1 ]
[ -f "$launchctl_state" ]
rm -f "$state/fail-list-after-migrate"
deploy "$candidate" >/dev/null
[ "$(cat "$state/todo.db")" = v2 ]
[ -f "$launchctl_state" ]

printf '%s\n' '<!-- rollback-sentinel -->' >>"$agent_plist"
plutil -lint "$agent_plist" >/dev/null
failed="$temporary/todo-failed-candidate"
failed_template="$temporary/todo-failed-candidate.template"
cat >"$failed_template" <<'EOF'
#!/bin/sh
set -eu
for argument in "$@"; do
    [ "$argument" != --version ] || {
        printf '%s\n' 'todo __TODO_VERSION__'
        exit 0
    }
done
case " $* " in
    *' init '*) exit 0 ;;
    *) exit 1 ;;
esac
EOF
sed "s/__TODO_VERSION__/$package_version/g" \
    "$failed_template" >"$failed"
chmod 0755 "$failed"
if deploy "$failed" >"$temporary/failed.out" 2>"$temporary/failed.err"; then
    printf '%s\n' 'deployment unexpectedly accepted a failed smoke test' >&2
    exit 1
fi
[ "$(readlink "$state/install/current")" = "$second_release" ]
[ "$(readlink "$state/install/previous")" = "$first_release" ]
[ ! -e "$state/install/.update-lock" ]
[ -f "$launchctl_state" ]
grep -Fx '<!-- rollback-sentinel -->' "$agent_plist" >/dev/null

rm -f "$todo_provider"
ln -s /foreign/todo-provider "$todo_provider"
if deploy "$candidate" >"$temporary/foreign-provider.out" \
    2>"$temporary/foreign-provider.err"
then
    printf '%s\n' 'deployment unexpectedly took over a foreign provider selector' >&2
    exit 1
fi
[ "$(readlink "$todo_provider")" = /foreign/todo-provider ]
rm -f "$todo_provider"
ln -s "$state/install/current/share/chancery/todo" "$todo_provider"

printf '%s\n' 'deploy test passed'
