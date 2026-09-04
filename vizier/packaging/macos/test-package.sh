#!/bin/sh
# Focused disposable package/publication proof. It exercises the existing
# selector-only deployer; it does not publish a release or test runtime readiness.
set -eu

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
ROOT=$(CDPATH='' cd "$SCRIPT_DIR/../../.." && pwd)
temporary=$(mktemp -d "${TMPDIR:-/tmp}/vizier-package-test.XXXXXX")
home="$temporary/home"
registry="$home/Library/Application Support/Chancery/providers"
candidate_template="$temporary/vizier.template"
candidate_one="$temporary/vizier-one"
candidate_two="$temporary/vizier-two"
candidate_fail="$temporary/vizier-fail"

cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    rm -rf "$temporary"
    exit "$status"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

# The candidate reader validates the source bundle before packaging. Preserve a
# digest so the install/update/rollback fixture can prove it did not rewrite it.
bundle_digest_before=$(find "$ROOT/vizier/chancery" -type f -exec shasum -a 256 {} \; | sort | shasum -a 256 | awk '{print $1}')
cargo run --quiet -p chancery --manifest-path "$ROOT/Cargo.toml" -- \
    validate "$ROOT/vizier/chancery" >/dev/null
bundle_digest_after=$(find "$ROOT/vizier/chancery" -type f -exec shasum -a 256 {} \; | sort | shasum -a 256 | awk '{print $1}')
[ "$bundle_digest_before" = "$bundle_digest_after" ]

mkdir -p "$home" "$registry"
# Supply the compatible reader with the declared dependency providers; the deployer
# supplies the installed Vizier selector under test.
cp -R "$ROOT/nucleus/chancery" "$registry/nucleus"
cp -R "$ROOT/semantics/chancery" "$registry/semantics"

version=$(awk '
    $0 == "[package]" { in_package = 1; next }
    in_package && /^\[/ { exit }
    in_package && /^[[:space:]]*version[[:space:]]*=/ {
        value = $0; sub(/^[^=]*=[[:space:]]*"/, "", value)
        sub(/"[[:space:]]*$/, "", value); print value; exit
    }
' "$ROOT/vizier/crates/vizier/Cargo.toml")
[ -n "$version" ]

cat >"$candidate_template" <<'SCRIPT'
#!/bin/sh
set -eu
case "$0" in
    *'/.local/bin/vizier') [ ! -f "${HOME:?}/fail-installed" ] || exit 70 ;;
esac
case "${1:-}" in
    --version) printf '%s\n' 'vizier @VERSION@'; exit 0 ;;
    --help) exit 0 ;;
esac
printf '%s\n' '@MARKER@'
SCRIPT
make_candidate() {
    sed -e "s/@VERSION@/$version/g" -e "s/@MARKER@/$2/g" \
        "$candidate_template" >"$1"
    chmod 0755 "$1"
}
make_candidate "$candidate_one" one
make_candidate "$candidate_two" two
make_candidate "$candidate_fail" failed

deploy() {
    HOME="$home" "$SCRIPT_DIR/deploy-user.sh" --binary "$1" --home "$home"
}

deploy "$candidate_one" >/dev/null
install="$home/Library/Application Support/Vizier/install"
first=$(readlink "$install/current")
[ -L "$registry/vizier" ]
[ "$(readlink "$registry/vizier")" = "$install/current/share/chancery/vizier" ]

deploy "$candidate_two" >/dev/null
second=$(readlink "$install/current")
[ "$second" != "$first" ]
[ "$(readlink "$install/previous")" = "$first" ]
: >"$home/fail-installed"
if deploy "$candidate_fail" >/dev/null 2>&1; then
    echo 'test: failed installed smoke was accepted' >&2
    exit 1
fi
rm "$home/fail-installed"
[ "$(readlink "$install/current")" = "$second" ]
[ "$(readlink "$install/previous")" = "$first" ]

# These are installed-registry presentation and dossier observations only.
# They do not establish Nucleus readiness, Vizier readiness, or run success.
reader() {
    cargo run --quiet -p chancery --manifest-path "$ROOT/Cargo.toml" -- \
        --registry "$registry" "$@"
}
reader doctor >"$temporary/doctor.out" 2>&1 || true
grep -F 'Chancery registry' "$temporary/doctor.out" >/dev/null
reader list | grep -F 'vizier.implementation.delegate' >/dev/null
for id in vizier.implementation.delegate vizier.workflow.operate vizier.develop.change; do
    reader show "$id" | grep -F "$id" >/dev/null
    reader resolve "$id" >"$temporary/$id.resolve" 2>&1 || true
    grep -F "$id" "$temporary/$id.resolve" >/dev/null
    grep -F 'Resolved outward promise' "$temporary/$id.resolve" >/dev/null
done

echo 'test-package.sh: green'
