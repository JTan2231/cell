#!/bin/sh

set -eu

MAX_SECONDS=60
EXPECTED_RUST_VERSION=1.97.1
SCRIPT_DIR=$(CDPATH= cd "$(dirname "$0")" && pwd)
SCRIPT_PATH="$SCRIPT_DIR/$(basename "$0")"

# Run the complete check suite under one wall-clock deadline. GNU/BusyBox
# timeout covers Linux; Perl provides the same process-group timeout on macOS.
if [ "${ANNALS_CI_TIMEOUT_ACTIVE:-0}" != "1" ]; then
    export ANNALS_CI_TIMEOUT_ACTIVE=1

    timeout_command=""
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
                printf 'ci.sh: exceeded the %s-second runtime limit\n' "$MAX_SECONDS" >&2
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

    printf 'ci.sh: timeout or Perl is required to enforce the %s-second limit\n' "$MAX_SECONDS" >&2
    exit 1
fi

cd "$SCRIPT_DIR"

# Rustup was installed without editing shell startup files. Make its usual
# Cargo directory available when it is present but not already on PATH.
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

if [ ! -f Cargo.lock ]; then
    printf 'ci.sh: Cargo.lock is required for reproducible builds\n' >&2
    exit 1
fi

export CARGO_BUILD_WARNINGS=deny

printf '%s\n' '==> rustfmt'
cargo fmt --all -- --check

printf '%s\n' '==> clippy'
cargo clippy --all-targets --locked --keep-going -- \
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
cargo test --locked --no-fail-fast

printf '%s\n' '==> rustdoc'
RUSTDOCFLAGS='-D warnings' cargo doc --no-deps --locked

printf '%s\n' '==> release build'
cargo build --release --locked

printf '%s\n' 'ci.sh: green'
