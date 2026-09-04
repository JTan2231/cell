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
    "$SCRIPT_DIR/semantics-worker.clockwork.toml.in" \
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

clockwork="$temporary/clockwork"
clockwork_loaded="$temporary/clockwork-loaded"
cat >"$clockwork" <<EOF
#!/bin/sh
set -eu
printf '%s\n' "\$*" >>'$temporary/clockwork.log'
[ "\${1:-}" = --json ] && shift
root="\${HOME:?}/Library/Application Support/Clockwork/test"
binding="\$root/semantics.worker"
mkdir -p "\$root" "$clockwork_loaded"
command=\${1:-}; shift || true
case "\$command:\${1:-}" in
    definition:register)
        shift
        digest=\$(shasum -a 256 "\$1" | awk '{print \$1}')
        cp "\$1" "\$root/definition.\$digest.toml"
        printf '{"ok":true,"data":{"digest":"%s"}}\n' "\$digest"
        ;;
    definition:show)
        shift
        digest=\$1
        definition="\$root/definition.\$digest.toml"
        [ -f "\$definition" ] || exit 1
        definition_value() {
            sed -n "s/^\$1 = \"\\(.*\\)\"$/\\1/p" "\$definition" | head -n 1
        }
        release_id=\$(definition_value release_id)
        release_root=\$(definition_value release_root)
        cwd=\$(definition_value cwd)
        interpreter_hash=\$(definition_value interpreter_sha256)
        script=\$(definition_value script)
        script_hash=\$(definition_value script_sha256)
        definition_home=\$(definition_value HOME)
        stdout=\$(definition_value stdout)
        stderr=\$(definition_value stderr)
        printf '{"ok":true,"data":{"digest":"%s","key":"semantics/worker","registered_at":1,"manifest":{"schema_version":1,"key":"semantics/worker","release_id":"%s","release_root":"%s","authority":"current-user-background","overlap":"skip","arguments":[],"cwd":"%s","schedule":{"kind":"interval","seconds":60,"run_at_load":false},"launch":{"kind":"interpreted","interpreter":"/bin/sh","interpreter_sha256":"%s","script":"%s","script_sha256":"%s"},"environment":{"HOME":"%s"},"output":{"stdout":"%s","stderr":"%s"}}}}\n' \
            "\$digest" "\$release_id" "\$release_root" "\$cwd" \
            "\$interpreter_hash" "\$script" "\$script_hash" "\$definition_home" \
            "\$stdout" "\$stderr"
        ;;
    binding:show)
        shift
        if [ ! -f "\$binding" ]; then
            printf '%s\n' '{"ok":false,"error":{"code":"binding_not_found","message":"absent"}}' >&2
            exit 1
        fi
        enabled=\$(sed -n '1p' "\$binding")
        digest=\$(sed -n '2p' "\$binding")
        if [ -n "\$digest" ]; then digest_json="\"\$digest\""; else digest_json=null; fi
        printf '{"ok":true,"data":{"key":"semantics/worker","definition_digest":%s,"enabled":%s,"updated_at":1}}\n' \
            "\$digest_json" "\$enabled"
        ;;
    binding:disable)
        shift
        if [ -f "$temporary/fail-clockwork-disable" ]; then
            rm -f "$temporary/fail-clockwork-disable"
            exit 1
        fi
        digest=
        [ ! -f "\$binding" ] || digest=\$(sed -n '2p' "\$binding")
        shift || true
        if [ "\${1:-}" = --select ]; then
            [ "\$#" -ge 2 ] || exit 1
            digest=\$2
        fi
        printf 'false\n%s\n' "\$digest" >"\$binding"
        rm -f "$clockwork_loaded/org.clockwork.semantics.worker"
        printf '%s\n' '{"ok":true,"data":{"enabled":false}}'
        ;;
    binding:switch)
        shift
        key=\$1; digest=\$2
        if [ -f "$fail_bootstrap_loaded" ]; then
            rm -f "$fail_bootstrap_loaded"
            printf 'true\n%s\n' "\$digest" >"\$binding"
            : >"$clockwork_loaded/org.clockwork.semantics.worker"
            : >"$temporary/fail-clockwork-disable"
            exit 1
        fi
        if [ -f "$fail_bootstrap" ]; then rm -f "$fail_bootstrap"; exit 1; fi
        printf 'true\n%s\n' "\$digest" >"\$binding"
        : >"$clockwork_loaded/org.clockwork.semantics.worker"
        printf '{"ok":true,"data":{"key":"%s","definition_digest":"%s","enabled":true}}\n' "\$key" "\$digest"
        ;;
    *) exit 1 ;;
esac
EOF
chmod 0755 "$clockwork"

make_home() {
    target_home=$1
    mkdir -p "$target_home/.local/bin"
    mkdir -p "$target_home/Library/Application Support/Annals/decisions"
    : >"$target_home/Library/Application Support/Annals/decisions/config.toml"
    for prerequisite in codex annals; do
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
    *' project activate-annals '*)
        [ -n "$database" ] || exit 1
        case " $* " in
            *' --final-decisions-watermark legacy-final '*) ;;
            *) exit 1 ;;
        esac
        grep -Fx 'schema one database' "$database" >/dev/null || exit 1
        [ ! -e "$HOME/.local/bin/semantics" ] || exit 1
        current="$HOME/Library/Application Support/Semantics/install/current"
        [ -L "$current" ] || exit 1
        selected=$(readlink "$current")
        [ "$HOME/Library/Application Support/Semantics/install/$selected/libexec/semantics" != "$0" ] \
            || exit 1
        printf '%s\n' "$selected" \
            >"$HOME/Library/Application Support/Semantics/candidate-activation-selector"
        printf '%s\n' 'schema two database' 'annals activated before selector switch' >"$database"
        printf '%s\n' '{"activation_watermark":"afe1_0000","library_id":"0123456789abcdef0123456789abcdef"}'
        ;;
    *' doctor '*)
        [ "${SECRET_SENTINEL+x}" != x ] || exit 1
        [ ! -f "$HOME/Library/Application Support/Semantics/fail-candidate-doctor" ] || exit 1
        [ -n "$database" ] || exit 1
        [ -f "$database" ] || printf '%s\n' 'schema two database' >"$database"
        ! grep -Fx 'schema one database' "$database" >/dev/null || exit 1
        activation_selector="$HOME/Library/Application Support/Semantics/candidate-activation-selector"
        if [ -f "$activation_selector" ]; then
            current="$HOME/Library/Application Support/Semantics/install/current"
            selected=$(readlink "$current")
            [ "$selected" = "$(cat "$activation_selector")" ] || exit 1
            rm -f "$activation_selector"
        fi
        printf '%s\n' '{"checks":[{"detail":"schema 2 at synthetic","name":"database","ok":true},{"detail":"synthetic","name":"participation_markers","ok":true},{"detail":"synthetic","name":"annals_decision_feed","ok":true},{"detail":"synthetic","name":"conversations_exact_cwd","ok":true},{"detail":"synthetic","name":"nucleus_reconciliation","ok":true}],"ok":true}'
        ;;
    *) exit 1 ;;
esac
EOF
chmod 0755 "$candidate"
legacy_candidate="$temporary/semantics-legacy"
sed 's/"\$SEMANTICS_TEST_VERSION"/0.1.0/' "$candidate" >"$legacy_candidate"
chmod 0755 "$legacy_candidate"

legacy_package="$temporary/legacy-package"
cp -R "$temporary/package" "$legacy_package"
legacy_bundle="$legacy_package/share/chancery/semantics"
/usr/bin/plutil -replace schema_version -integer 2 "$legacy_bundle/provider.json"
/usr/bin/plutil -remove promise_scope "$legacy_bundle/provider.json"
/usr/bin/plutil -replace provider.release -string 0.1.0 "$legacy_bundle/provider.json"
for legacy_entry in "$legacy_bundle"/entries/*.json; do
    /usr/bin/plutil -remove promise "$legacy_entry"
done
grep -Eq '"schema_version"[[:space:]]*:[[:space:]]*2' \
    "$legacy_bundle/provider.json"
! grep -Eq '"promise_scope"[[:space:]]*:' "$legacy_bundle/provider.json"
! grep -R -Eq '"promise"[[:space:]]*:' "$legacy_bundle/entries"

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

install_format_one_fixture() {
    format_one_home=$1
    make_home "$format_one_home"
    format_one_state="$format_one_home/Library/Application Support/Semantics"
    format_one_install="$format_one_state/install"
    format_one_logs="$format_one_home/Library/Logs/Semantics"
    format_one_plist="$format_one_home/Library/LaunchAgents/org.semantics.worker.plist"
    format_one_provider="$format_one_home/Library/Application Support/Chancery/providers/semantics"
    mkdir -p "$format_one_install/releases" "$format_one_logs" \
        "$format_one_home/Library/LaunchAgents" \
        "$format_one_home/Library/Application Support/Chancery/providers"

    format_one_stage=$(mktemp -d "$temporary/format-one-release.XXXXXX")
    mkdir -p "$format_one_stage/bin" "$format_one_stage/libexec" \
        "$format_one_stage/package" "$format_one_stage/share/chancery"
    install -m 0755 "$legacy_candidate" "$format_one_stage/libexec/semantics"
    install -m 0755 "$legacy_package/macos/semantics" \
        "$format_one_stage/bin/semantics"
    install -m 0755 "$legacy_package/macos/semantics-worker" \
        "$format_one_stage/bin/semantics-worker"
    install -m 0755 "$legacy_package/macos/semantics" \
        "$format_one_stage/package/semantics"
    install -m 0755 "$legacy_package/macos/semantics-worker" \
        "$format_one_stage/package/semantics-worker"
    install -m 0755 "$legacy_package/macos/deploy-user.sh" \
        "$format_one_stage/package/deploy-user.sh"
    install -m 0755 "$legacy_package/macos/uninstall-user.sh" \
        "$format_one_stage/package/uninstall-user.sh"
    install -m 0644 "$SCRIPT_DIR/org.semantics.worker.plist" \
        "$format_one_stage/package/org.semantics.worker.plist"
    cp -R "$legacy_bundle" "$format_one_stage/share/chancery/semantics"

    format_one_binary_hash=$(shasum -a 256 \
        "$format_one_stage/libexec/semantics" | awk '{print $1}')
    format_one_frontend_hash=$(shasum -a 256 \
        "$format_one_stage/bin/semantics" | awk '{print $1}')
    format_one_runner_hash=$(shasum -a 256 \
        "$format_one_stage/bin/semantics-worker" | awk '{print $1}')
    format_one_plist_hash=$(shasum -a 256 \
        "$format_one_stage/package/org.semantics.worker.plist" | awk '{print $1}')
    format_one_deployer_hash=$(shasum -a 256 \
        "$format_one_stage/package/deploy-user.sh" | awk '{print $1}')
    format_one_uninstaller_hash=$(shasum -a 256 \
        "$format_one_stage/package/uninstall-user.sh" | awk '{print $1}')
    format_one_chancery_hash=$(bundle_hash \
        "$format_one_stage/share/chancery/semantics")
    format_one_release_id=$(printf '%s\n' \
        "$format_one_binary_hash" "$format_one_frontend_hash" \
        "$format_one_runner_hash" "$format_one_plist_hash" \
        "$format_one_deployer_hash" "$format_one_uninstaller_hash" \
        "$format_one_chancery_hash" | shasum -a 256 | awk '{print $1}')
    {
        printf '%s\n' 'format=1'
        printf 'release_id=%s\n' "$format_one_release_id"
        printf '%s\n' 'version=0.1.0'
        printf 'binary_sha256=%s\n' "$format_one_binary_hash"
        printf 'frontend_sha256=%s\n' "$format_one_frontend_hash"
        printf 'runner_sha256=%s\n' "$format_one_runner_hash"
        printf 'plist_sha256=%s\n' "$format_one_plist_hash"
        printf 'deployer_sha256=%s\n' "$format_one_deployer_hash"
        printf 'uninstaller_sha256=%s\n' "$format_one_uninstaller_hash"
        printf 'chancery_sha256=%s\n' "$format_one_chancery_hash"
    } >"$format_one_stage/manifest.txt"
    chmod 0444 "$format_one_stage/manifest.txt"
    chmod -R go-w "$format_one_stage"
    format_one_release="$format_one_install/releases/$format_one_release_id"
    mv "$format_one_stage" "$format_one_release"
    [ "$("$format_one_release/libexec/semantics" --version)" = \
        'semantics 0.1.0' ]

    ln -s "releases/$format_one_release_id" "$format_one_install/current"
    ln -s "$format_one_install/current/bin/semantics" \
        "$format_one_home/.local/bin/semantics"
    ln -s "$format_one_install/current/share/chancery/semantics" \
        "$format_one_provider"
    printf '%s\n' 'schema one database' >"$format_one_state/semantics.db"
    chmod 0600 "$format_one_state/semantics.db"
    sed \
        -e "s|__SEMANTICS_WORKER_RUNNER__|$format_one_install/current/bin/semantics-worker|g" \
        -e "s|__SEMANTICS_STATE_DIR__|$format_one_state|g" \
        -e "s|__SEMANTICS_HOME__|$format_one_home|g" \
        -e "s|__SEMANTICS_WORKER_STDOUT__|$format_one_logs/worker.stdout.log|g" \
        -e "s|__SEMANTICS_WORKER_STDERR__|$format_one_logs/worker.stderr.log|g" \
        "$format_one_release/package/org.semantics.worker.plist" >"$format_one_plist"
    chmod 0644 "$format_one_plist"
    "$launchctl" bootstrap "gui/$(id -u)" "$format_one_plist"
}

# Exercise retained-state uninstall directly against the pre-Clockwork release
# shape, not a current release with a legacy plist added beside it.
legacy_uninstall_home="$temporary/LegacyUninstallHome"
install_format_one_fixture "$legacy_uninstall_home"
legacy_uninstall_state=$format_one_state
legacy_uninstall_plist=$format_one_plist
grep -Fx 'format=1' "$legacy_uninstall_state/install/current/manifest.txt" >/dev/null
chmod 0664 "$legacy_uninstall_plist"
if HOME="$legacy_uninstall_home" "$package/uninstall-user.sh" \
    --clockwork "$clockwork" --home "$legacy_uninstall_home" \
    --launchctl "$launchctl" >/dev/null 2>&1
then
    printf '%s\n' 'uninstall accepted a writable legacy plist' >&2
    exit 1
fi
[ -f "$legacy_uninstall_plist" ]
[ -f "$loaded/org.semantics.worker" ]
chmod 0644 "$legacy_uninstall_plist"
HOME="$legacy_uninstall_home" "$package/uninstall-user.sh" --clockwork "$clockwork" \
    --home "$legacy_uninstall_home" --launchctl "$launchctl" >/dev/null
[ ! -e "$legacy_uninstall_home/.local/bin/semantics" ]
[ ! -e "$legacy_uninstall_home/Library/Application Support/Chancery/providers/semantics" ]
[ ! -e "$legacy_uninstall_plist" ]
[ ! -f "$loaded/org.semantics.worker" ]
[ -L "$legacy_uninstall_state/install/current" ]
[ -f "$legacy_uninstall_state/.clockwork-maintenance" ]

legacy_home="$temporary/LegacyHome"
install_format_one_fixture "$legacy_home"
legacy_state=$format_one_state
legacy_plist=$format_one_plist
legacy_provider="$format_one_provider/provider.json"
grep -Eq '"schema_version"[[:space:]]*:[[:space:]]*2' "$legacy_provider"
grep -Fx 'format=1' "$legacy_state/install/current/manifest.txt" >/dev/null
grep -Fx 'version=0.1.0' "$legacy_state/install/current/manifest.txt" >/dev/null
# Model the one-time handoff from the owned direct LaunchAgent. The old
# format-one fixture has no Clockwork binding, so the candidate can prove that
# the two schedulers never coexist.
[ -f "$loaded/org.semantics.worker" ]
legacy_plist_expected="$temporary/legacy-worker.expected.plist"
cp "$legacy_plist" "$legacy_plist_expected"
chmod 0664 "$legacy_plist"
if HOME="$legacy_home" "$package/deploy-user.sh" --binary "$candidate" \
    --clockwork "$clockwork" --home "$legacy_home" --launchctl "$launchctl" \
    >/dev/null 2>&1
then
    printf '%s\n' 'deployment accepted a writable legacy plist' >&2
    exit 1
fi
[ -f "$loaded/org.semantics.worker" ]
chmod 0644 "$legacy_plist"
plutil -replace EnvironmentVariables.HOME -string /tmp/foreign-semantics-home \
    "$legacy_plist"
if HOME="$legacy_home" "$package/deploy-user.sh" --binary "$candidate" \
    --clockwork "$clockwork" --home "$legacy_home" --launchctl "$launchctl" \
    >/dev/null 2>&1
then
    printf '%s\n' 'deployment accepted a legacy plist with foreign process context' >&2
    exit 1
fi
[ -f "$loaded/org.semantics.worker" ]
grep -F /tmp/foreign-semantics-home "$legacy_plist" >/dev/null
cp "$legacy_plist_expected" "$legacy_plist"
touch "$legacy_state/fail-candidate-doctor"
if HOME="$legacy_home" "$package/deploy-user.sh" --binary "$candidate" \
    --clockwork "$clockwork" --home "$legacy_home" --launchctl "$launchctl" \
    --final-decisions-watermark legacy-final --keep-maintenance >/dev/null 2>&1
then
    printf '%s\n' 'legacy cutover committed despite candidate doctor failure' >&2
    exit 1
fi
rm -f "$legacy_state/fail-candidate-doctor"
grep -Fx 'schema one database' "$legacy_state/semantics.db" >/dev/null
[ -f "$legacy_plist" ]
[ -f "$loaded/org.semantics.worker" ]
[ -L "$legacy_home/.local/bin/semantics" ]
[ -L "$format_one_provider" ]
[ "$(readlink "$legacy_state/install/current")" = "releases/$format_one_release_id" ]
[ ! -e "$legacy_state/.clockwork-maintenance" ]
if HOME="$legacy_home" "$package/deploy-user.sh" --binary "$candidate" \
    --clockwork "$clockwork" --home "$legacy_home" --launchctl "$launchctl" \
    >/dev/null 2>&1
then
    printf '%s\n' 'legacy deployment activated Annals without the final Decisions watermark assertion' >&2
    exit 1
fi
[ -f "$loaded/org.semantics.worker" ]
[ -L "$legacy_home/.local/bin/semantics" ]
grep -Fx 'schema one database' "$legacy_state/semantics.db" >/dev/null
if HOME="$legacy_home" "$package/deploy-user.sh" --binary "$candidate" \
    --clockwork "$clockwork" --home "$legacy_home" --launchctl "$launchctl" \
    --final-decisions-watermark legacy-final >/dev/null 2>&1
then
    printf '%s\n' 'legacy cutover accepted a final watermark without a retained handoff' >&2
    exit 1
fi
HOME="$legacy_home" "$package/deploy-user.sh" --binary "$candidate" --clockwork "$clockwork" \
    --home "$legacy_home" --launchctl "$launchctl" \
    --final-decisions-watermark legacy-final --keep-maintenance >/dev/null
[ ! -e "$legacy_plist" ]
[ ! -f "$loaded/org.semantics.worker" ]
[ -f "$clockwork_loaded/org.clockwork.semantics.worker" ]
[ -f "$legacy_state/.clockwork-maintenance" ]
[ -f "$legacy_state/.deployment-maintenance.json" ]
[ "$(stat -f '%Lp' "$legacy_state/.deployment-maintenance.json")" = 600 ]
[ "$(stat -f '%l' "$legacy_state/.deployment-maintenance.json")" -eq 1 ]
legacy_selected_release=$(readlink "$legacy_state/install/current")
legacy_selected_release=${legacy_selected_release#releases/}
legacy_selected_definition=$(sed -n '2p' \
    "$legacy_home/Library/Application Support/Clockwork/test/semantics.worker")
[ "$(plutil -extract key raw "$legacy_state/.deployment-maintenance.json")" = \
    semantics/worker ]
[ "$(plutil -extract release_id raw "$legacy_state/.deployment-maintenance.json")" = \
    "$legacy_selected_release" ]
[ "$(plutil -extract definition_digest raw \
    "$legacy_state/.deployment-maintenance.json")" = "$legacy_selected_definition" ]
grep -Eq '"schema_version"[[:space:]]*:[[:space:]]*3' "$legacy_provider"
grep -Fx "version=$SEMANTICS_TEST_VERSION" \
    "$legacy_state/install/current/manifest.txt" >/dev/null
grep -Fx 'version=0.1.0' "$legacy_state/install/previous/manifest.txt" >/dev/null
HOME="$legacy_home" "$package/deploy-user.sh" --binary "$candidate" --clockwork "$clockwork" \
    --home "$legacy_home" --launchctl "$launchctl" >/dev/null
[ ! -e "$legacy_state/.clockwork-maintenance" ]
[ ! -e "$legacy_state/.deployment-maintenance.json" ]
HOME="$legacy_home" "$package/uninstall-user.sh" --clockwork "$clockwork" \
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
printf '%s\n' '{"ok":true,"checks":[{"name":"database","ok":true,"detail":"schema 1 at synthetic"},{"name":"participation_markers","ok":true,"detail":"synthetic"},{"name":"annals_decision_feed","ok":true,"detail":"synthetic"},{"name":"conversations_exact_cwd","ok":true,"detail":"synthetic"},{"name":"nucleus_reconciliation","ok":true,"detail":"synthetic"}]}'
EOF
chmod 0755 "$bad_candidate"

bad_home="$temporary/BadHome"
make_home "$bad_home"
if HOME="$bad_home" "$package/deploy-user.sh" --binary "$bad_candidate" --clockwork "$clockwork" \
    --home "$bad_home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'deployment accepted a candidate that did not prove schema 2' >&2
    exit 1
fi
[ ! -e "$bad_home/.local/bin/semantics" ]
[ ! -e "$bad_home/Library/Application Support/Semantics/semantics.db" ]
[ ! -e "$bad_home/Library/Application Support/Semantics/install/current" ]
[ "$(find "$bad_home/Library/Application Support/Semantics/install/releases" -mindepth 1 -maxdepth 1 | awk 'END { print NR }')" -eq 1 ]

home="$temporary/Home"
make_home "$home"
SECRET_SENTINEL=must-not-reach-doctor HOME="$home" "$package/deploy-user.sh" --binary "$candidate" --clockwork "$clockwork" \
    --home "$home" --launchctl "$launchctl" >/dev/null
state="$home/Library/Application Support/Semantics"
current="$state/install/current"
cli="$home/.local/bin/semantics"
provider="$home/Library/Application Support/Chancery/providers/semantics"
plist="$home/Library/LaunchAgents/org.semantics.worker.plist"
binding="$home/Library/Application Support/Clockwork/test/semantics.worker"
database="$state/semantics.db"
[ -L "$cli" ]
[ "$(readlink "$cli")" = "$state/install/current/bin/semantics" ]
[ -L "$provider" ]
[ "$(readlink "$provider")" = "$state/install/current/share/chancery/semantics" ]
[ -L "$current" ]
[ -x "$state/install/current/libexec/semantics" ]
[ -x "$state/install/current/bin/semantics-worker" ]
[ -f "$clockwork_loaded/org.clockwork.semantics.worker" ]
[ ! -e "$plist" ]
[ -f "$database" ]
first_release=$(readlink "$current")
first_definition=$(sed -n '2p' "$binding")
definition_file="$home/Library/Application Support/Clockwork/test/definition.$first_definition.toml"
grep -Fx 'key = "semantics/worker"' "$definition_file" >/dev/null
grep -Fx "release_root = \"$state/install/$first_release\"" "$definition_file" >/dev/null
grep -Fx 'seconds = 60' "$definition_file" >/dev/null
grep -Fx 'run_at_load = false' "$definition_file" >/dev/null
grep -Fx 'overlap = "skip"' "$definition_file" >/dev/null
! grep -F 'timeout_seconds' "$definition_file" >/dev/null
grep -Fx "cwd = \"$state\"" "$definition_file" >/dev/null
grep -Fx "script = \"$state/install/$first_release/bin/semantics-worker\"" "$definition_file" >/dev/null
grep -Fx "HOME = \"$home\"" "$definition_file" >/dev/null
grep -Fx "stdout = \"$home/Library/Logs/Semantics/worker.stdout.log\"" \
    "$definition_file" >/dev/null
grep -Fx "stderr = \"$home/Library/Logs/Semantics/worker.stderr.log\"" \
    "$definition_file" >/dev/null
! grep -E 'TOKEN|KEY|SECRET|CREDENTIAL|statement|rationale|prompt' "$definition_file" >/dev/null
first_release_count=$(find "$state/install/releases" -mindepth 1 -maxdepth 1 | awk 'END { print NR }')
database_before=$(cat "$database")

definition_backup="$temporary/owned-semantics-definition.toml"
cp "$definition_file" "$definition_backup"
sed 's|^release_root = ".*"$|release_root = "/tmp/foreign-semantics-release"|' \
    "$definition_backup" >"$definition_file"
ownership_mutations_before=$(grep -Ec '^--json binding (disable|switch) ' \
    "$temporary/clockwork.log")
if HOME="$home" "$package/deploy-user.sh" --binary "$candidate" \
    --clockwork "$clockwork" --home "$home" --launchctl "$launchctl" \
    >/dev/null 2>&1
then
    printf '%s\n' 'deployment adopted a foreign selected Clockwork definition' >&2
    exit 1
fi
ownership_mutations_after=$(grep -Ec '^--json binding (disable|switch) ' \
    "$temporary/clockwork.log")
[ "$ownership_mutations_after" -eq "$ownership_mutations_before" ]
[ "$(sed -n '2p' "$binding")" = "$first_definition" ]
cp "$definition_backup" "$definition_file"

printf '%s\n' 'existing worker log' >"$home/Library/Logs/Semantics/worker.stderr.log"
chmod 0644 "$home/Library/Logs/Semantics/worker.stderr.log"
HOME="$home" "$package/deploy-user.sh" --binary "$candidate" --clockwork "$clockwork" \
    --home "$home" --launchctl "$launchctl" >/dev/null
[ "$(stat -f '%Lp' "$home/Library/Logs/Semantics/worker.stderr.log")" = 600 ]
grep -Fx 'existing worker log' "$home/Library/Logs/Semantics/worker.stderr.log" >/dev/null
[ "$(readlink "$current")" = "$first_release" ]
[ "$(sed -n '2p' "$binding")" = "$first_definition" ]
[ -f "$clockwork_loaded/org.clockwork.semantics.worker" ]

/bin/sleep 60 <"$database" &
holder=$!
if HOME="$home" "$package/deploy-user.sh" --binary "$candidate" --clockwork "$clockwork" \
    --home "$home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'deployment touched an open database' >&2
    exit 1
fi
[ -L "$cli" ]
[ "$(sed -n '2p' "$binding")" = "$first_definition" ]
[ -f "$clockwork_loaded/org.clockwork.semantics.worker" ]
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
if HOME="$home" "$package/deploy-user.sh" --binary "$candidate" --clockwork "$clockwork" \
    --home "$home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'deployment raced an active manual worker lock' >&2
    exit 1
fi
[ -L "$cli" ]
[ "$(sed -n '2p' "$binding")" = "$first_definition" ]
[ -f "$clockwork_loaded/org.clockwork.semantics.worker" ]
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
printf '%s\n' '{"ok":true,"checks":[{"name":"database","ok":true,"detail":"schema 2 at synthetic"},{"name":"participation_markers","ok":true,"detail":"synthetic"},{"name":"annals_decision_feed","ok":true,"detail":"synthetic"},{"name":"conversations_exact_cwd","ok":true,"detail":"synthetic"},{"name":"nucleus_reconciliation","ok":true,"detail":"synthetic"}]}'
EOF
chmod 0755 "$candidate_two"

database_hardlink="$temporary/semantics.db.hardlink"
ln "$database" "$database_hardlink"
if HOME="$home" "$package/deploy-user.sh" --binary "$candidate_two" --clockwork "$clockwork" \
    --home "$home" --launchctl "$launchctl" >/dev/null 2>&1
then
    printf '%s\n' 'deployment opened a hard-linked Semantics database' >&2
    exit 1
fi
[ "$(cat "$database")" = "$database_before" ]
[ "$(cat "$database_hardlink")" = "$database_before" ]
[ "$(readlink "$current")" = "$first_release" ]
[ "$(sed -n '2p' "$binding")" = "$first_definition" ]
[ -f "$clockwork_loaded/org.clockwork.semantics.worker" ]
rm -f "$database_hardlink"

: >"$fail_bootstrap"
if HOME="$home" "$package/deploy-user.sh" --binary "$candidate_two" --clockwork "$clockwork" \
    --home "$home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'failed Clockwork activation unexpectedly committed' >&2
    exit 1
fi
[ "$(readlink "$current")" = "$first_release" ]
[ "$(readlink "$cli")" = "$state/install/current/bin/semantics" ]
[ "$(readlink "$provider")" = "$state/install/current/share/chancery/semantics" ]
[ "$(cat "$database")" = "$database_before" ]
[ "$(sed -n '2p' "$binding")" = "$first_definition" ]
[ -f "$clockwork_loaded/org.clockwork.semantics.worker" ]
[ "$(find "$state/install/releases" -mindepth 1 -maxdepth 1 | awk 'END { print NR }')" -eq "$((first_release_count + 1))" ]

mkdir "$state/install/.update-lock"
if HOME="$home" "$package/uninstall-user.sh" --clockwork "$clockwork" --home "$home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'uninstall raced an active deployment lock' >&2
    exit 1
fi
rmdir "$state/install/.update-lock"
[ -L "$cli" ]
[ -f "$clockwork_loaded/org.clockwork.semantics.worker" ]

printf '%s\n' 'uninstall maintenance evidence' >"$state/.clockwork-maintenance"
chmod 0600 "$state/.clockwork-maintenance"
ln "$state/.clockwork-maintenance" "$temporary/uninstall-maintenance-link"
if HOME="$home" "$package/uninstall-user.sh" --clockwork "$clockwork" \
    --home "$home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'uninstall accepted a hard-linked Semantics maintenance gate' >&2
    exit 1
fi
grep -Fx 'uninstall maintenance evidence' "$state/.clockwork-maintenance" >/dev/null
rm -f "$temporary/uninstall-maintenance-link" "$state/.clockwork-maintenance"

HOME="$home" "$package/uninstall-user.sh" --clockwork "$clockwork" --home "$home" --launchctl "$launchctl" >/dev/null
[ ! -e "$cli" ]
[ ! -e "$provider" ]
[ ! -e "$plist" ]
[ "$(sed -n '1p' "$binding")" = false ]
[ ! -f "$clockwork_loaded/org.clockwork.semantics.worker" ]
[ -f "$database" ]
[ -d "$state/install/releases" ]
[ -L "$current" ]

# A disabled prior binding must never be transiently re-enabled just to restore
# its old inactive digest after a failed candidate switch.
disabled_switches_before=$(grep -c '^--json binding switch ' "$temporary/clockwork.log")
: >"$fail_bootstrap"
if HOME="$home" "$package/deploy-user.sh" --binary "$candidate_two" --clockwork "$clockwork" \
    --home "$home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'failed deployment unexpectedly enabled a disabled prior binding' >&2
    exit 1
fi
disabled_switches_after=$(grep -c '^--json binding switch ' "$temporary/clockwork.log")
[ "$disabled_switches_after" -eq "$((disabled_switches_before + 1))" ]
[ "$(sed -n '1p' "$binding")" = false ]
[ ! -f "$clockwork_loaded/org.clockwork.semantics.worker" ]
[ ! -e "$cli" ]
[ ! -e "$provider" ]

HOME="$home" "$package/deploy-user.sh" --binary "$candidate" --clockwork "$clockwork" \
    --home "$home" --launchctl "$launchctl" >/dev/null
rm -f "$cli"
ln -s /tmp/foreign-semantics "$cli"
if HOME="$home" "$package/uninstall-user.sh" --clockwork "$clockwork" --home "$home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'uninstaller removed a foreign CLI selector' >&2
    exit 1
fi
[ "$(readlink "$cli")" = /tmp/foreign-semantics ]
[ -f "$clockwork_loaded/org.clockwork.semantics.worker" ]
rm -f "$cli"
ln -s "$state/install/current/bin/semantics" "$cli"

foreign_home="$temporary/ForeignHome"
make_home "$foreign_home"
mkdir -p "$foreign_home/Library/LaunchAgents"
cat >"$foreign_home/Library/LaunchAgents/org.semantics.worker.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict><key>Label</key><string>org.foreign.worker</string><key>ProgramArguments</key><array><string>/bin/false</string><string>/tmp/foreign</string></array></dict></plist>
EOF
if HOME="$foreign_home" "$package/deploy-user.sh" --binary "$candidate" --clockwork "$clockwork" \
    --home "$foreign_home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'deployment replaced a foreign worker plist' >&2
    exit 1
fi
grep -F 'org.foreign.worker' "$foreign_home/Library/LaunchAgents/org.semantics.worker.plist" >/dev/null

HOME="$home" "$package/uninstall-user.sh" --clockwork "$clockwork" --home "$home" --launchctl "$launchctl" >/dev/null
[ ! -f "$clockwork_loaded/org.clockwork.semantics.worker" ]
[ -f "$home/Library/Application Support/Semantics/.clockwork-maintenance" ]
printf '%s\n' 'retained maintenance evidence' \
    >"$home/Library/Application Support/Semantics/.clockwork-maintenance"
ln "$home/Library/Application Support/Semantics/.clockwork-maintenance" \
    "$temporary/semantics-maintenance-link"
if HOME="$home" "$package/deploy-user.sh" --binary "$candidate" --clockwork "$clockwork" \
    --home "$home" --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'deployment accepted a hard-linked Semantics maintenance gate' >&2
    exit 1
fi
grep -Fx 'retained maintenance evidence' \
    "$home/Library/Application Support/Semantics/.clockwork-maintenance" >/dev/null
rm -f "$temporary/semantics-maintenance-link"
HOME="$home" "$package/deploy-user.sh" --binary "$candidate" --clockwork "$clockwork" \
    --home "$home" --launchctl "$launchctl" >/dev/null
grep -Fx 'retained maintenance evidence' \
    "$home/Library/Application Support/Semantics/.clockwork-maintenance" >/dev/null
grep -Fx 'maintenance_preexisting=1' "$state/install/last-update.txt" >/dev/null
grep -Fx 'maintenance_owned=0' "$state/install/last-update.txt" >/dev/null
grep -Fx 'maintenance_retained=1' "$state/install/last-update.txt" >/dev/null
rm -f "$home/Library/Application Support/Semantics/.clockwork-maintenance"

null_recovery_home="$temporary/NullRecoveryHome"
make_home "$null_recovery_home"
: >"$fail_bootstrap_loaded"
if HOME="$null_recovery_home" "$package/deploy-user.sh" --binary "$candidate" \
    --clockwork "$clockwork" --home "$null_recovery_home" \
    --launchctl "$launchctl" >/dev/null 2>&1; then
    printf '%s\n' 'first deployment committed despite an unprovable Clockwork switch' >&2
    exit 1
fi
null_recovery_state="$null_recovery_home/Library/Application Support/Semantics"
null_recovery_binding="$null_recovery_home/Library/Application Support/Clockwork/test/semantics.worker"
[ -L "$null_recovery_state/install/current" ]
[ ! -e "$null_recovery_home/.local/bin/semantics" ]
[ ! -e "$null_recovery_home/Library/Application Support/Chancery/providers/semantics" ]
[ -f "$null_recovery_state/.clockwork-maintenance" ]
[ -f "$null_recovery_state/.deployment-maintenance.json" ]
[ "$(sed -n '1p' "$null_recovery_binding")" = false ]
grep -Eq '^[0-9a-f]{64}$' "$null_recovery_binding"
HOME="$null_recovery_home" "$package/deploy-user.sh" --binary "$candidate" \
    --clockwork "$clockwork" --home "$null_recovery_home" \
    --launchctl "$launchctl" >/dev/null
[ -L "$null_recovery_home/.local/bin/semantics" ]
[ ! -e "$null_recovery_state/.clockwork-maintenance" ]
[ ! -e "$null_recovery_state/.deployment-maintenance.json" ]
[ "$(sed -n '1p' "$null_recovery_binding")" = true ]

unsafe_home="$temporary/UnsafeRollbackHome"
make_home "$unsafe_home"
HOME="$unsafe_home" "$package/deploy-user.sh" --binary "$candidate" --clockwork "$clockwork" \
    --home "$unsafe_home" --launchctl "$launchctl" >/dev/null
unsafe_state="$unsafe_home/Library/Application Support/Semantics"
unsafe_database="$unsafe_state/semantics.db"
printf '%s\n' 'unsafe rollback original database' >"$unsafe_database"
: >"$fail_bootstrap_loaded"
if HOME="$unsafe_home" "$package/deploy-user.sh" --binary "$candidate_two" --clockwork "$clockwork" \
    --home "$unsafe_home" --launchctl "$launchctl" >"$temporary/unsafe.stdout" 2>"$temporary/unsafe.stderr"; then
    printf '%s\n' 'deployment committed despite an unquiescent rollback service' >&2
    exit 1
fi
[ ! -e "$unsafe_home/.local/bin/semantics" ]
grep -F 'domain admission is maintenance-gated' "$temporary/unsafe.stderr" >/dev/null
[ -f "$unsafe_state/.clockwork-maintenance" ]
[ -f "$unsafe_state/.deployment-maintenance.json" ]
[ -L "$unsafe_state/install/current" ]
[ ! -e "$unsafe_state/install/previous" ]
[ ! -e "$unsafe_home/Library/Application Support/Chancery/providers/semantics" ]
[ ! -e "$unsafe_home/Library/LaunchAgents/org.semantics.worker.plist" ]
[ -x "$unsafe_state/install/current/bin/semantics-worker" ]
unsafe_transaction=$(find "$unsafe_state/install" -mindepth 1 -maxdepth 1 -type d -name '.transaction.*' -print | head -1)
[ -n "$unsafe_transaction" ]
[ -f "$unsafe_transaction/semantics.db" ]
[ -f "$unsafe_transaction/prior-install.txt" ]
grep -Fx 'unsafe rollback original database' "$unsafe_transaction/semantics.db" >/dev/null
[ ! -f "$clockwork_loaded/org.clockwork.semantics.worker" ]
