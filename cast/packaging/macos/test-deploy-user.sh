#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
DEPLOYER="$SCRIPT_DIR/deploy-user.sh"
temporary=$(mktemp -d "${TMPDIR:-/tmp}/cast-deploy-test.XXXXXX")
cast_test_home="$temporary/Operator Home"
candidate_template="$temporary/cast-candidate.template"
candidate_one="$temporary/cast-one"
candidate_two="$temporary/cast-two"
candidate_three="$temporary/cast-three"
candidate_mismatch="$temporary/cast-mismatch"

cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    rm -rf "$temporary"
    exit "$status"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

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
' "$SCRIPT_DIR/../../Cargo.toml")
provider_version=$(awk -F '"' '/"release"[[:space:]]*:/ { print $4; exit }' \
    "$SCRIPT_DIR/../../chancery/provider.json")
[ -n "$package_version" ] && [ "$provider_version" = "$package_version" ] || {
    printf 'test: package version %s does not match provider release %s\n' \
        "$package_version" "$provider_version" >&2
    exit 1
}
mismatch_version="$package_version-provider-mismatch"

mkdir -p "$cast_test_home"
cat >"$candidate_template" <<'EOF'
#!/bin/sh
set -eu

case "$0" in
    *'/Library/Application Support/Cast/install/'*)
        [ ! -f "${HOME:?}/fail-installed" ] || exit 70
        ;;
esac

case "${1:-}" in
    --version)
        printf '%s\n' 'cast @VERSION@'
        exit 0
        ;;
    --help)
        printf '%s\n' 'fake Cast help'
        exit 0
        ;;
    marker)
        printf '%s\n' '@MARKER@'
        exit 0
        ;;
esac

env | sort >"$HOME/cast-environment.log"
: >"$HOME/cast-arguments.log"
for argument in "$@"; do
    printf '%s\n' "$argument" >>"$HOME/cast-arguments.log"
done
cat >"$HOME/cast-stdin.log"
printf '%s\n' 'Completed cast_test'
EOF

make_candidate() {
    path=$1
    version=$2
    marker=$3
    sed \
        -e "s/@VERSION@/$version/g" \
        -e "s/@MARKER@/$marker/g" \
        "$candidate_template" >"$path"
    chmod 0755 "$path"
}

make_candidate "$candidate_one" "$package_version" one
make_candidate "$candidate_two" "$package_version" two
make_candidate "$candidate_three" "$package_version" three
make_candidate "$candidate_mismatch" "$mismatch_version" mismatch

deploy() {
    candidate=$1
    shift
    HOME="$cast_test_home" "$DEPLOYER" --binary "$candidate" --home "$cast_test_home" "$@"
}

if deploy "$candidate_mismatch" >"$temporary/mismatch.out" \
    2>"$temporary/mismatch.err"
then
    printf '%s\n' 'test: provider/candidate version mismatch was accepted' >&2
    exit 1
fi
grep -F "provider release $provider_version does not match candidate $mismatch_version" \
    "$temporary/mismatch.err" >/dev/null

cat >"$cast_test_home/.zshrc" <<'EOF'
export THEIRSTACK_API_KEY='theirstack-test-secret'
export BRAVE_SEARCH_API_KEY='brave-test-secret'
export OTHER_CREDENTIAL='must-not-leak'
: >"$HOME/cast-zshrc-sourced"
EOF

deploy "$candidate_one" >/dev/null
state="$cast_test_home/Library/Application Support/Cast"
install_dir="$state/install"
cli="$cast_test_home/.local/bin/cast"
providers="$cast_test_home/Library/Application Support/Chancery/providers"
provider="$providers/cast"

[ -L "$cli" ]
[ -L "$install_dir/current" ]
[ ! -e "$install_dir/previous" ]
[ -x "$install_dir/current/bin/cast" ]
[ -x "$install_dir/current/libexec/cast" ]
[ -x "$install_dir/current/package/cast" ]
[ -x "$install_dir/current/package/deploy-user.sh" ]
[ -f "$install_dir/current/manifest.txt" ]
[ -f "$install_dir/current/share/chancery/cast/provider.json" ]
[ -L "$provider" ]
[ "$(readlink "$provider")" = "$install_dir/current/share/chancery/cast" ]
[ ! -e "$cast_test_home/.local/bin/chancery" ]
[ "$(HOME="$cast_test_home" "$cli" marker)" = one ]

rm -f "$cast_test_home/cast-zshrc-sourced"
HOME="$cast_test_home" "$cli" --help >"$temporary/help.out"
grep -Fx 'fake Cast help' "$temporary/help.out" >/dev/null
[ ! -e "$cast_test_home/cast-zshrc-sourced" ]

printf '%s' 'first line
second line' | HOME="$cast_test_home" CAST_STATE_DIR="/explicit/state" "$cli" \
    --state-dir 'state with spaces' run --force \
    >"$temporary/send.out"
grep -Fx 'Completed cast_test' "$temporary/send.out" >/dev/null
grep -Fx 'THEIRSTACK_API_KEY=theirstack-test-secret' "$cast_test_home/cast-environment.log" >/dev/null
grep -Fx 'CAST_STATE_DIR=/explicit/state' "$cast_test_home/cast-environment.log" >/dev/null
grep -Fx 'BRAVE_SEARCH_API_KEY=brave-test-secret' "$cast_test_home/cast-environment.log" >/dev/null
grep -Fx "HOME=$cast_test_home" "$cast_test_home/cast-environment.log" >/dev/null
grep -Fx 'PATH=/usr/bin:/bin:/usr/sbin:/sbin' "$cast_test_home/cast-environment.log" >/dev/null
if grep -F 'OTHER_CREDENTIAL=' "$cast_test_home/cast-environment.log" >/dev/null; then
    printf '%s\n' 'test: wrapper leaked an unrelated credential' >&2
    exit 1
fi
[ "$(sed -n '1p' "$cast_test_home/cast-arguments.log")" = '--state-dir' ]
[ "$(sed -n '2p' "$cast_test_home/cast-arguments.log")" = 'state with spaces' ]
[ "$(sed -n '3p' "$cast_test_home/cast-arguments.log")" = run ]
[ "$(sed -n '4p' "$cast_test_home/cast-arguments.log")" = --force ]
[ "$(cat "$cast_test_home/cast-stdin.log")" = 'first line
second line' ]

first_current=$(readlink "$install_dir/current")
ln -s "$temporary/preserved-provider" "$providers/preserved"
HOME="$cast_test_home" "$install_dir/current/package/deploy-user.sh" \
    --binary "$candidate_one" --home "$cast_test_home" >/dev/null
[ "$(readlink "$install_dir/current")" = "$first_current" ]
[ ! -e "$install_dir/previous" ]
[ "$(readlink "$providers/preserved")" = "$temporary/preserved-provider" ]

printf '%s\n' 'tampered' >>"$install_dir/current/libexec/cast"
if deploy "$candidate_one" >"$temporary/tamper.out" 2>"$temporary/tamper.err"; then
    printf '%s\n' 'test: tampered existing release was accepted' >&2
    exit 1
fi
install -m 0755 "$candidate_one" "$install_dir/current/libexec/cast"

printf '%s\n' 'tampered' >>"$install_dir/current/libexec/cast"
if deploy "$candidate_two" >"$temporary/tampered-prior.out" 2>"$temporary/tampered-prior.err"; then
    printf '%s\n' 'test: upgrade accepted a corrupt rollback release' >&2
    exit 1
fi
install -m 0755 "$candidate_one" "$install_dir/current/libexec/cast"

rm "$install_dir/current"
ln -s 'releases/../../foreign' "$install_dir/current"
if deploy "$candidate_two" >"$temporary/traversal.out" 2>"$temporary/traversal.err"; then
    printf '%s\n' 'test: traversal release selector was accepted' >&2
    exit 1
fi
rm "$install_dir/current"
ln -s "$first_current" "$install_dir/current"

deploy "$candidate_two" >/dev/null
second_current=$(readlink "$install_dir/current")
[ "$second_current" != "$first_current" ]
[ "$(readlink "$install_dir/previous")" = "$first_current" ]
[ "$(HOME="$cast_test_home" "$cli" marker)" = two ]
[ "$(readlink "$provider")" = "$install_dir/current/share/chancery/cast" ]

: >"$cast_test_home/fail-installed"
if deploy "$candidate_three" >"$temporary/failed.out" 2>"$temporary/failed.err"; then
    printf '%s\n' 'test: failing installed candidate was accepted' >&2
    exit 1
fi
[ "$(readlink "$install_dir/current")" = "$second_current" ]
[ "$(readlink "$install_dir/previous")" = "$first_current" ]
[ "$(readlink "$provider")" = "$install_dir/current/share/chancery/cast" ]
rm "$cast_test_home/fail-installed"
[ "$(HOME="$cast_test_home" "$cli" marker)" = two ]

deploy "$candidate_one" >/dev/null
[ "$(readlink "$install_dir/current")" = "$first_current" ]
[ "$(readlink "$install_dir/previous")" = "$second_current" ]
[ "$(HOME="$cast_test_home" "$cli" marker)" = one ]
[ "$(readlink "$provider")" = "$install_dir/current/share/chancery/cast" ]

mkdir "$install_dir/.update-lock"
if deploy "$candidate_one" >"$temporary/lock.out" 2>"$temporary/lock.err"; then
    printf '%s\n' 'test: deployment ignored its update lock' >&2
    exit 1
fi
rmdir "$install_dir/.update-lock"

if "$DEPLOYER" --binary cast --home "$cast_test_home" >/dev/null 2>&1; then
    printf '%s\n' 'test: relative candidate path was accepted' >&2
    exit 1
fi

rm "$provider"
ln -s "$temporary/foreign-provider" "$provider"
if deploy "$candidate_one" >"$temporary/foreign.out" 2>"$temporary/foreign.err"; then
    printf '%s\n' 'test: foreign Cast provider selector was accepted' >&2
    exit 1
fi

printf '%s\n' 'test-deploy-user.sh: green'
