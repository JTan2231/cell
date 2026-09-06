#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
temporary=$(mktemp -d "${TMPDIR:-/tmp}/annals-decisions-provision-test.XXXXXX")
temporary=$(CDPATH='' cd "$temporary" && pwd -P)

cleanup() {
    rm -rf "$temporary"
}
trap cleanup EXIT HUP INT TERM

home="$temporary/Operator Home"
clockwork="$temporary/clockwork"
nucleus_socket="$temporary/nucleus.sock"
fake_annals="$temporary/fake-annals"
mkdir -p "$home"

cat >"$fake_annals" <<'EOF'
#!/bin/sh
set -eu

library=
config=
while [ "$#" -gt 0 ]; do
    case "$1" in
        --library)
            library=$2
            shift 2
            ;;
        --config)
            config=$2
            shift 2
            ;;
        --json|--quiet)
            shift
            ;;
        --version)
            printf '%s\n' 'annals 0.15.0-test'
            exit 0
            ;;
        *) break ;;
    esac
done

command=${1:?}
shift
library_id=0123456789abcdef0123456789abcdef
if [ -n "$config" ]; then
    state=$(CDPATH='' cd "$(dirname "$config")" && pwd)
    library=$(sed -n 's/^library = "\(.*\)"$/\1/p' "$config")
    spool=$(sed -n 's/^root = "\(.*\)"$/\1/p' "$config")
    printf '%s\n' "$command ${1:-}" >>"$state/fake-annals.log"
fi

case "$command" in
    init)
        [ -n "$library" ]
        [ "${1:-}" = --kind ] && [ "${2:-}" = decisions ]
        : >"$library"
        printf '{"ok":true,"data":{"library":"%s","library_id":"%s","kind":"decisions","revision":0}}\n' \
            "$library" "$library_id"
        ;;
    inbox)
        subcommand=${1:?}
        case "$subcommand" in
            run)
                mkdir -p \
                    "$spool/incoming" "$spool/queued" "$spool/processing" \
                    "$spool/done" "$spool/duplicates" "$spool/failed" \
                    "$spool/skipped"
                if [ ! -e "$spool/.decision-feed-library.json" ]; then
                    printf '{"version":1,"library_id":"%s"}\n' "$library_id" \
                        >"$spool/.decision-feed-library.json"
                fi
                stopped=false
                [ ! -f "$spool/.maintenance" ] || stopped=true
                printf '{"ok":true,"data":{"stopped_for_maintenance":%s}}\n' \
                    "$stopped"
                ;;
            status)
                maintenance=false
                [ ! -f "$spool/.maintenance" ] || maintenance=true
                printf '{"ok":true,"data":{"locked":false,"maintenance":%s}}\n' \
                    "$maintenance"
                ;;
            *) exit 1 ;;
        esac
        ;;
    decision-feed)
        [ "${1:-}" = watermark ]
        printf '{"ok":true,"data":{"library_id":"%s","watermark":"test"}}\n' \
            "$library_id"
        ;;
    backup)
        cp "$library" "${1:?}"
        ;;
    migrate)
        printf '%s\n' migrated >>"$library"
        ;;
    *)
        printf 'unexpected fake Annals command: %s\n' "$command" >&2
        exit 1
        ;;
esac
EOF
chmod 0755 "$fake_annals"

cat >"$clockwork" <<'EOF'
#!/bin/sh
set -eu

[ "${1:-}" = --json ] && shift
root="${HOME:?}/clockwork-test"
binding="$root/annals.decisions-inbox"
log="$HOME/clockwork.log"
mkdir -p "$root"
printf '%s\n' "$*" >>"$log"

command=${1:-}
shift || true
case "$command:${1:-}" in
    definition:register)
        shift
        source=${1:?}
        digest=$(shasum -a 256 "$source" | awk '{print $1}')
        cp "$source" "$root/definition.$digest.toml"
        printf '{"ok":true,"data":{"digest":"%s","key":"annals/decisions-inbox"}}\n' \
            "$digest"
        ;;
    definition:show)
        shift
        digest=${1:?}
        definition="$root/definition.$digest.toml"
        [ -f "$definition" ] || exit 1
        release_id=$(sed -n 's/^release_id = "\(.*\)"$/\1/p' "$definition")
        release_root=$(sed -n 's/^release_root = "\(.*\)"$/\1/p' "$definition")
        cwd=$(sed -n 's/^cwd = "\(.*\)"$/\1/p' "$definition")
        seconds=$(sed -n 's/^seconds = \([0-9][0-9]*\)$/\1/p' "$definition")
        run_at_load=$(sed -n 's/^run_at_load = \(.*\)$/\1/p' "$definition")
        interpreter_hash=$(sed -n \
            's/^interpreter_sha256 = "\(.*\)"$/\1/p' "$definition")
        script=$(sed -n 's/^script = "\(.*\)"$/\1/p' "$definition")
        script_hash=$(sed -n 's/^script_sha256 = "\(.*\)"$/\1/p' "$definition")
        selected_home=$(sed -n 's/^HOME = "\(.*\)"$/\1/p' "$definition")
        selected_user=$(sed -n 's/^USER = "\(.*\)"$/\1/p' "$definition")
        selected_logname=$(sed -n 's/^LOGNAME = "\(.*\)"$/\1/p' "$definition")
        config=$(sed -n 's/^ANNALS_CONFIG = "\(.*\)"$/\1/p' "$definition")
        stdout=$(sed -n 's/^stdout = "\(.*\)"$/\1/p' "$definition")
        stderr=$(sed -n 's/^stderr = "\(.*\)"$/\1/p' "$definition")
        printf '{"ok":true,"data":{"digest":"%s","key":"annals/decisions-inbox","registered_at":1,"manifest":{"schema_version":1,"key":"annals/decisions-inbox","release_id":"%s","release_root":"%s","authority":"current-user-background","overlap":"skip","arguments":[],"cwd":"%s","schedule":{"kind":"interval","seconds":%s,"run_at_load":%s},"launch":{"kind":"interpreted","interpreter":"/bin/sh","interpreter_sha256":"%s","script":"%s","script_sha256":"%s"},"environment":{"HOME":"%s","USER":"%s","LOGNAME":"%s","ANNALS_CONFIG":"%s"},"output":{"stdout":"%s","stderr":"%s"}}}}\n' \
            "$digest" "$release_id" "$release_root" "$cwd" "$seconds" \
            "$run_at_load" "$interpreter_hash" "$script" "$script_hash" \
            "$selected_home" "$selected_user" "$selected_logname" "$config" \
            "$stdout" "$stderr"
        ;;
    binding:show)
        shift
        if [ ! -f "$binding" ]; then
            printf '%s\n' \
                '{"ok":false,"error":{"code":"binding_not_found","message":"absent"}}' >&2
            exit 1
        fi
        enabled=$(sed -n '1p' "$binding")
        digest=$(sed -n '2p' "$binding")
        if [ -n "$digest" ]; then
            digest_json="\"$digest\""
        else
            digest_json=null
        fi
        printf '{"ok":true,"data":{"key":"annals/decisions-inbox","definition_digest":%s,"enabled":%s,"updated_at":1}}\n' \
            "$digest_json" "$enabled"
        ;;
    binding:disable)
        shift
        key=${1:?}
        shift
        digest=
        [ ! -f "$binding" ] || digest=$(sed -n '2p' "$binding")
        if [ "${1:-}" = --select ]; then
            digest=${2:?}
        fi
        printf 'false\n%s\n' "$digest" >"$binding"
        if [ -n "$digest" ]; then
            digest_json="\"$digest\""
        else
            digest_json=null
        fi
        printf '{"ok":true,"data":{"key":"%s","definition_digest":%s,"enabled":false}}\n' \
            "$key" "$digest_json"
        ;;
    binding:switch)
        shift
        key=${1:?}
        digest=${2:?}
        definition="$root/definition.$digest.toml"
        config=$(sed -n 's/^ANNALS_CONFIG = "\(.*\)"$/\1/p' "$definition")
        state=$(CDPATH='' cd "$(dirname "$config")" && pwd)
        [ -f "$state/spool/.maintenance" ] || {
            printf '%s\n' 'candidate was switched without decisions maintenance' >&2
            exit 1
        }
        if [ -f "$HOME/fail-next-clockwork-switch" ]; then
            rm -f "$HOME/fail-next-clockwork-switch"
            exit 1
        fi
        printf 'true\n%s\n' "$digest" >"$binding"
        printf '{"ok":true,"data":{"key":"%s","definition_digest":"%s","enabled":true}}\n' \
            "$key" "$digest"
        ;;
    *) exit 1 ;;
esac
EOF
chmod 0755 "$clockwork"

bundle_hash() {
    bundle=$1
    (
        cd "$bundle"
        find . -type f -print | LC_ALL=C sort | while IFS= read -r file; do
            printf 'path=%s\n' "$file"
            shasum -a 256 "$file"
        done
    ) | shasum -a 256 | awk '{print $1}'
}

build_release() {
    variant=$1
    stage="$temporary/release-stage-$variant"
    mkdir -p \
        "$stage/libexec" "$stage/bin" "$stage/package" \
        "$stage/share/chancery"
    cp "$fake_annals" "$stage/libexec/annals"
    printf '# release variant %s\n' "$variant" >>"$stage/libexec/annals"
    cp "$fake_annals" "$stage/libexec/annals-usage"
    cp "$SCRIPT_DIR/annals-user" "$stage/bin/annals"
    cp "$SCRIPT_DIR/annals-inbox" "$stage/bin/annals-inbox"
    cp "$SCRIPT_DIR/annals-user" "$stage/package/annals-user"
    cp "$SCRIPT_DIR/annals-inbox" "$stage/package/annals-inbox"
    cp "$SCRIPT_DIR/deploy-user.sh" "$stage/package/deploy-user.sh"
    cp "$SCRIPT_DIR/annals-inbox.clockwork.toml.in" \
        "$stage/package/annals-inbox.clockwork.toml.in"
    cp "$SCRIPT_DIR/annals-decisions.toml.in" \
        "$stage/package/annals-decisions.toml.in"
    cp "$SCRIPT_DIR/annals-decisions-inbox.clockwork.toml.in" \
        "$stage/package/annals-decisions-inbox.clockwork.toml.in"
    cp "$SCRIPT_DIR/provision-decisions-user.sh" \
        "$stage/package/provision-decisions-user.sh"
    cp "$SCRIPT_DIR/org.annals.inbox.agent.plist" \
        "$stage/package/org.annals.inbox.agent.plist"
    cp -R "$SCRIPT_DIR/../../chancery/annals" "$stage/share/chancery/annals"
    cp -R "$SCRIPT_DIR/../../chancery/annals-usage" \
        "$stage/share/chancery/annals-usage"
    chmod 0755 \
        "$stage/libexec/annals" "$stage/libexec/annals-usage" \
        "$stage/bin/annals" "$stage/bin/annals-inbox" \
        "$stage/package/annals-user" "$stage/package/annals-inbox" \
        "$stage/package/deploy-user.sh" \
        "$stage/package/provision-decisions-user.sh"

    binary_hash=$(shasum -a 256 "$stage/libexec/annals" | awk '{print $1}')
    usage_hash=$(shasum -a 256 "$stage/libexec/annals-usage" | awk '{print $1}')
    frontend_hash=$(shasum -a 256 "$stage/bin/annals" | awk '{print $1}')
    runner_hash=$(shasum -a 256 "$stage/bin/annals-inbox" | awk '{print $1}')
    template_hash=$(shasum -a 256 \
        "$stage/package/annals-inbox.clockwork.toml.in" | awk '{print $1}')
    decisions_config_hash=$(shasum -a 256 \
        "$stage/package/annals-decisions.toml.in" | awk '{print $1}')
    decisions_template_hash=$(shasum -a 256 \
        "$stage/package/annals-decisions-inbox.clockwork.toml.in" | awk '{print $1}')
    decisions_provisioner_hash=$(shasum -a 256 \
        "$stage/package/provision-decisions-user.sh" | awk '{print $1}')
    plist_hash=$(shasum -a 256 \
        "$stage/package/org.annals.inbox.agent.plist" | awk '{print $1}')
    updater_hash=$(shasum -a 256 "$stage/package/deploy-user.sh" | awk '{print $1}')
    annals_bundle_hash=$(bundle_hash "$stage/share/chancery/annals")
    usage_bundle_hash=$(bundle_hash "$stage/share/chancery/annals-usage")
    release_id=$(printf '%s\n' \
        "$binary_hash" "$usage_hash" "$frontend_hash" "$runner_hash" \
        "$template_hash" "$decisions_config_hash" "$decisions_template_hash" \
        "$decisions_provisioner_hash" "$plist_hash" "$updater_hash" \
        "$annals_bundle_hash" "$usage_bundle_hash" \
        | shasum -a 256 | awk '{print $1}')
    {
        printf '{\n'
        printf '  "format": 4,\n'
        printf '  "release_id": "%s",\n' "$release_id"
        printf '  "binary_sha256": "%s",\n' "$binary_hash"
        printf '  "usage_binary_sha256": "%s",\n' "$usage_hash"
        printf '  "frontend_sha256": "%s",\n' "$frontend_hash"
        printf '  "runner_sha256": "%s",\n' "$runner_hash"
        printf '  "clockwork_template_sha256": "%s",\n' "$template_hash"
        printf '  "decisions_config_template_sha256": "%s",\n' \
            "$decisions_config_hash"
        printf '  "decisions_clockwork_template_sha256": "%s",\n' \
            "$decisions_template_hash"
        printf '  "decisions_provisioner_sha256": "%s",\n' \
            "$decisions_provisioner_hash"
        printf '  "legacy_agent_plist_sha256": "%s",\n' "$plist_hash"
        printf '  "updater_sha256": "%s",\n' "$updater_hash"
        printf '  "chancery_annals_sha256": "%s",\n' "$annals_bundle_hash"
        printf '  "chancery_usage_sha256": "%s",\n' "$usage_bundle_hash"
        printf '  "source_revision": "test",\n'
        printf '  "source_dirty": false\n'
        printf '}\n'
    } >"$stage/manifest.json"
    chmod 0600 "$stage/manifest.json"
    mkdir -p "$temporary/releases"
    mv "$stage" "$temporary/releases/$release_id"
    printf '%s\n' "$temporary/releases/$release_id"
}

release_one=$(build_release one)
release_two=$(build_release two)
for release in "$release_one" "$release_two"; do
    [ "$(sed -n 's/^  "format": \([0-9][0-9]*\),$/\1/p' \
        "$release/manifest.json")" = 4 ]
    grep -F '  "decisions_config_template_sha256": "' \
        "$release/manifest.json" >/dev/null
    grep -F '  "decisions_clockwork_template_sha256": "' \
        "$release/manifest.json" >/dev/null
    grep -F '  "decisions_provisioner_sha256": "' \
        "$release/manifest.json" >/dev/null
done

provision() {
    selected_release=$1
    shift
    HOME="$home" "$selected_release/package/provision-decisions-user.sh" \
        --release-root "$selected_release" \
        --nucleus-socket "$nucleus_socket" \
        --clockwork "$clockwork" \
        --home "$home" \
        "$@"
}

state="$home/Library/Application Support/Annals/decisions"
binding="$home/clockwork-test/annals.decisions-inbox"
primary_sentinel="$home/Library/Application Support/Annals/primary-sentinel"
mkdir -p "$(dirname "$primary_sentinel")"
printf '%s\n' primary-unchanged >"$primary_sentinel"

normal_output=$(provision "$release_one")
printf '%s\n' "$normal_output" >"$temporary/normal.json"
[ "$(plutil -extract ok raw "$temporary/normal.json")" = true ]
[ "$(plutil -extract data.config raw "$temporary/normal.json")" = \
    "$state/config.toml" ]
[ "$(plutil -extract data.library_id raw "$temporary/normal.json")" = \
    0123456789abcdef0123456789abcdef ]
[ "$(plutil -extract data.clockwork_key raw "$temporary/normal.json")" = \
    annals/decisions-inbox ]
[ "$(plutil -extract data.selected raw "$temporary/normal.json")" = true ]
[ "$(plutil -extract data.enabled raw "$temporary/normal.json")" = true ]
[ "$(plutil -extract data.maintenance raw "$temporary/normal.json")" = false ]
[ -f "$state/annals.db" ]
[ -f "$state/spool/.decision-feed-library.json" ]
[ -f "$state/backups/last-provision.json" ]
[ -f "$state/log/inbox.stdout.log" ]
[ -f "$state/log/inbox.stderr.log" ]
[ "$(stat -f '%Lp' "$state/log/inbox.stdout.log")" = 600 ]
[ "$(stat -f '%Lp' "$state/log/inbox.stderr.log")" = 600 ]
[ "$(stat -f '%l' "$state/log/inbox.stdout.log")" -eq 1 ]
[ "$(stat -f '%l' "$state/log/inbox.stderr.log")" -eq 1 ]
[ "$(stat -f '%d:%i' "$state/log/inbox.stdout.log")" != \
    "$(stat -f '%d:%i' "$state/log/inbox.stderr.log")" ]
[ ! -e "$state/spool/.maintenance" ]
[ ! -e "$state/.provision-maintenance.json" ]
first_digest=$(sed -n '2p' "$binding")
[ "$(sed -n '1p' "$binding")" = true ]
grep -Fx "ANNALS_CONFIG = \"$state/config.toml\"" \
    "$home/clockwork-test/definition.$first_digest.toml" >/dev/null
grep -Fx "stdout = \"$state/log/inbox.stdout.log\"" \
    "$home/clockwork-test/definition.$first_digest.toml" >/dev/null
grep -Fx primary-unchanged "$primary_sentinel" >/dev/null
if grep -E '(^| )annals/inbox( |$)' "$home/clockwork.log" >/dev/null; then
    printf '%s\n' 'decisions provisioner touched the primary Annals binding' >&2
    exit 1
fi

ln "$state/annals.db" "$temporary/annals-db-hardlink"
if provision "$release_one" >"$temporary/db-hardlink.out" \
    2>"$temporary/db-hardlink.err"
then
    printf '%s\n' 'hard-linked decisions library unexpectedly passed validation' >&2
    exit 1
fi
grep -F 'invalid decisions library' "$temporary/db-hardlink.err" >/dev/null
rm "$temporary/annals-db-hardlink"
[ "$(sed -n '1p' "$binding")" = true ]
[ "$(sed -n '2p' "$binding")" = "$first_digest" ]
[ ! -e "$state/spool/.maintenance" ]

ln "$state/config.toml" "$temporary/decisions-config-hardlink"
if provision "$release_one" >"$temporary/config-hardlink.out" \
    2>"$temporary/config-hardlink.err"
then
    printf '%s\n' 'hard-linked decisions config unexpectedly passed validation' >&2
    exit 1
fi
grep -F 'invalid decisions config' "$temporary/config-hardlink.err" >/dev/null
rm "$temporary/decisions-config-hardlink"
[ "$(sed -n '1p' "$binding")" = true ]
[ "$(sed -n '2p' "$binding")" = "$first_digest" ]
[ ! -e "$state/spool/.maintenance" ]

ln "$state/log/inbox.stdout.log" "$temporary/decisions-stdout-hardlink"
if provision "$release_one" >"$temporary/stdout-hardlink.out" \
    2>"$temporary/stdout-hardlink.err"
then
    printf '%s\n' 'hard-linked decisions log unexpectedly passed validation' >&2
    exit 1
fi
grep -F 'invalid decisions stdout log' "$temporary/stdout-hardlink.err" >/dev/null
rm "$temporary/decisions-stdout-hardlink"
[ "$(sed -n '1p' "$binding")" = true ]
[ "$(sed -n '2p' "$binding")" = "$first_digest" ]
[ ! -e "$state/spool/.maintenance" ]

mv "$state/log/inbox.stderr.log" "$temporary/decisions-stderr-log"
ln -s "$temporary/decisions-stderr-log" "$state/log/inbox.stderr.log"
if provision "$release_one" >"$temporary/stderr-symlink.out" \
    2>"$temporary/stderr-symlink.err"
then
    printf '%s\n' 'symbolic decisions log unexpectedly passed validation' >&2
    exit 1
fi
grep -F 'invalid decisions stderr log' "$temporary/stderr-symlink.err" >/dev/null
rm "$state/log/inbox.stderr.log"
mv "$temporary/decisions-stderr-log" "$state/log/inbox.stderr.log"
[ "$(sed -n '1p' "$binding")" = true ]
[ "$(sed -n '2p' "$binding")" = "$first_digest" ]
[ ! -e "$state/spool/.maintenance" ]

database_hash=$(shasum -a 256 "$state/annals.db" | awk '{print $1}')
config_hash=$(shasum -a 256 "$state/config.toml" | awk '{print $1}')
: >"$home/fail-next-clockwork-switch"
if provision "$release_two" >"$temporary/failed.out" 2>"$temporary/failed.err"; then
    printf '%s\n' 'failed decisions switch unexpectedly succeeded' >&2
    exit 1
fi
[ "$(sed -n '1p' "$binding")" = true ]
[ "$(sed -n '2p' "$binding")" = "$first_digest" ]
[ "$(shasum -a 256 "$state/annals.db" | awk '{print $1}')" = "$database_hash" ]
[ "$(shasum -a 256 "$state/config.toml" | awk '{print $1}')" = "$config_hash" ]
[ ! -e "$state/spool/.maintenance" ]

held_output=$(provision "$release_two" --keep-maintenance)
printf '%s\n' "$held_output" >"$temporary/held.json"
[ "$(plutil -extract data.maintenance raw "$temporary/held.json")" = true ]
[ -f "$state/spool/.maintenance" ]
[ -f "$state/.provision-maintenance.json" ]
second_digest=$(sed -n '2p' "$binding")
[ "$second_digest" != "$first_digest" ]
[ "$(plutil -extract definition_digest raw \
    "$state/.provision-maintenance.json")" = "$second_digest" ]

released_output=$(provision "$release_two")
printf '%s\n' "$released_output" >"$temporary/released.json"
[ "$(plutil -extract data.maintenance raw "$temporary/released.json")" = false ]
[ ! -e "$state/spool/.maintenance" ]
[ ! -e "$state/.provision-maintenance.json" ]

HOME="$home" "$clockwork" --json binding disable \
    annals/decisions-inbox >/dev/null
[ "$(sed -n '1p' "$binding")" = false ]
[ "$(sed -n '2p' "$binding")" = "$second_digest" ]
: >"$home/fail-next-clockwork-switch"
if provision "$release_one" >"$temporary/disabled-failed.out" \
    2>"$temporary/disabled-failed.err"
then
    printf '%s\n' 'disabled-selected rollback test unexpectedly succeeded' >&2
    exit 1
fi
[ "$(sed -n '1p' "$binding")" = false ]
[ "$(sed -n '2p' "$binding")" = "$second_digest" ]
[ ! -e "$state/spool/.maintenance" ]

printf 'false\n\n' >"$binding"
: >"$home/fail-next-clockwork-switch"
if provision "$release_one" >"$temporary/null-failed.out" \
    2>"$temporary/null-failed.err"
then
    printf '%s\n' 'disabled-null rollback test unexpectedly succeeded' >&2
    exit 1
fi
[ "$(sed -n '1p' "$binding")" = false ]
[ -z "$(sed -n '2p' "$binding")" ]
[ ! -e "$state/spool/.maintenance" ]

provision "$release_one" >/dev/null
owned_digest=$(sed -n '2p' "$binding")
owned_definition="$home/clockwork-test/definition.$owned_digest.toml"
cp "$owned_definition" "$temporary/owned-definition"
sed 's|^release_root = .*|release_root = "/foreign/release"|' \
    "$owned_definition" >"$temporary/foreign-definition"
mv "$temporary/foreign-definition" "$owned_definition"
if provision "$release_two" >"$temporary/foreign.out" \
    2>"$temporary/foreign.err"
then
    printf '%s\n' 'foreign decisions definition unexpectedly passed ownership proof' >&2
    exit 1
fi
[ "$(sed -n '1p' "$binding")" = true ]
[ "$(sed -n '2p' "$binding")" = "$owned_digest" ]
[ ! -e "$state/spool/.maintenance" ]
mv "$temporary/owned-definition" "$owned_definition"

printf '%s\n' '# tampered' \
    >>"$release_two/package/annals-decisions-inbox.clockwork.toml.in"
if provision "$release_two" >"$temporary/tampered-package.out" \
    2>"$temporary/tampered-package.err"
then
    printf '%s\n' 'tampered decisions package unexpectedly validated' >&2
    exit 1
fi
[ "$(sed -n '1p' "$binding")" = true ]
[ "$(sed -n '2p' "$binding")" = "$owned_digest" ]
[ ! -e "$state/spool/.maintenance" ]
cp "$SCRIPT_DIR/annals-decisions-inbox.clockwork.toml.in" \
    "$release_two/package/annals-decisions-inbox.clockwork.toml.in"
chmod 0600 "$release_two/package/annals-decisions-inbox.clockwork.toml.in"

absent_home="$temporary/Absent Home"
mkdir -p "$absent_home"
: >"$absent_home/fail-next-clockwork-switch"
if HOME="$absent_home" "$release_one/package/provision-decisions-user.sh" \
    --release-root "$release_one" \
    --nucleus-socket "$nucleus_socket" \
    --clockwork "$clockwork" \
    --home "$absent_home" \
    >"$temporary/absent-failed.out" 2>"$temporary/absent-failed.err"
then
    printf '%s\n' 'absent-binding rollback test unexpectedly succeeded' >&2
    exit 1
fi
[ ! -e "$absent_home/Library/Application Support/Annals/decisions" ]
[ ! -e "$absent_home/clockwork-test/annals.decisions-inbox" ]

grep -Fx primary-unchanged "$primary_sentinel" >/dev/null
if grep -E '(^| )annals/inbox( |$)' "$home/clockwork.log" >/dev/null; then
    printf '%s\n' 'decisions rollback touched the primary Annals binding' >&2
    exit 1
fi
