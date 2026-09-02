#!/bin/sh

set -eu

EXPECTED_RUST_VERSION=1.97.1
SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
WORKSPACE_DIR=$(CDPATH='' cd "$SCRIPT_DIR/.." && pwd)
WORKSPACE_MANIFEST="$WORKSPACE_DIR/Cargo.toml"
WORKSPACE_LOCK="$WORKSPACE_DIR/Cargo.lock"

cd "$SCRIPT_DIR"

if ! command -v cargo >/dev/null 2>&1; then
    cargo_home=${CARGO_HOME:-}
    if [ -z "$cargo_home" ] && [ -n "${HOME:-}" ]; then
        cargo_home="$HOME/.cargo"
    fi
    if [ -n "$cargo_home" ] && [ -x "$cargo_home/bin/cargo" ]; then
        PATH="$cargo_home/bin:$PATH"
        export PATH
    fi
fi

for tool in cargo rustc; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        printf 'ci.sh: required tool not found: %s\n' "$tool" >&2
        exit 1
    fi
done

case "$(rustc --version)" in
    "rustc $EXPECTED_RUST_VERSION "*) ;;
    *)
        printf 'ci.sh: Rust %s is required; found %s\n' \
            "$EXPECTED_RUST_VERSION" "$(rustc --version)" >&2
        exit 1
        ;;
esac

case "$(cargo --version)" in
    "cargo $EXPECTED_RUST_VERSION "*) ;;
    *)
        printf 'ci.sh: Cargo %s is required; found %s\n' \
            "$EXPECTED_RUST_VERSION" "$(cargo --version)" >&2
        exit 1
        ;;
esac

[ -f "$WORKSPACE_MANIFEST" ] || {
    printf 'ci.sh: workspace manifest not found: %s\n' \
        "$WORKSPACE_MANIFEST" >&2
    exit 1
}

if [ ! -f "$WORKSPACE_LOCK" ]; then
    printf 'ci.sh: workspace lockfile not found: %s\n' \
        "$WORKSPACE_LOCK" >&2
    exit 1
fi

export CARGO_BUILD_WARNINGS=deny

printf '%s\n' '==> shell and packaging'
for script in \
    release.sh \
    packaging/macos/deploy-user.sh \
    packaging/macos/test-deploy-user.sh
do
    [ -f "$script" ] || {
        printf 'ci.sh: required script not found: %s\n' "$script" >&2
        exit 1
    }
    sh -n "$script"
done
packaging/macos/test-deploy-user.sh

printf '%s\n' '==> Chancery provider bundle'
workspace_version=$(awk '
    $0 == "[workspace.package]" { in_package = 1; next }
    in_package && /^\[/ { exit }
    in_package && /^[[:space:]]*version[[:space:]]*=/ {
        value = $0
        sub(/^[^=]*=[[:space:]]*"/, "", value)
        sub(/"[[:space:]]*$/, "", value)
        print value
        exit
    }
' "$WORKSPACE_MANIFEST")
provider_release=$(awk -F '"' '/"release"[[:space:]]*:/ { print $4; exit }' \
    "$SCRIPT_DIR/chancery/provider.json")
[ -n "$workspace_version" ] && [ "$provider_release" = "$workspace_version" ] || {
    printf 'ci.sh: Nucleus provider release %s does not match workspace version %s\n' \
        "$provider_release" "$workspace_version" >&2
    exit 1
}
cargo run --manifest-path "$WORKSPACE_MANIFEST" \
    --package chancery --locked --quiet -- validate "$SCRIPT_DIR/chancery"

printf '%s\n' '==> rustfmt'
cargo fmt --manifest-path "$WORKSPACE_MANIFEST" \
    --package nucleus-cli \
    --package nucleus-client \
    --package nucleus-core \
    --package nucleus-codex \
    --package nucleus-daemon \
    --package nucleus-store \
    -- --check

printf '%s\n' '==> clippy'
cargo clippy --manifest-path "$WORKSPACE_MANIFEST" \
    --package nucleus-cli \
    --package nucleus-client \
    --package nucleus-core \
    --package nucleus-codex \
    --package nucleus-daemon \
    --package nucleus-store \
    --all-targets --locked --keep-going -- \
    -D warnings \
    -F unsafe_code \
    -D clippy::all \
    -D clippy::pedantic \
    -D clippy::dbg_macro \
    -D clippy::todo \
    -D clippy::unimplemented \
    -D clippy::unwrap_used \
    -D clippy::expect_used

printf '%s\n' '==> tests'
cargo test --manifest-path "$WORKSPACE_MANIFEST" \
    --package nucleus-cli \
    --package nucleus-client \
    --package nucleus-core \
    --package nucleus-codex \
    --package nucleus-daemon \
    --package nucleus-store \
    --locked --no-fail-fast

printf '%s\n' '==> rustdoc'
RUSTDOCFLAGS='-D warnings' cargo doc --manifest-path "$WORKSPACE_MANIFEST" \
    --package nucleus-cli \
    --package nucleus-client \
    --package nucleus-core \
    --package nucleus-codex \
    --package nucleus-daemon \
    --package nucleus-store \
    --no-deps --locked

printf '%s\n' '==> release build'
cargo build --manifest-path "$WORKSPACE_MANIFEST" \
    --package nucleus-cli \
    --package nucleus-client \
    --package nucleus-core \
    --package nucleus-codex \
    --package nucleus-daemon \
    --package nucleus-store \
    --release --locked

printf '%s\n' 'ci.sh: green'
