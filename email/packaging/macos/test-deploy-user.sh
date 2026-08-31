#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
DEPLOYER="$SCRIPT_DIR/deploy-user.sh"
temporary=$(mktemp -d "${TMPDIR:-/tmp}/email-deploy-test.XXXXXX")
home="$temporary/Operator Home"
candidate_template="$temporary/email-candidate.template"
candidate_one="$temporary/email-one"
candidate_two="$temporary/email-two"
candidate_three="$temporary/email-three"
candidate_mismatch="$temporary/email-mismatch"

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
' "$SCRIPT_DIR/../../crates/email/Cargo.toml")
provider_version=$(awk -F '"' '/"release"[[:space:]]*:/ { print $4; exit }' \
    "$SCRIPT_DIR/../../chancery/provider.json")
[ -n "$package_version" ] && [ "$provider_version" = "$package_version" ] || {
    printf 'test: package version %s does not match provider release %s\n' \
        "$package_version" "$provider_version" >&2
    exit 1
}
mismatch_version="$package_version-provider-mismatch"

mkdir -p "$home"
cat >"$candidate_template" <<'EOF'
#!/bin/sh
set -eu

case "$0" in
    *'/Library/Application Support/Email/install/'*)
        [ ! -f "${HOME:?}/fail-installed" ] || exit 70
        ;;
esac

case "${1:-}" in
    --version)
        printf '%s\n' 'email @VERSION@'
        exit 0
        ;;
    --help)
        printf '%s\n' 'fake Email help'
        exit 0
        ;;
    marker)
        printf '%s\n' '@MARKER@'
        exit 0
        ;;
esac

env | sort >"$HOME/email-environment.log"
: >"$HOME/email-arguments.log"
for argument in "$@"; do
    printf '%s\n' "$argument" >>"$HOME/email-arguments.log"
done
cat >"$HOME/email-stdin.log"
printf '%s\n' 'Sent email_test'
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
    HOME="$home" "$DEPLOYER" --binary "$candidate" --home "$home" "$@"
}

if deploy "$candidate_mismatch" >"$temporary/mismatch.out" \
    2>"$temporary/mismatch.err"
then
    printf '%s\n' 'test: provider/candidate version mismatch was accepted' >&2
    exit 1
fi
grep -F "provider release $provider_version does not match candidate $mismatch_version" \
    "$temporary/mismatch.err" >/dev/null

cat >"$home/.zshrc" <<'EOF'
export RESEND_API_KEY='resend-test-secret'
export OTHER_CREDENTIAL='must-not-leak'
EOF

deploy "$candidate_one" >/dev/null
state="$home/Library/Application Support/Email"
install_dir="$state/install"
cli="$home/.local/bin/email"
providers="$home/Library/Application Support/Chancery/providers"
provider="$providers/email"

[ -L "$cli" ]
[ -L "$install_dir/current" ]
[ ! -e "$install_dir/previous" ]
[ -x "$install_dir/current/bin/email" ]
[ -x "$install_dir/current/libexec/email" ]
[ -x "$install_dir/current/package/email" ]
[ -x "$install_dir/current/package/deploy-user.sh" ]
[ -f "$install_dir/current/manifest.txt" ]
[ -f "$install_dir/current/share/chancery/email/provider.json" ]
[ -L "$provider" ]
[ "$(readlink "$provider")" = "$install_dir/current/share/chancery/email" ]
[ ! -e "$home/.local/bin/chancery" ]
[ "$(HOME="$home" "$cli" marker)" = one ]

printf '%s' 'first line
second line' | HOME="$home" "$cli" 'A subject' - \
    >"$temporary/send.out"
grep -Fx 'Sent email_test' "$temporary/send.out" >/dev/null
grep -Fx 'RESEND_API_KEY=resend-test-secret' "$home/email-environment.log" >/dev/null
grep -Fx "HOME=$home" "$home/email-environment.log" >/dev/null
grep -Fx 'PATH=/usr/bin:/bin:/usr/sbin:/sbin' "$home/email-environment.log" >/dev/null
if grep -F 'OTHER_CREDENTIAL=' "$home/email-environment.log" >/dev/null; then
    printf '%s\n' 'test: wrapper leaked an unrelated credential' >&2
    exit 1
fi
[ "$(sed -n '1p' "$home/email-arguments.log")" = 'A subject' ]
[ "$(sed -n '2p' "$home/email-arguments.log")" = - ]
[ "$(cat "$home/email-stdin.log")" = 'first line
second line' ]

first_current=$(readlink "$install_dir/current")
ln -s "$temporary/preserved-provider" "$providers/preserved"
HOME="$home" "$install_dir/current/package/deploy-user.sh" \
    --binary "$candidate_one" --home "$home" >/dev/null
[ "$(readlink "$install_dir/current")" = "$first_current" ]
[ ! -e "$install_dir/previous" ]
[ "$(readlink "$providers/preserved")" = "$temporary/preserved-provider" ]

printf '%s\n' 'tampered' >>"$install_dir/current/libexec/email"
if deploy "$candidate_one" >"$temporary/tamper.out" 2>"$temporary/tamper.err"; then
    printf '%s\n' 'test: tampered existing release was accepted' >&2
    exit 1
fi
install -m 0755 "$candidate_one" "$install_dir/current/libexec/email"

deploy "$candidate_two" >/dev/null
second_current=$(readlink "$install_dir/current")
[ "$second_current" != "$first_current" ]
[ "$(readlink "$install_dir/previous")" = "$first_current" ]
[ "$(HOME="$home" "$cli" marker)" = two ]
[ "$(readlink "$provider")" = "$install_dir/current/share/chancery/email" ]

: >"$home/fail-installed"
if deploy "$candidate_three" >"$temporary/failed.out" 2>"$temporary/failed.err"; then
    printf '%s\n' 'test: failing installed candidate was accepted' >&2
    exit 1
fi
[ "$(readlink "$install_dir/current")" = "$second_current" ]
[ "$(readlink "$install_dir/previous")" = "$first_current" ]
[ "$(readlink "$provider")" = "$install_dir/current/share/chancery/email" ]
rm "$home/fail-installed"
[ "$(HOME="$home" "$cli" marker)" = two ]

deploy "$candidate_one" >/dev/null
[ "$(readlink "$install_dir/current")" = "$first_current" ]
[ "$(readlink "$install_dir/previous")" = "$second_current" ]
[ "$(HOME="$home" "$cli" marker)" = one ]
[ "$(readlink "$provider")" = "$install_dir/current/share/chancery/email" ]

mkdir "$install_dir/.update-lock"
if deploy "$candidate_one" >"$temporary/lock.out" 2>"$temporary/lock.err"; then
    printf '%s\n' 'test: deployment ignored its update lock' >&2
    exit 1
fi
rmdir "$install_dir/.update-lock"

if "$DEPLOYER" --binary email --home "$home" >/dev/null 2>&1; then
    printf '%s\n' 'test: relative candidate path was accepted' >&2
    exit 1
fi

rm "$provider"
ln -s "$temporary/foreign-provider" "$provider"
if deploy "$candidate_one" >"$temporary/foreign.out" 2>"$temporary/foreign.err"; then
    printf '%s\n' 'test: foreign Email provider selector was accepted' >&2
    exit 1
fi

printf '%s\n' 'test-deploy-user.sh: green'
