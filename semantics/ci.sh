#!/bin/sh

set -eu

EXPECTED_RUST_VERSION=1.97.1
SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
WORKSPACE_DIR=$(CDPATH='' cd "$SCRIPT_DIR/.." && pwd)

PATH=/Users/joey/.cargo/bin:$PATH
export PATH CARGO_BUILD_WARNINGS=deny CARGO_NET_OFFLINE=true
cd "$SCRIPT_DIR"
for tool in cargo rustc; do
    command -v "$tool" >/dev/null || { printf 'missing %s\n' "$tool" >&2; exit 1; }
done
case "$(rustc --version)" in "rustc $EXPECTED_RUST_VERSION "*) ;; *) printf '%s\n' 'wrong rustc version' >&2; exit 1 ;; esac
case "$(cargo --version)" in "cargo $EXPECTED_RUST_VERSION "*) ;; *) printf '%s\n' 'wrong cargo version' >&2; exit 1 ;; esac

printf '%s\n' '==> shell and packaging'
for script in \
    release.sh \
    packaging/macos/semantics \
    packaging/macos/deploy-user.sh \
    packaging/macos/uninstall-user.sh \
    packaging/macos/test-frontend.sh \
    packaging/macos/test-worker-runner.sh \
    packaging/macos/test-deploy-user.sh
do
    sh -n "$script"
done
/bin/zsh -n packaging/macos/semantics-worker
plutil -lint packaging/macos/org.semantics.worker.plist >/dev/null
packaging/macos/test-frontend.sh
packaging/macos/test-worker-runner.sh
packaging/macos/test-deploy-user.sh

printf '%s\n' '==> Chancery provider bundle'
package_version=$(awk '
    $0 == "[package]" { in_package = 1; next }
    in_package && /^version[[:space:]]*=/ {
        value = $0
        sub(/^[^=]*=[[:space:]]*"/, "", value)
        sub(/"[[:space:]]*$/, "", value)
        print value
        exit
    }
' Cargo.toml)
provider_version=$(awk -F '"' '/"release"[[:space:]]*:/ { print $4; exit }' chancery/provider.json)
[ "$package_version" = "$provider_version" ]
cargo run --manifest-path "$WORKSPACE_DIR/Cargo.toml" --package chancery --locked --offline --quiet -- validate "$SCRIPT_DIR/chancery"
registry=$(mktemp -d)
trap 'rm -rf "$registry"' EXIT HUP INT TERM
ln -s "$SCRIPT_DIR/chancery" "$registry/semantics"
ln -s "$WORKSPACE_DIR/decisions/chancery" "$registry/decisions"
ln -s "$WORKSPACE_DIR/conversations/chancery" "$registry/conversations"
ln -s "$WORKSPACE_DIR/nucleus/chancery" "$registry/nucleus"
catalog=$(cargo run --manifest-path "$WORKSPACE_DIR/Cargo.toml" --package chancery --locked --offline --quiet -- --registry "$registry" --json list)
for entry_id in semantics.repository.explore semantics.project.operate semantics.develop.change; do
    case "$catalog" in *"\"id\":\"$entry_id\""*) ;; *) printf 'catalog entry missing: %s\n' "$entry_id" >&2; exit 1 ;; esac
done

printf '%s\n' '==> rustfmt'
cargo fmt --manifest-path "$SCRIPT_DIR/Cargo.toml" --package semantics -- --check
printf '%s\n' '==> clippy'
cargo clippy --manifest-path "$SCRIPT_DIR/Cargo.toml" --package semantics --all-targets --locked --offline -- \
    -D warnings -F unsafe_code -D clippy::all -D clippy::pedantic -D clippy::dbg_macro \
    -D clippy::todo -D clippy::unimplemented -D clippy::unwrap_used -D clippy::expect_used
printf '%s\n' '==> tests'
cargo test --manifest-path "$SCRIPT_DIR/Cargo.toml" --package semantics --locked --offline
printf '%s\n' '==> rustdoc'
RUSTDOCFLAGS='-D warnings' cargo doc --manifest-path "$SCRIPT_DIR/Cargo.toml" --package semantics --no-deps --locked --offline
printf '%s\n' '==> release build'
cargo build --manifest-path "$SCRIPT_DIR/Cargo.toml" --package semantics --release --locked --offline
printf '%s\n' 'ci.sh: green'
