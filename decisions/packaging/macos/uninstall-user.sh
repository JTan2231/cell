#!/bin/sh

set -eu

PATH=/usr/bin:/bin:/usr/sbin:/sbin
export PATH
umask 077

ACTIVE_CLOCKWORK_KEY=krisis/observer
LEGACY_OBSERVER_CLOCKWORK_KEY=decisions/observer
LEGACY_DAILY_CLOCKWORK_KEY=decisions/daily-email
clockwork_path=
install_home=${HOME:-}

fail() {
    printf 'krisis user uninstall: %s\n' "$*" >&2
    exit 1
}

usage() {
    printf '%s\n' 'Usage: uninstall-user.sh --clockwork ABSOLUTE_PATH [--home ABSOLUTE_PATH]'
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --clockwork) [ "$#" -ge 2 ] || fail '--clockwork requires a path'; clockwork_path=$2; shift 2 ;;
        --home) [ "$#" -ge 2 ] || fail '--home requires a path'; install_home=$2; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *) fail "unknown argument: $1" ;;
    esac
done
case "$clockwork_path" in /*) ;; *) fail 'clockwork path must be absolute' ;; esac
case "$install_home" in /*) ;; *) fail 'home must be absolute' ;; esac
[ -x "$clockwork_path" ] && [ -f "$clockwork_path" ] && [ ! -L "$clockwork_path" ] || fail 'Clockwork executable is unavailable'
[ -d "$install_home" ] && [ ! -L "$install_home" ] || fail 'home is not a regular directory'
operator_uid=$(id -u)
[ "$operator_uid" -ne 0 ] || fail 'run as the Krisis operator, not root'
[ "$(stat -f '%u' "$install_home")" -eq "$operator_uid" ] || fail 'home is not owned by the operator'

STATE_DIR="$install_home/Library/Application Support/Decisions"
INSTALL_DIR="$STATE_DIR/install"
CURRENT_LINK="$INSTALL_DIR/current"
LOCK_DIR="$INSTALL_DIR/.update-lock"
BINDING_RECEIPT="$INSTALL_DIR/krisis-observer-binding.txt"
MAINTENANCE_MARKER="$STATE_DIR/.clockwork-maintenance"
CLI_PATH="$install_home/.local/bin/krisis"
LEGACY_CLI_PATH="$install_home/.local/bin/decisions"
PROVIDER_LINK="$install_home/Library/Application Support/Chancery/providers/krisis"
LEGACY_PROVIDER_LINK="$install_home/Library/Application Support/Chancery/providers/decisions"
HOOKS_PATH="$install_home/.codex/hooks.json"
OBSERVER_PLIST="$install_home/Library/LaunchAgents/org.decisions.observer.plist"
DAILY_PLIST="$install_home/Library/LaunchAgents/org.decisions.daily-email.plist"
LOG_DIR="$install_home/Library/Logs/Decisions"

[ -d "$INSTALL_DIR" ] && [ ! -L "$INSTALL_DIR" ] || fail 'Krisis installation is unavailable'
[ -L "$CURRENT_LINK" ] || fail 'current release selector is unavailable'
current=$(readlink "$CURRENT_LINK")
printf '%s\n' "$current" | grep -Eq '^releases/[0-9a-f]{64}$' || fail 'current release selector is foreign'
release="$INSTALL_DIR/$current"
manifest="$release/manifest.txt"
[ -d "$release" ] && [ ! -L "$release" ] && [ -f "$manifest" ] && [ ! -L "$manifest" ] || fail 'current release is unsafe'
[ "$(sed -n '1s/^format=//p' "$manifest")" = 4 ] && [ "$(awk 'END {print NR}' "$manifest")" -eq 12 ] || fail 'current release is not a canonical Krisis release'
release_id=${current#releases/}
[ "$(sed -n '2s/^release_id=//p' "$manifest")" = "$release_id" ] || fail 'current release manifest identity differs'
binary_hash=$(sed -n '4s/^binary_sha256=//p' "$manifest")
frontend_hash=$(sed -n '5s/^frontend_sha256=//p' "$manifest")
runner_hash=$(sed -n '6s/^observer_runner_sha256=//p' "$manifest")
definition_hash=$(sed -n '7s/^observer_clockwork_definition_sha256=//p' "$manifest")
hooks_hash=$(sed -n '8s/^hooks_sha256=//p' "$manifest")
deployer_hash=$(sed -n '9s/^deployer_sha256=//p' "$manifest")
uninstaller_hash=$(sed -n '10s/^uninstaller_sha256=//p' "$manifest")
provider_hash=$(sed -n '11s/^krisis_chancery_sha256=//p' "$manifest")
legacy_provider_hash=$(sed -n '12s/^decisions_chancery_sha256=//p' "$manifest")
printf '%s\n' "$binary_hash" "$frontend_hash" "$runner_hash" "$definition_hash" "$hooks_hash" "$deployer_hash" "$uninstaller_hash" "$provider_hash" "$legacy_provider_hash" \
    | grep -Eqv '^[0-9a-f]{64}$' && fail 'current release manifest digest is invalid'
bundle_hash() {
    bundle=$1
    (
        cd "$bundle"
        find . -type f -print | LC_ALL=C sort | while IFS= read -r file; do
            printf 'path=%s\n' "$file"
            shasum -a 256 "$file"
        done
    ) | shasum -a 256 | awk '{print $1}'
}
validate_bundle() {
    bundle=$1
    [ -d "$bundle" ] && [ ! -L "$bundle" ] || fail "provider bundle is not a regular directory: $bundle"
    [ -f "$bundle/provider.json" ] && [ ! -L "$bundle/provider.json" ] || fail "provider manifest is unavailable: $bundle"
}
validate_release_tree() {
    if find "$release" -type l -print | grep -q .; then fail 'current release contains a symbolic link'; fi
    if find "$release" ! -type d ! -type f -print | grep -q .; then fail 'current release contains a non-file entry'; fi
    if find "$release" -type d -empty -print | grep -q .; then fail 'current release contains an uncommitted empty directory'; fi
    if ! find "$release" -type d -o -type f | while IFS= read -r release_entry; do
        [ "$(stat -f '%u' "$release_entry")" -eq "$operator_uid" ] || exit 1
        release_mode=$(stat -f '%Lp' "$release_entry")
        release_other=${release_mode#"${release_mode%?}"}
        release_without_other=${release_mode%?}
        release_group=${release_without_other#"${release_without_other%?}"}
        case "$release_group$release_other" in
            00|01|04|05|10|11|14|15|40|41|44|45|50|51|54|55) ;;
            *) exit 1 ;;
        esac
        [ ! -f "$release_entry" ] || [ "$(stat -f '%l' "$release_entry")" -eq 1 ] || exit 1
    done; then
        fail 'current release is writable outside the operator or contains shared files'
    fi
    if ! find "$release" -type f -print | while IFS= read -r release_file; do
        relative=${release_file#"$release/"}
        case "$relative" in
            manifest.txt|libexec/krisis|bin/krisis|bin/krisis-observer|package/deploy-user.sh|package/uninstall-user.sh|package/krisis-observer.clockwork.toml.in|package/hooks.json|share/chancery/krisis/*|share/chancery/decisions/*) ;;
            *) exit 1 ;;
        esac
    done; then
        fail 'current release contains a path outside its canonical layout'
    fi
    validate_bundle "$release/share/chancery/krisis"
    validate_bundle "$release/share/chancery/decisions"
}
validate_release_tree
[ "$(shasum -a 256 "$release/libexec/krisis" | awk '{print $1}')" = "$binary_hash" ] || fail 'current Krisis binary is tampered'
[ "$(shasum -a 256 "$release/bin/krisis" | awk '{print $1}')" = "$frontend_hash" ] || fail 'current Krisis frontend is tampered'
[ -f "$release/bin/krisis-observer" ] && [ ! -L "$release/bin/krisis-observer" ] \
    && [ "$(shasum -a 256 "$release/bin/krisis-observer" | awk '{print $1}')" = "$runner_hash" ] \
    || fail 'current Krisis runner is not release-owned'
[ "$(shasum -a 256 "$release/package/krisis-observer.clockwork.toml.in" | awk '{print $1}')" = "$definition_hash" ] || fail 'current Krisis definition template is tampered'
[ "$(shasum -a 256 "$release/package/hooks.json" | awk '{print $1}')" = "$hooks_hash" ] || fail 'current Krisis hooks are tampered'
[ "$(shasum -a 256 "$release/package/deploy-user.sh" | awk '{print $1}')" = "$deployer_hash" ] || fail 'current Krisis deployer is tampered'
[ "$(shasum -a 256 "$release/package/uninstall-user.sh" | awk '{print $1}')" = "$uninstaller_hash" ] || fail 'current Krisis uninstaller is tampered'
[ "$(bundle_hash "$release/share/chancery/krisis")" = "$provider_hash" ] || fail 'current Krisis provider is tampered'
[ "$(bundle_hash "$release/share/chancery/decisions")" = "$legacy_provider_hash" ] || fail 'current Decisions compatibility provider is tampered'
actual_release_id=$(printf '%s\n' "$binary_hash" "$frontend_hash" "$runner_hash" "$definition_hash" "$hooks_hash" "$deployer_hash" "$uninstaller_hash" "$provider_hash" "$legacy_provider_hash" | shasum -a 256 | awk '{print $1}')
[ "$actual_release_id" = "$release_id" ] || fail 'current release content identity differs'
[ -x "$release/libexec/krisis" ] && [ ! -L "$release/libexec/krisis" ] || fail 'current release is not Krisis'
[ -f "$BINDING_RECEIPT" ] && [ ! -L "$BINDING_RECEIPT" ] \
    && [ "$(stat -f '%u' "$BINDING_RECEIPT")" -eq "$operator_uid" ] \
    && [ "$(stat -f '%Lp' "$BINDING_RECEIPT")" = 600 ] \
    && [ "$(stat -f '%l' "$BINDING_RECEIPT")" -eq 1 ] \
    && [ "$(awk 'END {print NR}' "$BINDING_RECEIPT")" -eq 6 ] \
    && [ "$(sed -n '1p' "$BINDING_RECEIPT")" = format=1 ] \
    || fail 'installed observer binding receipt is unavailable or invalid'
receipt_release_id=$(sed -n '2s/^release_id=//p' "$BINDING_RECEIPT")
receipt_definition_digest=$(sed -n '3s/^definition_digest=//p' "$BINDING_RECEIPT")
receipt_annals_binary=$(sed -n '4s/^annals_binary=//p' "$BINDING_RECEIPT")
receipt_annals_config=$(sed -n '5s/^annals_config=//p' "$BINDING_RECEIPT")
receipt_annals_library_id=$(sed -n '6s/^annals_library_id=//p' "$BINDING_RECEIPT")
[ "$receipt_release_id" = "$release_id" ] \
    && printf '%s\n' "$receipt_definition_digest" | grep -Eq '^[0-9a-f]{64}$' \
    && [ -n "$receipt_annals_binary" ] && [ -n "$receipt_annals_config" ] \
    || fail 'installed observer binding receipt does not describe the current release'
case "$receipt_annals_library_id" in
    [0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f]) ;;
    *) fail 'installed observer binding receipt has an invalid Annals library ID' ;;
esac

for selector in "$CLI_PATH" "$PROVIDER_LINK" "$LEGACY_PROVIDER_LINK"; do [ -L "$selector" ] || fail "owned public selector is unavailable: $selector"; done
[ "$(readlink "$CLI_PATH")" = "$CURRENT_LINK/bin/krisis" ] || fail 'public command selector is foreign'
[ "$(readlink "$PROVIDER_LINK")" = "$CURRENT_LINK/share/chancery/krisis" ] || fail 'Krisis provider selector is foreign'
[ "$(readlink "$LEGACY_PROVIDER_LINK")" = "$CURRENT_LINK/share/chancery/decisions" ] || fail 'Decisions compatibility provider selector is foreign'
if [ -e "$LEGACY_CLI_PATH" ] || [ -L "$LEGACY_CLI_PATH" ]; then fail 'legacy Decisions command selector is not owned by Krisis'; fi
if [ -L "$HOOKS_PATH" ] || { [ -e "$HOOKS_PATH" ] && [ ! -f "$HOOKS_PATH" ]; }; then fail 'Codex hooks path is unsafe'; fi
[ -f "$HOOKS_PATH" ] && cmp -s "$HOOKS_PATH" "$release/package/hooks.json" || [ ! -e "$HOOKS_PATH" ] || fail 'Codex hooks file is foreign or modified'
for plist in "$OBSERVER_PLIST" "$DAILY_PLIST"; do [ ! -e "$plist" ] && [ ! -L "$plist" ] || fail "legacy LaunchAgent is not owned by Krisis: $plist"; done

binding_snapshot() {
    binding_key=$1
    binding_name=$2
    output="$transaction/$binding_name-binding.json"
    if HOME="$install_home" "$clockwork_path" --json binding show "$binding_key" >"$output" 2>"$output.stderr"; then
        [ "$(plutil -extract ok raw "$output" 2>/dev/null)" = true ] \
            && [ "$(plutil -extract data.key raw "$output" 2>/dev/null)" = "$binding_key" ] \
            || fail "$binding_name binding response is invalid"
        enabled=$(plutil -extract data.enabled raw "$output" 2>/dev/null) || fail "$binding_name binding has no enabled state"
        case "$enabled" in true) enabled=1 ;; false) enabled=0 ;; *) fail "$binding_name binding enabled state is invalid" ;; esac
        digest=$(plutil -extract data.definition_digest raw "$output" 2>/dev/null || true)
        [ "$enabled" -eq 0 ] || [ -n "$digest" ] || fail "$binding_name enabled binding has no definition"
        eval "${binding_name}_exists=1"
        eval "${binding_name}_enabled=$enabled"
        eval "${binding_name}_digest=\$digest"
    else
        grep -F '"code":"binding_not_found"' "$output.stderr" >/dev/null || fail "unable to inspect $binding_name binding"
    fi
}

active_exists=0
active_enabled=0
active_digest=
legacy_observer_exists=0
legacy_observer_enabled=0
legacy_observer_digest=
legacy_daily_exists=0
legacy_daily_enabled=0
legacy_daily_digest=

mkdir "$LOCK_DIR" 2>/dev/null || fail 'another Krisis installation operation is active'
transaction=$(mktemp -d "$INSTALL_DIR/.uninstall.XXXXXX") || fail 'unable to create uninstall transaction directory'
trap 'status=$?; trap - EXIT HUP INT TERM; rm -rf "$transaction"; rmdir "$LOCK_DIR" 2>/dev/null; exit "$status"' EXIT HUP INT TERM

binding_snapshot "$ACTIVE_CLOCKWORK_KEY" active
binding_snapshot "$LEGACY_OBSERVER_CLOCKWORK_KEY" legacy_observer
binding_snapshot "$LEGACY_DAILY_CLOCKWORK_KEY" legacy_daily
[ "$legacy_observer_enabled" -eq 0 ] || fail 'enabled legacy observer binding is not owned by Krisis; left untouched'
[ "$legacy_daily_enabled" -eq 0 ] || fail 'enabled legacy daily binding is not owned by Krisis; left untouched'

if [ -n "$active_digest" ]; then
    printf '%s\n' "$active_digest" | grep -Eq '^[0-9a-f]{64}$' || fail 'active binding digest is invalid'
    [ "$active_digest" = "$receipt_definition_digest" ] || fail 'active binding differs from its installed ownership receipt'
    definition="$transaction/active-definition.json"
    HOME="$install_home" "$clockwork_path" --json definition show "$active_digest" >"$definition" 2>"$definition.stderr" || fail 'unable to inspect the active Krisis definition'
    interpreter_hash=$(shasum -a 256 /bin/sh | awk '{print $1}')
    [ "$(plutil -extract ok raw "$definition" 2>/dev/null)" = true ] \
        && [ "$(plutil -extract data.digest raw "$definition" 2>/dev/null)" = "$active_digest" ] \
        && [ "$(plutil -extract data.key raw "$definition" 2>/dev/null)" = "$ACTIVE_CLOCKWORK_KEY" ] \
        && [ "$(plutil -extract data.manifest.schema_version raw "$definition" 2>/dev/null)" = 1 ] \
        && [ "$(plutil -extract data.manifest.key raw "$definition" 2>/dev/null)" = "$ACTIVE_CLOCKWORK_KEY" ] \
        && [ "$(plutil -extract data.manifest.release_id raw "$definition" 2>/dev/null)" = "$release_id" ] \
        && [ "$(plutil -extract data.manifest.release_root raw "$definition" 2>/dev/null)" = "$release" ] \
        && [ "$(plutil -extract data.manifest.authority raw "$definition" 2>/dev/null)" = current-user-background ] \
        && [ "$(plutil -extract data.manifest.overlap raw "$definition" 2>/dev/null)" = skip ] \
        && [ "$(plutil -extract data.manifest.cwd raw "$definition" 2>/dev/null)" = "$STATE_DIR" ] \
        && [ "$(plutil -extract data.manifest.schedule.kind raw "$definition" 2>/dev/null)" = interval ] \
        && [ "$(plutil -extract data.manifest.schedule.seconds raw "$definition" 2>/dev/null)" = 60 ] \
        && [ "$(plutil -extract data.manifest.schedule.run_at_load raw "$definition" 2>/dev/null)" = false ] \
        && [ "$(plutil -extract data.manifest.launch.kind raw "$definition" 2>/dev/null)" = interpreted ] \
        && [ "$(plutil -extract data.manifest.launch.interpreter raw "$definition" 2>/dev/null)" = /bin/sh ] \
        && [ "$(plutil -extract data.manifest.launch.interpreter_sha256 raw "$definition" 2>/dev/null)" = "$interpreter_hash" ] \
        && [ "$(plutil -extract data.manifest.launch.script raw "$definition" 2>/dev/null)" = "$release/bin/krisis-observer" ] \
        && [ "$(plutil -extract data.manifest.launch.script_sha256 raw "$definition" 2>/dev/null)" = "$runner_hash" ] \
        && [ "$(plutil -extract data.manifest.environment.HOME raw "$definition" 2>/dev/null)" = "$install_home" ] \
        && [ "$(plutil -extract data.manifest.environment.KRISIS_ANNALS_BINARY raw "$definition" 2>/dev/null)" = "$receipt_annals_binary" ] \
        && [ "$(plutil -extract data.manifest.environment.KRISIS_ANNALS_CONFIG raw "$definition" 2>/dev/null)" = "$receipt_annals_config" ] \
        && [ "$(plutil -extract data.manifest.environment.KRISIS_ANNALS_LIBRARY_ID raw "$definition" 2>/dev/null)" = "$receipt_annals_library_id" ] \
        && [ "$(plutil -extract data.manifest.output.stdout raw "$definition" 2>/dev/null)" = "$LOG_DIR/observer.stdout.log" ] \
        && [ "$(plutil -extract data.manifest.output.stderr raw "$definition" 2>/dev/null)" = "$LOG_DIR/observer.stderr.log" ] \
        || fail 'active Clockwork definition is not owned by the current Krisis release'
    direct_key_count() {
        plutil -extract "$1" xml1 -o - "$definition" 2>/dev/null | awk '
            /<dict>/ { depth++; next }
            /<\/dict>/ { depth--; next }
            depth == 1 && /<key>/ { count++ }
            END { print count+0 }
        '
    }
    [ "$(direct_key_count data.manifest)" -eq 12 ] \
        && [ "$(direct_key_count data.manifest.schedule)" -eq 3 ] \
        && [ "$(direct_key_count data.manifest.launch)" -eq 5 ] \
        && [ "$(direct_key_count data.manifest.environment)" -eq 4 ] \
        && [ "$(direct_key_count data.manifest.output)" -eq 2 ] \
        || fail 'active Clockwork definition contains foreign manifest fields'
    if plutil -extract data.manifest.arguments.0 raw "$definition" >/dev/null 2>&1 \
        || plutil -extract data.manifest.timeout_seconds raw "$definition" >/dev/null 2>&1; then
        fail 'active Clockwork definition contains unsupported arguments or timeout'
    fi
    environment_count=$(plutil -extract data.manifest.environment xml1 -o - "$definition" 2>/dev/null | awk '/<key>/{count++} END {print count+0}')
    [ "$environment_count" -eq 4 ] || fail 'active Clockwork definition contains foreign environment entries'
fi

if [ -L "$MAINTENANCE_MARKER" ] || { [ -e "$MAINTENANCE_MARKER" ] && [ ! -f "$MAINTENANCE_MARKER" ]; }; then fail 'maintenance gate is invalid'; fi
if [ ! -e "$MAINTENANCE_MARKER" ]; then (set -C; : >"$MAINTENANCE_MARKER") || fail 'unable to create maintenance gate'; chmod 0600 "$MAINTENANCE_MARKER"; fi
[ "$(stat -f '%u' "$MAINTENANCE_MARKER")" -eq "$operator_uid" ] \
    && [ "$(stat -f '%Lp' "$MAINTENANCE_MARKER")" = 600 ] \
    && [ "$(stat -f '%l' "$MAINTENANCE_MARKER")" -eq 1 ] \
    || fail 'maintenance gate is not private'

if [ "$active_enabled" -eq 1 ]; then
    current_binding="$transaction/active-binding-current.json"
    HOME="$install_home" "$clockwork_path" --json binding show "$ACTIVE_CLOCKWORK_KEY" >"$current_binding" 2>"$current_binding.stderr" || fail 'active binding disappeared before uninstall'
    [ "$(plutil -extract data.enabled raw "$current_binding" 2>/dev/null)" = true ] \
        && [ "$(plutil -extract data.definition_digest raw "$current_binding" 2>/dev/null)" = "$active_digest" ] \
        || fail 'active binding changed before uninstall'
    HOME="$install_home" "$clockwork_path" --json binding disable "$ACTIVE_CLOCKWORK_KEY" >/dev/null || fail 'unable to disable the owned Krisis binding'
fi
rm -f "$HOOKS_PATH" "$CLI_PATH" "$PROVIDER_LINK" "$LEGACY_PROVIDER_LINK"

printf '%s\n' 'uninstalled Krisis public surfaces; retained the maintenance gate, database, baseline, receipts, releases, logs, legacy Decisions history, and Clockwork history'
