#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
DEPLOYER="$SCRIPT_DIR/deploy-user.sh"
TEST_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/chancery-deploy.XXXXXX")
TEST_HOME="$TEST_ROOT/home"
CANDIDATE_ONE="$TEST_ROOT/chancery-one"
CANDIDATE_TWO="$TEST_ROOT/chancery-two"
CANDIDATE_MISMATCH="$TEST_ROOT/chancery-mismatch"

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
' "$SCRIPT_DIR/../../crates/chancery/Cargo.toml")
provider_version=$(awk -F '"' '/"release"[[:space:]]*:/ { print $4; exit }' \
    "$SCRIPT_DIR/../../provider/provider.json")
[ -n "$package_version" ] && [ "$provider_version" = "$package_version" ] || {
    printf 'test: package version %s does not match provider release %s\n' \
        "$package_version" "$provider_version" >&2
    exit 1
}
mismatch_version="$package_version-provider-mismatch"

cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    rm -rf "$TEST_ROOT"
    exit "$status"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

mkdir "$TEST_HOME"

make_candidate() {
    path=$1
    version=$2
    marker=$3
    sed \
        -e "s/@VERSION@/$version/g" \
        -e "s/@MARKER@/$marker/g" \
        "$SCRIPT_DIR/test-fixture-chancery.sh" >"$path"
    chmod 0755 "$path"
}

make_candidate "$CANDIDATE_ONE" "$package_version" one
make_candidate "$CANDIDATE_TWO" "$package_version" two
make_candidate "$CANDIDATE_MISMATCH" "$mismatch_version" mismatch

if "$DEPLOYER" --binary "$CANDIDATE_MISMATCH" --home "$TEST_HOME" \
    >"$TEST_ROOT/mismatch.out" 2>"$TEST_ROOT/mismatch.err"
then
    printf '%s\n' 'test: provider/candidate version mismatch was accepted' >&2
    exit 1
fi
grep -F "provider release $provider_version does not match candidate $mismatch_version" \
    "$TEST_ROOT/mismatch.err" >/dev/null

"$DEPLOYER" --binary "$CANDIDATE_ONE" --home "$TEST_HOME" >/dev/null
CLI="$TEST_HOME/.local/bin/chancery"
INSTALL="$TEST_HOME/Library/Application Support/Chancery/install"
PROVIDERS="$TEST_HOME/Library/Application Support/Chancery/providers"
PROVIDER="$PROVIDERS/chancery"

[ -L "$CLI" ] || { printf '%s\n' 'test: command link missing' >&2; exit 1; }
[ -d "$PROVIDERS" ] && [ ! -L "$PROVIDERS" ] \
    || { printf '%s\n' 'test: provider registry missing' >&2; exit 1; }
[ -L "$PROVIDER" ] \
    || { printf '%s\n' 'test: Chancery provider selector missing' >&2; exit 1; }
[ "$(readlink "$PROVIDER")" = "$INSTALL/current/share/chancery" ] \
    || { printf '%s\n' 'test: Chancery provider selector is invalid' >&2; exit 1; }
[ -f "$PROVIDER/provider.json" ] \
    || { printf '%s\n' 'test: staged Chancery provider missing' >&2; exit 1; }
[ "$($CLI marker)" = one ] \
    || { printf '%s\n' 'test: first candidate not installed' >&2; exit 1; }
first_current=$(readlink "$INSTALL/current")
OTHER_PROVIDER="$PROVIDERS/example"
ln -s "$TEST_ROOT/example-provider" "$OTHER_PROVIDER"

"$DEPLOYER" --binary "$CANDIDATE_ONE" --home "$TEST_HOME" >/dev/null
[ "$(readlink "$INSTALL/current")" = "$first_current" ] \
    || { printf '%s\n' 'test: identical deployment changed release' >&2; exit 1; }
[ "$(readlink "$OTHER_PROVIDER")" = "$TEST_ROOT/example-provider" ] \
    || { printf '%s\n' 'test: deployment changed another provider selector' >&2; exit 1; }

FIRST_MANIFEST="$INSTALL/$first_current/manifest.txt"
chmod 0644 "$FIRST_MANIFEST"
printf '%s\n' 'tampered=true' >>"$FIRST_MANIFEST"
if "$DEPLOYER" --binary "$CANDIDATE_ONE" --home "$TEST_HOME" >/dev/null 2>&1; then
    printf '%s\n' 'test: tampered existing release was accepted' >&2
    exit 1
fi
sed '$d' "$FIRST_MANIFEST" >"$FIRST_MANIFEST.repaired"
mv "$FIRST_MANIFEST.repaired" "$FIRST_MANIFEST"
chmod 0444 "$FIRST_MANIFEST"

"$DEPLOYER" --binary "$CANDIDATE_TWO" --home "$TEST_HOME" >/dev/null
[ "$($CLI marker)" = two ] \
    || { printf '%s\n' 'test: second candidate not installed' >&2; exit 1; }
[ "$(readlink "$INSTALL/previous")" = "$first_current" ] \
    || { printf '%s\n' 'test: previous selector not retained' >&2; exit 1; }
[ -f "$PROVIDER/provider.json" ] \
    || { printf '%s\n' 'test: provider selector did not follow current' >&2; exit 1; }

mkdir "$INSTALL/.update-lock"
if "$DEPLOYER" --binary "$CANDIDATE_ONE" --home "$TEST_HOME" >/dev/null 2>&1; then
    printf '%s\n' 'test: deployment ignored update lock' >&2
    exit 1
fi
rmdir "$INSTALL/.update-lock"

if "$DEPLOYER" --binary chancery --home "$TEST_HOME" >/dev/null 2>&1; then
    printf '%s\n' 'test: relative candidate path was accepted' >&2
    exit 1
fi

rm "$PROVIDER"
ln -s "$TEST_ROOT/not-owned" "$PROVIDER"
if "$DEPLOYER" --binary "$CANDIDATE_ONE" --home "$TEST_HOME" >/dev/null 2>&1; then
    printf '%s\n' 'test: foreign provider selector was accepted' >&2
    exit 1
fi

printf '%s\n' 'test-deploy-user.sh: green'
