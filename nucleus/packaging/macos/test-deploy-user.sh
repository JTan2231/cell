#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
temporary=$(mktemp -d "${TMPDIR:-/tmp}/nucleus-deploy-test.XXXXXX")
phase=setup
failure_reason=
fail() {
    failure_reason=$1
    exit 1
}
cleanup() {
    status=$?
    trap - 0 1 2 15
    if [ "$status" -eq 0 ]; then
        rm -rf "$temporary"
    else
        printf 'test-deploy-user.sh: %s; evidence: %s\n' \
            "${failure_reason:-unexpected failure during $phase}" "$temporary" >&2
    fi
    exit "$status"
}
trap cleanup 0
trap 'exit 129' 1
trap 'exit 130' 2
trap 'exit 143' 15

home="$temporary/Operator Home"
codex_home="$temporary/Codex Home"
binary="$temporary/nucleus candidate"
daemon="$temporary/nucleusd candidate"
codex="$temporary/codex candidate"
capture="$temporary/capture"
mkdir -p "$home" "$codex_home"

package_version=$(awk '
    $0 == "[workspace.package]" { in_package = 1; next }
    in_package && /^\[/ { exit }
    in_package && /^[[:space:]]*version[[:space:]]*=/ {
        value = $0
        sub(/^[^=]*=[[:space:]]*"/, "", value)
        sub(/"[[:space:]]*$/, "", value)
        print value
        exit
    }
' "$SCRIPT_DIR/../../../Cargo.toml")
provider_version=$(awk -F '"' '/"release"[[:space:]]*:/ { print $4; exit }' \
    "$SCRIPT_DIR/../../chancery/provider.json")
[ -n "$package_version" ] && [ "$provider_version" = "$package_version" ] || {
    fail "package version $package_version does not match provider release $provider_version"
}
mismatch_version="$package_version-provider-mismatch"
binary_template="$temporary/nucleus candidate.template"
daemon_template="$temporary/nucleusd candidate.template"

cat >"$binary_template" <<'EOF'
#!/bin/sh
set -eu
case "${1:-}" in
    --version)
        printf '%s\n' 'nucleus __NUCLEUS_VERSION__'
        ;;
    service)
        [ "${2:-}" = install ]
        shift 2
        daemon_source=
        previous=
        : >"${NUCLEUS_DEPLOY_CAPTURE:?}"
        printf 'home=%s\n' "${HOME:-}" >>"$NUCLEUS_DEPLOY_CAPTURE"
        for argument in "$@"; do
            printf 'argument=%s\n' "$argument" >>"$NUCLEUS_DEPLOY_CAPTURE"
            if [ "$previous" = --daemon ]; then
                daemon_source=$argument
            fi
            previous=$argument
        done
        if [ "${NUCLEUS_DEPLOY_KEEP_CANDIDATE:-0}" -eq 1 ]; then
            mkdir -p "$HOME/.local/bin" "$HOME/.local/libexec"
            cp "$0" "$HOME/.local/bin/nucleus"
            cp "${daemon_source:?}" "$HOME/.local/libexec/nucleusd"
            chmod 0755 "$HOME/.local/bin/nucleus" "$HOME/.local/libexec/nucleusd"
        fi
        [ "${NUCLEUS_DEPLOY_FAIL:-0}" -eq 0 ] || exit 42
        printf '%s\n' '{"installed":true}'
        ;;
    *) exit 1 ;;
esac
EOF
sed "s/__NUCLEUS_VERSION__/$package_version/g" "$binary_template" >"$binary"

cat >"$daemon_template" <<'EOF'
#!/bin/sh
case "${1:-}" in
    --version) printf '%s\n' 'nucleusd __NUCLEUS_VERSION__' ;;
    *) exit 1 ;;
esac
EOF
sed "s/__NUCLEUS_VERSION__/$package_version/g" "$daemon_template" >"$daemon"

cat >"$codex" <<'EOF'
#!/bin/sh
case "${1:-}" in
    --version) printf '%s\n' 'codex test' ;;
    *) exit 1 ;;
esac
EOF
chmod 0755 "$binary" "$daemon" "$codex"

phase='initial deployment'
NUCLEUS_DEPLOY_CAPTURE="$capture" \
    "$SCRIPT_DIR/deploy-user.sh" \
    --binary "$binary" \
    --daemon "$daemon" \
    --codex "$codex" \
    --codex-home "$codex_home" \
    --home "$home" >"$temporary/install.out"
sed -n '1p' "$temporary/install.out" \
    | grep -Fx '{"installed":true}' >/dev/null \
    || fail 'installer success JSON missing from install.out'
grep -F 'Nucleus packaged release: ' "$temporary/install.out" >/dev/null \
    || fail 'packaged release receipt missing from install.out'
grep -F 'Chancery provider: ' "$temporary/install.out" >/dev/null \
    || fail 'provider receipt missing from install.out'

state="$home/Library/Application Support/Nucleus"
install_root="$state/install"
chancery_providers="$home/Library/Application Support/Chancery/providers"
nucleus_provider="$chancery_providers/nucleus"
[ -L "$install_root/current" ] || fail 'current release selector missing'
[ -f "$install_root/current/bin/nucleus" ] || fail 'installed CLI missing'
[ -f "$install_root/current/libexec/nucleusd" ] || fail 'installed daemon missing'
[ -f "$install_root/current/share/chancery/nucleus/provider.json" ] \
    || fail 'installed provider manifest missing'
[ -L "$nucleus_provider" ] || fail 'provider selector missing'
[ "$(readlink "$nucleus_provider")" = \
    "$install_root/current/share/chancery/nucleus" ] \
    || fail 'provider selector does not point to the current release'
[ -f "$nucleus_provider/provider.json" ] || fail 'selected provider manifest missing'
first_current=$(readlink "$install_root/current")
ln -s /preserved/provider "$chancery_providers/preserved"

expected="$temporary/expected"
{
    printf 'home=%s\n' "$home"
    printf '%s\n' 'argument=--daemon' \
        "argument=$install_root/current/libexec/nucleusd"
    printf '%s\n' 'argument=--codex' "argument=$codex"
    printf '%s\n' 'argument=--codex-home' "argument=$codex_home"
} >"$expected"
diff -u "$expected" "$capture" >"$temporary/arguments.diff" \
    || fail 'installer arguments differ from expected (arguments.diff)'

phase='idempotent deployment without Codex home'
NUCLEUS_DEPLOY_CAPTURE="$capture" \
    "$SCRIPT_DIR/deploy-user.sh" \
    --binary "$binary" \
    --daemon "$daemon" \
    --codex "$codex" \
    --home "$home" >/dev/null
if grep -F -- '--codex-home' "$capture" >/dev/null; then
    fail 'deployment unexpectedly forwarded an omitted Codex home'
fi
[ "$(readlink "$install_root/current")" = "$first_current" ] \
    || fail 'idempotent deployment changed the current release'
[ "$(readlink "$chancery_providers/preserved")" = /preserved/provider ] \
    || fail 'idempotent deployment changed an unrelated provider'

phase='tampered provider rejection'
printf '%s\n' ' ' >>"$install_root/current/share/chancery/nucleus/provider.json"
if NUCLEUS_DEPLOY_CAPTURE="$capture" \
    "$SCRIPT_DIR/deploy-user.sh" \
    --binary "$binary" \
    --daemon "$daemon" \
    --codex "$codex" \
    --home "$home" >"$temporary/tampered.out" 2>"$temporary/tampered.err"
then
    fail 'deployment unexpectedly accepted a tampered Chancery bundle'
fi
install -m 0600 "$SCRIPT_DIR/../../chancery/provider.json" \
    "$install_root/current/share/chancery/nucleus/provider.json"

lock="$home/Library/Application Support/Nucleus/.deploy-lock"
[ ! -e "$lock" ] || fail 'deployment lock remained after tamper rejection'
phase='installer failure rollback'
printf '%s\n' '# rejected release' >>"$binary"
if NUCLEUS_DEPLOY_CAPTURE="$capture" NUCLEUS_DEPLOY_FAIL=1 \
    "$SCRIPT_DIR/deploy-user.sh" \
    --binary "$binary" \
    --daemon "$daemon" \
    --codex "$codex" \
    --home "$home" >"$temporary/failure.out" 2>"$temporary/failure.err"
then
    fail 'deployment unexpectedly hid an installer failure'
fi
[ ! -e "$lock" ] || fail 'deployment lock remained after installer failure'
[ "$(readlink "$install_root/current")" = "$first_current" ] \
    || fail 'installer failure did not restore the current release'
[ ! -e "$install_root/previous" ] || fail 'rollback left a previous release selector'
[ -f "$nucleus_provider/provider.json" ] || fail 'rollback lost the provider manifest'
[ "$(readlink "$chancery_providers/preserved")" = /preserved/provider ] \
    || fail 'rollback changed an unrelated provider'

phase='deployment lock rejection'
mkdir "$lock"
if NUCLEUS_DEPLOY_CAPTURE="$capture" \
    "$SCRIPT_DIR/deploy-user.sh" \
    --binary "$binary" \
    --daemon "$daemon" \
    --codex "$codex" \
    --home "$home" >"$temporary/locked.out" 2>"$temporary/locked.err"
then
    fail 'deployment unexpectedly ignored the update lock'
fi
grep -F 'another deployment holds' "$temporary/locked.err" >/dev/null \
    || fail 'lock rejection reason missing from locked.err'
rmdir "$lock"

phase='candidate version mismatch rejection'
mismatched_daemon="$temporary/mismatched nucleusd"
sed "s/__NUCLEUS_VERSION__/$mismatch_version/g" \
    "$daemon_template" >"$mismatched_daemon"
chmod 0755 "$mismatched_daemon"
if NUCLEUS_DEPLOY_CAPTURE="$capture" \
    "$SCRIPT_DIR/deploy-user.sh" \
    --binary "$binary" \
    --daemon "$mismatched_daemon" \
    --codex "$codex" \
    --home "$home" >"$temporary/mismatch.out" 2>"$temporary/mismatch.err"
then
    fail 'deployment unexpectedly accepted mismatched binaries'
fi
grep -F 'candidate versions do not match' "$temporary/mismatch.err" >/dev/null \
    || fail 'version rejection reason missing from mismatch.err'

phase='provider version mismatch rejection'
mismatched_provider_binary="$temporary/mismatched-provider-nucleus"
sed "s/__NUCLEUS_VERSION__/$mismatch_version/g" "$binary_template" \
    >"$mismatched_provider_binary"
chmod 0755 "$mismatched_provider_binary"
if NUCLEUS_DEPLOY_CAPTURE="$capture" \
    "$SCRIPT_DIR/deploy-user.sh" \
    --binary "$mismatched_provider_binary" \
    --daemon "$daemon" \
    --codex "$codex" \
    --home "$home" >"$temporary/provider-mismatch.out" \
    2>"$temporary/provider-mismatch.err"
then
    fail 'deployment unexpectedly accepted a provider/candidate mismatch'
fi
grep -F "provider release $provider_version does not match candidate $mismatch_version" \
    "$temporary/provider-mismatch.err" >/dev/null \
    || fail 'provider version rejection reason missing from provider-mismatch.err'

phase='relative path rejection'
if "$SCRIPT_DIR/deploy-user.sh" \
    --binary relative/nucleus \
    --daemon "$daemon" \
    --codex "$codex" \
    --home "$home" >"$temporary/relative.out" 2>"$temporary/relative.err"
then
    fail 'deployment unexpectedly accepted a relative binary path'
fi
grep -F -- '--binary must be an absolute path' "$temporary/relative.err" >/dev/null \
    || fail 'relative path rejection reason missing from relative.err'

phase='unsafe rollback candidate preservation'
if NUCLEUS_DEPLOY_CAPTURE="$capture" \
    NUCLEUS_DEPLOY_FAIL=1 \
    NUCLEUS_DEPLOY_KEEP_CANDIDATE=1 \
    "$SCRIPT_DIR/deploy-user.sh" \
    --binary "$binary" \
    --daemon "$daemon" \
    --codex "$codex" \
    --home "$home" >"$temporary/unsafe-rollback.out" \
    2>"$temporary/unsafe-rollback.err"
then
    fail 'deployment unexpectedly hid an unsafe rollback failure'
fi
kept_current=$(readlink "$install_root/current")
[ "$kept_current" != "$first_current" ] \
    || fail 'unsafe rollback did not retain the candidate release'
[ "$(readlink "$install_root/previous")" = "$first_current" ] \
    || fail 'unsafe rollback lost the previous release selector'
[ "$(shasum -a 256 "$home/.local/bin/nucleus" | awk '{print $1}')" = \
    "$(shasum -a 256 "$install_root/current/bin/nucleus" | awk '{print $1}')" ] \
    || fail 'retained CLI does not match the selected candidate'
[ "$(shasum -a 256 "$home/.local/libexec/nucleusd" | awk '{print $1}')" = \
    "$(shasum -a 256 "$install_root/current/libexec/nucleusd" | awk '{print $1}')" ] \
    || fail 'retained daemon does not match the selected candidate'
[ -f "$nucleus_provider/provider.json" ] \
    || fail 'unsafe rollback lost the selected provider manifest'
grep -F 'preserving their matching packaged release' \
    "$temporary/unsafe-rollback.err" >/dev/null \
    || fail 'candidate preservation reason missing from unsafe-rollback.err'

phase='foreign provider rejection'
rm -f "$nucleus_provider"
ln -s /foreign/nucleus-provider "$nucleus_provider"
if NUCLEUS_DEPLOY_CAPTURE="$capture" \
    "$SCRIPT_DIR/deploy-user.sh" \
    --binary "$binary" \
    --daemon "$daemon" \
    --codex "$codex" \
    --home "$home" >"$temporary/foreign-provider.out" \
    2>"$temporary/foreign-provider.err"
then
    fail 'deployment unexpectedly took over a foreign provider selector'
fi
[ "$(readlink "$nucleus_provider")" = /foreign/nucleus-provider ] \
    || fail 'rejected deployment changed the foreign provider selector'
rm -f "$nucleus_provider"
ln -s "$install_root/current/share/chancery/nucleus" "$nucleus_provider"

printf '%s\n' 'deploy test passed'
