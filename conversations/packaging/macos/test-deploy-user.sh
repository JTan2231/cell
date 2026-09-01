#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
temporary=$(mktemp -d "${TMPDIR:-/tmp}/conversations-deploy-test.XXXXXX")
home="$temporary/Operator Home"
candidate_template="$temporary/conversations.template"
candidate_one="$temporary/conversations-one"
candidate_two="$temporary/conversations-two"
candidate_three="$temporary/conversations-three"
candidate_mismatch="$temporary/conversations-mismatch"

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
' "$SCRIPT_DIR/../../crates/conversations/Cargo.toml")
provider_version=$(awk -F '"' '/"release"[[:space:]]*:/ { print $4; exit }' \
    "$SCRIPT_DIR/../../chancery/provider.json")
[ -n "$package_version" ] && [ "$provider_version" = "$package_version" ] || {
    printf 'test: package %s does not match provider %s\n' \
        "$package_version" "$provider_version" >&2
    exit 1
}
mismatch_version="$package_version-provider-mismatch"

mkdir -p "$home"
cat >"$candidate_template" <<'EOF'
#!/bin/sh
set -eu

case "$0" in
    *'/.local/bin/conversations')
        [ ! -f "${HOME:?}/fail-installed" ] || exit 70
        ;;
esac
case "${1:-}" in
    --version) printf '%s\n' 'conversations @VERSION@'; exit 0 ;;
    --help) printf '%s\n' 'fake Conversations help'; exit 0 ;;
    marker) printf '%s\n' '@MARKER@'; exit 0 ;;
esac
exit 64
EOF

make_candidate() {
    path=$1
    version=$2
    marker=$3
    sed -e "s/@VERSION@/$version/g" -e "s/@MARKER@/$marker/g" \
        "$candidate_template" >"$path"
    chmod 0755 "$path"
}

make_candidate "$candidate_one" "$package_version" one
make_candidate "$candidate_two" "$package_version" two
make_candidate "$candidate_three" "$package_version" three
make_candidate "$candidate_mismatch" "$mismatch_version" mismatch

deploy() {
    candidate=$1
    HOME="$home" "$SCRIPT_DIR/deploy-user.sh" \
        --binary "$candidate" --home "$home"
}

if deploy "$candidate_mismatch" >"$temporary/mismatch.out" \
    2>"$temporary/mismatch.err"; then
    printf '%s\n' 'test: provider/candidate mismatch was accepted' >&2
    exit 1
fi
grep -F "provider release $provider_version does not match candidate $mismatch_version" \
    "$temporary/mismatch.err" >/dev/null

deploy "$candidate_one" >/dev/null
state="$home/Library/Application Support/Conversations"
install_dir="$state/install"
cli="$home/.local/bin/conversations"
providers="$home/Library/Application Support/Chancery/providers"
provider="$providers/conversations"
[ -L "$cli" ]
[ -L "$install_dir/current" ]
[ ! -e "$install_dir/previous" ]
[ -x "$install_dir/current/bin/conversations" ]
[ -x "$install_dir/current/package/deploy-user.sh" ]
[ -f "$install_dir/current/manifest.txt" ]
[ -f "$install_dir/current/share/chancery/conversations/provider.json" ]
[ -L "$provider" ]
[ "$(readlink "$provider")" = \
    "$install_dir/current/share/chancery/conversations" ]
[ "$(HOME="$home" "$cli" marker)" = one ]
first_current=$(readlink "$install_dir/current")

rm "$install_dir/current"
ln -s 'releases/../../foreign-release' "$install_dir/current"
if deploy "$candidate_two" >"$temporary/traversal.out" \
    2>"$temporary/traversal.err"; then
    printf '%s\n' 'test: traversal current selector was accepted' >&2
    exit 1
fi
rm "$install_dir/current"
ln -s "$first_current" "$install_dir/current"

fabricated_id=0000000000000000000000000000000000000000000000000000000000000000
mkdir "$install_dir/releases/$fabricated_id"
rm "$install_dir/current"
ln -s "releases/$fabricated_id" "$install_dir/current"
if deploy "$candidate_two" >"$temporary/fabricated.out" \
    2>"$temporary/fabricated.err"; then
    printf '%s\n' 'test: fabricated current release was accepted' >&2
    exit 1
fi
rm "$install_dir/current"
rmdir "$install_dir/releases/$fabricated_id"
ln -s "$first_current" "$install_dir/current"

ln -s 'releases/../../foreign-release' "$install_dir/previous"
if deploy "$candidate_two" >"$temporary/previous.out" \
    2>"$temporary/previous.err"; then
    printf '%s\n' 'test: traversal previous selector was accepted' >&2
    exit 1
fi
rm "$install_dir/previous"

rm "$install_dir/current" "$cli"
if deploy "$candidate_two" >"$temporary/provider-only.out" \
    2>"$temporary/provider-only.err"; then
    printf '%s\n' 'test: provider selector without a current release was accepted' >&2
    exit 1
fi
ln -s "$first_current" "$install_dir/current"
ln -s "$install_dir/current/bin/conversations" "$cli"

ln -s "$temporary/preserved-provider" "$providers/preserved"
deploy "$candidate_one" >/dev/null
[ "$(readlink "$install_dir/current")" = "$first_current" ]
[ ! -e "$install_dir/previous" ]
[ "$(readlink "$providers/preserved")" = "$temporary/preserved-provider" ]

printf '%s\n' '# tampered' >>"$install_dir/current/bin/conversations"
if deploy "$candidate_two" >"$temporary/tampered-old.out" \
    2>"$temporary/tampered-old.err"; then
    printf '%s\n' 'test: tampered old current release was accepted' >&2
    exit 1
fi
if deploy "$candidate_one" >"$temporary/tamper.out" 2>"$temporary/tamper.err"; then
    printf '%s\n' 'test: tampered release was accepted' >&2
    exit 1
fi
install -m 0755 "$candidate_one" "$install_dir/current/bin/conversations"

deploy "$candidate_two" >/dev/null
second_current=$(readlink "$install_dir/current")
[ "$second_current" != "$first_current" ]
[ "$(readlink "$install_dir/previous")" = "$first_current" ]
[ "$(HOME="$home" "$cli" marker)" = two ]

: >"$home/fail-installed"
if deploy "$candidate_three" >"$temporary/failed.out" 2>"$temporary/failed.err"; then
    printf '%s\n' 'test: failing installed smoke was accepted' >&2
    exit 1
fi
[ "$(readlink "$install_dir/current")" = "$second_current" ]
[ "$(readlink "$install_dir/previous")" = "$first_current" ]
[ "$(readlink "$provider")" = \
    "$install_dir/current/share/chancery/conversations" ]
[ "$(readlink "$providers/preserved")" = "$temporary/preserved-provider" ]

rm -f "$home/fail-installed"
mkdir "$install_dir/.update-lock"
if deploy "$candidate_two" >"$temporary/lock.out" 2>"$temporary/lock.err"; then
    printf '%s\n' 'test: deployment ignored update lock' >&2
    exit 1
fi
rmdir "$install_dir/.update-lock"

rm -f "$provider"
ln -s "$temporary/foreign-provider" "$provider"
if deploy "$candidate_two" >"$temporary/foreign.out" 2>"$temporary/foreign.err"; then
    printf '%s\n' 'test: foreign provider selector was replaced' >&2
    exit 1
fi

printf '%s\n' 'test-deploy-user.sh: green'
