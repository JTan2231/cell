#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
python3 "$SCRIPT_DIR/../../../deployment/generate.py" --check --product pratica
temporary=$(mktemp -d "${TMPDIR:-/tmp}/pratica-deploy-test.XXXXXX")
home="$temporary/Operator Home"
package="$temporary/package/macos"
share="$temporary/package/share/chancery"
candidate_template="$temporary/pratica.template"
candidate_one="$temporary/pratica-one"
candidate_two="$temporary/pratica-two"
candidate_three="$temporary/pratica-three"
candidate_mismatch="$temporary/pratica-mismatch"

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
' "$SCRIPT_DIR/../../crates/pratica/Cargo.toml")
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
cp -R "$SCRIPT_DIR/../../chancery" "$share/pratica"

cat >"$candidate_template" <<'EOF'
#!/bin/sh
set -eu

case "$0" in
    *'/.local/bin/pratica')
        [ ! -f "${HOME:?}/fail-installed" ] || exit 70
        ;;
esac
case "${1:-}" in
    --version) printf '%s\n' 'pratica @VERSION@'; exit 0 ;;
    --help) printf '%s\n' 'fake Pratica help'; exit 0 ;;
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
    shift
    HOME="$home" "$package/deploy-user.sh" \
        --binary "$candidate" --home "$home" "$@"
}

if deploy "$candidate_mismatch" >"$temporary/mismatch.out" \
    2>"$temporary/mismatch.err"; then
    printf '%s\n' 'test: provider/candidate mismatch was accepted' >&2
    exit 1
fi
grep -F "provider release $provider_version does not match candidate $mismatch_version" \
    "$temporary/mismatch.err" >/dev/null

deploy "$candidate_one" >/dev/null
state="$home/Library/Application Support/Pratica"
install_dir="$state/install"
releases="$install_dir/releases"
cli="$home/.local/bin/pratica"
providers="$home/Library/Application Support/Chancery/providers"
provider="$providers/pratica"
[ -L "$cli" ]
[ "$(readlink "$cli")" = "$install_dir/current/bin/pratica" ]
[ -L "$install_dir/current" ]
[ ! -e "$install_dir/previous" ]
[ -L "$provider" ]
[ "$(readlink "$provider")" = \
    "$install_dir/current/share/chancery/pratica" ]
[ -x "$install_dir/current/bin/pratica" ]
[ -x "$install_dir/current/package/deploy-user.sh" ]
[ -f "$install_dir/current/manifest.txt" ]
[ -f "$provider/provider.json" ]
[ "$(HOME="$home" "$cli" marker)" = one ]
[ ! -e "$state/pratica.db" ]
[ ! -e "$state/pratica.db-wal" ]
[ ! -e "$state/pratica.db-shm" ]
[ "$(stat -f '%Lp' "$state")" = 700 ]
first_current=$(readlink "$install_dir/current")
first_provider_path=$(CDPATH='' cd "$provider" && pwd -P)
first_release_path=$(CDPATH='' cd "$install_dir/$first_current" && pwd -P)
[ "$first_provider_path" = "$first_release_path/share/chancery/pratica" ]

deploy "$candidate_one" >/dev/null
[ "$(readlink "$install_dir/current")" = "$first_current" ]
[ ! -e "$install_dir/previous" ]
[ "$(find "$releases" -mindepth 1 -maxdepth 1 -type d | wc -l | tr -d ' ')" -eq 1 ]
[ ! -e "$state/pratica.db" ]

printf '%s\n' 'unmanifested payload' >"$install_dir/current/extra.txt"
if deploy "$candidate_two" >"$temporary/extra-file.out" \
    2>"$temporary/extra-file.err"; then
    printf '%s\n' 'test: unmanifested release file was accepted' >&2
    exit 1
fi
grep -F 'installed release tree has an unexpected file' \
    "$temporary/extra-file.err" >/dev/null
rm "$install_dir/current/extra.txt"

rm "$install_dir/current"
ln -s 'releases/../../foreign-release' "$install_dir/current"
if deploy "$candidate_two" >"$temporary/traversal.out" \
    2>"$temporary/traversal.err"; then
    printf '%s\n' 'test: traversal current selector was accepted' >&2
    exit 1
fi
rm "$install_dir/current"
ln -s "$first_current" "$install_dir/current"

printf '%s\n' '# tampered binary' >>"$install_dir/current/bin/pratica"
if deploy "$candidate_two" >"$temporary/tampered-binary.out" \
    2>"$temporary/tampered-binary.err"; then
    printf '%s\n' 'test: tampered current binary was accepted' >&2
    exit 1
fi
install -m 0755 "$candidate_one" "$install_dir/current/bin/pratica"

current_manual="$install_dir/current/share/chancery/pratica/manuals/integration-negotiate.md"
printf '%s\n' 'tampered provider' >>"$current_manual"
if deploy "$candidate_two" >"$temporary/tampered-provider.out" \
    2>"$temporary/tampered-provider.err"; then
    printf '%s\n' 'test: tampered current provider was accepted' >&2
    exit 1
fi
install -m 0644 "$share/pratica/manuals/integration-negotiate.md" \
    "$current_manual"

deploy "$candidate_two" >/dev/null
second_current=$(readlink "$install_dir/current")
[ "$second_current" != "$first_current" ]
[ "$(readlink "$install_dir/previous")" = "$first_current" ]
[ "$(readlink "$cli")" = "$install_dir/current/bin/pratica" ]
[ "$(readlink "$provider")" = \
    "$install_dir/current/share/chancery/pratica" ]
[ "$(HOME="$home" "$cli" marker)" = two ]
second_provider_path=$(CDPATH='' cd "$provider" && pwd -P)
second_release_path=$(CDPATH='' cd "$install_dir/$second_current" && pwd -P)
[ "$second_provider_path" = "$second_release_path/share/chancery/pratica" ]
[ ! -e "$state/pratica.db" ]

if deploy "$candidate_three" --expected-current absent \
    >"$temporary/stale.out" 2>"$temporary/stale.err"; then
    printf '%s\n' 'test: stale expected-current precondition was accepted' >&2
    exit 1
fi
grep -F 'stale deployment: expected current absent' \
    "$temporary/stale.err" >/dev/null
[ "$(readlink "$install_dir/current")" = "$second_current" ]

: >"$home/fail-installed"
if deploy "$candidate_three" >"$temporary/failed.out" \
    2>"$temporary/failed.err"; then
    printf '%s\n' 'test: failing installed smoke was accepted' >&2
    exit 1
fi
[ "$(readlink "$install_dir/current")" = "$second_current" ]
[ "$(readlink "$install_dir/previous")" = "$first_current" ]
[ "$(readlink "$cli")" = "$install_dir/current/bin/pratica" ]
[ "$(readlink "$provider")" = \
    "$install_dir/current/share/chancery/pratica" ]
rolled_back_provider_path=$(CDPATH='' cd "$provider" && pwd -P)
[ "$rolled_back_provider_path" = "$second_release_path/share/chancery/pratica" ]
rm "$home/fail-installed"
[ "$(HOME="$home" "$cli" marker)" = two ]
[ ! -e "$state/pratica.db" ]

deploy "$candidate_two" >/dev/null
[ "$(readlink "$install_dir/current")" = "$second_current" ]
[ "$(readlink "$install_dir/previous")" = "$first_current" ]

stale_pid=999999
while kill -0 "$stale_pid" 2>/dev/null; do
    stale_pid=$((stale_pid + 1))
done
/usr/bin/shlock -p "$stale_pid" -f "$install_dir/.update-lock"
deploy "$candidate_two" >/dev/null
[ ! -e "$install_dir/.update-lock" ]

mkdir "$install_dir/.update-lock"
if deploy "$candidate_two" >"$temporary/lock.out" \
    2>"$temporary/lock.err"; then
    printf '%s\n' 'test: deployment ignored its update lock' >&2
    exit 1
fi
rmdir "$install_dir/.update-lock"

foreign_command_home="$temporary/Foreign Command Home"
mkdir -p "$foreign_command_home/.local/bin"
printf '%s\n' 'foreign command' >"$foreign_command_home/.local/bin/pratica"
chmod 0755 "$foreign_command_home/.local/bin/pratica"
if HOME="$foreign_command_home" "$package/deploy-user.sh" \
    --binary "$candidate_one" --home "$foreign_command_home" \
    >"$temporary/foreign-command.out" \
    2>"$temporary/foreign-command.err"; then
    printf '%s\n' 'test: foreign command was replaced' >&2
    exit 1
fi
grep -Fx 'foreign command' \
    "$foreign_command_home/.local/bin/pratica" >/dev/null

rm "$provider"
ln -s "$temporary/foreign-provider" "$provider"
if deploy "$candidate_two" >"$temporary/foreign-provider.out" \
    2>"$temporary/foreign-provider.err"; then
    printf '%s\n' 'test: foreign provider selector was replaced' >&2
    exit 1
fi
[ "$(readlink "$provider")" = "$temporary/foreign-provider" ]

[ ! -e "$state/pratica.db" ]
[ ! -e "$state/pratica.db-wal" ]
[ ! -e "$state/pratica.db-shm" ]
printf '%s\n' 'test-deploy-user.sh: green'
