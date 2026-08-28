#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
temporary=$(mktemp -d "${TMPDIR:-/tmp}/nucleus-deploy-test.XXXXXX")
cleanup() {
    rm -rf "$temporary"
}
trap cleanup EXIT HUP INT TERM

home="$temporary/Operator Home"
codex_home="$temporary/Codex Home"
binary="$temporary/nucleus candidate"
daemon="$temporary/nucleusd candidate"
codex="$temporary/codex candidate"
capture="$temporary/capture"
mkdir -p "$home" "$codex_home"

cat >"$binary" <<'EOF'
#!/bin/sh
set -eu
case "${1:-}" in
    --version)
        printf '%s\n' 'nucleus 0.1.0-test'
        ;;
    service)
        [ "${2:-}" = install ]
        shift 2
        : >"${NUCLEUS_DEPLOY_CAPTURE:?}"
        printf 'home=%s\n' "${HOME:-}" >>"$NUCLEUS_DEPLOY_CAPTURE"
        for argument in "$@"; do
            printf 'argument=%s\n' "$argument" >>"$NUCLEUS_DEPLOY_CAPTURE"
        done
        [ "${NUCLEUS_DEPLOY_FAIL:-0}" -eq 0 ] || exit 42
        printf '%s\n' '{"installed":true}'
        ;;
    *) exit 1 ;;
esac
EOF

cat >"$daemon" <<'EOF'
#!/bin/sh
case "${1:-}" in
    --version) printf '%s\n' 'nucleusd 0.1.0-test' ;;
    *) exit 1 ;;
esac
EOF

cat >"$codex" <<'EOF'
#!/bin/sh
case "${1:-}" in
    --version) printf '%s\n' 'codex test' ;;
    *) exit 1 ;;
esac
EOF
chmod 0755 "$binary" "$daemon" "$codex"

NUCLEUS_DEPLOY_CAPTURE="$capture" \
    "$SCRIPT_DIR/deploy-user.sh" \
    --binary "$binary" \
    --daemon "$daemon" \
    --codex "$codex" \
    --codex-home "$codex_home" \
    --home "$home" >"$temporary/install.out"
printf '%s\n' '{"installed":true}' >"$temporary/install.expected"
diff -u "$temporary/install.expected" "$temporary/install.out"

expected="$temporary/expected"
{
    printf 'home=%s\n' "$home"
    printf '%s\n' 'argument=--daemon' "argument=$daemon"
    printf '%s\n' 'argument=--codex' "argument=$codex"
    printf '%s\n' 'argument=--codex-home' "argument=$codex_home"
} >"$expected"
diff -u "$expected" "$capture"

NUCLEUS_DEPLOY_CAPTURE="$capture" \
    "$SCRIPT_DIR/deploy-user.sh" \
    --binary "$binary" \
    --daemon "$daemon" \
    --codex "$codex" \
    --home "$home" >/dev/null
if grep -F -- '--codex-home' "$capture" >/dev/null; then
    printf '%s\n' 'deployment unexpectedly forwarded an omitted Codex home' >&2
    exit 1
fi

lock="$home/Library/Application Support/Nucleus/.deploy-lock"
[ ! -e "$lock" ]
if NUCLEUS_DEPLOY_CAPTURE="$capture" NUCLEUS_DEPLOY_FAIL=1 \
    "$SCRIPT_DIR/deploy-user.sh" \
    --binary "$binary" \
    --daemon "$daemon" \
    --codex "$codex" \
    --home "$home" >"$temporary/failure.out" 2>"$temporary/failure.err"
then
    printf '%s\n' 'deployment unexpectedly hid an installer failure' >&2
    exit 1
fi
[ ! -e "$lock" ]

mkdir "$lock"
if NUCLEUS_DEPLOY_CAPTURE="$capture" \
    "$SCRIPT_DIR/deploy-user.sh" \
    --binary "$binary" \
    --daemon "$daemon" \
    --codex "$codex" \
    --home "$home" >"$temporary/locked.out" 2>"$temporary/locked.err"
then
    printf '%s\n' 'deployment unexpectedly ignored the update lock' >&2
    exit 1
fi
grep -F 'another deployment holds' "$temporary/locked.err" >/dev/null
rmdir "$lock"

mismatched_daemon="$temporary/mismatched nucleusd"
cat >"$mismatched_daemon" <<'EOF'
#!/bin/sh
printf '%s\n' 'nucleusd 9.9.9'
EOF
chmod 0755 "$mismatched_daemon"
if NUCLEUS_DEPLOY_CAPTURE="$capture" \
    "$SCRIPT_DIR/deploy-user.sh" \
    --binary "$binary" \
    --daemon "$mismatched_daemon" \
    --codex "$codex" \
    --home "$home" >"$temporary/mismatch.out" 2>"$temporary/mismatch.err"
then
    printf '%s\n' 'deployment unexpectedly accepted mismatched binaries' >&2
    exit 1
fi
grep -F 'candidate versions do not match' "$temporary/mismatch.err" >/dev/null

if "$SCRIPT_DIR/deploy-user.sh" \
    --binary relative/nucleus \
    --daemon "$daemon" \
    --codex "$codex" \
    --home "$home" >"$temporary/relative.out" 2>"$temporary/relative.err"
then
    printf '%s\n' 'deployment unexpectedly accepted a relative binary path' >&2
    exit 1
fi
grep -F -- '--binary must be an absolute path' "$temporary/relative.err" >/dev/null

printf '%s\n' 'deploy test passed'
