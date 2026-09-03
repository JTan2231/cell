#!/bin/sh

set -eu

PATH=/usr/bin:/bin:/usr/sbin:/sbin
export PATH
umask 077

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
temporary_home=$(mktemp -d "${TMPDIR:-/tmp}/clockwork-deploy-test.XXXXXX")
candidate="$temporary_home/clockwork-candidate"
chancery_candidate="$temporary_home/chancery-candidate"
rejecting_chancery="$temporary_home/rejecting-chancery-candidate"
validation_log="$temporary_home/chancery-validation.log"

cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    rm -rf "$temporary_home"
    exit "$status"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

cat >"$candidate" <<'EOF'
#!/bin/sh
case "${1:-}" in
    --version) printf '%s\n' 'clockwork 0.1.0' ;;
    --help) printf '%s\n' 'synthetic Clockwork packaging candidate' ;;
    *) printf '%s\n' 'synthetic candidate supports only --version and --help' >&2; exit 1 ;;
esac
EOF
chmod 0755 "$candidate"

cat >"$chancery_candidate" <<'EOF'
#!/bin/sh
set -eu
script_dir=${0%/*}
case "${1:-}" in
    validate)
        [ "$#" -eq 2 ]
        case "$2" in
            "$script_dir/Library/Application Support/Clockwork/install/releases/"*/share/chancery/clockwork) ;;
            *) exit 1 ;;
        esac
        [ -f "$2/provider.json" ] && [ ! -L "$2/provider.json" ]
        grep -F '"id": "clockwork"' "$2/provider.json" >/dev/null
        printf 'validate:%s\n' "$2" >>"$script_dir/chancery-validation.log"
        ;;
    --registry)
        [ "$#" -eq 4 ] && [ "$3" = show ]
        case "$4" in
            clockwork.install.operate|clockwork.schedule.operate|clockwork.develop.change) ;;
            *) exit 1 ;;
        esac
        [ -f "$2/clockwork/provider.json" ]
        printf 'show:%s:%s\n' "$2" "$4" >>"$script_dir/chancery-validation.log"
        ;;
    *) exit 1 ;;
esac
EOF
cat >"$rejecting_chancery" <<'EOF'
#!/bin/sh
[ "${1:-}" != validate ]
EOF
chmod 0755 "$chancery_candidate" "$rejecting_chancery"

command_selector="$temporary_home/.local/bin/clockwork"
provider_selector="$temporary_home/Library/Application Support/Chancery/providers/clockwork"
current_selector="$temporary_home/Library/Application Support/Clockwork/install/current"
if "$SCRIPT_DIR/deploy-user.sh" --binary "$candidate" \
    --chancery "$rejecting_chancery" --home "$temporary_home" >/dev/null 2>&1
then
    printf '%s\n' 'test-deploy-user.sh: deploy accepted a rejecting candidate Chancery reader' >&2
    exit 1
fi
[ ! -e "$command_selector" ] && [ ! -L "$command_selector" ]
[ ! -e "$provider_selector" ] && [ ! -L "$provider_selector" ]
[ ! -e "$current_selector" ] && [ ! -L "$current_selector" ]

"$SCRIPT_DIR/deploy-user.sh" --binary "$candidate" --chancery "$chancery_candidate" \
    --home "$temporary_home" \
    >/dev/null
"$SCRIPT_DIR/deploy-user.sh" --binary "$candidate" --chancery "$chancery_candidate" \
    --home "$temporary_home" \
    >/dev/null

[ -L "$command_selector" ]
[ -L "$provider_selector" ]
[ -L "$current_selector" ]
[ "$("$command_selector" --version)" = 'clockwork 0.1.0' ]
validated_bundle="$temporary_home/Library/Application Support/Clockwork/install/$(readlink "$current_selector")/share/chancery/clockwork"
[ "$(awk 'END { print NR }' "$validation_log")" -eq 8 ]
[ "$(grep -Fc "validate:$validated_bundle" "$validation_log")" -eq 2 ]
for entry_id in clockwork.install.operate clockwork.schedule.operate \
    clockwork.develop.change
do
    [ "$(grep -Fc "show:$temporary_home/Library/Application Support/Chancery/providers:$entry_id" \
        "$validation_log")" -eq 2 ]
done

"$SCRIPT_DIR/uninstall-user.sh" --home "$temporary_home" >/dev/null
[ ! -e "$command_selector" ] && [ ! -L "$command_selector" ]
[ ! -e "$provider_selector" ] && [ ! -L "$provider_selector" ]
[ ! -e "$current_selector" ] && [ ! -L "$current_selector" ]
find "$temporary_home/Library/Application Support/Clockwork/install/releases" \
    -mindepth 1 -maxdepth 1 -type d -print | grep -q .

"$SCRIPT_DIR/deploy-user.sh" --binary "$candidate" --chancery "$chancery_candidate" \
    --home "$temporary_home" \
    >/dev/null
mkdir -p "$temporary_home/Library/LaunchAgents"
: >"$temporary_home/Library/LaunchAgents/org.clockwork.test.job.plist"
if "$SCRIPT_DIR/uninstall-user.sh" --home "$temporary_home" >/dev/null 2>&1; then
    printf '%s\n' 'test-deploy-user.sh: uninstall accepted a remaining plist' >&2
    exit 1
fi
[ -L "$command_selector" ]

printf '%s\n' 'test-deploy-user.sh: green'
