#!/bin/sh

set -eu

PATH=/usr/bin:/bin:/usr/sbin:/sbin
export PATH
umask 077

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
SOURCE_DEPLOYER="$SCRIPT_DIR/deploy-user.sh"
SOURCE_UNINSTALLER="$SCRIPT_DIR/uninstall-user.sh"
if [ -d "$SCRIPT_DIR/../share/chancery/clockwork" ]; then
    SOURCE_CHANCERY="$SCRIPT_DIR/../share/chancery/clockwork"
else
    SOURCE_CHANCERY="$SCRIPT_DIR/../../chancery"
fi

binary_path=
chancery_path=
install_home=${HOME:-}

usage() {
    cat <<'EOF'
Usage: deploy-user.sh --binary ABSOLUTE_PATH --chancery ABSOLUTE_PATH [OPTIONS]

Install or update the current user's macOS Clockwork CLI and product-owned
Chancery provider. Deployment never registers, switches, disables, or runs a
product binding and never initializes or migrates Clockwork runtime state.

Options:
  --chancery ABSOLUTE_PATH  Candidate Chancery reader used to validate the staged provider
  --home ABSOLUTE_PATH  Override the operator home (primarily for tests)
EOF
}

fail() {
    printf 'clockwork user deploy: %s\n' "$*" >&2
    exit 1
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --binary)
            [ "$#" -ge 2 ] || fail '--binary requires a path'
            binary_path=$2
            shift 2
            ;;
        --chancery)
            [ "$#" -ge 2 ] || fail '--chancery requires a path'
            chancery_path=$2
            shift 2
            ;;
        --home)
            [ "$#" -ge 2 ] || fail '--home requires a path'
            install_home=$2
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *) fail "unknown argument: $1" ;;
    esac
done

[ -n "$binary_path" ] || fail '--binary is required'
[ -n "$chancery_path" ] || fail '--chancery is required'
[ -n "$install_home" ] || fail '--home is required'
case "$binary_path" in /*) ;; *) fail 'binary path must be absolute' ;; esac
case "$chancery_path" in /*) ;; *) fail 'Chancery path must be absolute' ;; esac
case "$install_home" in /*) ;; *) fail 'home path must be absolute' ;; esac

[ -d "$install_home" ] && [ ! -L "$install_home" ] \
    || fail "operator home is not a regular directory: $install_home"
[ -f "$binary_path" ] && [ ! -L "$binary_path" ] && [ -x "$binary_path" ] \
    || fail "candidate is not an executable regular file: $binary_path"
[ -f "$chancery_path" ] && [ ! -L "$chancery_path" ] && [ -x "$chancery_path" ] \
    || fail "Chancery candidate is not an executable regular file: $chancery_path"
for source_script in "$SOURCE_DEPLOYER" "$SOURCE_UNINSTALLER"; do
    [ -f "$source_script" ] && [ ! -L "$source_script" ] \
        || fail "packaging source is not a regular file: $source_script"
done
for command in awk chmod cp find grep id install ln mkdir mktemp mv readlink \
    rm rmdir sed sh shasum sort stat; do
    command -v "$command" >/dev/null 2>&1 \
        || fail "required command not found: $command"
done
[ -x /usr/bin/shlock ] \
    || fail 'required command not found: /usr/bin/shlock'

operator_uid=$(id -u)
[ "$operator_uid" -ne 0 ] || fail 'run as the Clockwork operator, not root'
if home_uid=$(stat -f '%u' "$install_home" 2>/dev/null); then
    :
elif home_uid=$(stat -c '%u' "$install_home" 2>/dev/null); then
    :
else
    fail "unable to inspect home ownership: $install_home"
fi
[ "$home_uid" = "$operator_uid" ] \
    || fail "operator home is not owned by uid $operator_uid: $install_home"

validate_bundle() {
    bundle=$1
    [ -d "$bundle" ] && [ ! -L "$bundle" ] \
        || fail "Chancery bundle is not a regular directory: $bundle"
    [ -f "$bundle/provider.json" ] && [ ! -L "$bundle/provider.json" ] \
        || fail "Chancery bundle has no regular provider.json: $bundle"
    if find "$bundle" -type l -print | grep -q .; then
        fail "Chancery bundle contains a symbolic link: $bundle"
    fi
    if find "$bundle" ! -type d ! -type f -print | grep -q .; then
        fail "Chancery bundle contains a non-file entry: $bundle"
    fi
    actual_bundle_tree=$(
        cd "$bundle"
        find . -print | LC_ALL=C sort
    )
    expected_bundle_tree=$(printf '%s\n' \
        . \
        ./entries \
        ./entries/develop-change.json \
        ./entries/install-operate.json \
        ./entries/schedule-operate.json \
        ./manuals \
        ./manuals/develop-change.md \
        ./manuals/install-operate.md \
        ./manuals/schedule-operate.md \
        ./provider.json \
        | LC_ALL=C sort)
    [ "$actual_bundle_tree" = "$expected_bundle_tree" ] \
        || fail "Chancery bundle tree is not the exact Clockwork v0.1 layout: $bundle"
    bundle_provider=$(awk -F '"' \
        '/"id"[[:space:]]*:/ { print $4; exit }' \
        "$bundle/provider.json")
    [ "$bundle_provider" = clockwork ] \
        || fail "Chancery bundle has an unexpected provider id: $bundle_provider"
}

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

validate_release_selector() {
    selector=$1
    printf '%s\n' "$selector" | grep -Eq '^releases/[0-9a-f]{64}$' \
        || fail "invalid Clockwork release selector: $selector"
}

validate_installed_release() {
    selector=$1
    validate_release_selector "$selector"
    inspected_id=${selector#releases/}
    inspected_path="$INSTALL_DIR/$selector"

    for directory in "$inspected_path" "$inspected_path/bin" \
        "$inspected_path/package" "$inspected_path/share" \
        "$inspected_path/share/chancery" \
        "$inspected_path/share/chancery/clockwork"; do
        [ -d "$directory" ] && [ ! -L "$directory" ] \
            || fail "installed release has an invalid directory: $selector"
    done
    if find "$inspected_path" -type l -print | grep -q .; then
        fail "installed release contains a symbolic link: $selector"
    fi
    if find "$inspected_path" ! -type d ! -type f -print | grep -q .; then
        fail "installed release contains a non-file entry: $selector"
    fi
    actual_release_tree=$(
        cd "$inspected_path"
        find . -print | LC_ALL=C sort
    )
    expected_release_tree=$(printf '%s\n' \
        . \
        ./bin \
        ./bin/clockwork \
        ./manifest.txt \
        ./package \
        ./package/deploy-user.sh \
        ./package/uninstall-user.sh \
        ./share \
        ./share/chancery \
        ./share/chancery/clockwork \
        ./share/chancery/clockwork/entries \
        ./share/chancery/clockwork/entries/develop-change.json \
        ./share/chancery/clockwork/entries/install-operate.json \
        ./share/chancery/clockwork/entries/schedule-operate.json \
        ./share/chancery/clockwork/manuals \
        ./share/chancery/clockwork/manuals/develop-change.md \
        ./share/chancery/clockwork/manuals/install-operate.md \
        ./share/chancery/clockwork/manuals/schedule-operate.md \
        ./share/chancery/clockwork/provider.json \
        | LC_ALL=C sort)
    [ "$actual_release_tree" = "$expected_release_tree" ] \
        || fail "installed release tree is not the exact Clockwork v0.1 layout: $selector"
    [ -f "$inspected_path/bin/clockwork" ] \
        && [ ! -L "$inspected_path/bin/clockwork" ] \
        && [ -x "$inspected_path/bin/clockwork" ] \
        || fail "installed release has an invalid binary: $selector"
    if inspected_binary_uid=$(stat -f '%u' "$inspected_path/bin/clockwork" 2>/dev/null); then
        inspected_binary_mode=$(stat -f '%Lp' "$inspected_path/bin/clockwork")
        inspected_binary_links=$(stat -f '%l' "$inspected_path/bin/clockwork")
    else
        inspected_binary_uid=$(stat -c '%u' "$inspected_path/bin/clockwork")
        inspected_binary_mode=$(stat -c '%a' "$inspected_path/bin/clockwork")
        inspected_binary_links=$(stat -c '%h' "$inspected_path/bin/clockwork")
    fi
    [ "$inspected_binary_uid" = "$operator_uid" ] \
        && [ "$inspected_binary_links" -eq 1 ] \
        && [ $((0$inspected_binary_mode & 0100)) -ne 0 ] \
        && [ $((0$inspected_binary_mode & 0022)) -eq 0 ] \
        || fail "installed release binary ownership or mode is unsafe: $selector"
    for installed_script in deploy-user.sh uninstall-user.sh; do
        [ -f "$inspected_path/package/$installed_script" ] \
            && [ ! -L "$inspected_path/package/$installed_script" ] \
            && [ -x "$inspected_path/package/$installed_script" ] \
            || fail "installed release has an invalid packaging script: $selector"
    done
    [ -f "$inspected_path/manifest.txt" ] \
        && [ ! -L "$inspected_path/manifest.txt" ] \
        || fail "installed release has an invalid manifest: $selector"
    validate_bundle "$inspected_path/share/chancery/clockwork"

    inspected_manifest="$inspected_path/manifest.txt"
    [ "$(awk 'END { print NR }' "$inspected_manifest")" -eq 8 ] \
        || fail "installed release manifest is not canonical: $selector"
    [ "$(sed -n '1p' "$inspected_manifest")" = 'format=1' ] \
        || fail "installed release manifest format is unsupported: $selector"
    [ "$(sed -n '2p' "$inspected_manifest")" = 'product=clockwork' ] \
        || fail "installed release manifest has foreign ownership: $selector"
    inspected_manifest_id=$(sed -n '3s/^release_id=//p' "$inspected_manifest")
    inspected_version=$(sed -n '4s/^version=//p' "$inspected_manifest")
    inspected_binary_hash=$(sed -n '5s/^binary_sha256=//p' "$inspected_manifest")
    inspected_deployer_hash=$(sed -n '6s/^deployer_sha256=//p' "$inspected_manifest")
    inspected_uninstaller_hash=$(sed -n '7s/^uninstaller_sha256=//p' "$inspected_manifest")
    inspected_chancery_hash=$(sed -n '8s/^chancery_sha256=//p' "$inspected_manifest")
    [ "$inspected_manifest_id" = "$inspected_id" ] \
        || fail "installed release manifest identity is invalid: $selector"
    printf '%s\n' "$inspected_manifest_id" "$inspected_binary_hash" \
        "$inspected_deployer_hash" "$inspected_uninstaller_hash" \
        "$inspected_chancery_hash" | grep -Eqv '^[0-9a-f]{64}$' \
        && fail "installed release manifest hashes are invalid: $selector"
    printf '%s\n' "$inspected_version" \
        | grep -Eq '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$' \
        || fail "installed release version is invalid: $selector"

    computed_binary_hash=$(shasum -a 256 \
        "$inspected_path/bin/clockwork" | awk '{print $1}')
    computed_deployer_hash=$(shasum -a 256 \
        "$inspected_path/package/deploy-user.sh" | awk '{print $1}')
    computed_uninstaller_hash=$(shasum -a 256 \
        "$inspected_path/package/uninstall-user.sh" | awk '{print $1}')
    computed_chancery_hash=$(bundle_hash \
        "$inspected_path/share/chancery/clockwork")
    [ "$computed_binary_hash" = "$inspected_binary_hash" ] \
        || fail "installed release binary is tampered: $selector"
    [ "$computed_deployer_hash" = "$inspected_deployer_hash" ] \
        || fail "installed release deployer is tampered: $selector"
    [ "$computed_uninstaller_hash" = "$inspected_uninstaller_hash" ] \
        || fail "installed release uninstaller is tampered: $selector"
    [ "$computed_chancery_hash" = "$inspected_chancery_hash" ] \
        || fail "installed release provider is tampered: $selector"
    computed_id=$(printf '%s\n' "$computed_binary_hash" \
        "$computed_deployer_hash" "$computed_uninstaller_hash" \
        "$computed_chancery_hash" | shasum -a 256 | awk '{print $1}')
    [ "$computed_id" = "$inspected_id" ] \
        || fail "installed release content identity is invalid: $selector"

    inspected_provider_version=$(awk -F '"' \
        '/"release"[[:space:]]*:/ { print $4; exit }' \
        "$inspected_path/share/chancery/clockwork/provider.json")
    [ "$inspected_provider_version" = "$inspected_version" ] \
        || fail "installed release provider version is invalid: $selector"
    expected_manifest_hash=$(
        {
            printf '%s\n' 'format=1' 'product=clockwork'
            printf 'release_id=%s\n' "$inspected_id"
            printf 'version=%s\n' "$inspected_version"
            printf 'binary_sha256=%s\n' "$computed_binary_hash"
            printf 'deployer_sha256=%s\n' "$computed_deployer_hash"
            printf 'uninstaller_sha256=%s\n' "$computed_uninstaller_hash"
            printf 'chancery_sha256=%s\n' "$computed_chancery_hash"
        } | shasum -a 256 | awk '{print $1}'
    )
    inspected_manifest_hash=$(shasum -a 256 \
        "$inspected_manifest" | awk '{print $1}')
    [ "$inspected_manifest_hash" = "$expected_manifest_hash" ] \
        || fail "installed release manifest is invalid: $selector"
}

validate_bundle "$SOURCE_CHANCERY"
candidate_version=$("$binary_path" --version) \
    || fail 'unable to read the Clockwork candidate version'
case "$candidate_version" in
    'clockwork '*) version=${candidate_version#clockwork } ;;
    *) fail "candidate reported an unexpected version: $candidate_version" ;;
esac
printf '%s\n' "$version" \
    | grep -Eq '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$' \
    || fail "candidate reported an invalid version: $version"
provider_release=$(awk -F '"' \
    '/"release"[[:space:]]*:/ { print $4; exit }' \
    "$SOURCE_CHANCERY/provider.json")
[ "$provider_release" = "$version" ] \
    || fail "provider release $provider_release does not match candidate $version"
"$binary_path" --help >/dev/null || fail 'unable to read candidate help'
sh -n "$SOURCE_DEPLOYER"
sh -n "$SOURCE_UNINSTALLER"

STATE_DIR="$install_home/Library/Application Support/Clockwork"
INSTALL_DIR="$STATE_DIR/install"
RELEASES_DIR="$INSTALL_DIR/releases"
CURRENT_LINK="$INSTALL_DIR/current"
PREVIOUS_LINK="$INSTALL_DIR/previous"
UPDATE_LOCK="$STATE_DIR/.update-lock"
CLI_DIR="$install_home/.local/bin"
CLI_PATH="$CLI_DIR/clockwork"
CHANCERY_STATE="$install_home/Library/Application Support/Chancery"
CHANCERY_PROVIDERS="$CHANCERY_STATE/providers"
CHANCERY_LINK="$CHANCERY_PROVIDERS/clockwork"
EXPECTED_CLI="$INSTALL_DIR/current/bin/clockwork"
EXPECTED_CHANCERY="$INSTALL_DIR/current/share/chancery/clockwork"

for path in "$STATE_DIR" "$INSTALL_DIR" "$RELEASES_DIR"; do
    [ ! -L "$path" ] || fail "refusing symbolic-link directory: $path"
    [ ! -e "$path" ] || [ -d "$path" ] \
        || fail "directory path is occupied by a non-directory: $path"
    install -d -m 0700 "$path"
done

ensure_shared_directory() {
    path=$1
    create_mode=$2
    [ ! -L "$path" ] || fail "refusing symbolic-link directory: $path"
    if [ -e "$path" ]; then
        [ -d "$path" ] \
            || fail "directory path is occupied by a non-directory: $path"
        if shared_uid=$(stat -f '%u' "$path" 2>/dev/null); then
            shared_mode=$(stat -f '%Lp' "$path")
        else
            shared_uid=$(stat -c '%u' "$path")
            shared_mode=$(stat -c '%a' "$path")
        fi
        [ "$shared_uid" = "$operator_uid" ] \
            && [ $((0$shared_mode & 0022)) -eq 0 ] \
            || fail "shared directory ownership or mode is unsafe: $path"
        return
    fi
    install -d -m "$create_mode" "$path"
}

ensure_shared_directory "$CLI_DIR" 0755
ensure_shared_directory "$CHANCERY_STATE" 0700
ensure_shared_directory "$CHANCERY_PROVIDERS" 0700

temporary_release=
old_current=
old_previous=
old_cli=
old_provider=
switched=0
committed=0
lock_created=0

atomic_symlink() {
    target=$1
    path=$2
    temporary="$path.tmp.$$"
    rm -f "$temporary"
    ln -s "$target" "$temporary"
    if mv -fh "$temporary" "$path" 2>/dev/null; then
        return 0
    fi
    mv -fT "$temporary" "$path"
}

cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    set +e
    if [ "$status" -ne 0 ] && [ "$committed" -eq 0 ] \
        && [ "$switched" -eq 1 ]; then
        rollback_ready=1
        if [ -n "$old_current" ]; then
            atomic_symlink "$old_current" "$CURRENT_LINK" || rollback_ready=0
        else
            rm -f "$CURRENT_LINK" || rollback_ready=0
        fi
        if [ -n "$old_previous" ]; then
            atomic_symlink "$old_previous" "$PREVIOUS_LINK" || rollback_ready=0
        else
            rm -f "$PREVIOUS_LINK" || rollback_ready=0
        fi
        if [ -n "$old_cli" ]; then
            atomic_symlink "$old_cli" "$CLI_PATH" || rollback_ready=0
        else
            rm -f "$CLI_PATH" || rollback_ready=0
        fi
        if [ -n "$old_provider" ]; then
            atomic_symlink "$old_provider" "$CHANCERY_LINK" || rollback_ready=0
        else
            rm -f "$CHANCERY_LINK" || rollback_ready=0
        fi
        if [ "$rollback_ready" -eq 0 ]; then
            detach_proven=1
            for selector in "$CLI_PATH" "$CHANCERY_LINK" "$CURRENT_LINK" "$PREVIOUS_LINK"; do
                rm -f "$selector" || detach_proven=0
                [ ! -e "$selector" ] && [ ! -L "$selector" ] || detach_proven=0
            done
            if [ "$detach_proven" -eq 1 ]; then
                printf '%s\n' \
                    'clockwork user deploy: rollback could not restore a coherent selector view; public Clockwork selectors were detached' >&2
            else
                printf '%s\n' \
                    'clockwork user deploy: rollback failed and a detached public selector view could not be proved' >&2
            fi
        fi
    fi
    [ -z "$temporary_release" ] || rm -rf "$temporary_release"
    if [ "$lock_created" -eq 1 ] && [ -f "$UPDATE_LOCK" ] \
        && [ ! -L "$UPDATE_LOCK" ] \
        && [ "$(sed -n '1p' "$UPDATE_LOCK" 2>/dev/null || true)" = "$$" ]; then
        rm -f "$UPDATE_LOCK" >/dev/null 2>&1 || true
    fi
    exit "$status"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

acquire_update_lock() {
    /usr/bin/shlock -p "$$" -f "$UPDATE_LOCK" \
        || fail "another Clockwork installation operation is active, or its lock is not safely recoverable: $UPDATE_LOCK"
}

acquire_update_lock
lock_created=1

if [ -L "$CURRENT_LINK" ]; then
    old_current=$(readlink "$CURRENT_LINK")
elif [ -e "$CURRENT_LINK" ]; then
    fail "$CURRENT_LINK must be a symbolic link"
fi
if [ -L "$PREVIOUS_LINK" ]; then
    old_previous=$(readlink "$PREVIOUS_LINK")
elif [ -e "$PREVIOUS_LINK" ]; then
    fail "$PREVIOUS_LINK must be a symbolic link"
fi
if [ -L "$CLI_PATH" ]; then
    old_cli=$(readlink "$CLI_PATH")
elif [ -e "$CLI_PATH" ]; then
    fail "$CLI_PATH exists and is not a symbolic link"
fi
if [ -L "$CHANCERY_LINK" ]; then
    old_provider=$(readlink "$CHANCERY_LINK")
elif [ -e "$CHANCERY_LINK" ]; then
    fail "$CHANCERY_LINK exists and is not a symbolic link"
fi

if [ -n "$old_current" ]; then
    validate_installed_release "$old_current"
    [ "$old_cli" = "$EXPECTED_CLI" ] \
        || fail "command selector is not owned by this installation: $CLI_PATH"
    [ "$old_provider" = "$EXPECTED_CHANCERY" ] \
        || fail "provider selector is not owned by this installation: $CHANCERY_LINK"
    if [ -n "$old_previous" ]; then
        validate_installed_release "$old_previous"
    fi
elif [ -n "$old_previous" ] || [ -n "$old_cli" ] || [ -n "$old_provider" ]; then
    fail 'installed selectors have no current Clockwork release'
fi

binary_hash=$(shasum -a 256 "$binary_path" | awk '{print $1}')
deployer_hash=$(shasum -a 256 "$SOURCE_DEPLOYER" | awk '{print $1}')
uninstaller_hash=$(shasum -a 256 "$SOURCE_UNINSTALLER" | awk '{print $1}')
chancery_hash=$(bundle_hash "$SOURCE_CHANCERY")
release_id=$(printf '%s\n' "$binary_hash" "$deployer_hash" \
    "$uninstaller_hash" "$chancery_hash" | shasum -a 256 | awk '{print $1}')
release_path="$RELEASES_DIR/$release_id"

if [ -L "$release_path" ]; then
    fail "existing release must not be a symbolic link: $release_id"
elif [ -e "$release_path" ] && [ ! -d "$release_path" ]; then
    fail "existing release is not a directory: $release_id"
fi
if [ -d "$release_path" ]; then
    validate_installed_release "releases/$release_id"
else
    temporary_release=$(mktemp -d "$RELEASES_DIR/.stage.XXXXXX")
    install -d -m 0755 "$temporary_release/bin" \
        "$temporary_release/package" "$temporary_release/share" \
        "$temporary_release/share/chancery"
    install -m 0755 "$binary_path" "$temporary_release/bin/clockwork"
    install -m 0755 "$SOURCE_DEPLOYER" \
        "$temporary_release/package/deploy-user.sh"
    install -m 0755 "$SOURCE_UNINSTALLER" \
        "$temporary_release/package/uninstall-user.sh"
    cp -R "$SOURCE_CHANCERY" \
        "$temporary_release/share/chancery/clockwork"
    {
        printf '%s\n' 'format=1' 'product=clockwork'
        printf 'release_id=%s\n' "$release_id"
        printf 'version=%s\n' "$version"
        printf 'binary_sha256=%s\n' "$binary_hash"
        printf 'deployer_sha256=%s\n' "$deployer_hash"
        printf 'uninstaller_sha256=%s\n' "$uninstaller_hash"
        printf 'chancery_sha256=%s\n' "$chancery_hash"
    } >"$temporary_release/manifest.txt"
    chmod 0444 "$temporary_release/manifest.txt"
    mv "$temporary_release" "$release_path"
    temporary_release=
    validate_installed_release "releases/$release_id"
fi

# Validate the exact provider copy retained by this release with the explicitly
# supplied candidate reader before changing any public selector.
"$chancery_path" validate "$release_path/share/chancery/clockwork" >/dev/null \
    || fail 'candidate Chancery reader rejected the staged Clockwork provider'

switched=1
if [ -n "$old_current" ]; then
    if [ "$old_current" != "releases/$release_id" ]; then
        atomic_symlink "$old_current" "$PREVIOUS_LINK"
        atomic_symlink "releases/$release_id" "$CURRENT_LINK"
    fi
else
    atomic_symlink "$EXPECTED_CLI" "$CLI_PATH"
    atomic_symlink "$EXPECTED_CHANCERY" "$CHANCERY_LINK"
    atomic_symlink "releases/$release_id" "$CURRENT_LINK"
fi

installed_version=$(HOME="$install_home" "$CLI_PATH" --version) \
    || fail 'installed Clockwork version check failed'
[ "$installed_version" = "clockwork $version" ] \
    || fail "installed Clockwork reported an unexpected version: $installed_version"
HOME="$install_home" "$CLI_PATH" --help >/dev/null \
    || fail 'installed Clockwork help check failed'
[ -f "$CHANCERY_LINK/provider.json" ] \
    || fail 'installed Clockwork Chancery provider is unavailable'
for entry_id in clockwork.install.operate clockwork.schedule.operate \
    clockwork.develop.change
do
    "$chancery_path" --registry "$CHANCERY_PROVIDERS" show "$entry_id" \
        >/dev/null \
        || fail "candidate Chancery reader cannot discover installed entry: $entry_id"
done
validate_installed_release "$(readlink "$CURRENT_LINK")"

committed=1
printf 'Installed Clockwork release %s\n' "$release_id"
printf 'Command: %s\n' "$CLI_PATH"
printf 'Chancery provider: %s\n' "$CHANCERY_LINK"
printf '%s\n' 'Product bindings and runtime state: unchanged'
