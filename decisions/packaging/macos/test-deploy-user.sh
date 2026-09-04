#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
krisis_version=$(awk '
    $0 == "[package]" { in_package = 1; next }
    in_package && $1 == "version" {
        gsub(/"/, "", $3)
        print $3
        exit
    }
' "$SCRIPT_DIR/../../crates/decisions/Cargo.toml")
[ -n "$krisis_version" ] || {
    printf '%s\n' 'unable to read the Krisis package version' >&2
    exit 1
}
temporary=$(mktemp -d "${TMPDIR:-/tmp}/krisis-deploy.XXXXXX")
trap 'rm -rf "$temporary"' EXIT HUP INT TERM
home="$temporary/Home"
candidate="$temporary/krisis"
clockwork="$temporary/clockwork"
annals="$temporary/annals"
launchctl="$temporary/launchctl"
config="$temporary/decisions.toml"
clockwork_state="$temporary/clockwork-state"
clockwork_capture="$temporary/clockwork.capture"
launchctl_capture="$temporary/launchctl.capture"
candidate_capture="$temporary/candidate.capture"
mkdir -p "$home/.local/bin" "$clockwork_state/definitions" "$clockwork_state/bindings"
: >"$clockwork_capture"
: >"$launchctl_capture"

cat >"$candidate" <<'EOF'
#!/bin/sh
set -eu
case " $* " in
    *' --version '*) printf '%s\n' 'krisis __KRISIS_VERSION__' ;;
    *' doctor '*)
        [ -z "${KRISIS_TEST_LEAK_ME:-}" ] || exit 70
        database="$HOME/Library/Application Support/Decisions/decisions.db"
        if [ ! -e "$database" ]; then : >"$database"; chmod 0600 "$database"; fi
        printf '%s\n' '{"ok":true,"schema_version":4,"annals_library_id":"0123456789abcdef0123456789abcdef"}'
        ;;
    *) [ -z "${KRISIS_TEST_LEAK_ME:-}" ] || exit 70; [ -z "${KRISIS_CANDIDATE_CAPTURE:-}" ] || printf '%s\n' "$*" >>"$KRISIS_CANDIDATE_CAPTURE" ;;
esac
EOF
sed "s/__KRISIS_VERSION__/$krisis_version/g" "$candidate" \
    >"$candidate.versioned"
mv "$candidate.versioned" "$candidate"

cat >"$clockwork" <<'EOF'
#!/bin/sh
set -eu
printf '%s\n' "$*" >>"$KRISIS_CLOCKWORK_CAPTURE"
state=$KRISIS_CLOCKWORK_STATE
command=$2
operation=$3
if [ "$command" = definition ] && [ "$operation" = register ]; then
    source=$4
    digest=$(shasum -a 256 "$source" | awk '{print $1}')
    cp "$source" "$state/definitions/$digest.toml"
    printf '{"ok":true,"data":{"digest":"%s"}}\n' "$digest"
    exit 0
fi
if [ "$command" = definition ] && [ "$operation" = show ]; then
    digest=$4
    source="$state/definitions/$digest.toml"
    [ -f "$source" ] || { printf '%s\n' '{"code":"definition_not_found"}' >&2; exit 1; }
    value() { sed -n "s/^$1 = \"\(.*\)\"/\1/p" "$source"; }
    key=$(value key)
    release_id=$(value release_id)
    release_root=$(value release_root)
    cwd=$(value cwd)
    interpreter=$(value interpreter)
    interpreter_sha256=$(value interpreter_sha256)
    script=$(value script)
    script_sha256=$(value script_sha256)
    env_home=$(value HOME)
    annals_binary=$(value KRISIS_ANNALS_BINARY)
    annals_config=$(value KRISIS_ANNALS_CONFIG)
    library_id=$(value KRISIS_ANNALS_LIBRARY_ID)
    stdout=$(value stdout)
    stderr=$(value stderr)
    case "$key" in
        decisions/daily-email)
            schedule_json='{"kind":"local-calendar","hour":9,"minute":0,"run_at_load":false}'
            environment_json="{\"HOME\":\"$env_home\"}"
            ;;
        decisions/observer)
            schedule_json='{"kind":"interval","seconds":60,"run_at_load":false}'
            environment_json="{\"HOME\":\"$env_home\"}"
            ;;
        krisis/observer)
            schedule_json='{"kind":"interval","seconds":60,"run_at_load":false}'
            environment_json="{\"HOME\":\"$env_home\",\"KRISIS_ANNALS_BINARY\":\"$annals_binary\",\"KRISIS_ANNALS_CONFIG\":\"$annals_config\",\"KRISIS_ANNALS_LIBRARY_ID\":\"$library_id\"}"
            ;;
        *) exit 1 ;;
    esac
    printf '{"ok":true,"data":{"digest":"%s","key":"%s","manifest":{"schema_version":1,"key":"%s","release_id":"%s","release_root":"%s","authority":"current-user-background","overlap":"skip","arguments":[],"cwd":"%s","schedule":%s,"launch":{"kind":"interpreted","interpreter":"%s","interpreter_sha256":"%s","script":"%s","script_sha256":"%s"},"environment":%s,"output":{"stdout":"%s","stderr":"%s"}}}}\n' \
        "$digest" "$key" "$key" "$release_id" "$release_root" "$cwd" \
        "$schedule_json" "$interpreter" "$interpreter_sha256" "$script" \
        "$script_sha256" "$environment_json" "$stdout" "$stderr"
    exit 0
fi
if [ "$command" = binding ]; then
    key=$4
    binding="$state/bindings/$(printf '%s' "$key" | tr / _).binding"
    if [ "$operation" = show ]; then
        if [ ! -f "$binding" ]; then printf '%s\n' '{"code":"binding_not_found"}' >&2; exit 1; fi
        IFS='|' read -r enabled digest <"$binding"
        if [ "$key" = krisis/observer ] && [ -n "${KRISIS_BLOCK_GATE_RELEASE_DIR:-}" ]; then
            chmod 0500 "$KRISIS_BLOCK_GATE_RELEASE_DIR"
        fi
        if [ "${KRISIS_FAIL_VERIFY_SWITCH:-0}" -eq 1 ] && [ -f "$state/fail-next-show" ] && [ "$key" = krisis/observer ]; then
            rm -f "$state/fail-next-show"
            printf '{"ok":true,"data":{"key":"%s","enabled":true,"definition_digest":"ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"}}\n' "$key"
            exit 0
        fi
        if [ "$digest" = null ]; then digest_json=null; else digest_json="\"$digest\""; fi
        printf '{"ok":true,"data":{"key":"%s","enabled":%s,"definition_digest":%s}}\n' "$key" "$enabled" "$digest_json"
        exit 0
    fi
    if [ "$operation" = disable ]; then
        digest=null
        [ ! -f "$binding" ] || { IFS='|' read -r _ digest <"$binding"; }
        if [ "${5:-}" = --select ]; then digest=$6; fi
        printf 'false|%s\n' "$digest" >"$binding"
        if [ "${KRISIS_FAIL_AFTER_DISABLE_KEY:-}" = "$key" ]; then
            printf '%s\n' '{"code":"injected_post_disable_failure"}' >&2
            exit 1
        fi
        printf '%s\n' '{"ok":true,"data":{}}'
        exit 0
    fi
    if [ "$operation" = switch ]; then
        digest=$5
        if [ "$key" = krisis/observer ]; then
            for legacy in decisions_observer decisions_daily-email; do
                legacy_binding="$state/bindings/$legacy.binding"
                [ ! -f "$legacy_binding" ] || { IFS='|' read -r legacy_enabled _ <"$legacy_binding"; [ "$legacy_enabled" != true ] || { printf '%s\n' '{"code":"joint_observers"}' >&2; exit 1; }; }
            done
        fi
        if [ "$key" = decisions/observer ]; then
            active_binding="$state/bindings/krisis_observer.binding"
            [ ! -f "$active_binding" ] || { IFS='|' read -r active_enabled _ <"$active_binding"; [ "$active_enabled" != true ] || { printf '%s\n' '{"code":"joint_observers"}' >&2; exit 1; }; }
        fi
        printf 'true|%s\n' "$digest" >"$binding"
        if [ "${KRISIS_FAIL_VERIFY_SWITCH:-0}" -eq 1 ] && [ "$key" = krisis/observer ]; then : >"$state/fail-next-show"; fi
        printf '%s\n' '{"ok":true,"data":{}}'
        exit 0
    fi
fi
printf '%s\n' '{"code":"unsupported"}' >&2
exit 1
EOF

cat >"$annals" <<'EOF'
#!/bin/sh
exit 0
EOF
cat >"$launchctl" <<'EOF'
#!/bin/sh
printf '%s\n' "$*" >>"$KRISIS_LAUNCHCTL_CAPTURE"
case "$1" in print) exit 1 ;; *) exit 0 ;; esac
EOF
cat >"$home/.local/bin/codex" <<'EOF'
#!/bin/sh
exit 0
EOF
printf '%s\n' '[decision_feed]' 'expected_library_id = "0123456789abcdef0123456789abcdef"' >"$config"
chmod 0755 "$candidate" "$clockwork" "$annals" "$launchctl" "$home/.local/bin/codex"

deploy_for() {
    test_home=$1
    test_clockwork_state=$2
    shift 2
    HOME="$test_home" KRISIS_CLOCKWORK_CAPTURE="$clockwork_capture" \
        KRISIS_CLOCKWORK_STATE="$test_clockwork_state" \
        KRISIS_LAUNCHCTL_CAPTURE="$launchctl_capture" \
        KRISIS_CANDIDATE_CAPTURE="$candidate_capture" \
        /bin/sh "$SCRIPT_DIR/deploy-user.sh" \
        --binary "$candidate" --clockwork "$clockwork" --annals "$annals" \
        --annals-config "$config" --annals-library-id 0123456789abcdef0123456789abcdef \
        --home "$test_home" --launchctl "$launchctl" "$@"
}

deploy() {
    deploy_for "$home" "$clockwork_state" "$@"
}

# A valid but unrelated same-user gate is not adopted, rewritten, or removed.
unrelated_home="$temporary/UnrelatedHome"
unrelated_clockwork_state="$temporary/unrelated-clockwork-state"
unrelated_state="$unrelated_home/Library/Application Support/Decisions"
mkdir -p "$unrelated_state" "$unrelated_clockwork_state/definitions" \
    "$unrelated_clockwork_state/bindings"
printf '%s\n' 'operator-owned maintenance' \
    >"$unrelated_state/.clockwork-maintenance"
chmod 0600 "$unrelated_state/.clockwork-maintenance"
unrelated_capture_lines=$(wc -l <"$clockwork_capture")
if deploy_for "$unrelated_home" "$unrelated_clockwork_state" \
    >"$temporary/unrelated-gate.out" 2>"$temporary/unrelated-gate.err"
then
    printf '%s\n' 'deployer adopted an unrelated pre-existing maintenance gate' >&2
    exit 1
fi
grep -F 'has no Krisis hold receipt' "$temporary/unrelated-gate.err" >/dev/null
grep -Fx 'operator-owned maintenance' \
    "$unrelated_state/.clockwork-maintenance" >/dev/null
[ ! -e "$unrelated_state/install/krisis-maintenance-hold.txt" ]
[ "$(wc -l <"$clockwork_capture")" -eq "$unrelated_capture_lines" ]

deploy >"$temporary/prepare.out"
grep -F "prepared krisis $krisis_version" "$temporary/prepare.out" >/dev/null
[ -f "$home/Library/Application Support/Decisions/.clockwork-maintenance" ]
[ -f "$home/Library/Application Support/Decisions/install/krisis-maintenance-hold.txt" ]
[ ! -e "$home/Library/Application Support/Decisions/install/current" ]
[ ! -e "$home/.local/bin/krisis" ]
[ ! -e "$home/.codex/hooks.json" ]
if grep -E 'binding (disable|switch)' "$clockwork_capture" >/dev/null; then
    printf '%s\n' 'prepare mode mutated a Clockwork binding' >&2
    exit 1
fi
grep -F 'definition show' "$clockwork_capture" >/dev/null

prepared_release_id=$(sed -n '3s/^release_id=//p' \
    "$home/Library/Application Support/Decisions/install/krisis-maintenance-hold.txt")
prepared_release="$home/Library/Application Support/Decisions/install/releases/$prepared_release_id"
[ -x "$prepared_release/package/krisis" ]
[ -x "$prepared_release/package/krisis-observer" ]
cmp -s "$prepared_release/bin/krisis" "$prepared_release/package/krisis"
cmp -s "$prepared_release/bin/krisis-observer" \
    "$prepared_release/package/krisis-observer"

installed_deploy() {
    HOME="$home" KRISIS_CLOCKWORK_CAPTURE="$clockwork_capture" \
        KRISIS_CLOCKWORK_STATE="$clockwork_state" \
        KRISIS_LAUNCHCTL_CAPTURE="$launchctl_capture" \
        KRISIS_CANDIDATE_CAPTURE="$candidate_capture" \
        /bin/sh "$prepared_release/package/deploy-user.sh" \
        --binary "$candidate" --clockwork "$clockwork" --annals "$annals" \
        --annals-config "$config" \
        --annals-library-id 0123456789abcdef0123456789abcdef \
        --home "$home" --launchctl "$launchctl" "$@"
}

installed_deploy >"$temporary/installed-prepare.out"
grep -F "prepared krisis $krisis_version" \
    "$temporary/installed-prepare.out" >/dev/null

KRISIS_TEST_LEAK_ME=forbidden installed_deploy \
    --final-cutover --keep-maintenance \
    >"$temporary/deploy.out"
grep -F "installed krisis $krisis_version" "$temporary/deploy.out" >/dev/null
grep -F 'authenticated maintenance hold retained' "$temporary/deploy.out" >/dev/null
[ -L "$home/.local/bin/krisis" ]
[ ! -e "$home/.local/bin/decisions" ]
[ -L "$home/Library/Application Support/Chancery/providers/krisis" ]
[ -L "$home/Library/Application Support/Chancery/providers/decisions" ]
[ -f "$home/Library/Application Support/Decisions/.clockwork-maintenance" ]
[ -f "$home/Library/Application Support/Decisions/install/krisis-maintenance-hold.txt" ]
cmp -s "$home/.codex/hooks.json" "$SCRIPT_DIR/hooks.json"
active_binding="$clockwork_state/bindings/krisis_observer.binding"
IFS='|' read -r active_enabled first_digest <"$active_binding"
[ "$active_enabled" = true ]
first_current=$(readlink "$home/Library/Application Support/Decisions/install/current")
first_release="$home/Library/Application Support/Decisions/install/$first_current"

# Releasing a committed handoff is its own authenticated, idempotent operation.
# A release failure keeps the exact hold, and the same operation resumes safely.
state="$home/Library/Application Support/Decisions"
if KRISIS_BLOCK_GATE_RELEASE_DIR="$state" installed_deploy --release-maintenance \
    >"$temporary/release-blocked.out" 2>"$temporary/release-blocked.err"
then
    printf '%s\n' 'maintenance release succeeded after its gate became undeletable' >&2
    exit 1
fi
unset KRISIS_BLOCK_GATE_RELEASE_DIR
chmod 0700 "$state"
grep -F 'could not release its authenticated maintenance hold' \
    "$temporary/release-blocked.err" >/dev/null
[ -f "$state/.clockwork-maintenance" ]
[ -f "$state/install/krisis-maintenance-hold.txt" ]
installed_deploy --release-maintenance >"$temporary/released.out"
grep -F 'released authenticated Krisis maintenance hold' \
    "$temporary/released.out" >/dev/null
[ ! -e "$state/.clockwork-maintenance" ]
[ -f "$state/install/krisis-maintenance-hold.txt" ]
release_lines=$(wc -l <"$clockwork_capture")
installed_deploy --release-maintenance >"$temporary/released-again.out"
grep -F 'released authenticated Krisis maintenance hold' \
    "$temporary/released-again.out" >/dev/null
[ ! -e "$state/.clockwork-maintenance" ]
tail -n "+$((release_lines + 1))" "$clockwork_capture" \
    | grep -E 'binding (disable|switch)' >/dev/null && {
        printf '%s\n' 'idempotent maintenance release mutated a binding' >&2
        exit 1
    }

# Existing content-addressed releases are closed regular-file trees. A
# byte-identical external symlink or an uncommitted extra path is rejected by
# both deploy and uninstall before either can touch a binding.
external_runner="$temporary/external-observer-runner"
cp "$first_release/bin/krisis-observer" "$external_runner"
rm -f "$first_release/bin/krisis-observer"
ln -s "$external_runner" "$first_release/bin/krisis-observer"
release_audit_lines=$(wc -l <"$clockwork_capture")
if deploy --final-cutover >"$temporary/symlink-deploy.out" 2>"$temporary/symlink-deploy.err"; then
    printf '%s\n' 'deployer accepted a symlinked release member' >&2
    exit 1
fi
if HOME="$home" KRISIS_CLOCKWORK_CAPTURE="$clockwork_capture" KRISIS_CLOCKWORK_STATE="$clockwork_state" \
    /bin/sh "$SCRIPT_DIR/uninstall-user.sh" --clockwork "$clockwork" --home "$home" \
    >"$temporary/symlink-uninstall.out" 2>"$temporary/symlink-uninstall.err"; then
    printf '%s\n' 'uninstaller accepted a symlinked release member' >&2
    exit 1
fi
rm -f "$first_release/bin/krisis-observer"
install -m 0755 "$external_runner" "$first_release/bin/krisis-observer"

# The release-owned deployer depends on its packaged runner sibling. Both the
# deployer and uninstaller reject a changed copy even when bin/ remains intact.
packaged_runner_backup="$temporary/packaged-observer-runner"
cp "$first_release/package/krisis-observer" "$packaged_runner_backup"
printf '%s\n' 'tampered packaged runner' \
    >>"$first_release/package/krisis-observer"
if deploy --final-cutover \
    >"$temporary/package-runner-deploy.out" \
    2>"$temporary/package-runner-deploy.err"
then
    printf '%s\n' 'deployer accepted a changed packaged runner' >&2
    exit 1
fi
if HOME="$home" KRISIS_CLOCKWORK_CAPTURE="$clockwork_capture" \
    KRISIS_CLOCKWORK_STATE="$clockwork_state" \
    /bin/sh "$SCRIPT_DIR/uninstall-user.sh" --clockwork "$clockwork" \
    --home "$home" >"$temporary/package-runner-uninstall.out" \
    2>"$temporary/package-runner-uninstall.err"
then
    printf '%s\n' 'uninstaller accepted a changed packaged runner' >&2
    exit 1
fi
install -m 0755 "$packaged_runner_backup" \
    "$first_release/package/krisis-observer"

printf '%s\n' 'unexpected' >"$first_release/package/unexpected"
if deploy --final-cutover >"$temporary/extra-deploy.out" 2>"$temporary/extra-deploy.err"; then
    printf '%s\n' 'deployer accepted an uncommitted release member' >&2
    exit 1
fi
if HOME="$home" KRISIS_CLOCKWORK_CAPTURE="$clockwork_capture" KRISIS_CLOCKWORK_STATE="$clockwork_state" \
    /bin/sh "$SCRIPT_DIR/uninstall-user.sh" --clockwork "$clockwork" --home "$home" \
    >"$temporary/extra-uninstall.out" 2>"$temporary/extra-uninstall.err"; then
    printf '%s\n' 'uninstaller accepted an uncommitted release member' >&2
    exit 1
fi
rm -f "$first_release/package/unexpected"
tail -n "+$((release_audit_lines + 1))" "$clockwork_capture" | grep -E 'binding (disable|switch)' >/dev/null && {
    printf '%s\n' 'release-tree rejection happened after binding mutation' >&2
    exit 1
}

active_definition="$clockwork_state/definitions/$first_digest.toml"
cp "$active_definition" "$temporary/active-definition.backup"
sed 's/0123456789abcdef0123456789abcdef/fedcba9876543210fedcba9876543210/' \
    "$active_definition" >"$temporary/retargeted-definition.toml"
mv "$temporary/retargeted-definition.toml" "$active_definition"
retarget_lines=$(wc -l <"$clockwork_capture")
if HOME="$home" KRISIS_CLOCKWORK_CAPTURE="$clockwork_capture" KRISIS_CLOCKWORK_STATE="$clockwork_state" \
    /bin/sh "$SCRIPT_DIR/uninstall-user.sh" --clockwork "$clockwork" --home "$home" \
    >"$temporary/retarget-uninstall.out" 2>"$temporary/retarget-uninstall.err"; then
    printf '%s\n' 'uninstaller accepted a retargeted observer definition' >&2
    exit 1
fi
[ "$(cat "$active_binding")" = "true|$first_digest" ]
tail -n "+$((retarget_lines + 1))" "$clockwork_capture" | grep -E 'binding (disable|switch)' >/dev/null && {
    printf '%s\n' 'retargeted definition rejection happened after binding mutation' >&2
    exit 1
}
mv "$temporary/active-definition.backup" "$active_definition"

database="$home/Library/Application Support/Decisions/decisions.db"
database_alias="$temporary/decisions-db-alias"
ln "$database" "$database_alias"
database_audit_lines=$(wc -l <"$clockwork_capture")
if deploy --final-cutover >"$temporary/hardlink-db.out" 2>"$temporary/hardlink-db.err"; then
    printf '%s\n' 'deployer accepted a hard-linked database' >&2
    exit 1
fi
rm -f "$database_alias"
tail -n "+$((database_audit_lines + 1))" "$clockwork_capture" | grep -E 'binding (disable|switch)' >/dev/null && {
    printf '%s\n' 'database rejection happened after binding mutation' >&2
    exit 1
}

# The prior-active matrix starts with absent above and enabled here. A failure
# immediately after touching the enabled key restores its exact selection.
if KRISIS_FAIL_AFTER_DISABLE_KEY=krisis/observer deploy --final-cutover \
    >"$temporary/fail-active-disable.out" 2>"$temporary/fail-active-disable.err"; then
    printf '%s\n' 'deployer ignored a failure after disabling the active key' >&2
    exit 1
fi
unset KRISIS_FAIL_AFTER_DISABLE_KEY
[ "$(cat "$active_binding")" = "true|$first_digest" ]
[ "$(readlink "$home/Library/Application Support/Decisions/install/current")" = "$first_current" ]
[ ! -e "$home/Library/Application Support/Decisions/.clockwork-maintenance" ]

# A foreign legacy binding is observed and refused without mutation.
foreign_digest=cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc
printf 'true|%s\n' "$foreign_digest" >"$clockwork_state/bindings/decisions_observer.binding"
capture_lines=$(wc -l <"$clockwork_capture")
if deploy --final-cutover >"$temporary/foreign.out" 2>"$temporary/foreign.err"; then
    printf '%s\n' 'deployer accepted an enabled foreign legacy binding' >&2
    exit 1
fi
[ "$(cat "$clockwork_state/bindings/decisions_observer.binding")" = "true|$foreign_digest" ]
tail -n "+$((capture_lines + 1))" "$clockwork_capture" | grep -F 'binding disable decisions/observer' >/dev/null && {
    printf '%s\n' 'deployer mutated a foreign legacy binding' >&2
    exit 1
}
printf 'false|%s\n' "$foreign_digest" >"$clockwork_state/bindings/decisions_observer.binding"

# A foreign legacy plist is rejected before launchctl or binding mutation.
plist="$home/Library/LaunchAgents/org.decisions.observer.plist"
printf '%s\n' '<plist>foreign</plist>' >"$plist"
clockwork_before=$(wc -l <"$clockwork_capture")
launchctl_before=$(wc -l <"$launchctl_capture")
if deploy --final-cutover >"$temporary/plist.out" 2>"$temporary/plist.err"; then
    printf '%s\n' 'deployer accepted a foreign legacy plist' >&2
    exit 1
fi
[ "$(cat "$plist")" = '<plist>foreign</plist>' ]
[ "$(wc -l <"$launchctl_capture")" -eq "$launchctl_before" ]
tail -n "+$((clockwork_before + 1))" "$clockwork_capture" | grep -E 'binding (disable|switch)' >/dev/null && {
    printf '%s\n' 'deployer mutated bindings before rejecting a foreign plist' >&2
    exit 1
}
rm -f "$plist"

# A format-3 release supplies exact ownership evidence for both legacy
# definitions. Faults immediately after each disable must restore every key
# touched so far, and a complete handoff must never enable both observers.
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

legacy_home="$temporary/LegacyHome"
legacy_clockwork_state="$temporary/legacy-clockwork-state"
legacy_state="$legacy_home/Library/Application Support/Decisions"
legacy_install="$legacy_state/install"
legacy_stage="$legacy_install/release-stage"
legacy_logs="$legacy_home/Library/Logs/Decisions"
mkdir -p "$legacy_home/.local/bin" "$legacy_clockwork_state/definitions" \
    "$legacy_clockwork_state/bindings" "$legacy_install/releases" "$legacy_logs" \
    "$legacy_stage/bin" "$legacy_stage/libexec" "$legacy_stage/package" \
    "$legacy_stage/share/chancery/decisions"
for legacy_executable in decisions decisions-daily-email decisions-observer; do
    printf '%s\n' '#!/bin/sh' 'exit 0' >"$legacy_stage/bin/$legacy_executable"
    chmod 0755 "$legacy_stage/bin/$legacy_executable"
done
printf '%s\n' '#!/bin/sh' 'exit 0' >"$legacy_stage/libexec/decisions"
printf '%s\n' '#!/bin/sh' 'exit 0' >"$legacy_stage/package/deploy-user.sh"
printf '%s\n' '#!/bin/sh' 'exit 0' >"$legacy_stage/package/uninstall-user.sh"
cp "$legacy_stage/bin/decisions" "$legacy_stage/package/decisions"
cp "$legacy_stage/bin/decisions-daily-email" \
    "$legacy_stage/package/decisions-daily-email"
cp "$legacy_stage/bin/decisions-observer" \
    "$legacy_stage/package/decisions-observer"
chmod 0755 "$legacy_stage/libexec/decisions" "$legacy_stage/package/deploy-user.sh" \
    "$legacy_stage/package/uninstall-user.sh" "$legacy_stage/package/decisions" \
    "$legacy_stage/package/decisions-daily-email" \
    "$legacy_stage/package/decisions-observer"
printf '%s\n' 'legacy daily definition template' \
    >"$legacy_stage/package/decisions-daily-email.clockwork.toml.in"
printf '%s\n' 'legacy observer definition template' \
    >"$legacy_stage/package/decisions-observer.clockwork.toml.in"
printf '%s\n' '{}' >"$legacy_stage/package/hooks.json"
printf '%s\n' '{}' >"$legacy_stage/share/chancery/decisions/provider.json"

legacy_binary_hash=$(shasum -a 256 "$legacy_stage/libexec/decisions" | awk '{print $1}')
legacy_frontend_hash=$(shasum -a 256 "$legacy_stage/bin/decisions" | awk '{print $1}')
legacy_daily_runner_hash=$(shasum -a 256 "$legacy_stage/bin/decisions-daily-email" | awk '{print $1}')
legacy_observer_runner_hash=$(shasum -a 256 "$legacy_stage/bin/decisions-observer" | awk '{print $1}')
legacy_daily_definition_hash=$(shasum -a 256 "$legacy_stage/package/decisions-daily-email.clockwork.toml.in" | awk '{print $1}')
legacy_observer_definition_hash=$(shasum -a 256 "$legacy_stage/package/decisions-observer.clockwork.toml.in" | awk '{print $1}')
legacy_hooks_hash=$(shasum -a 256 "$legacy_stage/package/hooks.json" | awk '{print $1}')
legacy_deployer_hash=$(shasum -a 256 "$legacy_stage/package/deploy-user.sh" | awk '{print $1}')
legacy_uninstaller_hash=$(shasum -a 256 "$legacy_stage/package/uninstall-user.sh" | awk '{print $1}')
legacy_provider_hash=$(fixture_bundle_hash "$legacy_stage/share/chancery/decisions")
legacy_release_id=$(printf '%s\n' "$legacy_binary_hash" "$legacy_frontend_hash" \
    "$legacy_daily_runner_hash" "$legacy_observer_runner_hash" \
    "$legacy_daily_definition_hash" "$legacy_observer_definition_hash" \
    "$legacy_hooks_hash" "$legacy_deployer_hash" "$legacy_uninstaller_hash" \
    "$legacy_provider_hash" | shasum -a 256 | awk '{print $1}')
{
    printf '%s\n' 'format=3'
    printf 'release_id=%s\n' "$legacy_release_id"
    printf '%s\n' 'version=0.3.4'
    printf 'binary_sha256=%s\n' "$legacy_binary_hash"
    printf 'frontend_sha256=%s\n' "$legacy_frontend_hash"
    printf 'daily_runner_sha256=%s\n' "$legacy_daily_runner_hash"
    printf 'observer_runner_sha256=%s\n' "$legacy_observer_runner_hash"
    printf 'daily_clockwork_definition_sha256=%s\n' "$legacy_daily_definition_hash"
    printf 'observer_clockwork_definition_sha256=%s\n' "$legacy_observer_definition_hash"
    printf 'hooks_sha256=%s\n' "$legacy_hooks_hash"
    printf 'deployer_sha256=%s\n' "$legacy_deployer_hash"
    printf 'uninstaller_sha256=%s\n' "$legacy_uninstaller_hash"
    printf 'chancery_sha256=%s\n' "$legacy_provider_hash"
} >"$legacy_stage/manifest.txt"
chmod 0444 "$legacy_stage/manifest.txt"
legacy_release="$legacy_install/releases/$legacy_release_id"
mv "$legacy_stage" "$legacy_release"
ln -s "releases/$legacy_release_id" "$legacy_install/current"
printf '%s\n' '#!/bin/sh' 'exit 0' >"$legacy_home/.local/bin/codex"
chmod 0755 "$legacy_home/.local/bin/codex"

legacy_interpreter_hash=$(shasum -a 256 /bin/sh | awk '{print $1}')
legacy_observer_definition="$temporary/legacy-observer.toml"
cat >"$legacy_observer_definition" <<EOF
schema_version = 1
key = "decisions/observer"
release_id = "$legacy_release_id"
release_root = "$legacy_release"
authority = "current-user-background"
overlap = "skip"
arguments = []
cwd = "$legacy_state"
[schedule]
kind = "interval"
seconds = 60
run_at_load = false
[launch]
kind = "interpreted"
interpreter = "/bin/sh"
interpreter_sha256 = "$legacy_interpreter_hash"
script = "$legacy_release/bin/decisions-observer"
script_sha256 = "$legacy_observer_runner_hash"
[environment]
HOME = "$legacy_home"
[output]
stdout = "$legacy_logs/observer.stdout.log"
stderr = "$legacy_logs/observer.stderr.log"
EOF
legacy_daily_definition="$temporary/legacy-daily.toml"
cat >"$legacy_daily_definition" <<EOF
schema_version = 1
key = "decisions/daily-email"
release_id = "$legacy_release_id"
release_root = "$legacy_release"
authority = "current-user-background"
overlap = "skip"
arguments = []
cwd = "$legacy_state"
[schedule]
kind = "local-calendar"
hour = 9
minute = 0
run_at_load = false
[launch]
kind = "interpreted"
interpreter = "/bin/sh"
interpreter_sha256 = "$legacy_interpreter_hash"
script = "$legacy_release/bin/decisions-daily-email"
script_sha256 = "$legacy_daily_runner_hash"
[environment]
HOME = "$legacy_home"
[output]
stdout = "$legacy_logs/daily-email.stdout.log"
stderr = "$legacy_logs/daily-email.stderr.log"
EOF
legacy_observer_digest=$(shasum -a 256 "$legacy_observer_definition" | awk '{print $1}')
legacy_daily_digest=$(shasum -a 256 "$legacy_daily_definition" | awk '{print $1}')
install -m 0600 "$legacy_observer_definition" \
    "$legacy_clockwork_state/definitions/$legacy_observer_digest.toml"
install -m 0600 "$legacy_daily_definition" \
    "$legacy_clockwork_state/definitions/$legacy_daily_digest.toml"
legacy_observer_binding="$legacy_clockwork_state/bindings/decisions_observer.binding"
legacy_daily_binding="$legacy_clockwork_state/bindings/decisions_daily-email.binding"
legacy_active_binding="$legacy_clockwork_state/bindings/krisis_observer.binding"
printf 'true|%s\n' "$legacy_observer_digest" >"$legacy_observer_binding"
printf 'true|%s\n' "$legacy_daily_digest" >"$legacy_daily_binding"

if KRISIS_FAIL_AFTER_DISABLE_KEY=decisions/observer \
    deploy_for "$legacy_home" "$legacy_clockwork_state" --final-cutover \
    >"$temporary/fail-legacy-observer.out" 2>"$temporary/fail-legacy-observer.err"; then
    printf '%s\n' 'deployer ignored a failure after disabling the legacy observer' >&2
    exit 1
fi
unset KRISIS_FAIL_AFTER_DISABLE_KEY
[ "$(cat "$legacy_observer_binding")" = "true|$legacy_observer_digest" ]
[ "$(cat "$legacy_daily_binding")" = "true|$legacy_daily_digest" ]
[ ! -e "$legacy_active_binding" ]
[ ! -e "$legacy_state/.clockwork-maintenance" ]

if KRISIS_FAIL_AFTER_DISABLE_KEY=decisions/daily-email \
    deploy_for "$legacy_home" "$legacy_clockwork_state" --final-cutover \
    >"$temporary/fail-legacy-daily.out" 2>"$temporary/fail-legacy-daily.err"; then
    printf '%s\n' 'deployer ignored a failure after disabling the legacy daily key' >&2
    exit 1
fi
unset KRISIS_FAIL_AFTER_DISABLE_KEY
[ "$(cat "$legacy_observer_binding")" = "true|$legacy_observer_digest" ]
[ "$(cat "$legacy_daily_binding")" = "true|$legacy_daily_digest" ]
[ ! -e "$legacy_active_binding" ]
[ ! -e "$legacy_state/.clockwork-maintenance" ]

deploy_for "$legacy_home" "$legacy_clockwork_state" --final-cutover \
    >"$temporary/legacy-cutover.out"
IFS='|' read -r legacy_active_enabled _ <"$legacy_active_binding"
[ "$legacy_active_enabled" = true ]
[ "$(cat "$legacy_observer_binding")" = "false|$legacy_observer_digest" ]
[ "$(cat "$legacy_daily_binding")" = "false|$legacy_daily_digest" ]

# A failure after selecting a new candidate restores the exact prior digest,
# current selector, hook, and enabled state while retaining maintenance.
# Krisis 0.4.0 and 0.4.1 did not stage release-owned executable siblings.
# A newer deployer accepts that exact historical omission while preparing the
# successor, then all newly staged releases carry the authenticated copies.
first_manifest_backup="$temporary/first-manifest.backup"
cp "$first_release/manifest.txt" "$first_manifest_backup"
awk -v current="$krisis_version" '
    $0 == "version=" current {
        print "version=0.4.1"
        changed = 1
        next
    }
    { print }
    END { if (changed != 1) exit 1 }
' "$first_manifest_backup" \
    >"$temporary/first-manifest.legacy"
install -m 0444 "$temporary/first-manifest.legacy" \
    "$first_release/manifest.txt"
rm -f "$first_release/package/krisis" \
    "$first_release/package/krisis-observer"
printf '%s\n' '# second candidate bytes' >>"$candidate"
deploy >"$temporary/prepare-second.out"
install -m 0444 "$first_manifest_backup" "$first_release/manifest.txt"
install -m 0755 "$first_release/bin/krisis" \
    "$first_release/package/krisis"
install -m 0755 "$first_release/bin/krisis-observer" \
    "$first_release/package/krisis-observer"
second_digest=$(awk '{print $NF}' "$temporary/prepare-second.out")
[ "$second_digest" != "$first_digest" ]
if KRISIS_FAIL_VERIFY_SWITCH=1 deploy --final-cutover >"$temporary/fail-switch.out" 2>"$temporary/fail-switch.err"; then
    printf '%s\n' 'deployer ignored a failed post-switch verification' >&2
    exit 1
fi
[ "$(cat "$active_binding")" = "true|$first_digest" ]
[ "$(readlink "$home/Library/Application Support/Decisions/install/current")" = "$first_current" ]
cmp -s "$home/.codex/hooks.json" "$SCRIPT_DIR/hooks.json"
[ -f "$home/Library/Application Support/Decisions/.clockwork-maintenance" ]

# A disabled selected prior definition is restored byte-for-byte and remains
# disabled when verification fails after the candidate switch.
printf 'false|%s\n' "$first_digest" >"$active_binding"
if KRISIS_FAIL_VERIFY_SWITCH=1 deploy --final-cutover >"$temporary/fail-disabled-selected.out" 2>"$temporary/fail-disabled-selected.err"; then
    printf '%s\n' 'deployer ignored disabled-selected rollback failure' >&2
    exit 1
fi
[ "$(cat "$active_binding")" = "false|$first_digest" ]
[ "$(readlink "$home/Library/Application Support/Decisions/install/current")" = "$first_current" ]
printf 'true|%s\n' "$first_digest" >"$active_binding"

# Uninstall refuses an enabled foreign legacy key, then touches only the owned
# active key once the foreign key is disabled.
printf 'true|%s\n' "$foreign_digest" >"$clockwork_state/bindings/decisions_observer.binding"
uninstall_lines=$(wc -l <"$clockwork_capture")
if HOME="$home" KRISIS_CLOCKWORK_CAPTURE="$clockwork_capture" KRISIS_CLOCKWORK_STATE="$clockwork_state" \
    /bin/sh "$SCRIPT_DIR/uninstall-user.sh" --clockwork "$clockwork" --home "$home" \
    >"$temporary/uninstall-foreign.out" 2>"$temporary/uninstall-foreign.err"; then
    printf '%s\n' 'uninstaller accepted an enabled foreign legacy binding' >&2
    exit 1
fi
[ "$(cat "$clockwork_state/bindings/decisions_observer.binding")" = "true|$foreign_digest" ]
tail -n "+$((uninstall_lines + 1))" "$clockwork_capture" | grep -F 'binding disable decisions/observer' >/dev/null && exit 1
printf 'false|%s\n' "$foreign_digest" >"$clockwork_state/bindings/decisions_observer.binding"
HOME="$home" KRISIS_CLOCKWORK_CAPTURE="$clockwork_capture" KRISIS_CLOCKWORK_STATE="$clockwork_state" \
    /bin/sh "$SCRIPT_DIR/uninstall-user.sh" --clockwork "$clockwork" --home "$home" \
    >"$temporary/uninstall.out"
grep -F 'uninstalled Krisis public surfaces' "$temporary/uninstall.out" >/dev/null
[ "$(cat "$active_binding")" = "false|$first_digest" ]
[ ! -e "$home/.local/bin/krisis" ]
[ ! -e "$home/.codex/hooks.json" ]
[ ! -e "$home/Library/Application Support/Chancery/providers/krisis" ]
[ ! -e "$home/Library/Application Support/Chancery/providers/decisions" ]
[ -f "$home/Library/Application Support/Decisions/.clockwork-maintenance" ]

# Clockwork cannot restore a selected definition to null. A post-switch
# failure from that prior state therefore disables the owned candidate and
# retains explicit maintenance recovery evidence instead of claiming rollback.
printf '%s\n' 'false|null' >"$active_binding"
if KRISIS_FAIL_VERIFY_SWITCH=1 deploy --final-cutover >"$temporary/fail-null.out" 2>"$temporary/fail-null.err"; then
    printf '%s\n' 'deployer ignored null-selector recovery failure' >&2
    exit 1
fi
grep -F 'maintenance recovery is retained' "$temporary/fail-null.err" >/dev/null
[ "$(cat "$active_binding")" = "false|$second_digest" ]
[ -f "$home/Library/Application Support/Decisions/.clockwork-maintenance" ]

bad_home="$temporary/BadHome"
mkdir "$bad_home"
if HOME="$bad_home" KRISIS_CLOCKWORK_CAPTURE="$clockwork_capture" KRISIS_CLOCKWORK_STATE="$clockwork_state" \
    /bin/sh "$SCRIPT_DIR/deploy-user.sh" --binary "$candidate" --clockwork "$clockwork" \
    --annals "$annals" --annals-config "$config" \
    --annals-library-id ABCDEF0123456789abcdef0123456789 \
    --home "$bad_home" --launchctl "$launchctl" >/dev/null 2>"$temporary/bad.err"; then
    printf '%s\n' 'deployer accepted an uppercase Annals library ID' >&2
    exit 1
fi
grep -F '32 lowercase hexadecimal' "$temporary/bad.err" >/dev/null
printf '%s\n' 'Krisis guarded deployer test passed'
