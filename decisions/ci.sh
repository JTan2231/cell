#!/bin/sh

set -eu

EXPECTED_RUST_VERSION=1.97.1
SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
WORKSPACE_DIR=$(CDPATH='' cd "$SCRIPT_DIR/.." && pwd)

PATH=/Users/joey/.cargo/bin:$PATH
export PATH CARGO_BUILD_WARNINGS=deny
cd "$SCRIPT_DIR"
for tool in cargo rustc; do command -v "$tool" >/dev/null || { printf 'missing %s\n' "$tool" >&2; exit 1; }; done
case "$(rustc --version)" in "rustc $EXPECTED_RUST_VERSION "*) ;; *) printf '%s\n' 'wrong rustc version' >&2; exit 1 ;; esac
case "$(cargo --version)" in "cargo $EXPECTED_RUST_VERSION "*) ;; *) printf '%s\n' 'wrong cargo version' >&2; exit 1 ;; esac

printf '%s\n' '==> shell and packaging'
for script in release.sh packaging/macos/decisions packaging/macos/deploy-user.sh packaging/macos/uninstall-user.sh packaging/macos/test-frontend.sh packaging/macos/test-scheduled-runner.sh packaging/macos/test-observer-runner.sh packaging/macos/test-deploy-user.sh; do
    sh -n "$script"
done
/bin/sh -n packaging/macos/decisions-daily-email
/bin/sh -n packaging/macos/decisions-observer
plutil -lint packaging/macos/org.decisions.daily-email.plist >/dev/null
plutil -lint packaging/macos/org.decisions.observer.plist >/dev/null
plutil -convert binary1 -o /dev/null -- packaging/macos/hooks.json
packaging/macos/test-frontend.sh
packaging/macos/test-scheduled-runner.sh
packaging/macos/test-observer-runner.sh
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
' crates/decisions/Cargo.toml)
provider_version=$(awk -F '"' '/"release"[[:space:]]*:/ { print $4; exit }' chancery/provider.json)
[ "$package_version" = "$provider_version" ]
cargo run --manifest-path "$WORKSPACE_DIR/Cargo.toml" --package chancery --locked --quiet -- validate "$SCRIPT_DIR/chancery"
registry=$(mktemp -d)
trap 'rm -rf "$registry"' EXIT HUP INT TERM
ln -s "$SCRIPT_DIR/chancery" "$registry/decisions"
ln -s "$WORKSPACE_DIR/conversations/chancery" "$registry/conversations"
ln -s "$WORKSPACE_DIR/nucleus/chancery" "$registry/nucleus"
ln -s "$WORKSPACE_DIR/email/chancery" "$registry/email"
catalog=$(cargo run --manifest-path "$WORKSPACE_DIR/Cargo.toml" --package chancery --locked --quiet -- --registry "$registry" --json list)
case "$catalog" in *'"id":"decisions.daily.digest"'*) ;; *) printf '%s\n' 'daily digest missing from catalog' >&2; exit 1 ;; esac
case "$catalog" in *'"id":"decisions.lifecycle.consume"'*) ;; *) printf '%s\n' 'lifecycle stream missing from catalog' >&2; exit 1 ;; esac
resolution=$(cargo run --manifest-path "$WORKSPACE_DIR/Cargo.toml" --package chancery --locked --quiet -- --registry "$registry" --json resolve decisions.lifecycle.consume --require completeness_and_freshness)
case "$resolution" in *'"status":"resolved_not_ready"'*) ;; *) printf '%s\n' 'lifecycle promise did not resolve' >&2; exit 1 ;; esac
case "$resolution" in *'"state":"mixed"'*) ;; *) printf '%s\n' 'lifecycle promise did not retain mixed declared and unspecified facets' >&2; exit 1 ;; esac

printf '%s\n' '==> rustfmt'
cargo fmt --manifest-path "$WORKSPACE_DIR/Cargo.toml" --package decisions -- --check
printf '%s\n' '==> clippy'
cargo clippy --manifest-path "$WORKSPACE_DIR/Cargo.toml" --package decisions --all-targets --locked -- -D warnings -F unsafe_code -D clippy::all -D clippy::pedantic -D clippy::dbg_macro -D clippy::todo -D clippy::unimplemented -D clippy::unwrap_used -D clippy::expect_used
printf '%s\n' '==> tests'
cargo test --manifest-path "$WORKSPACE_DIR/Cargo.toml" --package decisions --locked
printf '%s\n' '==> rustdoc'
RUSTDOCFLAGS='-D warnings' cargo doc --manifest-path "$WORKSPACE_DIR/Cargo.toml" --package decisions --no-deps --locked
printf '%s\n' '==> release build'
cargo build --manifest-path "$WORKSPACE_DIR/Cargo.toml" --package decisions --release --locked
printf '%s\n' 'ci.sh: green'
