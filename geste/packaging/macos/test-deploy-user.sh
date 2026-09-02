#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
temporary=$(mktemp -d "${TMPDIR:-/tmp}/geste-deploy-test.XXXXXX")
home="$temporary/Operator Home"
package="$temporary/package/macos"
share="$temporary/package/share/chancery"
candidate_template="$temporary/geste.template"
candidate_one="$temporary/geste-one"
candidate_two="$temporary/geste-two"
candidate_three="$temporary/geste-three"
candidate_mismatch="$temporary/geste-mismatch"

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
' "$SCRIPT_DIR/../../crates/geste/Cargo.toml")
provider_version=$(awk -F '"' \
    '/"release"[[:space:]]*:/ { print $4; exit }' \
    "$SCRIPT_DIR/../../chancery/provider.json")
[ -n "$package_version" ] && [ "$provider_version" = "$package_version" ] || {
    printf 'test: package %s does not match provider %s\n' \
        "$package_version" "$provider_version" >&2
    exit 1
}
mismatch_version=9.9.9

mkdir -p "$home" "$package" "$share"
install -m 0755 "$SCRIPT_DIR/deploy-user.sh" "$package/deploy-user.sh"
cp -R "$SCRIPT_DIR/../../chancery" "$share/geste"

cat >"$candidate_template" <<'EOF'
#!/bin/sh
set -eu

case "$0" in
    *'/.local/bin/geste')
        [ ! -f "${HOME:?}/fail-installed" ] || exit 70
        ;;
esac
case "${1:-}" in
    --version) printf '%s\n' 'geste @VERSION@'; exit 0 ;;
    --help) printf '%s\n' 'fake Geste help'; exit 0 ;;
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
    HOME="$home" "$package/deploy-user.sh" \
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
state="$home/Library/Application Support/Geste"
install_dir="$state/install"
releases="$install_dir/releases"
cli="$home/.local/bin/geste"
providers="$home/Library/Application Support/Chancery/providers"
provider="$providers/geste"
[ -L "$cli" ]
[ "$(readlink "$cli")" = "$install_dir/current/bin/geste" ]
[ -L "$install_dir/current" ]
[ ! -e "$install_dir/previous" ]
[ -L "$provider" ]
[ "$(readlink "$provider")" = \
    "$install_dir/current/share/chancery/geste" ]
[ -x "$install_dir/current/bin/geste" ]
[ -x "$install_dir/current/package/deploy-user.sh" ]
[ -f "$install_dir/current/manifest.txt" ]
[ -f "$provider/provider.json" ]
[ "$(HOME="$home" "$cli" marker)" = one ]
[ ! -e "$state/geste.db" ]
[ ! -e "$state/geste.db-wal" ]
[ ! -e "$state/geste.db-shm" ]
[ "$(stat -f '%Lp' "$state")" = 700 ]
first_current=$(readlink "$install_dir/current")
first_provider_path=$(CDPATH='' cd "$provider" && pwd -P)
first_release_path=$(CDPATH='' cd "$install_dir/$first_current" && pwd -P)
[ "$first_provider_path" = "$first_release_path/share/chancery/geste" ]

deploy "$candidate_one" >/dev/null
[ "$(readlink "$install_dir/current")" = "$first_current" ]
[ ! -e "$install_dir/previous" ]
[ "$(find "$releases" -mindepth 1 -maxdepth 1 -type d | wc -l | tr -d ' ')" -eq 1 ]
[ ! -e "$state/geste.db" ]

printf '%s\n' 'unmanifested payload' >"$install_dir/current/extra.txt"
if deploy "$candidate_two" >"$temporary/extra-file.out" \
    2>"$temporary/extra-file.err"; then
    printf '%s\n' 'test: unmanifested release file was accepted' >&2
    exit 1
fi
grep -F 'installed release tree is not the exact Geste v0.1 layout' \
    "$temporary/extra-file.err" >/dev/null
rm "$install_dir/current/extra.txt"

mkdir "$install_dir/current/extra-directory"
if deploy "$candidate_two" >"$temporary/extra-directory.out" \
    2>"$temporary/extra-directory.err"; then
    printf '%s\n' 'test: unmanifested release directory was accepted' >&2
    exit 1
fi
grep -F 'installed release tree is not the exact Geste v0.1 layout' \
    "$temporary/extra-directory.err" >/dev/null
rmdir "$install_dir/current/extra-directory"

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
mkdir "$releases/$fabricated_id"
rm "$install_dir/current"
ln -s "releases/$fabricated_id" "$install_dir/current"
if deploy "$candidate_two" >"$temporary/fabricated.out" \
    2>"$temporary/fabricated.err"; then
    printf '%s\n' 'test: fabricated current release was accepted' >&2
    exit 1
fi
rm "$install_dir/current"
rmdir "$releases/$fabricated_id"
ln -s "$first_current" "$install_dir/current"

ln -s 'releases/../../foreign-release' "$install_dir/previous"
if deploy "$candidate_two" >"$temporary/previous.out" \
    2>"$temporary/previous.err"; then
    printf '%s\n' 'test: traversal previous selector was accepted' >&2
    exit 1
fi
rm "$install_dir/previous"

printf '%s\n' '# tampered binary' >>"$install_dir/current/bin/geste"
if deploy "$candidate_two" >"$temporary/tampered-binary.out" \
    2>"$temporary/tampered-binary.err"; then
    printf '%s\n' 'test: tampered current binary was accepted' >&2
    exit 1
fi
install -m 0755 "$candidate_one" "$install_dir/current/bin/geste"

current_manual="$install_dir/current/share/chancery/geste/manuals/episode-explore.md"
printf '%s\n' 'tampered provider' >>"$current_manual"
if deploy "$candidate_two" >"$temporary/tampered-provider.out" \
    2>"$temporary/tampered-provider.err"; then
    printf '%s\n' 'test: tampered current provider was accepted' >&2
    exit 1
fi
install -m 0644 "$share/geste/manuals/episode-explore.md" "$current_manual"

deploy "$candidate_two" >/dev/null
second_current=$(readlink "$install_dir/current")
[ "$second_current" != "$first_current" ]
[ "$(readlink "$install_dir/previous")" = "$first_current" ]
[ "$(readlink "$cli")" = "$install_dir/current/bin/geste" ]
[ "$(readlink "$provider")" = \
    "$install_dir/current/share/chancery/geste" ]
[ "$(HOME="$home" "$cli" marker)" = two ]
second_provider_path=$(CDPATH='' cd "$provider" && pwd -P)
second_release_path=$(CDPATH='' cd "$install_dir/$second_current" && pwd -P)
[ "$second_provider_path" = "$second_release_path/share/chancery/geste" ]
[ ! -e "$state/geste.db" ]

: >"$home/fail-installed"
if deploy "$candidate_three" >"$temporary/failed.out" \
    2>"$temporary/failed.err"; then
    printf '%s\n' 'test: failing installed smoke was accepted' >&2
    exit 1
fi
[ "$(readlink "$install_dir/current")" = "$second_current" ]
[ "$(readlink "$install_dir/previous")" = "$first_current" ]
[ "$(readlink "$cli")" = "$install_dir/current/bin/geste" ]
[ "$(readlink "$provider")" = \
    "$install_dir/current/share/chancery/geste" ]
rolled_back_provider_path=$(CDPATH='' cd "$provider" && pwd -P)
[ "$rolled_back_provider_path" = \
    "$second_release_path/share/chancery/geste" ]
rm "$home/fail-installed"
[ "$(HOME="$home" "$cli" marker)" = two ]
[ ! -e "$state/geste.db" ]

deploy "$candidate_two" >/dev/null
[ "$(readlink "$install_dir/current")" = "$second_current" ]
[ "$(readlink "$install_dir/previous")" = "$first_current" ]

mkdir "$install_dir/.update-lock"
if deploy "$candidate_two" >"$temporary/lock.out" \
    2>"$temporary/lock.err"; then
    printf '%s\n' 'test: deployment ignored its update lock' >&2
    exit 1
fi
rmdir "$install_dir/.update-lock"

foreign_command_home="$temporary/Foreign Command Home"
mkdir -p "$foreign_command_home/.local/bin"
printf '%s\n' 'foreign command' >"$foreign_command_home/.local/bin/geste"
chmod 0755 "$foreign_command_home/.local/bin/geste"
if HOME="$foreign_command_home" "$package/deploy-user.sh" \
    --binary "$candidate_one" --home "$foreign_command_home" \
    >"$temporary/foreign-command.out" \
    2>"$temporary/foreign-command.err"; then
    printf '%s\n' 'test: foreign command was replaced' >&2
    exit 1
fi
grep -Fx 'foreign command' "$foreign_command_home/.local/bin/geste" >/dev/null

rm "$provider"
ln -s "$temporary/foreign-provider" "$provider"
if deploy "$candidate_two" >"$temporary/foreign-provider.out" \
    2>"$temporary/foreign-provider.err"; then
    printf '%s\n' 'test: foreign provider selector was replaced' >&2
    exit 1
fi
[ "$(readlink "$provider")" = "$temporary/foreign-provider" ]

printf '%s\n' 'test-deploy-user.sh: green'
