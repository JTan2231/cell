#!/bin/sh

set -eu

MAX_SECONDS=60
EXPECTED_RUST_VERSION=1.97.1
SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
WORKSPACE_DIR=$(CDPATH='' cd "$SCRIPT_DIR/.." && pwd)
WORKSPACE_MANIFEST="$WORKSPACE_DIR/Cargo.toml"
WORKSPACE_LOCK="$WORKSPACE_DIR/Cargo.lock"
SCRIPT_PATH="$SCRIPT_DIR/$(basename "$0")"

if [ "${CHANCERY_CI_TIMEOUT_ACTIVE:-0}" != 1 ]; then
    export CHANCERY_CI_TIMEOUT_ACTIVE=1

    timeout_command=
    if command -v timeout >/dev/null 2>&1; then
        timeout_command=$(command -v timeout)
    elif command -v gtimeout >/dev/null 2>&1; then
        timeout_command=$(command -v gtimeout)
    fi

    if [ -n "$timeout_command" ]; then
        if "$timeout_command" -s KILL "$MAX_SECONDS" "$SCRIPT_PATH" "$@"; then
            exit 0
        else
            status=$?
            if [ "$status" -eq 124 ] || [ "$status" -eq 137 ]; then
                printf 'ci.sh: exceeded the %s-second runtime limit\n' \
                    "$MAX_SECONDS" >&2
            fi
            exit "$status"
        fi
    fi

    if command -v perl >/dev/null 2>&1; then
        exec perl -MPOSIX=setpgid -e '
            use strict;
            use warnings;
            my ($limit, @command) = @ARGV;
            my $pid = fork();
            die "ci.sh: unable to start timeout wrapper: $!\n" if !defined $pid;
            if ($pid == 0) {
                setpgid(0, 0);
                exec @command;
                die "ci.sh: unable to run @command: $!\n";
            }
            setpgid($pid, $pid);
            local $SIG{ALRM} = sub {
                print STDERR "ci.sh: exceeded the ${limit}-second runtime limit\n";
                kill "KILL", -$pid;
                waitpid($pid, 0);
                exit 124;
            };
            alarm $limit;
            waitpid($pid, 0);
            my $status = $?;
            alarm 0;
            exit(128 + ($status & 127)) if $status & 127;
            exit($status >> 8);
        ' "$MAX_SECONDS" "$SCRIPT_PATH" "$@"
    fi

    printf 'ci.sh: timeout or Perl is required to enforce the %s-second limit\n' \
        "$MAX_SECONDS" >&2
    exit 1
fi

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
    command -v "$tool" >/dev/null 2>&1 || {
        printf 'ci.sh: required tool not found: %s\n' "$tool" >&2
        exit 1
    }
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
    printf '%s\n' 'ci.sh: root workspace Cargo.toml is required' >&2
    exit 1
}
[ -f "$WORKSPACE_LOCK" ] || {
    printf '%s\n' 'ci.sh: root Cargo.lock is required' >&2
    exit 1
}

export CARGO_BUILD_WARNINGS=deny

printf '%s\n' '==> shell and packaging'
for script in \
    release.sh \
    packaging/macos/deploy-user.sh \
    packaging/macos/test-deploy-user.sh \
    packaging/macos/test-fixture-chancery.sh
do
    [ -f "$script" ] || {
        printf 'ci.sh: required script not found: %s\n' "$script" >&2
        exit 1
    }
    sh -n "$script"
done
package_version=$(awk '
    $0 == "[package]" { in_package = 1; next }
    in_package && /^\[/ { exit }
    in_package && /^[[:space:]]*version[[:space:]]*=/ {
        version = $0
        sub(/^[^=]*=[[:space:]]*"/, "", version)
        sub(/"[[:space:]]*$/, "", version)
        print version
        exit
    }
' crates/chancery/Cargo.toml)
provider_release=$(awk -F '"' '/"release"[[:space:]]*:/ { print $4; exit }' \
    provider/provider.json)
[ -n "$package_version" ] && [ "$provider_release" = "$package_version" ] || {
    printf 'ci.sh: provider release %s does not match package version %s\n' \
        "$provider_release" "$package_version" >&2
    exit 1
}
if [ "$(uname -s)" = Darwin ]; then
    packaging/macos/test-deploy-user.sh
fi

printf '%s\n' '==> rustfmt'
cargo fmt --manifest-path "$WORKSPACE_MANIFEST" --package chancery -- --check

printf '%s\n' '==> clippy'
cargo clippy --manifest-path "$WORKSPACE_MANIFEST" \
    --package chancery --all-targets --locked --keep-going -- \
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
    --package chancery --locked --no-fail-fast

printf '%s\n' '==> Chancery provider bundle'
cargo run --manifest-path "$WORKSPACE_MANIFEST" \
    --package chancery --locked --quiet -- validate "$SCRIPT_DIR/provider"

printf '%s\n' '==> rustdoc'
RUSTDOCFLAGS='-D warnings' cargo doc --manifest-path "$WORKSPACE_MANIFEST" \
    --package chancery --no-deps --locked

printf '%s\n' '==> release build'
cargo build --manifest-path "$WORKSPACE_MANIFEST" \
    --package chancery --release --locked

printf '%s\n' 'ci.sh: green'
