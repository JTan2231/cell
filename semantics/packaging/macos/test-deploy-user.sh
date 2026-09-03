#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
temporary=$(mktemp -d "${TMPDIR:-/tmp}/semantics-deploy.XXXXXX")
holder=
cleanup() {
    if [ -n "$holder" ]; then
        kill "$holder" >/dev/null 2>&1 || true
        wait "$holder" >/dev/null 2>&1 || true
    fi
    rm -rf "$temporary"
}
trap cleanup EXIT HUP INT TERM

package="$temporary/package/macos"
share="$temporary/package/share/chancery"
mkdir -p "$package" "$share"
cp "$SCRIPT_DIR/semantics" "$SCRIPT_DIR/semantics-worker" \
    "$SCRIPT_DIR/deploy-user.sh" "$SCRIPT_DIR/uninstall-user.sh" \
    "$SCRIPT_DIR/org.semantics.worker.plist" "$package/"
cp -R "$SCRIPT_DIR/../../chancery" "$share/semantics"
chmod 0755 "$package/semantics" "$package/semantics-worker" \
    "$package/deploy-user.sh" "$package/uninstall-user.sh"
SEMANTICS_TEST_VERSION=$(awk -F '"' \
    '/"release"[[:space:]]*:/ { print $4; exit }' \
    "$share/semantics/provider.json")
[ -n "$SEMANTICS_TEST_VERSION" ]
export SEMANTICS_TEST_VERSION

launchctl="$temporary/launchctl"
loaded="$temporary/loaded"
launch_log="$temporary/launchctl.log"
fail_bootstrap="$temporary/fail-bootstrap"
fail_bootstrap_loaded="$temporary/fail-bootstrap-loaded"
fail_bootout="$temporary/fail-bootout"
cat >"$launchctl" <<EOF
#!/bin/sh
printf '%s\n' "\$*" >>"$launch_log"
mkdir -p "$loaded"
case "\${1:-}" in
    print)
        service=\${2##*/}
        [ -f "$loaded/\$service" ]
        ;;
    bootout)
        service=\${2##*/}
        if [ -f "$fail_bootout" ]; then
            rm -f "$fail_bootout"
            exit 1
        fi
        rm -f "$loaded/\$service"
        ;;
    bootstrap)
        if [ -f "$fail_bootstrap_loaded" ]; then
            rm -f "$fail_bootstrap_loaded"
            service=\$(plutil -extract Label raw "\$3")
            : >"$loaded/\$service"
            : >"$fail_bootout"
            exit 1
        fi
        if [ -f "$fail_bootstrap" ]; then
            rm -f "$fail_bootstrap"
            exit 1
        fi
        service=\$(plutil -extract Label raw "\$3")
        : >"$loaded/\$service"
        ;;
    *) exit 1 ;;
esac
EOF
chmod 0755 "$launchctl"

make_home() {
    target_home=$1
    mkdir -p "$target_home/.local/bin"
    for prerequisite in codex decisions; do
        cat >"$target_home/.local/bin/$prerequisite" <<'EOF'
#!/bin/sh
exit 0
EOF
        chmod 0755 "$target_home/.local/bin/$prerequisite"
    done
}

candidate="$temporary/semantics"
cat >"$candidate" <<'EOF'
#!/bin/sh
set -eu
if [ "${1:-}" = --version ]; then printf 'semantics %s\n' "$SEMANTICS_TEST_VERSION"; exit 0; fi
database=
previous=
for argument in "$@"; do
    if [ "$previous" = database ]; then database=$argument; previous=; continue; fi
    if [ "$argument" = --database ]; then previous=database; fi
done
case " $* " in
    *' doctor '*)
        [ "${SECRET_SENTINEL+x}" != x ] || exit 1
        [ -n "$database" ] || exit 1
        [ -f "$database" ] || printf '%s\n' 'schema one database' >"$database"
        printf '%s\n' '{"checks":[{"detail":"schema 1 at synthetic","name":"database","ok":true},{"detail":"synthetic","name":"participation_markers","ok":true},{"detail":"synthetic","name":"decisions_lifecycle","ok":true},{"detail":"synthetic","name":"conversations_exact_cwd","ok":true},{"detail":"synthetic","name":"nucleus_reconciliation","ok":true}],"ok":true}'
        ;;
    *) exit 1 ;;
esac
EOF
chmod 0755 "$candidate"

legacy_package="$temporary/legacy-package"
cp -R "$temporary/package" "$legacy_package"
legacy_bundle="$legacy_package/share/chancery/semantics"
/usr/bin/plutil -replace schema_version -integer 2 "$legacy_bundle/provider.json"
/usr/bin/plutil -remove promise_scope "$legacy_bundle/provider.json"
/usr/bin/plutil -replace provider.release -string 0.1.0 "$legacy_bundle/provider.json"
for legacy_entry in "$legacy_bundle"/entries/*.json; do
    /usr/bin/plutil -remove promise "$legacy_entry"
done
/usr/bin/perl -0pi -e \
    's/validate_bundle "\$SOURCE_CHANCERY" source/validate_bundle "\$SOURCE_CHANCERY" installed/' \
    "$legacy_package/macos/deploy-user.sh"
grep -Eq '"schema_version"[[:space:]]*:[[:space:]]*2' \
    "$legacy_bundle/provider.json"
! grep -Eq '"promise_scope"[[:space:]]*:' "$legacy_bundle/provider.json"
! grep -R -Eq '"promise"[[:space:]]*:' "$legacy_bundle/entries"

legacy_home="$temporary/LegacyHome"
make_home "$legacy_home"
SEMANTICS_TEST_VERSION=0.1.0 HOME="$legacy_home" \
    "$legacy_package/macos/deploy-user.sh" --binary "$candidate" \
    --home "$legacy_home" --launchctl "$launchctl" >/dev/null
legacy_provider="$legacy_home/Library/Application Support/Chancery/providers/semantics/provider.json"
grep -Eq '"schema_version"[[:space:]]*:[[:space:]]*2' "$legacy_provider"
legacy_state="$legacy_home/Library/Application Support/Semantics"
grep -Fx 'version=0.1.0' "$legacy_state/install/current/manifest.txt" >/dev/null
HOME="$legacy_home" "$package/deploy-user.sh" --binary "$candidate" \
    --home "$legacy_home" --launchctl "$launchctl" >/dev/null
grep -Eq '"schema_version"[[:space:]]*:[[:space:]]*3' "$legacy_provider"
grep -Fx "version=$SEMANTICS_TEST_VERSION" \
    "$legacy_state/install/current/manifest.txt" >/dev/null
grep -Fx 'version=0.1.0' "$legacy_state/install/previous/manifest.txt" >/dev/null
HOME="$legacy_home" "$package/uninstall-user.sh" \
    --home "$legacy_home" --launchctl "$launchctl" >/dev/null

bad_candidate="$temporary/semantics-bad"
cat >"$bad_candidate" <<'EOF'
#!/bin/sh
set -eu
if [ "${1:-}" = --version ]; then printf 'semantics %s\n' "$SEMANTICS_TEST_VERSION"; exit 0; fi
database=
previous=
for argument in "$@"; do
    if [ "$previous" = database ]; then database=$argument; previous=; continue; fi
    if [ "$argument" = --database ]; then previous=database; fi
done
[ -n "$database" ] && printf '%s\n' 'bad candidate mutation' >"$database"
printf '%s\n' '{"ok":true,"checks":[{"name":"database","ok":true,"detail":"schema 2 at synthetic"},{"name":"participation_markers","ok":true,"detail":"synthetic"},{"name":"decisions_lifecycle","ok":true,"detail":"synthetic"},{"name":"conversations_exact_cwd","ok":true,"detail":"synthetic"},{"name":"nucleus_reconciliation","ok":true,"detail":"synthetic"}]}'
EOF
chmod 0755 "$bad_candidate"

bad_home="$temporary/BadHome"
make_home "$bad_home"
if HOME="$bad_home" "$package/deploy-user.sh" --binary "$bad_candidate" \
    --home "$bad_home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'deployment accepted a candidate that did not prove schema 1' >&2
    exit 1
fi
[ ! -e "$bad_home/.local/bin/semantics" ]
[ ! -e "$bad_home/Library/Application Support/Semantics/semantics.db" ]
[ ! -e "$bad_home/Library/Application Support/Semantics/install/current" ]
[ "$(find "$bad_home/Library/Application Support/Semantics/install/releases" -mindepth 1 -maxdepth 1 | awk 'END { print NR }')" -eq 0 ]

home="$temporary/Home"
make_home "$home"
SECRET_SENTINEL=must-not-reach-doctor HOME="$home" "$package/deploy-user.sh" --binary "$candidate" \
    --home "$home" --launchctl "$launchctl" >/dev/null
state="$home/Library/Application Support/Semantics"
current="$state/install/current"
cli="$home/.local/bin/semantics"
provider="$home/Library/Application Support/Chancery/providers/semantics"
plist="$home/Library/LaunchAgents/org.semantics.worker.plist"
database="$state/semantics.db"
[ -L "$cli" ]
[ "$(readlink "$cli")" = "$state/install/current/bin/semantics" ]
[ -L "$provider" ]
[ "$(readlink "$provider")" = "$state/install/current/share/chancery/semantics" ]
[ -L "$current" ]
[ -x "$state/install/current/libexec/semantics" ]
[ -x "$state/install/current/bin/semantics-worker" ]
[ -f "$loaded/org.semantics.worker" ]
[ -f "$database" ]
plutil -lint "$plist" >/dev/null
[ "$(plutil -extract Label raw "$plist")" = org.semantics.worker ]
[ "$(plutil -extract StartInterval raw "$plist")" -eq 60 ]
[ "$(plutil -extract WorkingDirectory raw "$plist")" = "$state" ]
[ "$(plutil -extract ProcessType raw "$plist")" = Background ]
[ "$(plutil -extract Umask raw "$plist")" = 077 ]
! plutil -extract RunAtLoad raw "$plist" >/dev/null 2>&1
! grep -E 'TOKEN|KEY|SECRET|CREDENTIAL|statement|rationale|prompt' "$plist" >/dev/null
first_release=$(readlink "$current")
first_release_count=$(find "$state/install/releases" -mindepth 1 -maxdepth 1 | awk 'END { print NR }')
database_before=$(cat "$database")
plist_before=$(shasum -a 256 "$plist" | awk '{print $1}')

HOME="$home" "$package/deploy-user.sh" --binary "$candidate" \
    --home "$home" --launchctl "$launchctl" >/dev/null
[ "$(readlink "$current")" = "$first_release" ]
[ -f "$loaded/org.semantics.worker" ]

/bin/sleep 60 <"$database" &
holder=$!
if HOME="$home" "$package/deploy-user.sh" --binary "$candidate" \
    --home "$home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'deployment touched an open database' >&2
    exit 1
fi
[ -L "$cli" ]
[ -f "$loaded/org.semantics.worker" ]
[ "$(cat "$database")" = "$database_before" ]
kill "$holder"
wait "$holder" >/dev/null 2>&1 || true
holder=

worker_ready="$temporary/manual-worker-ready"
/usr/bin/perl -MFcntl=:flock -e '
    my ($path, $ready) = @ARGV;
    open(my $lock, ">>", $path) or exit 1;
    flock($lock, LOCK_EX) or exit 1;
    open(my $signal, ">", $ready) or exit 1;
    close($signal);
    sleep 60;
' "$database.worker.lock" "$worker_ready" &
holder=$!
worker_wait=0
while [ ! -f "$worker_ready" ]; do
    worker_wait=$((worker_wait + 1))
    [ "$worker_wait" -le 40 ] || { printf '%s\n' 'manual worker lock did not start' >&2; exit 1; }
    /bin/sleep 0.05
done
if HOME="$home" "$package/deploy-user.sh" --binary "$candidate" \
    --home "$home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'deployment raced an active manual worker lock' >&2
    exit 1
fi
[ -L "$cli" ]
[ -f "$loaded/org.semantics.worker" ]
[ "$(cat "$database")" = "$database_before" ]
kill "$holder"
wait "$holder" >/dev/null 2>&1 || true
holder=

candidate_two="$temporary/semantics-two"
cat >"$candidate_two" <<'EOF'
#!/bin/sh
set -eu
if [ "${1:-}" = --version ]; then printf 'semantics %s\n' "$SEMANTICS_TEST_VERSION"; exit 0; fi
database=
previous=
for argument in "$@"; do
    if [ "$previous" = database ]; then database=$argument; previous=; continue; fi
    if [ "$argument" = --database ]; then previous=database; fi
done
[ -n "$database" ] || exit 1
printf '%s\n' 'candidate two mutation' >"$database"
printf '%s\n' '{"ok":true,"checks":[{"name":"database","ok":true,"detail":"schema 1 at synthetic"},{"name":"participation_markers","ok":true,"detail":"synthetic"},{"name":"decisions_lifecycle","ok":true,"detail":"synthetic"},{"name":"conversations_exact_cwd","ok":true,"detail":"synthetic"},{"name":"nucleus_reconciliation","ok":true,"detail":"synthetic"}]}'
EOF
chmod 0755 "$candidate_two"
: >"$fail_bootstrap"
if HOME="$home" "$package/deploy-user.sh" --binary "$candidate_two" \
    --home "$home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'failed launchd activation unexpectedly committed' >&2
    exit 1
fi
[ "$(readlink "$current")" = "$first_release" ]
[ "$(readlink "$cli")" = "$state/install/current/bin/semantics" ]
[ "$(readlink "$provider")" = "$state/install/current/share/chancery/semantics" ]
[ "$(cat "$database")" = "$database_before" ]
[ "$(shasum -a 256 "$plist" | awk '{print $1}')" = "$plist_before" ]
[ -f "$loaded/org.semantics.worker" ]
[ "$(find "$state/install/releases" -mindepth 1 -maxdepth 1 | awk 'END { print NR }')" -eq "$first_release_count" ]

mkdir "$state/install/.update-lock"
if HOME="$home" "$package/uninstall-user.sh" --home "$home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'uninstall raced an active deployment lock' >&2
    exit 1
fi
rmdir "$state/install/.update-lock"
[ -L "$cli" ]
[ -f "$loaded/org.semantics.worker" ]

HOME="$home" "$package/uninstall-user.sh" --home "$home" --launchctl "$launchctl" >/dev/null
[ ! -e "$cli" ]
[ ! -e "$provider" ]
[ ! -e "$plist" ]
[ ! -f "$loaded/org.semantics.worker" ]
[ -f "$database" ]
[ -d "$state/install/releases" ]
[ -L "$current" ]

HOME="$home" "$package/deploy-user.sh" --binary "$candidate" \
    --home "$home" --launchctl "$launchctl" >/dev/null
rm -f "$cli"
ln -s /tmp/foreign-semantics "$cli"
if HOME="$home" "$package/uninstall-user.sh" --home "$home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'uninstaller removed a foreign CLI selector' >&2
    exit 1
fi
[ "$(readlink "$cli")" = /tmp/foreign-semantics ]
[ -f "$loaded/org.semantics.worker" ]
rm -f "$cli"
ln -s "$state/install/current/bin/semantics" "$cli"

foreign_home="$temporary/ForeignHome"
make_home "$foreign_home"
mkdir -p "$foreign_home/Library/LaunchAgents"
cat >"$foreign_home/Library/LaunchAgents/org.semantics.worker.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict><key>Label</key><string>org.foreign.worker</string><key>ProgramArguments</key><array><string>/bin/false</string><string>/tmp/foreign</string></array></dict></plist>
EOF
if HOME="$foreign_home" "$package/deploy-user.sh" --binary "$candidate" \
    --home "$foreign_home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'deployment replaced a foreign worker plist' >&2
    exit 1
fi
grep -F 'org.foreign.worker' "$foreign_home/Library/LaunchAgents/org.semantics.worker.plist" >/dev/null

HOME="$home" "$package/uninstall-user.sh" --home "$home" --launchctl "$launchctl" >/dev/null
[ ! -f "$loaded/org.semantics.worker" ]

unsafe_home="$temporary/UnsafeRollbackHome"
make_home "$unsafe_home"
HOME="$unsafe_home" "$package/deploy-user.sh" --binary "$candidate" \
    --home "$unsafe_home" --launchctl "$launchctl" >/dev/null
unsafe_state="$unsafe_home/Library/Application Support/Semantics"
unsafe_database="$unsafe_state/semantics.db"
printf '%s\n' 'unsafe rollback original database' >"$unsafe_database"
: >"$fail_bootstrap_loaded"
if HOME="$unsafe_home" "$package/deploy-user.sh" --binary "$candidate_two" \
    --home "$unsafe_home" --launchctl "$launchctl" >"$temporary/unsafe.stdout" 2>"$temporary/unsafe.stderr"; then
    printf '%s\n' 'deployment committed despite an unquiescent rollback service' >&2
    exit 1
fi
[ ! -e "$unsafe_home/.local/bin/semantics" ]
grep -F 'current runner, plist, and public selectors are disabled' "$temporary/unsafe.stderr" >/dev/null
[ ! -e "$unsafe_state/install/current" ]
[ ! -e "$unsafe_state/install/previous" ]
[ ! -e "$unsafe_home/Library/Application Support/Chancery/providers/semantics" ]
[ ! -e "$unsafe_home/Library/LaunchAgents/org.semantics.worker.plist" ]
[ ! -x "$unsafe_state/install/current/bin/semantics-worker" ]
unsafe_transaction=$(find "$unsafe_state/install" -mindepth 1 -maxdepth 1 -type d -name '.transaction.*' -print | head -1)
[ -n "$unsafe_transaction" ]
[ -f "$unsafe_transaction/semantics.db" ]
[ -f "$unsafe_transaction/prior-worker.plist" ]
[ -f "$unsafe_transaction/prior-install.txt" ]
grep -Fx 'unsafe rollback original database' "$unsafe_transaction/semantics.db" >/dev/null
[ -f "$loaded/org.semantics.worker" ]
