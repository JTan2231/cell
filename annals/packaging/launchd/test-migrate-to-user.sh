#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
MIGRATOR="$SCRIPT_DIR/migrate-to-user.sh"
temporary=$(mktemp -d "${TMPDIR:-/tmp}/annals-migration-test.XXXXXX")

cleanup() {
    rm -rf "$temporary"
}
trap cleanup EXIT HUP INT TERM

operator=$(id -un)
operator_group=$(id -gn)

make_tools() {
    fixture=$1
    tools_dir="$fixture/tools"
    mkdir -p "$tools_dir"

    cat >"$tools_dir/annals" <<'EOF'
#!/bin/sh
case " $* " in
    *' --json inbox status '*) printf '%s\n' '{"ok":true,"data":{"locked":false}}' ;;
    *' --version '*) printf '%s\n' 'annals test' ;;
    *) exit 0 ;;
esac
EOF
    cat >"$tools_dir/annals-usage" <<'EOF'
#!/bin/sh
case " $* " in
    *' --version '*) printf '%s\n' 'annals-usage test' ;;
    *) exit 0 ;;
esac
EOF
    cat >"$tools_dir/nucleus" <<'EOF'
#!/bin/sh
exit 0
EOF
    cat >"$tools_dir/clockwork" <<'EOF'
#!/bin/sh
set -eu
fixture=$(CDPATH='' cd "$(dirname "$0")/.." && pwd)
transaction="$fixture/legacy/Library/Application Support/Annals.migrate-to-user"
target="$fixture/home/Library/Application Support/Annals"
digest=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa

[ "${1:-}" = --json ] && shift
case "${1:-}:${2:-}" in
    definition:register)
        [ -f "${3:-}" ] || exit 95
        if [ "$(sed -n '1p' "$transaction/phase" 2>/dev/null || true)" != committed ]; then
            : >"$fixture/clockwork-before-commit"
            exit 96
        fi
        printf '%s\n' "{\"ok\":true,\"data\":{\"digest\":\"$digest\"}}"
        ;;
    binding:disable)
        rm -f "$fixture/clockwork-enabled"
        printf '%s\n' '{"ok":true,"data":{"key":"annals/inbox","definition_digest":null,"enabled":false,"updated_at":1}}'
        ;;
    binding:show)
        if [ -f "$fixture/clockwork-disabled-foreign" ]; then
            printf '%s\n' '{"ok":true,"data":{"key":"annals/inbox","definition_digest":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","enabled":false,"updated_at":1}}'
            exit 0
        elif [ -f "$fixture/clockwork-disabled-null" ]; then
            printf '%s\n' '{"ok":true,"data":{"key":"annals/inbox","definition_digest":null,"enabled":false,"updated_at":1}}'
            exit 0
        elif [ -f "$fixture/clockwork-enabled" ]; then
            printf '%s\n' "{\"ok\":true,\"data\":{\"key\":\"annals/inbox\",\"definition_digest\":\"$digest\",\"enabled\":true,\"updated_at\":1}}"
            exit 0
        fi
        printf '%s\n' '{"ok":false,"error":{"code":"binding_not_found","message":"absent"}}' >&2
        exit 1
        ;;
    binding:switch)
        [ "${4:-}" = "$digest" ] || exit 97
        if [ "$(sed -n '1p' "$transaction/phase" 2>/dev/null || true)" != committed ]; then
            : >"$fixture/clockwork-before-commit"
            exit 98
        fi
        [ -f "$target/spool/.maintenance" ] || {
            : >"$fixture/clockwork-without-maintenance"
            exit 99
        }
        rm -f "$fixture/clockwork-disabled-null"
        : >"$fixture/clockwork-enabled"
        printf '%s\n' "{\"ok\":true,\"data\":{\"key\":\"annals/inbox\",\"definition_digest\":\"$digest\",\"enabled\":true,\"updated_at\":1}}"
        ;;
    *) exit 1 ;;
esac
EOF
    : >"$tools_dir/nucleus.sock"
    cat >"$tools_dir/operator-runner" <<'EOF'
#!/bin/sh
[ "$1" = -u ] || exit 91
shift 2
exec "$@"
EOF
    cat >"$tools_dir/dscl" <<EOF
#!/bin/sh
printf '%s\n' 'NFSHomeDirectory: $fixture/home'
EOF
    cat >"$tools_dir/launchctl" <<EOF
#!/bin/sh
set -eu
printf '%s\n' "\$*" >>'$fixture/launchctl.log'
command=\$1
shift
case "\$command" in
    print)
        case "\$1" in
            system/*)
                if [ -f '$fixture/system-loaded' ]; then
                    exit 0
                fi
                printf '%s\n' 'Could not find service' >&2
                exit 1
                ;;
            gui/*)
                if [ -f '$fixture/user-loaded' ]; then
                    exit 0
                fi
                printf '%s\n' 'Could not find service' >&2
                exit 1
                ;;
        esac
        ;;
    disable|enable) ;;
    bootout)
        case "\$1" in
            system/*)
                [ ! -f '$fixture/fail-system-bootout' ] || exit 93
                [ -f '$fixture/keep-system-loaded-after-bootout' ] \
                    || rm -f '$fixture/system-loaded'
                ;;
            gui/*) rm -f '$fixture/user-loaded' ;;
        esac
        ;;
    bootstrap)
        case "\$1" in
            system) : >'$fixture/system-loaded' ;;
            gui/*) : >'$fixture/user-loaded' ;;
        esac
        ;;
    kickstart) ;;
    *) exit 92 ;;
esac
EOF
    chmod 0755 "$tools_dir"/*
}

make_deployer() {
    fixture=$1
    fail_deploy=$2
    deploy="$fixture/tools/deploy-user.sh"
    cat >"$deploy" <<EOF
#!/bin/sh
set -eu
home=
launchctl=
usage_binary=
clockwork=
migration_clockwork_handoff=0
while [ "\$#" -gt 0 ]; do
    case "\$1" in
        --home) home=\$2; shift 2 ;;
        --launchctl) launchctl=\$2; shift 2 ;;
        --usage-binary) usage_binary=\$2; shift 2 ;;
        --clockwork) clockwork=\$2; shift 2 ;;
        --fresh-state) shift ;;
        --migration-clockwork-handoff) migration_clockwork_handoff=1; shift ;;
        --binary|--nucleus|--nucleus-socket) shift 2 ;;
        *) exit 93 ;;
    esac
done
[ -x "\$usage_binary" ]
[ -x "\$clockwork" ]
[ "\$migration_clockwork_handoff" -eq 1 ]
mkdir -p "\$home/Library/Application Support/Annals/install" \
    "\$home/.local/bin"
ln -s "\$home/Library/Application Support/Annals/install/current" \
    "\$home/.local/bin/annals"
ln -s "\$home/Library/Application Support/Annals/install/current" \
    "\$home/.local/bin/annals-usage"
[ '$fail_deploy' -eq 0 ] || exit 94
printf '%s\n' 'fixture definition' \
    >"\$home/Library/Application Support/Annals/install/.migration-annals-inbox.clockwork.toml"
cat >"\$home/Library/Application Support/Annals/install/last-update.json" <<'RECEIPT'
{
  "clockwork_definition": null,
  "completed_at": "fixture"
}
RECEIPT
EOF
    chmod 0755 "$deploy"
}

make_fixture() {
    fixture=$1
    prefix="$fixture/legacy"
    state="$prefix/Library/Application Support/Annals"
    mkdir -p "$prefix/usr/local/bin" "$prefix/usr/local/libexec/annals" \
        "$prefix/Library/LaunchDaemons" "$state/codex-home" \
        "$state/log" "$state/spool/incoming" "$state/spool/processing" \
        "$state/spool/queued/waiting/material" \
        "$state/spool/processing/retry/material" \
        "$state/spool/done/job/material" \
        "$state/spool/duplicates/repeated/material" \
        "$state/spool/failed/rejected/material" \
        "$state/spool/skipped/operator-skipped/material" \
        "$fixture/home"
    cp "$fixture/tools/annals" "$prefix/usr/local/bin/annals"
    cp "$fixture/tools/annals" "$prefix/usr/local/libexec/annals/annals"
    daemon_plist="$prefix/Library/LaunchDaemons/org.annals.inbox.plist"
    cp "$SCRIPT_DIR/org.annals.inbox.plist" "$daemon_plist"
    plutil -replace UserName -string "$operator" "$daemon_plist"
    plutil -replace GroupName -string "$operator_group" "$daemon_plist"
    chmod 0644 "$daemon_plist"
    cat >"$state/config.toml" <<EOF
library = "/Library/Application Support/Annals/annals.db"

[inbox]
root = "/Library/Application Support/Annals/spool"

[liaison]
quality = "high"
codex = "$fixture/tools/nucleus"
EOF
    printf '%s\n' database >"$state/annals.db"
    printf '%s\n' wal >"$state/annals.db-wal"
    printf '%s\n' shm >"$state/annals.db-shm"
    printf '%s\n' auth >"$state/codex-home/auth.json"
    printf '%s\n' config >"$state/codex-home/config.toml"
    printf '%s\n' waiting >"$state/spool/queued/waiting/material/waiting.txt"
    printf '%s\n' material >"$state/spool/done/job/material/source.txt"
    printf '%s\n' repeated >"$state/spool/duplicates/repeated/material/repeated.txt"
    printf '%s\n' retry >"$state/spool/processing/retry/material/retry.txt"
    printf '%s\n' rejected >"$state/spool/failed/rejected/material/rejected.txt"
    printf '%s\n' skipped >"$state/spool/skipped/operator-skipped/material/skipped.txt"
    printf '%s\n' '{"version":1,"next_sequence":4,"entries":{}}' >"$state/spool/.queue.json"
    : >"$state/spool/.paused"
    : >"$state/log/inbox.stdout.log"
    : >"$state/log/inbox.stderr.log"
    : >"$fixture/system-loaded"
}

run_migration() {
    fixture=$1
    ANNALS_MIGRATION_TEST_CRASH_AFTER_MOVE=${ANNALS_MIGRATION_TEST_CRASH_AFTER_MOVE:-0} \
        "$MIGRATOR" \
        --binary "$fixture/tools/annals" \
        --usage-binary "$fixture/tools/annals-usage" \
        --nucleus "$fixture/tools/nucleus" \
        --nucleus-socket "$fixture/tools/nucleus.sock" \
        --clockwork "$fixture/tools/clockwork" \
        --legacy-prefix "$fixture/legacy" \
        --launchctl "$fixture/tools/launchctl" \
        --dscl "$fixture/tools/dscl" \
        --operator-runner "$fixture/tools/operator-runner" \
        --deploy "$fixture/tools/deploy-user.sh"
}

foreign_plist="$temporary/foreign-plist"
mkdir -p "$foreign_plist"
make_tools "$foreign_plist"
make_deployer "$foreign_plist" 0
make_fixture "$foreign_plist"
plutil -insert KeepAlive -bool true \
    "$foreign_plist/legacy/Library/LaunchDaemons/org.annals.inbox.plist"
if run_migration "$foreign_plist" >/dev/null 2>&1; then
    printf '%s\n' 'migration unexpectedly removed a LaunchDaemon with extra behavior' >&2
    exit 1
fi
[ -f "$foreign_plist/legacy/Library/LaunchDaemons/org.annals.inbox.plist" ]
[ -d "$foreign_plist/legacy/Library/Application Support/Annals" ]
[ -f "$foreign_plist/system-loaded" ]
[ ! -e "$foreign_plist/home/Library/Application Support/Annals" ]

foreign_binding="$temporary/foreign-binding"
mkdir -p "$foreign_binding"
make_tools "$foreign_binding"
make_deployer "$foreign_binding" 0
make_fixture "$foreign_binding"
: >"$foreign_binding/clockwork-disabled-foreign"
if run_migration "$foreign_binding" >/dev/null 2>&1; then
    printf '%s\n' 'migration unexpectedly replaced a disabled foreign Clockwork definition' >&2
    exit 1
fi
[ -f "$foreign_binding/clockwork-disabled-foreign" ]
[ -f "$foreign_binding/system-loaded" ]
[ -f "$foreign_binding/legacy/Library/LaunchDaemons/org.annals.inbox.plist" ]
[ -d "$foreign_binding/legacy/Library/Application Support/Annals" ]
[ ! -e "$foreign_binding/home/Library/Application Support/Annals" ]

success="$temporary/success"
mkdir -p "$success"
make_tools "$success"
make_deployer "$success" 0
make_fixture "$success"
: >"$success/clockwork-disabled-null"
run_migration "$success" >/dev/null
new_state="$success/home/Library/Application Support/Annals"
[ -d "$new_state" ]
[ ! -e "$success/legacy/Library/Application Support/Annals" ]
[ ! -e "$success/legacy/Library/LaunchDaemons/org.annals.inbox.plist" ]
[ ! -e "$success/legacy/usr/local/bin/annals" ]
grep -Fx 'library = "annals.db"' "$new_state/config.toml" >/dev/null
grep -Fx 'root = "spool"' "$new_state/config.toml" >/dev/null
grep -Fx waiting "$new_state/spool/queued/waiting/material/waiting.txt" >/dev/null
grep -Fx material "$new_state/spool/done/job/material/source.txt" >/dev/null
grep -Fx repeated \
    "$new_state/spool/duplicates/repeated/material/repeated.txt" >/dev/null
grep -Fx retry "$new_state/spool/processing/retry/material/retry.txt" >/dev/null
grep -Fx rejected "$new_state/spool/failed/rejected/material/rejected.txt" >/dev/null
grep -Fx skipped \
    "$new_state/spool/skipped/operator-skipped/material/skipped.txt" >/dev/null
grep -Fx wal "$new_state/annals.db-wal" >/dev/null
grep -Fx shm "$new_state/annals.db-shm" >/dev/null
grep -Fx auth "$new_state/codex-home/auth.json" >/dev/null
[ -f "$success/clockwork-enabled" ]
[ ! -e "$success/clockwork-disabled-null" ]
[ ! -e "$success/clockwork-before-commit" ]
[ ! -e "$success/clockwork-without-maintenance" ]
[ ! -e "$success/user-loaded" ]
[ ! -e "$success/home/Library/LaunchAgents/org.annals.inbox.plist" ]
[ -L "$success/home/.local/bin/annals-usage" ]
[ ! -e "$new_state/spool/.maintenance" ]
[ ! -e "$new_state/install/.migration-annals-inbox.clockwork.toml" ]
grep -F '"clockwork_definition": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"' \
    "$new_state/install/last-update.json" >/dev/null
[ -f "$new_state/spool/.paused" ]

retire_failure="$temporary/retire-failure"
mkdir -p "$retire_failure"
make_tools "$retire_failure"
make_deployer "$retire_failure" 0
make_fixture "$retire_failure"
: >"$retire_failure/fail-system-bootout"
if run_migration "$retire_failure" >/dev/null 2>&1; then
    printf '%s\n' 'migration unexpectedly retired a still-loaded system service' >&2
    exit 1
fi
retained_state="$retire_failure/home/Library/Application Support/Annals"
retained_transaction="$retire_failure/legacy/Library/Application Support/Annals.migrate-to-user"
[ -d "$retained_state" ]
[ -d "$retained_transaction" ]
[ "$(sed -n '1p' "$retained_transaction/phase")" = committed ]
[ -f "$retained_state/spool/.maintenance" ]
[ -f "$retained_state/install/.migration-annals-inbox.clockwork.toml" ]
[ -f "$retire_failure/clockwork-enabled" ]
[ -f "$retire_failure/system-loaded" ]
[ -f "$retire_failure/legacy/Library/LaunchDaemons/org.annals.inbox.plist" ]
[ -f "$retire_failure/legacy/usr/local/bin/annals" ]
rm -f "$retire_failure/fail-system-bootout"
: >"$retire_failure/keep-system-loaded-after-bootout"
if run_migration "$retire_failure" >/dev/null 2>&1; then
    printf '%s\n' 'migration unexpectedly accepted a still-visible system service' >&2
    exit 1
fi
[ -d "$retained_transaction" ]
[ -f "$retained_state/spool/.maintenance" ]
[ -f "$retire_failure/system-loaded" ]
[ -f "$retire_failure/legacy/Library/LaunchDaemons/org.annals.inbox.plist" ]
rm -f "$retire_failure/keep-system-loaded-after-bootout"
run_migration "$retire_failure" >/dev/null
[ ! -e "$retained_transaction" ]
[ ! -e "$retained_state/spool/.maintenance" ]
[ ! -e "$retained_state/install/.migration-annals-inbox.clockwork.toml" ]
[ ! -e "$retire_failure/system-loaded" ]
[ ! -e "$retire_failure/legacy/Library/LaunchDaemons/org.annals.inbox.plist" ]
[ ! -e "$retire_failure/legacy/usr/local/bin/annals" ]

failure="$temporary/failure"
mkdir -p "$failure"
make_tools "$failure"
make_deployer "$failure" 1
make_fixture "$failure"
if run_migration "$failure" >/dev/null 2>&1; then
    printf '%s\n' 'migration unexpectedly succeeded with a failing deployer' >&2
    exit 1
fi
old_state="$failure/legacy/Library/Application Support/Annals"
[ -d "$old_state" ]
[ ! -e "$failure/home/Library/Application Support/Annals" ]
grep -Fx 'library = "/Library/Application Support/Annals/annals.db"' \
    "$old_state/config.toml" >/dev/null
grep -Fx 'root = "/Library/Application Support/Annals/spool"' \
    "$old_state/config.toml" >/dev/null
grep -Fx repeated \
    "$old_state/spool/duplicates/repeated/material/repeated.txt" >/dev/null
grep -Fx skipped \
    "$old_state/spool/skipped/operator-skipped/material/skipped.txt" >/dev/null
[ -f "$old_state/spool/.paused" ]
[ -f "$failure/system-loaded" ]
[ ! -e "$failure/user-loaded" ]
[ ! -e "$failure/clockwork-enabled" ]
[ ! -e "$failure/clockwork-before-commit" ]
[ ! -e "$failure/home/Library/LaunchAgents/org.annals.inbox.plist" ]
[ ! -e "$failure/home/.local/bin/annals" ]
[ ! -e "$failure/home/.local/bin/annals-usage" ]
[ ! -e "$old_state.migrate-to-user" ]

recovery="$temporary/recovery"
mkdir -p "$recovery"
make_tools "$recovery"
make_deployer "$recovery" 0
make_fixture "$recovery"
set +e
ANNALS_MIGRATION_TEST_CRASH_AFTER_MOVE=1 \
    run_migration "$recovery" >/dev/null 2>&1
crash_status=$?
set -e
unset ANNALS_MIGRATION_TEST_CRASH_AFTER_MOVE
if [ "$crash_status" -eq 0 ]; then
    printf '%s\n' 'migration unexpectedly survived its crash fixture' >&2
    exit 1
fi
[ -d "$recovery/home/Library/Application Support/Annals" ]
[ -d "$recovery/legacy/Library/Application Support/Annals.migrate-to-user" ]
run_migration "$recovery" >/dev/null
[ -d "$recovery/home/Library/Application Support/Annals" ]
[ ! -e "$recovery/legacy/Library/Application Support/Annals" ]
[ ! -e "$recovery/legacy/Library/Application Support/Annals.migrate-to-user" ]
[ -L "$recovery/home/.local/bin/annals-usage" ]
grep -Fx repeated \
    "$recovery/home/Library/Application Support/Annals/spool/duplicates/repeated/material/repeated.txt" \
    >/dev/null
grep -Fx skipped \
    "$recovery/home/Library/Application Support/Annals/spool/skipped/operator-skipped/material/skipped.txt" \
    >/dev/null
[ -f "$recovery/home/Library/Application Support/Annals/spool/.paused" ]
[ -f "$recovery/clockwork-enabled" ]
[ ! -e "$recovery/clockwork-before-commit" ]
[ ! -e "$recovery/clockwork-without-maintenance" ]
[ ! -e "$recovery/user-loaded" ]

printf '%s\n' 'migration fixture tests passed'
