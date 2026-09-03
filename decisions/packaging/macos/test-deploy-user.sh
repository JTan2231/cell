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
clockwork="$temporary/clockwork"
log="$temporary/launchctl.log"
fail_switch="$temporary/fail-switch"
concurrent_hook="$temporary/concurrent-hook"
concurrent_hook_started="$temporary/concurrent-hook-started"
clockwork_without_maintenance="$temporary/clockwork-without-maintenance"
loaded="$temporary/loaded"
baseline="$home/observer-baseline"
mkdir -p "$home/.local/bin" "$package" "$share"
cp "$SCRIPT_DIR/decisions" "$SCRIPT_DIR/decisions-daily-email" "$SCRIPT_DIR/decisions-observer" \
    "$SCRIPT_DIR/deploy-user.sh" "$SCRIPT_DIR/uninstall-user.sh" \
    "$SCRIPT_DIR/org.decisions.daily-email.plist" "$SCRIPT_DIR/org.decisions.observer.plist" \
    "$SCRIPT_DIR/decisions-daily-email.clockwork.toml.in" \
    "$SCRIPT_DIR/decisions-observer.clockwork.toml.in" \
    "$SCRIPT_DIR/hooks.json" "$package/"
cp -R "$SCRIPT_DIR/../../chancery" "$share/decisions"
DECISIONS_TEST_VERSION=$(awk -F '"' '/"release"[[:space:]]*:/ { print $4; exit }' \
    "$share/decisions/provider.json")
[ -n "$DECISIONS_TEST_VERSION" ]
export DECISIONS_TEST_VERSION

fixture_bundle_hash() {
    fixture_bundle=$1
    (
        cd "$fixture_bundle"
        find . -type f -print | LC_ALL=C sort | while IFS= read -r fixture_file; do
            printf 'path=%s\n' "$fixture_file"
            shasum -a 256 "$fixture_file"
        done
    ) | shasum -a 256 | awk '{print $1}'
}

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
if [ "${1:-}" = --version ]; then printf 'decisions %s\n' "$DECISIONS_TEST_VERSION"; exit 0; fi
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
cat >"$clockwork" <<EOF
#!/bin/sh
set -eu
if [ "\${1:-}" = --json ]; then shift; fi
state="\$HOME/.clockwork-test"
mkdir -p "\$state"
require_maintenance() {
    if [ ! -f "\$HOME/Library/Application Support/Decisions/.clockwork-maintenance" ]; then
        : >"$clockwork_without_maintenance"
        exit 1
    fi
}
case "\${1:-} \${2:-}" in
    'definition register')
        require_maintenance
        manifest=\$3
        key=\$(sed -n 's/^key = "\([^"]*\)"/\1/p' "\$manifest")
        digest=\$(shasum -a 256 "\$manifest" | awk '{print \$1}')
        safe_key=\$(printf '%s' "\$key" | tr / -)
        cp "\$manifest" "\$state/definition-\$safe_key.toml"
        cp "\$manifest" "\$state/definition-\$digest.toml"
        printf '{"ok":true,"data":{"digest":"%s","key":"%s"}}\n' "\$digest" "\$key"
        ;;
    'definition show')
        digest=\$3
        definition="\$state/definition-\$digest.toml"
        if [ ! -f "\$definition" ]; then
            printf '%s\n' '{"ok":false,"error":{"code":"definition_not_found","message":"missing"}}' >&2
            exit 1
        fi
        key=\$(sed -n 's/^key = "\([^"]*\)"/\1/p' "\$definition")
        schema_version=\$(sed -n 's/^schema_version = //p' "\$definition")
        release_id=\$(sed -n 's/^release_id = "\([^"]*\)"/\1/p' "\$definition")
        release_root=\$(sed -n 's/^release_root = "\([^"]*\)"/\1/p' "\$definition")
        authority=\$(sed -n 's/^authority = "\([^"]*\)"/\1/p' "\$definition")
        overlap=\$(sed -n 's/^overlap = "\([^"]*\)"/\1/p' "\$definition")
        cwd=\$(sed -n 's/^cwd = "\([^"]*\)"/\1/p' "\$definition")
        schedule_kind=\$(sed -n 's/^kind = "\([^"]*\)"/\1/p' "\$definition" | sed -n '1p')
        run_at_load=\$(sed -n 's/^run_at_load = //p' "\$definition")
        launch_kind=\$(sed -n 's/^kind = "\([^"]*\)"/\1/p' "\$definition" | sed -n '2p')
        interpreter=\$(sed -n 's/^interpreter = "\([^"]*\)"/\1/p' "\$definition")
        interpreter_sha256=\$(sed -n 's/^interpreter_sha256 = "\([^"]*\)"/\1/p' "\$definition")
        script=\$(sed -n 's/^script = "\([^"]*\)"/\1/p' "\$definition")
        script_sha256=\$(sed -n 's/^script_sha256 = "\([^"]*\)"/\1/p' "\$definition")
        home=\$(sed -n 's/^HOME = "\([^"]*\)"/\1/p' "\$definition")
        stdout=\$(sed -n 's/^stdout = "\([^"]*\)"/\1/p' "\$definition")
        stderr=\$(sed -n 's/^stderr = "\([^"]*\)"/\1/p' "\$definition")
        case "\$schedule_kind" in
            interval)
                seconds=\$(sed -n 's/^seconds = //p' "\$definition")
                schedule_json="{\"kind\":\"interval\",\"seconds\":\$seconds,\"run_at_load\":\$run_at_load}"
                ;;
            local-calendar)
                hour=\$(sed -n 's/^hour = //p' "\$definition")
                minute=\$(sed -n 's/^minute = //p' "\$definition")
                schedule_json="{\"kind\":\"local-calendar\",\"hour\":\$hour,\"minute\":\$minute,\"run_at_load\":\$run_at_load}"
                ;;
            *) exit 1 ;;
        esac
        printf '{"ok":true,"data":{"digest":"%s","key":"%s","registered_at":0,"manifest":{"schema_version":%s,"key":"%s","release_id":"%s","release_root":"%s","authority":"%s","overlap":"%s","arguments":[],"cwd":"%s","schedule":%s,"launch":{"kind":"%s","interpreter":"%s","interpreter_sha256":"%s","script":"%s","script_sha256":"%s"},"environment":{"HOME":"%s"},"output":{"stdout":"%s","stderr":"%s"}}}}\n' \
            "\$digest" "\$key" "\$schema_version" "\$key" "\$release_id" "\$release_root" \
            "\$authority" "\$overlap" "\$cwd" "\$schedule_json" "\$launch_kind" \
            "\$interpreter" "\$interpreter_sha256" "\$script" "\$script_sha256" \
            "\$home" "\$stdout" "\$stderr"
        ;;
    'binding show')
        key=\$3
        safe_key=\$(printf '%s' "\$key" | tr / -)
        binding="\$state/binding-\$safe_key"
        if [ ! -f "\$binding" ]; then
            printf '%s\n' '{"ok":false,"error":{"code":"binding_not_found","message":"missing"}}' >&2
            exit 1
        fi
        IFS='|' read -r enabled digest <"\$binding"
        if [ -n "\$digest" ]; then digest_json="\"\$digest\""; else digest_json=null; fi
        if [ "\$enabled" -eq 1 ]; then enabled_json=true; else enabled_json=false; fi
        printf '{"ok":true,"data":{"key":"%s","definition_digest":%s,"enabled":%s,"updated_at":0}}\n' \
            "\$key" "\$digest_json" "\$enabled_json"
        ;;
    'binding disable')
        key=\$3
        require_maintenance
        safe_key=\$(printf '%s' "\$key" | tr / -)
        binding="\$state/binding-\$safe_key"
        digest=
        if [ -f "\$binding" ]; then IFS='|' read -r ignored digest <"\$binding"; fi
        if [ "\${4:-}" = --select ]; then digest=\${5:?}; fi
        printf '0|%s\n' "\$digest" >"\$binding"
        if [ -n "\$digest" ]; then digest_json="\"\$digest\""; else digest_json=null; fi
        printf '{"ok":true,"data":{"key":"%s","definition_digest":%s,"enabled":false,"updated_at":0}}\n' \
            "\$key" "\$digest_json"
        ;;
    'binding switch')
        key=\$3
        digest=\$4
        require_maintenance
        if [ "\$key" = decisions/observer ] && [ -f "$fail_switch" ]; then
            if [ -f "$concurrent_hook" ]; then
                : >"$concurrent_hook_started"
                /bin/sleep 2 <"\$HOME/Library/Application Support/Decisions/decisions.db" >/dev/null 2>&1 &
            fi
            rm -f "$fail_switch"
            exit 1
        fi
        safe_key=\$(printf '%s' "\$key" | tr / -)
        printf '1|%s\n' "\$digest" >"\$state/binding-\$safe_key"
        printf '{"ok":true,"data":{"key":"%s","definition_digest":"%s","enabled":true,"updated_at":0}}\n' "\$key" "\$digest"
        ;;
    *) printf '%s\n' 'unsupported fake Clockwork invocation' >&2; exit 1 ;;
esac
EOF
chmod 0755 "$clockwork"
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
if [ "${1:-}" = --version ]; then printf 'decisions %s\n' "$DECISIONS_TEST_VERSION"; exit 0; fi
case " $* " in
    *' doctor '*) printf '%s\n' '{"schema_version":2}' ;;
    *' events watermark '*) printf '%s\n' '{"stream":"decisions.lifecycle","envelope_version":1,"cursor":"opaque"}' ;;
esac
exit 0
EOF
chmod 0755 "$bad_schema_candidate"
if HOME="$bad_schema_home" "$package/deploy-user.sh" --binary "$bad_schema_candidate" \
    --clockwork "$clockwork" --home "$bad_schema_home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'deployment accepted a candidate that did not prove schema version 3' >&2
    exit 1
fi
[ ! -e "$bad_schema_home/.local/bin/decisions" ]

foreign_binding_home="$temporary/ForeignBindingHome"
mkdir -p "$foreign_binding_home/.local/bin" "$foreign_binding_home/.clockwork-test"
: >"$foreign_binding_home/.local/bin/email"
: >"$foreign_binding_home/.local/bin/codex"
chmod 0755 "$foreign_binding_home/.local/bin/email" "$foreign_binding_home/.local/bin/codex"
foreign_binding_digest=$(printf '%064d' 0 | tr 0 f)
printf '1|%s\n' "$foreign_binding_digest" \
    >"$foreign_binding_home/.clockwork-test/binding-decisions-observer"
if HOME="$foreign_binding_home" "$package/deploy-user.sh" --binary "$candidate" \
    --clockwork "$clockwork" --home "$foreign_binding_home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'deployment adopted a foreign Clockwork binding' >&2
    exit 1
fi
grep -Fx "1|$foreign_binding_digest" \
    "$foreign_binding_home/.clockwork-test/binding-decisions-observer" >/dev/null
printf '0|%s\n' "$foreign_binding_digest" \
    >"$foreign_binding_home/.clockwork-test/binding-decisions-observer"
if HOME="$foreign_binding_home" "$package/deploy-user.sh" --binary "$candidate" \
    --clockwork "$clockwork" --home "$foreign_binding_home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'deployment adopted a disabled foreign Clockwork binding' >&2
    exit 1
fi
grep -Fx "0|$foreign_binding_digest" \
    "$foreign_binding_home/.clockwork-test/binding-decisions-observer" >/dev/null
if HOME="$foreign_binding_home" "$package/uninstall-user.sh" --clockwork "$clockwork" \
    --home "$foreign_binding_home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'uninstaller accepted a selected binding without an owned current release' >&2
    exit 1
fi
grep -Fx "0|$foreign_binding_digest" \
    "$foreign_binding_home/.clockwork-test/binding-decisions-observer" >/dev/null
printf '%s\n' '0|' \
    >"$foreign_binding_home/.clockwork-test/binding-decisions-observer"
HOME="$foreign_binding_home" "$package/uninstall-user.sh" --clockwork "$clockwork" \
    --home "$foreign_binding_home" --launchctl "$launchctl" >/dev/null
grep -Fx '0|' \
    "$foreign_binding_home/.clockwork-test/binding-decisions-observer" >/dev/null

HOME="$home" "$package/deploy-user.sh" --binary "$candidate" --clockwork "$clockwork" --home "$home" --launchctl "$launchctl" >/dev/null
state="$home/Library/Application Support/Decisions"
[ ! -e "$state/.clockwork-maintenance" ]
[ ! -e "$clockwork_without_maintenance" ]
[ -L "$home/.local/bin/decisions" ]
[ -L "$state/install/current" ]
[ -x "$state/install/current/libexec/decisions" ]
[ -x "$state/install/current/bin/decisions-daily-email" ]
[ -x "$state/install/current/bin/decisions-observer" ]
[ -L "$home/Library/Application Support/Chancery/providers/decisions" ]
daily_plist="$home/Library/LaunchAgents/org.decisions.daily-email.plist"
observer_plist="$home/Library/LaunchAgents/org.decisions.observer.plist"
daily_definition="$home/.clockwork-test/definition-decisions-daily-email.toml"
observer_definition="$home/.clockwork-test/definition-decisions-observer.toml"
[ ! -e "$daily_plist" ]
[ ! -e "$observer_plist" ]
grep -Fx 'kind = "local-calendar"' "$daily_definition" >/dev/null
grep -Fx 'hour = 9' "$daily_definition" >/dev/null
grep -Fx 'minute = 0' "$daily_definition" >/dev/null
grep -Fx 'run_at_load = false' "$daily_definition" >/dev/null
grep -Fx 'kind = "interval"' "$observer_definition" >/dev/null
grep -Fx 'seconds = 60' "$observer_definition" >/dev/null
grep -Fx "cwd = \"$state\"" "$daily_definition" "$observer_definition" >/dev/null
grep -F '/install/releases/' "$daily_definition" "$observer_definition" >/dev/null
! grep -F '/install/current/' "$daily_definition" "$observer_definition" >/dev/null
! grep -F 'RESEND_API_KEY' "$daily_definition" "$observer_definition" >/dev/null
cmp -s "$package/hooks.json" "$home/.codex/hooks.json"
[ -f "$baseline" ]
[ ! -f "$home/prebaseline-cli" ]
[ ! -f "$home/prebaseline-hook" ]
[ ! -f "$temporary/prebaseline" ]
grep -Eq '^1\|[0-9a-f]{64}$' "$home/.clockwork-test/binding-decisions-daily-email"
grep -Eq '^1\|[0-9a-f]{64}$' "$home/.clockwork-test/binding-decisions-observer"
[ ! -f "$loaded/org.decisions.daily-email" ]
[ ! -f "$loaded/org.decisions.observer" ]
mkdir "$state/install/.update-lock"
if HOME="$home" "$package/uninstall-user.sh" --clockwork "$clockwork" \
    --home "$home" --launchctl "$launchctl" >/dev/null 2>&1
then
    printf '%s\n' 'uninstall ignored an active Decisions deployment lock' >&2
    exit 1
fi
[ -L "$home/.local/bin/decisions" ]
[ -L "$state/install/current" ]
rmdir "$state/install/.update-lock"
first_release=$(readlink "$state/install/current")
baseline_before=$(cat "$baseline")
owned_observer_binding=$(cat "$home/.clockwork-test/binding-decisions-observer")
sed 's|^release_root = ".*"$|release_root = "/tmp/foreign-decisions-release"|' "$observer_definition" \
    >"$home/.clockwork-test/definition-$foreign_binding_digest.toml"
printf '1|%s\n' "$foreign_binding_digest" \
    >"$home/.clockwork-test/binding-decisions-observer"
if HOME="$home" "$package/deploy-user.sh" --binary "$candidate" --clockwork "$clockwork" \
    --home "$home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'deployment adopted a same-key Clockwork definition with a foreign release root' >&2
    exit 1
fi
grep -Fx "1|$foreign_binding_digest" \
    "$home/.clockwork-test/binding-decisions-observer" >/dev/null
printf '%s\n' "$owned_observer_binding" \
    >"$home/.clockwork-test/binding-decisions-observer"
HOME="$home" "$package/deploy-user.sh" --binary "$candidate" --clockwork "$clockwork" --home "$home" --launchctl "$launchctl" >/dev/null
[ "$(cat "$baseline")" = "$baseline_before" ]
[ ! -f "$temporary/prebaseline" ]
grep -Eq '^1\|[0-9a-f]{64}$' "$home/.clockwork-test/binding-decisions-daily-email"
grep -Eq '^1\|[0-9a-f]{64}$' "$home/.clockwork-test/binding-decisions-observer"
printf '%s\n' 'original database bytes' >"$state/decisions.db"
chmod 0600 "$state/decisions.db"
/bin/sleep 60 <"$state/decisions.db" &
holder=$!
if HOME="$home" "$package/deploy-user.sh" --binary "$candidate" --clockwork "$clockwork" --home "$home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'deployment migrated an open database' >&2
    exit 1
fi
[ -L "$home/.local/bin/decisions" ]
grep -Eq '^1\|[0-9a-f]{64}$' "$home/.clockwork-test/binding-decisions-daily-email"
grep -Eq '^1\|[0-9a-f]{64}$' "$home/.clockwork-test/binding-decisions-observer"
grep -Fx 'original database bytes' "$state/decisions.db" >/dev/null
kill "$holder"
wait "$holder" >/dev/null 2>&1 || true
holder=
candidate_two="$temporary/decisions-two"
cat >"$candidate_two" <<'EOF'
#!/bin/sh
if [ "${1:-}" = --version ]; then printf 'decisions %s\n' "$DECISIONS_TEST_VERSION"; exit 0; fi
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
prior_disabled_daily_digest=$(sed -n 's/^1|\([0-9a-f]\{64\}\)$/\1/p' \
    "$home/.clockwork-test/binding-decisions-daily-email")
[ -n "$prior_disabled_daily_digest" ]
printf '0|%s\n' "$prior_disabled_daily_digest" \
    >"$home/.clockwork-test/binding-decisions-daily-email"
: >"$fail_switch"
: >"$concurrent_hook"
if HOME="$home" "$package/deploy-user.sh" --binary "$candidate_two" --clockwork "$clockwork" --home "$home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'failed Clockwork switch unexpectedly committed' >&2
    exit 1
fi
[ -f "$concurrent_hook_started" ]
[ "$(readlink "$state/install/current")" = "$first_release" ]
[ "$(readlink "$home/.local/bin/decisions")" = "$state/install/current/bin/decisions" ]
[ "$(readlink "$home/Library/Application Support/Chancery/providers/decisions")" = "$state/install/current/share/chancery/decisions" ]
grep -Fx 'original database bytes' "$state/decisions.db" >/dev/null
grep -Fx "0|$prior_disabled_daily_digest" \
    "$home/.clockwork-test/binding-decisions-daily-email" >/dev/null
grep -Eq '^1\|[0-9a-f]{64}$' "$home/.clockwork-test/binding-decisions-observer"
cmp -s "$package/hooks.json" "$home/.codex/hooks.json"
[ ! -e "$state/.clockwork-maintenance" ]
[ ! -e "$clockwork_without_maintenance" ]
printf '%s\n' 'uninstall maintenance evidence' >"$state/.clockwork-maintenance"
chmod 0600 "$state/.clockwork-maintenance"
ln "$state/.clockwork-maintenance" "$temporary/uninstall-maintenance-link"
if HOME="$home" "$package/uninstall-user.sh" --clockwork "$clockwork" \
    --home "$home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'uninstall accepted a hard-linked maintenance gate' >&2
    exit 1
fi
grep -Fx 'uninstall maintenance evidence' "$state/.clockwork-maintenance" >/dev/null
rm -f "$temporary/uninstall-maintenance-link" "$state/.clockwork-maintenance"
HOME="$home" "$package/uninstall-user.sh" --clockwork "$clockwork" --home "$home" --launchctl "$launchctl" >/dev/null
[ ! -e "$home/.local/bin/decisions" ]
[ ! -e "$home/.codex/hooks.json" ]
[ ! -e "$home/Library/LaunchAgents/org.decisions.daily-email.plist" ]
[ ! -e "$home/Library/LaunchAgents/org.decisions.observer.plist" ]
[ -d "$state/install/releases" ]
grep -Eq '^0\|[0-9a-f]{64}$' "$home/.clockwork-test/binding-decisions-daily-email"
grep -Eq '^0\|[0-9a-f]{64}$' "$home/.clockwork-test/binding-decisions-observer"
[ -f "$state/.clockwork-maintenance" ]
[ "$(stat -f '%Lp' "$state/.clockwork-maintenance")" = 600 ]
[ "$(stat -f '%u' "$state/.clockwork-maintenance")" -eq "$(id -u)" ]
printf '%s\n' 'retained maintenance evidence' >"$state/.clockwork-maintenance"
ln "$state/.clockwork-maintenance" "$temporary/maintenance-link"
if HOME="$home" "$package/deploy-user.sh" --binary "$candidate" \
    --clockwork "$clockwork" --home "$home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'deployment accepted a hard-linked maintenance gate' >&2
    exit 1
fi
grep -Fx 'retained maintenance evidence' "$state/.clockwork-maintenance" >/dev/null
rm -f "$temporary/maintenance-link"
chmod 0644 "$state/.clockwork-maintenance"
if HOME="$home" "$package/deploy-user.sh" --binary "$candidate" \
    --clockwork "$clockwork" --home "$home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'deployment accepted a non-private maintenance gate' >&2
    exit 1
fi
[ "$(stat -f '%Lp' "$state/.clockwork-maintenance")" = 644 ]
chmod 0600 "$state/.clockwork-maintenance"
printf '%s\n' 'existing observer log' >"$home/Library/Logs/Decisions/observer.stderr.log"
chmod 0644 "$home/Library/Logs/Decisions/observer.stderr.log"
HOME="$home" "$package/deploy-user.sh" --binary "$candidate" --clockwork "$clockwork" --home "$home" --launchctl "$launchctl" >/dev/null
[ -L "$home/.local/bin/decisions" ]
[ -L "$home/Library/Application Support/Chancery/providers/decisions" ]
[ "$(stat -f '%Lp' "$home/Library/Logs/Decisions/observer.stderr.log")" = 600 ]
grep -Fx 'existing observer log' "$home/Library/Logs/Decisions/observer.stderr.log" >/dev/null
grep -Eq '^1\|[0-9a-f]{64}$' "$home/.clockwork-test/binding-decisions-daily-email"
grep -Eq '^1\|[0-9a-f]{64}$' "$home/.clockwork-test/binding-decisions-observer"
[ ! -e "$state/.clockwork-maintenance" ]

unsafe_home="$temporary/UnsafeRollbackHome"
mkdir -p "$unsafe_home/.local/bin"
: >"$unsafe_home/.local/bin/email"
: >"$unsafe_home/.local/bin/codex"
chmod 0755 "$unsafe_home/.local/bin/email" "$unsafe_home/.local/bin/codex"
: >"$fail_switch"
if HOME="$unsafe_home" "$package/deploy-user.sh" --binary "$candidate" \
    --clockwork "$clockwork" --home "$unsafe_home" --launchctl "$launchctl" \
    >"$temporary/unsafe.stdout" 2>"$temporary/unsafe.stderr"; then
    printf '%s\n' 'deployment committed after an unprovable Clockwork rollback' >&2
    exit 1
fi
unsafe_state="$unsafe_home/Library/Application Support/Decisions"
[ -f "$unsafe_state/.clockwork-maintenance" ]
[ "$(stat -f '%Lp' "$unsafe_state/.clockwork-maintenance")" = 600 ]
[ "$(stat -f '%u' "$unsafe_state/.clockwork-maintenance")" -eq "$(id -u)" ]
[ ! -e "$unsafe_home/.local/bin/decisions" ]
grep -Eq '^0\|[0-9a-f]{64}$' \
    "$unsafe_home/.clockwork-test/binding-decisions-daily-email"
grep -Fx '0|' "$unsafe_home/.clockwork-test/binding-decisions-observer" >/dev/null
grep -F 'a valid maintenance gate is retained' \
    "$temporary/unsafe.stderr" >/dev/null
[ ! -e "$clockwork_without_maintenance" ]
unsafe_transaction=$(find "$unsafe_state/install" -maxdepth 1 -type d \
    -name '.transaction.*' -print | sed -n '1p')
[ -n "$unsafe_transaction" ]
[ -f "$unsafe_transaction/prior-install.txt" ]

owned_observer_binding=$(cat "$home/.clockwork-test/binding-decisions-observer")
printf '1|%s\n' "$foreign_binding_digest" \
    >"$home/.clockwork-test/binding-decisions-observer"
if HOME="$home" "$package/uninstall-user.sh" --clockwork "$clockwork" --home "$home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'uninstaller disabled a foreign Clockwork binding' >&2
    exit 1
fi
grep -Fx "1|$foreign_binding_digest" \
    "$home/.clockwork-test/binding-decisions-observer" >/dev/null
printf '0|%s\n' "$foreign_binding_digest" \
    >"$home/.clockwork-test/binding-decisions-observer"
if HOME="$home" "$package/uninstall-user.sh" --clockwork "$clockwork" --home "$home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'uninstaller disabled a selected foreign Clockwork definition' >&2
    exit 1
fi
grep -Fx "0|$foreign_binding_digest" \
    "$home/.clockwork-test/binding-decisions-observer" >/dev/null
printf '%s\n' "$owned_observer_binding" \
    >"$home/.clockwork-test/binding-decisions-observer"
printf '%s\n' '{"hooks":{"Stop":[]}}' >"$home/.codex/hooks.json"
if HOME="$home" "$package/uninstall-user.sh" --clockwork "$clockwork" --home "$home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'uninstaller removed a modified Codex hook' >&2
    exit 1
fi
grep -Fx '{"hooks":{"Stop":[]}}' "$home/.codex/hooks.json" >/dev/null
grep -Eq '^1\|[0-9a-f]{64}$' "$home/.clockwork-test/binding-decisions-daily-email"
grep -Eq '^1\|[0-9a-f]{64}$' "$home/.clockwork-test/binding-decisions-observer"
cp "$package/hooks.json" "$home/.codex/hooks.json"
rm -f "$home/.local/bin/decisions"
ln -s /tmp/foreign-decisions "$home/.local/bin/decisions"
if HOME="$home" "$package/uninstall-user.sh" --clockwork "$clockwork" --home "$home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'uninstaller removed a foreign selector' >&2
    exit 1
fi
[ "$(readlink "$home/.local/bin/decisions")" = /tmp/foreign-decisions ]
rm -f "$home/.local/bin/decisions"
printf '%s\n' 'foreign command' >"$home/.local/bin/decisions"
chmod 0755 "$home/.local/bin/decisions"
if HOME="$home" "$package/deploy-user.sh" --binary "$candidate" --clockwork "$clockwork" --home "$home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'foreign command unexpectedly replaced' >&2
    exit 1
fi
grep -Fx 'foreign command' "$home/.local/bin/decisions" >/dev/null

quote_home="$temporary/Home\"Quote"
mkdir -p "$quote_home"
if HOME="$quote_home" "$package/deploy-user.sh" --binary "$candidate" \
    --clockwork "$clockwork" --home "$quote_home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'deployment rendered a quoted home into Clockwork TOML' >&2
    exit 1
fi
if HOME="$quote_home" "$package/uninstall-user.sh" --clockwork "$clockwork" \
    --home "$quote_home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'uninstall accepted a quoted schedule-rendering home' >&2
    exit 1
fi
backslash_home="$temporary/Home\\Backslash"
mkdir -p "$backslash_home"
if HOME="$backslash_home" "$package/deploy-user.sh" --binary "$candidate" \
    --clockwork "$clockwork" --home "$backslash_home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'deployment rendered a backslash home into Clockwork TOML' >&2
    exit 1
fi
if HOME="$backslash_home" "$package/uninstall-user.sh" --clockwork "$clockwork" \
    --home "$backslash_home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'uninstall accepted a backslash schedule-rendering home' >&2
    exit 1
fi

foreign_home="$temporary/ForeignHome"
mkdir -p "$foreign_home/.local/bin" "$foreign_home/Library/LaunchAgents"
: >"$foreign_home/.local/bin/email"
chmod 0755 "$foreign_home/.local/bin/email"
cat >"$foreign_home/Library/LaunchAgents/org.decisions.daily-email.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict><key>Label</key><string>org.foreign.service</string><key>ProgramArguments</key><array><string>/bin/false</string><string>/tmp/foreign</string></array></dict></plist>
EOF
if HOME="$foreign_home" "$package/deploy-user.sh" --binary "$candidate" --clockwork "$clockwork" --home "$foreign_home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'foreign plist unexpectedly replaced' >&2
    exit 1
fi
grep -F 'org.foreign.service' "$foreign_home/Library/LaunchAgents/org.decisions.daily-email.plist" >/dev/null

foreign_hooks_home="$temporary/ForeignHooksHome"
mkdir -p "$foreign_hooks_home/.local/bin" "$foreign_hooks_home/.codex"
: >"$foreign_hooks_home/.local/bin/email"
chmod 0755 "$foreign_hooks_home/.local/bin/email"
printf '%s\n' '{"hooks":{"Stop":[]}}' >"$foreign_hooks_home/.codex/hooks.json"
if HOME="$foreign_hooks_home" "$package/deploy-user.sh" --binary "$candidate" --clockwork "$clockwork" --home "$foreign_hooks_home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'foreign Codex hooks unexpectedly replaced' >&2
    exit 1
fi
grep -Fx '{"hooks":{"Stop":[]}}' "$foreign_hooks_home/.codex/hooks.json" >/dev/null

traversal_home="$temporary/TraversalHome"
mkdir -p "$traversal_home/.local/bin" "$traversal_home/Library/Application Support/Decisions/install/releases"
: >"$traversal_home/.local/bin/email"
chmod 0755 "$traversal_home/.local/bin/email"
ln -s 'releases/../foreign' "$traversal_home/Library/Application Support/Decisions/install/current"
if HOME="$traversal_home" "$package/deploy-user.sh" --binary "$candidate" --clockwork "$clockwork" --home "$traversal_home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'traversal selector unexpectedly accepted' >&2
    exit 1
fi
[ "$(readlink "$traversal_home/Library/Application Support/Decisions/install/current")" = 'releases/../foreign' ]

legacy_home="$temporary/LegacyHome"
legacy_state="$legacy_home/Library/Application Support/Decisions"
legacy_install="$legacy_state/install"
legacy_stage="$legacy_install/release-stage"
legacy_logs="$legacy_home/Library/Logs/Decisions"
mkdir -p "$legacy_home/.local/bin" "$legacy_home/.codex" \
    "$legacy_home/Library/LaunchAgents" \
    "$legacy_home/Library/Application Support/Chancery/providers" \
    "$legacy_install/releases" "$legacy_logs" \
    "$legacy_stage/bin" "$legacy_stage/libexec" "$legacy_stage/package" \
    "$legacy_stage/share/chancery"
: >"$legacy_home/.local/bin/email"
: >"$legacy_home/.local/bin/codex"
printf '%s\n' baseline >"$legacy_home/observer-baseline"
chmod 0755 "$legacy_home/.local/bin/email" "$legacy_home/.local/bin/codex"
install -m 0755 "$candidate" "$legacy_stage/libexec/decisions"
install -m 0755 "$package/decisions" "$legacy_stage/bin/decisions"
install -m 0755 "$package/decisions-daily-email" \
    "$legacy_stage/bin/decisions-daily-email"
install -m 0755 "$package/decisions-observer" \
    "$legacy_stage/bin/decisions-observer"
install -m 0755 "$package/decisions" "$legacy_stage/package/decisions"
install -m 0755 "$package/decisions-daily-email" \
    "$legacy_stage/package/decisions-daily-email"
install -m 0755 "$package/decisions-observer" \
    "$legacy_stage/package/decisions-observer"
install -m 0755 "$package/deploy-user.sh" "$legacy_stage/package/deploy-user.sh"
install -m 0755 "$package/uninstall-user.sh" "$legacy_stage/package/uninstall-user.sh"
install -m 0644 "$package/org.decisions.daily-email.plist" \
    "$legacy_stage/package/org.decisions.daily-email.plist"
install -m 0644 "$package/org.decisions.observer.plist" \
    "$legacy_stage/package/org.decisions.observer.plist"
install -m 0644 "$package/hooks.json" "$legacy_stage/package/hooks.json"
cp -R "$share/decisions" "$legacy_stage/share/chancery/decisions"
legacy_binary_hash=$(shasum -a 256 "$legacy_stage/libexec/decisions" | awk '{print $1}')
legacy_frontend_hash=$(shasum -a 256 "$legacy_stage/bin/decisions" | awk '{print $1}')
legacy_daily_runner_hash=$(shasum -a 256 \
    "$legacy_stage/bin/decisions-daily-email" | awk '{print $1}')
legacy_observer_runner_hash=$(shasum -a 256 \
    "$legacy_stage/bin/decisions-observer" | awk '{print $1}')
legacy_daily_plist_hash=$(shasum -a 256 \
    "$legacy_stage/package/org.decisions.daily-email.plist" | awk '{print $1}')
legacy_observer_plist_hash=$(shasum -a 256 \
    "$legacy_stage/package/org.decisions.observer.plist" | awk '{print $1}')
legacy_hooks_hash=$(shasum -a 256 "$legacy_stage/package/hooks.json" | awk '{print $1}')
legacy_deployer_hash=$(shasum -a 256 \
    "$legacy_stage/package/deploy-user.sh" | awk '{print $1}')
legacy_uninstaller_hash=$(shasum -a 256 \
    "$legacy_stage/package/uninstall-user.sh" | awk '{print $1}')
legacy_chancery_hash=$(fixture_bundle_hash "$legacy_stage/share/chancery/decisions")
legacy_release_id=$(printf '%s\n' "$legacy_binary_hash" "$legacy_frontend_hash" \
    "$legacy_daily_runner_hash" "$legacy_observer_runner_hash" \
    "$legacy_daily_plist_hash" "$legacy_observer_plist_hash" "$legacy_hooks_hash" \
    "$legacy_deployer_hash" "$legacy_uninstaller_hash" "$legacy_chancery_hash" \
    | shasum -a 256 | awk '{print $1}')
{
    printf '%s\n' 'format=2'
    printf 'release_id=%s\n' "$legacy_release_id"
    printf 'version=%s\n' "$DECISIONS_TEST_VERSION"
    printf 'binary_sha256=%s\n' "$legacy_binary_hash"
    printf 'frontend_sha256=%s\n' "$legacy_frontend_hash"
    printf 'daily_runner_sha256=%s\n' "$legacy_daily_runner_hash"
    printf 'observer_runner_sha256=%s\n' "$legacy_observer_runner_hash"
    printf 'daily_plist_sha256=%s\n' "$legacy_daily_plist_hash"
    printf 'observer_plist_sha256=%s\n' "$legacy_observer_plist_hash"
    printf 'hooks_sha256=%s\n' "$legacy_hooks_hash"
    printf 'deployer_sha256=%s\n' "$legacy_deployer_hash"
    printf 'uninstaller_sha256=%s\n' "$legacy_uninstaller_hash"
    printf 'chancery_sha256=%s\n' "$legacy_chancery_hash"
} >"$legacy_stage/manifest.txt"
chmod 0444 "$legacy_stage/manifest.txt"
legacy_release="$legacy_install/releases/$legacy_release_id"
mv "$legacy_stage" "$legacy_release"
ln -s "releases/$legacy_release_id" "$legacy_install/current"
ln -s "$legacy_install/current/bin/decisions" "$legacy_home/.local/bin/decisions"
ln -s "$legacy_install/current/share/chancery/decisions" \
    "$legacy_home/Library/Application Support/Chancery/providers/decisions"
install -m 0600 "$legacy_release/package/hooks.json" "$legacy_home/.codex/hooks.json"
legacy_daily_plist="$legacy_home/Library/LaunchAgents/org.decisions.daily-email.plist"
legacy_observer_plist="$legacy_home/Library/LaunchAgents/org.decisions.observer.plist"
sed \
    -e "s|__DECISIONS_RUNNER__|$legacy_install/current/bin/decisions-daily-email|g" \
    -e "s|__DECISIONS_STATE_DIR__|$legacy_state|g" \
    -e "s|__DECISIONS_HOME__|$legacy_home|g" \
    -e "s|__DECISIONS_STDOUT__|$legacy_logs/daily-email.stdout.log|g" \
    -e "s|__DECISIONS_STDERR__|$legacy_logs/daily-email.stderr.log|g" \
    "$legacy_release/package/org.decisions.daily-email.plist" >"$legacy_daily_plist"
sed \
    -e "s|__DECISIONS_OBSERVER_RUNNER__|$legacy_install/current/bin/decisions-observer|g" \
    -e "s|__DECISIONS_STATE_DIR__|$legacy_state|g" \
    -e "s|__DECISIONS_HOME__|$legacy_home|g" \
    -e "s|__DECISIONS_OBSERVER_STDOUT__|$legacy_logs/observer.stdout.log|g" \
    -e "s|__DECISIONS_OBSERVER_STDERR__|$legacy_logs/observer.stderr.log|g" \
    "$legacy_release/package/org.decisions.observer.plist" >"$legacy_observer_plist"
chmod 0644 "$legacy_daily_plist" "$legacy_observer_plist"
legacy_daily_exact="$temporary/legacy-daily-exact.plist"
legacy_observer_exact="$temporary/legacy-observer-exact.plist"
cp "$legacy_daily_plist" "$legacy_daily_exact"
cp "$legacy_observer_plist" "$legacy_observer_exact"
: >"$loaded/org.decisions.daily-email"
: >"$loaded/org.decisions.observer"
chmod 0600 "$legacy_daily_plist"
if HOME="$legacy_home" "$package/deploy-user.sh" --binary "$candidate" \
    --clockwork "$clockwork" --home "$legacy_home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'deployment adopted a legacy plist with the wrong mode' >&2
    exit 1
fi
[ "$(stat -f '%Lp' "$legacy_daily_plist")" = 600 ]
[ ! -e "$legacy_state/.clockwork-maintenance" ]
chmod 0644 "$legacy_daily_plist"
plutil -insert KeepAlive -bool true "$legacy_daily_plist"
chmod 0644 "$legacy_daily_plist"
if HOME="$legacy_home" "$package/deploy-user.sh" --binary "$candidate" \
    --clockwork "$clockwork" --home "$legacy_home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'deployment adopted a legacy plist with extra launchd behavior' >&2
    exit 1
fi
plutil -extract KeepAlive raw "$legacy_daily_plist" >/dev/null
[ ! -e "$legacy_state/.clockwork-maintenance" ]
install -m 0644 "$legacy_daily_exact" "$legacy_daily_plist"
if HOME="$legacy_home" "$package/deploy-user.sh" --binary "$bad_schema_candidate" \
    --clockwork "$clockwork" --home "$legacy_home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'legacy migration accepted an invalid candidate doctor' >&2
    exit 1
fi
cmp -s "$legacy_daily_exact" "$legacy_daily_plist"
cmp -s "$legacy_observer_exact" "$legacy_observer_plist"
[ "$(stat -f '%Lp' "$legacy_daily_plist")" = 644 ]
[ "$(stat -f '%Lp' "$legacy_observer_plist")" = 644 ]
[ "$(stat -f '%u' "$legacy_daily_plist")" -eq "$(id -u)" ]
[ "$(stat -f '%u' "$legacy_observer_plist")" -eq "$(id -u)" ]
[ -f "$loaded/org.decisions.daily-email" ]
[ -f "$loaded/org.decisions.observer" ]
[ ! -e "$legacy_state/.clockwork-maintenance" ]
plutil -insert KeepAlive -bool true "$legacy_observer_plist"
chmod 0644 "$legacy_observer_plist"
if HOME="$legacy_home" "$package/uninstall-user.sh" --clockwork "$clockwork" \
    --home "$legacy_home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'uninstaller adopted a legacy plist with extra launchd behavior' >&2
    exit 1
fi
plutil -extract KeepAlive raw "$legacy_observer_plist" >/dev/null
[ ! -e "$legacy_state/.clockwork-maintenance" ]
install -m 0644 "$legacy_observer_exact" "$legacy_observer_plist"
HOME="$legacy_home" "$package/deploy-user.sh" --binary "$candidate" \
    --clockwork "$clockwork" --home "$legacy_home" --launchctl "$launchctl" >/dev/null
[ ! -e "$legacy_daily_plist" ]
[ ! -e "$legacy_observer_plist" ]
[ ! -f "$loaded/org.decisions.daily-email" ]
[ ! -f "$loaded/org.decisions.observer" ]
[ ! -e "$legacy_state/.clockwork-maintenance" ]

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
    printf 'version=%s\n' "$DECISIONS_TEST_VERSION"
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
if HOME="$fabricated_home" "$package/deploy-user.sh" --binary "$candidate" --clockwork "$clockwork" --home "$fabricated_home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'deployer trusted a fabricated release manifest' >&2
    exit 1
fi
if HOME="$fabricated_home" "$package/uninstall-user.sh" --clockwork "$clockwork" --home "$fabricated_home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'uninstaller trusted a fabricated release manifest' >&2
    exit 1
fi
[ "$(readlink "$fabricated_install/current")" = "releases/$fabricated_id" ]
printf '%s\n' 'deploy test passed'
