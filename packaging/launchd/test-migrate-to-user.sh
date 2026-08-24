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
    cat >"$tools_dir/codex" <<'EOF'
#!/bin/sh
exit 0
EOF
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
            system/*) [ -f '$fixture/system-loaded' ] ;;
            gui/*) [ -f '$fixture/user-loaded' ] ;;
        esac
        ;;
    disable|enable) ;;
    bootout)
        case "\$1" in
            system/*) rm -f '$fixture/system-loaded' ;;
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
while [ "\$#" -gt 0 ]; do
    case "\$1" in
        --home) home=\$2; shift 2 ;;
        --launchctl) launchctl=\$2; shift 2 ;;
        --usage-binary) usage_binary=\$2; shift 2 ;;
        --fresh-state) shift ;;
        --binary|--codex) shift 2 ;;
        *) exit 93 ;;
    esac
done
[ -x "\$usage_binary" ]
mkdir -p "\$home/Library/Application Support/Annals/install" \
    "\$home/Library/LaunchAgents" "\$home/.local/bin"
: >"\$home/Library/LaunchAgents/org.annals.inbox.plist"
ln -s "\$home/Library/Application Support/Annals/install/current" \
    "\$home/.local/bin/annals"
ln -s "\$home/Library/Application Support/Annals/install/current" \
    "\$home/.local/bin/annals-usage"
"\$launchctl" bootstrap "gui/\$(id -u)" \
    "\$home/Library/LaunchAgents/org.annals.inbox.plist"
[ '$fail_deploy' -eq 0 ] || exit 94
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
        "$fixture/home"
    cp "$fixture/tools/annals" "$prefix/usr/local/bin/annals"
    cp "$fixture/tools/annals" "$prefix/usr/local/libexec/annals/annals"
    cat >"$prefix/Library/LaunchDaemons/org.annals.inbox.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>org.annals.inbox</string>
<key>UserName</key><string>$operator</string>
<key>GroupName</key><string>$operator_group</string>
</dict></plist>
EOF
    cat >"$state/config.toml" <<EOF
library = "/Library/Application Support/Annals/annals.db"

[inbox]
root = "/Library/Application Support/Annals/spool"

[liaison]
quality = "high"
codex = "$fixture/tools/codex"
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
        --codex "$fixture/tools/codex" \
        --legacy-prefix "$fixture/legacy" \
        --launchctl "$fixture/tools/launchctl" \
        --dscl "$fixture/tools/dscl" \
        --operator-runner "$fixture/tools/operator-runner" \
        --deploy "$fixture/tools/deploy-user.sh"
}

success="$temporary/success"
mkdir -p "$success"
make_tools "$success"
make_deployer "$success" 0
make_fixture "$success"
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
grep -Fx wal "$new_state/annals.db-wal" >/dev/null
grep -Fx shm "$new_state/annals.db-shm" >/dev/null
grep -Fx auth "$new_state/codex-home/auth.json" >/dev/null
[ -f "$success/user-loaded" ]
[ -L "$success/home/.local/bin/annals-usage" ]
[ ! -e "$new_state/spool/.maintenance" ]
[ -f "$new_state/spool/.paused" ]

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
[ -f "$old_state/spool/.paused" ]
[ -f "$failure/system-loaded" ]
[ ! -e "$failure/user-loaded" ]
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
[ -f "$recovery/home/Library/Application Support/Annals/spool/.paused" ]
[ -f "$recovery/user-loaded" ]

printf '%s\n' 'migration fixture tests passed'
