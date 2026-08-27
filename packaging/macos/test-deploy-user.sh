#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
temporary=$(mktemp -d "${TMPDIR:-/tmp}/todo-deploy-test.XXXXXX")
cleanup() {
    rm -rf "$temporary"
}
trap cleanup EXIT HUP INT TERM

package="$temporary/package"
home="$temporary/Operator Home"
candidate="$temporary/todo-candidate"
mkdir -p "$package" "$home"
cp "$SCRIPT_DIR/deploy-user.sh" "$package/deploy-user.sh"
cp "$SCRIPT_DIR/todo" "$package/todo"
chmod 0755 "$package/deploy-user.sh" "$package/todo"

cat >"$candidate" <<'EOF'
#!/bin/sh
set -eu
config=
json=0
while [ "$#" -gt 0 ]; do
    case "$1" in
        --version)
            printf '%s\n' 'todo 0.0.0-test'
            exit 0
            ;;
        --config)
            config=$2
            shift 2
            ;;
        --config=*)
            config=${1#*=}
            shift
            ;;
        --json)
            json=1
            shift
            ;;
        --quiet|-v|--verbose)
            shift
            ;;
        *) break ;;
    esac
done
[ -n "$config" ] || config=${TODO_CONFIG:?}
state=$(CDPATH='' cd "$(dirname "$config")" && pwd)
command=${1:?}
shift
printf '%s\n' "$command" >>"$state/commands.log"
case "$command" in
    init)
        : >"$state/todo.db"
        ;;
    list)
        [ -f "$state/todo.db" ]
        [ "$json" -eq 1 ]
        [ "${1:-}" = --limit ]
        [ "${2:-}" = 1 ]
        printf '%s\n' '{"ok":true,"data":[]}'
        ;;
    *) exit 1 ;;
esac
EOF
chmod 0755 "$candidate"

mkdir -p "$home/.local/bin"
cat >"$home/.local/bin/nucleus" <<'EOF'
#!/bin/sh
[ "${1:-}" = --compact ] || exit 1
[ "${2:-}" = health ] || exit 1
printf '%s\n' '{"version":1,"status":"ok","daemonVersion":"test","acceptingJobs":true,"checkedAt":"2026-08-27T00:00:00Z","supportedProtocolVersions":[1],"harness":{"harness":"codex","harnessVersion":"0.146.0","adapterVersion":"test"},"capabilities":["exact-model","reasoning-effort","workspace-read-only","builtin-local-execution","builtin-web-search","dynamic-client-tools","developer-instructions","experimental-raw-events","persistent-file-authentication"],"authentication":{"codexHome":"/tmp/codex-home","configured":true,"authenticated":true}}'
EOF
chmod 0755 "$home/.local/bin/nucleus"

deploy() {
    HOME="$home" "$package/deploy-user.sh" \
        --binary "$1" \
        --home "$home"
}

deploy "$candidate" >/dev/null
state="$home/Library/Application Support/Todo"
cli="$home/.local/bin/todo"
[ -L "$cli" ]
[ -L "$state/install/current" ]
[ ! -e "$state/install/previous" ]
[ -x "$state/install/current/bin/todo" ]
[ -x "$state/install/current/libexec/todo" ]
[ -x "$state/install/current/package/todo" ]
[ -f "$state/install/current/package/deploy-user.sh" ]
[ -f "$state/install/current/manifest.txt" ]
[ -f "$state/todo.db" ]
grep -Fx 'database = "todo.db"' "$state/config.toml" >/dev/null
grep -Fx '[liaison]' "$state/config.toml" >/dev/null
! grep -F 'codex =' "$state/config.toml" >/dev/null
grep -Fx 'quality = "high"' "$state/config.toml" >/dev/null
[ "$(tail -n 2 "$state/commands.log" | tr '\n' ' ')" = 'init list ' ]
HOME="$home" "$cli" --json list --limit 1 >/dev/null

first_release=$(readlink "$state/install/current")
HOME="$home" "$state/install/current/package/deploy-user.sh" \
    --binary "$state/install/current/libexec/todo" \
    --home "$home" >/dev/null
[ "$(readlink "$state/install/current")" = "$first_release" ]
[ ! -e "$state/install/previous" ]
[ "$(grep -c '^init$' "$state/commands.log")" -eq 1 ]

printf '%s\n' '# second release' >>"$candidate"
deploy "$candidate" >/dev/null
second_release=$(readlink "$state/install/current")
[ "$second_release" != "$first_release" ]
[ "$(readlink "$state/install/previous")" = "$first_release" ]
[ "$(grep -c '^init$' "$state/commands.log")" -eq 1 ]

nucleus_cli="$home/.local/bin/nucleus"
cp "$nucleus_cli" "$temporary/nucleus-healthy"
cat >"$nucleus_cli" <<'EOF'
#!/bin/sh
printf '%s\n' '{"version":1,"status":"degraded","acceptingJobs":false,"supportedProtocolVersions":[1],"authentication":{"authenticated":false}}'
EOF
chmod 0755 "$nucleus_cli"
if deploy "$candidate" >"$temporary/degraded.out" 2>"$temporary/degraded.err"; then
    printf '%s\n' 'deployment unexpectedly accepted a degraded Nucleus service' >&2
    exit 1
fi
[ "$(readlink "$state/install/current")" = "$second_release" ]
[ "$(readlink "$state/install/previous")" = "$first_release" ]
install -m 0755 "$temporary/nucleus-healthy" "$nucleus_cli"

printf '%s\n' '# tampered payload' >>"$state/install/current/libexec/todo"
if deploy "$candidate" >"$temporary/tampered.out" 2>"$temporary/tampered.err"; then
    printf '%s\n' 'deployment unexpectedly accepted a tampered release' >&2
    exit 1
fi
[ "$(readlink "$state/install/current")" = "$second_release" ]
[ "$(readlink "$state/install/previous")" = "$first_release" ]
[ ! -e "$state/install/.update-lock" ]
install -m 0755 "$candidate" "$state/install/current/libexec/todo"

failed="$temporary/todo-failed-candidate"
cat >"$failed" <<'EOF'
#!/bin/sh
set -eu
for argument in "$@"; do
    [ "$argument" != --version ] || {
        printf '%s\n' 'todo 0.0.0-failed'
        exit 0
    }
done
case " $* " in
    *' init '*) exit 0 ;;
    *) exit 1 ;;
esac
EOF
chmod 0755 "$failed"
if deploy "$failed" >"$temporary/failed.out" 2>"$temporary/failed.err"; then
    printf '%s\n' 'deployment unexpectedly accepted a failed smoke test' >&2
    exit 1
fi
[ "$(readlink "$state/install/current")" = "$second_release" ]
[ "$(readlink "$state/install/previous")" = "$first_release" ]
[ ! -e "$state/install/.update-lock" ]

printf '%s\n' 'deploy test passed'
