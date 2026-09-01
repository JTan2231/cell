#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
temporary=$(mktemp -d "${TMPDIR:-/tmp}/decisions-deploy.XXXXXX")
holder=
cleanup() {
    if [ -n "$holder" ]; then
        kill "$holder" >/dev/null 2>&1 || true
        wait "$holder" >/dev/null 2>&1 || true
    fi
    rm -rf "$temporary"
}
trap cleanup EXIT HUP INT TERM
home="$temporary/Home"
package="$temporary/package/macos"
share="$temporary/package/share/chancery"
candidate="$temporary/decisions"
launchctl="$temporary/launchctl"
log="$temporary/launchctl.log"
fail_bootstrap="$temporary/fail-bootstrap"
concurrent_hook="$temporary/concurrent-hook"
concurrent_hook_started="$temporary/concurrent-hook-started"
loaded="$temporary/loaded"
baseline="$home/observer-baseline"
mkdir -p "$home/.local/bin" "$package" "$share"
cp "$SCRIPT_DIR/decisions" "$SCRIPT_DIR/decisions-daily-email" "$SCRIPT_DIR/decisions-observer" \
    "$SCRIPT_DIR/deploy-user.sh" "$SCRIPT_DIR/uninstall-user.sh" \
    "$SCRIPT_DIR/org.decisions.daily-email.plist" "$SCRIPT_DIR/org.decisions.observer.plist" \
    "$SCRIPT_DIR/hooks.json" "$package/"
cp -R "$SCRIPT_DIR/../../chancery" "$share/decisions"
chmod 0755 "$package/decisions" "$package/decisions-daily-email" "$package/decisions-observer" \
    "$package/deploy-user.sh" "$package/uninstall-user.sh"
hook_plist="$temporary/hooks.plist"
plutil -convert xml1 -o "$hook_plist" -- "$package/hooks.json"
[ "$(plutil -extract hooks.Stop.0.hooks.0.type raw "$hook_plist")" = command ]
[ "$(plutil -extract hooks.Stop.0.hooks.0.command raw "$hook_plist")" = '"$HOME/.local/bin/decisions" observe ingest' ]
[ "$(plutil -extract hooks.Stop.0.hooks.0.timeout raw "$hook_plist")" -eq 3 ]
! plutil -extract hooks.Stop.0.hooks.0.async raw "$hook_plist" >/dev/null 2>&1
cat >"$candidate" <<'EOF'
#!/bin/sh
if [ "${1:-}" = --version ]; then printf '%s\n' 'decisions 0.3.0'; exit 0; fi
case " $* " in
    *' doctor '*) printf '%s\n' '{"schema_version":3}' ;;
    *' events watermark '*) printf '%s\n' '{"stream":"decisions.lifecycle","envelope_version":1,"cursor":"opaque"}' ;;
    *' observe activate '*)
        if [ -e "$HOME/.local/bin/decisions" ] || [ -L "$HOME/.local/bin/decisions" ]; then
            printf '%s\n' 'public observer command existed before baseline' >"$HOME/prebaseline-cli"
            exit 1
        fi
        if [ ! -f "$HOME/observer-baseline" ] && [ -e "$HOME/.codex/hooks.json" ]; then
            printf '%s\n' 'observer hook existed before baseline' >"$HOME/prebaseline-hook"
            exit 1
        fi
        if [ ! -f "$HOME/observer-baseline" ]; then
            printf '%s\n' baseline >"$HOME/observer-baseline"
        fi
        ;;
esac
exit 0
EOF
chmod 0755 "$candidate"
cat >"$launchctl" <<EOF
#!/bin/sh
printf '%s\n' "\$*" >>"$log"
mkdir -p "$loaded"
if [ "\${1:-}" = print ]; then label=\${2##*/}; [ -f "$loaded/\$label" ]; exit; fi
if [ "\${1:-}" = bootout ]; then label=\${2##*/}; rm -f "$loaded/\$label"; exit 0; fi
if [ "\${1:-}" = bootstrap ] && [ -f "$fail_bootstrap" ]; then
    if [ -f "$concurrent_hook" ]; then
        : >"$concurrent_hook_started"
        /bin/sleep 2 <"$home/Library/Application Support/Decisions/decisions.db" >/dev/null 2>&1 &
    fi
    rm -f "$fail_bootstrap"
    exit 1
fi
if [ "\${1:-}" = bootstrap ]; then
    label=\$(plutil -extract Label raw "\$3")
    if [ "\$label" = org.decisions.observer ] && [ ! -f "$baseline" ]; then
        printf '%s\n' 'observer bootstrapped before baseline' >"$temporary/prebaseline"
        exit 1
    fi
    : >"$loaded/\$label"
fi
exit 0
EOF
chmod 0755 "$launchctl"
: >"$home/.local/bin/email"
chmod 0755 "$home/.local/bin/email"
bad_schema_home="$temporary/BadSchemaHome"
bad_schema_candidate="$temporary/decisions-schema-two"
mkdir -p "$bad_schema_home/.local/bin"
: >"$bad_schema_home/.local/bin/email"
: >"$bad_schema_home/.local/bin/codex"
chmod 0755 "$bad_schema_home/.local/bin/email" "$bad_schema_home/.local/bin/codex"
cat >"$bad_schema_candidate" <<'EOF'
#!/bin/sh
if [ "${1:-}" = --version ]; then printf '%s\n' 'decisions 0.3.0'; exit 0; fi
case " $* " in
    *' doctor '*) printf '%s\n' '{"schema_version":2}' ;;
    *' events watermark '*) printf '%s\n' '{"stream":"decisions.lifecycle","envelope_version":1,"cursor":"opaque"}' ;;
esac
exit 0
EOF
chmod 0755 "$bad_schema_candidate"
if HOME="$bad_schema_home" "$package/deploy-user.sh" --binary "$bad_schema_candidate" \
    --home "$bad_schema_home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'deployment accepted a candidate that did not prove schema version 3' >&2
    exit 1
fi
[ ! -e "$bad_schema_home/.local/bin/decisions" ]
HOME="$home" "$package/deploy-user.sh" --binary "$candidate" --home "$home" --launchctl "$launchctl" >/dev/null
state="$home/Library/Application Support/Decisions"
[ -L "$home/.local/bin/decisions" ]
[ -L "$state/install/current" ]
[ -x "$state/install/current/libexec/decisions" ]
[ -x "$state/install/current/bin/decisions-daily-email" ]
[ -x "$state/install/current/bin/decisions-observer" ]
[ -L "$home/Library/Application Support/Chancery/providers/decisions" ]
daily_plist="$home/Library/LaunchAgents/org.decisions.daily-email.plist"
observer_plist="$home/Library/LaunchAgents/org.decisions.observer.plist"
plutil -lint "$daily_plist" >/dev/null
plutil -lint "$observer_plist" >/dev/null
[ "$(plutil -extract StartCalendarInterval.Hour raw "$daily_plist")" -eq 9 ]
[ "$(plutil -extract StartCalendarInterval.Minute raw "$daily_plist")" -eq 0 ]
[ "$(plutil -extract WorkingDirectory raw "$daily_plist")" = "$state" ]
[ "$(plutil -extract ProcessType raw "$daily_plist")" = Background ]
[ "$(plutil -extract Umask raw "$daily_plist")" = 077 ]
! plutil -extract RunAtLoad raw "$daily_plist" >/dev/null 2>&1
[ "$(plutil -extract StartInterval raw "$observer_plist")" -eq 60 ]
[ "$(plutil -extract WorkingDirectory raw "$observer_plist")" = "$state" ]
[ "$(plutil -extract ProcessType raw "$observer_plist")" = Background ]
[ "$(plutil -extract Umask raw "$observer_plist")" = 077 ]
! plutil -extract RunAtLoad raw "$observer_plist" >/dev/null 2>&1
! grep -F 'RESEND_API_KEY' "$daily_plist" "$observer_plist" >/dev/null
cmp -s "$package/hooks.json" "$home/.codex/hooks.json"
[ -f "$baseline" ]
[ ! -f "$home/prebaseline-cli" ]
[ ! -f "$home/prebaseline-hook" ]
[ ! -f "$temporary/prebaseline" ]
[ -f "$loaded/org.decisions.daily-email" ]
[ -f "$loaded/org.decisions.observer" ]
first_release=$(readlink "$state/install/current")
baseline_before=$(cat "$baseline")
HOME="$home" "$package/deploy-user.sh" --binary "$candidate" --home "$home" --launchctl "$launchctl" >/dev/null
[ "$(cat "$baseline")" = "$baseline_before" ]
[ ! -f "$temporary/prebaseline" ]
[ -f "$loaded/org.decisions.daily-email" ]
[ -f "$loaded/org.decisions.observer" ]
printf '%s\n' 'original database bytes' >"$state/decisions.db"
chmod 0600 "$state/decisions.db"
/bin/sleep 60 <"$state/decisions.db" &
holder=$!
if HOME="$home" "$package/deploy-user.sh" --binary "$candidate" --home "$home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'deployment migrated an open database' >&2
    exit 1
fi
[ -L "$home/.local/bin/decisions" ]
[ -f "$loaded/org.decisions.daily-email" ]
[ -f "$loaded/org.decisions.observer" ]
grep -Fx 'original database bytes' "$state/decisions.db" >/dev/null
kill "$holder"
wait "$holder" >/dev/null 2>&1 || true
holder=
candidate_two="$temporary/decisions-two"
cat >"$candidate_two" <<'EOF'
#!/bin/sh
if [ "${1:-}" = --version ]; then printf '%s\n' 'decisions 0.3.0'; exit 0; fi
database=
previous=
for argument in "$@"; do
    if [ "$previous" = database ]; then database=$argument; previous=; continue; fi
    if [ "$argument" = --database ]; then previous=database; continue; fi
    if [ "$argument" = doctor ] && [ -n "$database" ]; then
        printf '%s\n' 'candidate changed database' >"$database"
    fi
done
case " $* " in
    *' doctor '*) printf '%s\n' '{"schema_version":3}' ;;
    *' events watermark '*) printf '%s\n' '{"stream":"decisions.lifecycle","envelope_version":1,"cursor":"opaque"}' ;;
    *' observe activate '*)
        if [ -e "$HOME/.local/bin/decisions" ] || [ -L "$HOME/.local/bin/decisions" ]; then
            printf '%s\n' 'public observer command existed before baseline' >"$HOME/prebaseline-cli"
            exit 1
        fi
        if [ ! -f "$HOME/observer-baseline" ]; then
            printf '%s\n' baseline >"$HOME/observer-baseline"
        fi
        ;;
esac
exit 0
EOF
chmod 0755 "$candidate_two"
: >"$fail_bootstrap"
: >"$concurrent_hook"
if HOME="$home" "$package/deploy-user.sh" --binary "$candidate_two" --home "$home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'failed bootstrap unexpectedly committed' >&2
    exit 1
fi
[ -f "$concurrent_hook_started" ]
[ "$(readlink "$state/install/current")" = "$first_release" ]
[ "$(readlink "$home/.local/bin/decisions")" = "$state/install/current/bin/decisions" ]
[ "$(readlink "$home/Library/Application Support/Chancery/providers/decisions")" = "$state/install/current/share/chancery/decisions" ]
grep -Fx 'original database bytes' "$state/decisions.db" >/dev/null
[ -f "$loaded/org.decisions.daily-email" ]
[ -f "$loaded/org.decisions.observer" ]
cmp -s "$package/hooks.json" "$home/.codex/hooks.json"
HOME="$home" "$package/uninstall-user.sh" --home "$home" --launchctl "$launchctl" >/dev/null
[ ! -e "$home/.local/bin/decisions" ]
[ ! -e "$home/.codex/hooks.json" ]
[ ! -e "$home/Library/LaunchAgents/org.decisions.daily-email.plist" ]
[ ! -e "$home/Library/LaunchAgents/org.decisions.observer.plist" ]
[ -d "$state/install/releases" ]
HOME="$home" "$package/deploy-user.sh" --binary "$candidate" --home "$home" --launchctl "$launchctl" >/dev/null
[ -L "$home/.local/bin/decisions" ]
[ -L "$home/Library/Application Support/Chancery/providers/decisions" ]
[ -f "$loaded/org.decisions.daily-email" ]
[ -f "$loaded/org.decisions.observer" ]
printf '%s\n' '{"hooks":{"Stop":[]}}' >"$home/.codex/hooks.json"
if HOME="$home" "$package/uninstall-user.sh" --home "$home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'uninstaller removed a modified Codex hook' >&2
    exit 1
fi
grep -Fx '{"hooks":{"Stop":[]}}' "$home/.codex/hooks.json" >/dev/null
[ -f "$loaded/org.decisions.daily-email" ]
[ -f "$loaded/org.decisions.observer" ]
cp "$package/hooks.json" "$home/.codex/hooks.json"
rm -f "$home/.local/bin/decisions"
ln -s /tmp/foreign-decisions "$home/.local/bin/decisions"
if HOME="$home" "$package/uninstall-user.sh" --home "$home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'uninstaller removed a foreign selector' >&2
    exit 1
fi
[ "$(readlink "$home/.local/bin/decisions")" = /tmp/foreign-decisions ]
rm -f "$home/.local/bin/decisions"
printf '%s\n' 'foreign command' >"$home/.local/bin/decisions"
chmod 0755 "$home/.local/bin/decisions"
if HOME="$home" "$package/deploy-user.sh" --binary "$candidate" --home "$home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'foreign command unexpectedly replaced' >&2
    exit 1
fi
grep -Fx 'foreign command' "$home/.local/bin/decisions" >/dev/null

foreign_home="$temporary/ForeignHome"
mkdir -p "$foreign_home/.local/bin" "$foreign_home/Library/LaunchAgents"
: >"$foreign_home/.local/bin/email"
chmod 0755 "$foreign_home/.local/bin/email"
cat >"$foreign_home/Library/LaunchAgents/org.decisions.daily-email.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict><key>Label</key><string>org.foreign.service</string><key>ProgramArguments</key><array><string>/bin/false</string><string>/tmp/foreign</string></array></dict></plist>
EOF
if HOME="$foreign_home" "$package/deploy-user.sh" --binary "$candidate" --home "$foreign_home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'foreign plist unexpectedly replaced' >&2
    exit 1
fi
grep -F 'org.foreign.service' "$foreign_home/Library/LaunchAgents/org.decisions.daily-email.plist" >/dev/null

foreign_hooks_home="$temporary/ForeignHooksHome"
mkdir -p "$foreign_hooks_home/.local/bin" "$foreign_hooks_home/.codex"
: >"$foreign_hooks_home/.local/bin/email"
chmod 0755 "$foreign_hooks_home/.local/bin/email"
printf '%s\n' '{"hooks":{"Stop":[]}}' >"$foreign_hooks_home/.codex/hooks.json"
if HOME="$foreign_hooks_home" "$package/deploy-user.sh" --binary "$candidate" --home "$foreign_hooks_home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'foreign Codex hooks unexpectedly replaced' >&2
    exit 1
fi
grep -Fx '{"hooks":{"Stop":[]}}' "$foreign_hooks_home/.codex/hooks.json" >/dev/null

traversal_home="$temporary/TraversalHome"
mkdir -p "$traversal_home/.local/bin" "$traversal_home/Library/Application Support/Decisions/install/releases"
: >"$traversal_home/.local/bin/email"
chmod 0755 "$traversal_home/.local/bin/email"
ln -s 'releases/../foreign' "$traversal_home/Library/Application Support/Decisions/install/current"
if HOME="$traversal_home" "$package/deploy-user.sh" --binary "$candidate" --home "$traversal_home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'traversal selector unexpectedly accepted' >&2
    exit 1
fi
[ "$(readlink "$traversal_home/Library/Application Support/Decisions/install/current")" = 'releases/../foreign' ]

fabricated_home="$temporary/FabricatedHome"
fabricated_install="$fabricated_home/Library/Application Support/Decisions/install"
fabricated_id=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
fabricated_release="$fabricated_install/releases/$fabricated_id"
mkdir -p "$fabricated_home/.local/bin" \
    "$fabricated_release/bin" \
    "$fabricated_release/libexec" \
    "$fabricated_release/package" \
    "$fabricated_release/share/chancery/decisions"
: >"$fabricated_home/.local/bin/email"
chmod 0755 "$fabricated_home/.local/bin/email"
for fabricated_file in \
    "$fabricated_release/bin/decisions" \
    "$fabricated_release/bin/decisions-daily-email" \
    "$fabricated_release/bin/decisions-observer" \
    "$fabricated_release/libexec/decisions" \
    "$fabricated_release/package/decisions" \
    "$fabricated_release/package/decisions-daily-email" \
    "$fabricated_release/package/decisions-observer" \
    "$fabricated_release/package/deploy-user.sh" \
    "$fabricated_release/package/uninstall-user.sh" \
    "$fabricated_release/package/org.decisions.daily-email.plist" \
    "$fabricated_release/package/org.decisions.observer.plist" \
    "$fabricated_release/package/hooks.json"
do
    printf '%s\n' 'fabricated payload' >"$fabricated_file"
done
printf '%s\n' '{}' >"$fabricated_release/share/chancery/decisions/provider.json"
{
    printf '%s\n' 'format=2'
    printf 'release_id=%s\n' "$fabricated_id"
    printf '%s\n' 'version=0.3.0'
    printf '%s\n' 'binary_sha256=0000000000000000000000000000000000000000000000000000000000000000'
    printf '%s\n' 'frontend_sha256=0000000000000000000000000000000000000000000000000000000000000000'
    printf '%s\n' 'daily_runner_sha256=0000000000000000000000000000000000000000000000000000000000000000'
    printf '%s\n' 'observer_runner_sha256=0000000000000000000000000000000000000000000000000000000000000000'
    printf '%s\n' 'daily_plist_sha256=0000000000000000000000000000000000000000000000000000000000000000'
    printf '%s\n' 'observer_plist_sha256=0000000000000000000000000000000000000000000000000000000000000000'
    printf '%s\n' 'hooks_sha256=0000000000000000000000000000000000000000000000000000000000000000'
    printf '%s\n' 'deployer_sha256=0000000000000000000000000000000000000000000000000000000000000000'
    printf '%s\n' 'uninstaller_sha256=0000000000000000000000000000000000000000000000000000000000000000'
    printf '%s\n' 'chancery_sha256=0000000000000000000000000000000000000000000000000000000000000000'
} >"$fabricated_release/manifest.txt"
ln -s "releases/$fabricated_id" "$fabricated_install/current"
if HOME="$fabricated_home" "$package/deploy-user.sh" --binary "$candidate" --home "$fabricated_home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'deployer trusted a fabricated release manifest' >&2
    exit 1
fi
if HOME="$fabricated_home" "$package/uninstall-user.sh" --home "$fabricated_home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'uninstaller trusted a fabricated release manifest' >&2
    exit 1
fi
[ "$(readlink "$fabricated_install/current")" = "releases/$fabricated_id" ]
printf '%s\n' 'deploy test passed'
