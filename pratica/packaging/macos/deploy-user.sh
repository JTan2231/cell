#!/bin/sh

set -eu

PATH=/usr/bin:/bin:/usr/sbin:/sbin
export PATH
umask 077

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
SOURCE_DEPLOYER="$SCRIPT_DIR/deploy-user.sh"
if [ -d "$SCRIPT_DIR/../share/chancery/pratica" ]; then
    SOURCE_CHANCERY="$SCRIPT_DIR/../share/chancery/pratica"
else
    SOURCE_CHANCERY="$SCRIPT_DIR/../../chancery"
fi

binary_path=
install_home=${HOME:-}

usage() {
    cat <<'EOF'
Usage: deploy-user.sh --binary ABSOLUTE_PATH [OPTIONS]

Install or update the current user's macOS Pratica CLI and product-owned
Chancery provider. Deployment does not open, initialize, or migrate negotiation
storage and does not invoke or restart Nucleus.

Options:
  --home ABSOLUTE_PATH  Override the operator home (primarily for tests)
EOF
}

fail() {
    printf 'pratica user deploy: %s\n' "$*" >&2
    exit 1
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --binary)
            [ "$#" -ge 2 ] || fail '--binary requires a path'
            binary_path=$2
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
[ -n "$install_home" ] || fail '--home is required'
case "$binary_path" in /*) ;; *) fail 'binary path must be absolute' ;; esac
case "$install_home" in /*) ;; *) fail 'home path must be absolute' ;; esac

[ -d "$install_home" ] && [ ! -L "$install_home" ] \
    || fail "operator home is not a regular directory: $install_home"
[ -f "$binary_path" ] && [ ! -L "$binary_path" ] && [ -x "$binary_path" ] \
    || fail "candidate is not an executable regular file: $binary_path"
[ -f "$SOURCE_DEPLOYER" ] && [ ! -L "$SOURCE_DEPLOYER" ] \
    || fail "packaged deployer is not a regular file: $SOURCE_DEPLOYER"
for command in awk cp find grep id install mktemp mv readlink shasum sort stat; do
    command -v "$command" >/dev/null 2>&1 \
        || fail "required command not found: $command"
done

operator_uid=$(id -u)
[ "$operator_uid" -ne 0 ] || fail 'run as the Pratica operator, not root'
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
    actual_bundle_tree=$(cd "$bundle" && find . -print | LC_ALL=C sort)
    expected_bundle_tree=$(printf '%s\n' \
        . \
        ./entries \
        ./entries/agreement-explore.json \
        ./entries/develop-change.json \
        ./entries/install-operate.json \
        ./entries/integration-negotiate.json \
        ./manuals \
        ./manuals/agreement-explore.md \
        ./manuals/develop-change.md \
        ./manuals/install-operate.md \
        ./manuals/integration-negotiate.md \
        ./provider.json \
        | LC_ALL=C sort)
    [ "$actual_bundle_tree" = "$expected_bundle_tree" ] \
        || fail "Chancery bundle tree is not the exact Pratica v0.1 layout: $bundle"
    bundle_provider=$(awk -F '"' \
        '/"id"[[:space:]]*:/ { print $4; exit }' "$bundle/provider.json")
    [ "$bundle_provider" = pratica ] \
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
        || fail "invalid Pratica release selector: $selector"
}

validate_installed_release() {
    selector=$1
    validate_release_selector "$selector"
    inspected_id=${selector#releases/}
    inspected_path="$INSTALL_DIR/$selector"

    for directory in "$inspected_path" "$inspected_path/bin" \
        "$inspected_path/package" "$inspected_path/share" \
        "$inspected_path/share/chancery" \
        "$inspected_path/share/chancery/pratica"; do
        [ -d "$directory" ] && [ ! -L "$directory" ] \
            || fail "installed release has an invalid directory: $selector"
    done
    if find "$inspected_path" -type l -print | grep -q .; then
        fail "installed release contains a symbolic link: $selector"
    fi
    if find "$inspected_path" ! -type d ! -type f -print | grep -q .; then
        fail "installed release contains a non-file entry: $selector"
    fi
    actual_release_tree=$(cd "$inspected_path" && find . -print | LC_ALL=C sort)
    expected_release_tree=$(printf '%s\n' \
        . \
        ./bin \
        ./bin/pratica \
        ./manifest.txt \
        ./package \
        ./package/deploy-user.sh \
        ./share \
        ./share/chancery \
        ./share/chancery/pratica \
        ./share/chancery/pratica/entries \
        ./share/chancery/pratica/entries/agreement-explore.json \
        ./share/chancery/pratica/entries/develop-change.json \
        ./share/chancery/pratica/entries/install-operate.json \
        ./share/chancery/pratica/entries/integration-negotiate.json \
        ./share/chancery/pratica/manuals \
        ./share/chancery/pratica/manuals/agreement-explore.md \
        ./share/chancery/pratica/manuals/develop-change.md \
        ./share/chancery/pratica/manuals/install-operate.md \
        ./share/chancery/pratica/manuals/integration-negotiate.md \
        ./share/chancery/pratica/provider.json \
        | LC_ALL=C sort)
    [ "$actual_release_tree" = "$expected_release_tree" ] \
        || fail "installed release tree is not the exact Pratica v0.1 layout: $selector"
    [ -f "$inspected_path/bin/pratica" ] \
        && [ ! -L "$inspected_path/bin/pratica" ] \
        && [ -x "$inspected_path/bin/pratica" ] \
        || fail "installed release has an invalid binary: $selector"
    [ -f "$inspected_path/package/deploy-user.sh" ] \
        && [ ! -L "$inspected_path/package/deploy-user.sh" ] \
        && [ -x "$inspected_path/package/deploy-user.sh" ] \
        || fail "installed release has an invalid deployer: $selector"
    [ -f "$inspected_path/manifest.txt" ] \
        && [ ! -L "$inspected_path/manifest.txt" ] \
        || fail "installed release has an invalid manifest: $selector"
    validate_bundle "$inspected_path/share/chancery/pratica"

    inspected_manifest="$inspected_path/manifest.txt"
    [ "$(awk 'END { print NR }' "$inspected_manifest")" -eq 7 ] \
        || fail "installed release manifest is not canonical: $selector"
    [ "$(sed -n '1p' "$inspected_manifest")" = 'format=1' ] \
        || fail "installed release manifest format is unsupported: $selector"
    [ "$(sed -n '2p' "$inspected_manifest")" = 'product=pratica' ] \
        || fail "installed release manifest has foreign ownership: $selector"
    inspected_manifest_id=$(sed -n '3s/^release_id=//p' "$inspected_manifest")
    inspected_version=$(sed -n '4s/^version=//p' "$inspected_manifest")
    inspected_binary_hash=$(sed -n '5s/^binary_sha256=//p' "$inspected_manifest")
    inspected_deployer_hash=$(sed -n '6s/^deployer_sha256=//p' "$inspected_manifest")
    inspected_chancery_hash=$(sed -n '7s/^chancery_sha256=//p' "$inspected_manifest")
    [ "$inspected_manifest_id" = "$inspected_id" ] \
        || fail "installed release manifest identity is invalid: $selector"
    printf '%s\n' "$inspected_manifest_id" "$inspected_binary_hash" \
        "$inspected_deployer_hash" "$inspected_chancery_hash" \
        | grep -Eqv '^[0-9a-f]{64}$' \
        && fail "installed release manifest hashes are invalid: $selector"
    printf '%s\n' "$inspected_version" \
        | grep -Eq '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$' \
        || fail "installed release version is invalid: $selector"

    computed_binary_hash=$(shasum -a 256 \
        "$inspected_path/bin/pratica" | awk '{print $1}')
    computed_deployer_hash=$(shasum -a 256 \
        "$inspected_path/package/deploy-user.sh" | awk '{print $1}')
    computed_chancery_hash=$(bundle_hash \
        "$inspected_path/share/chancery/pratica")
    [ "$computed_binary_hash" = "$inspected_binary_hash" ] \
        || fail "installed release binary is tampered: $selector"
    [ "$computed_deployer_hash" = "$inspected_deployer_hash" ] \
        || fail "installed release deployer is tampered: $selector"
    [ "$computed_chancery_hash" = "$inspected_chancery_hash" ] \
        || fail "installed release provider is tampered: $selector"
    computed_id=$(printf '%s\n' "$computed_binary_hash" \
        "$computed_deployer_hash" "$computed_chancery_hash" \
        | shasum -a 256 | awk '{print $1}')
    [ "$computed_id" = "$inspected_id" ] \
        || fail "installed release content identity is invalid: $selector"

    inspected_provider_version=$(awk -F '"' \
        '/"release"[[:space:]]*:/ { print $4; exit }' \
        "$inspected_path/share/chancery/pratica/provider.json")
    [ "$inspected_provider_version" = "$inspected_version" ] \
        || fail "installed release provider version is invalid: $selector"
    expected_manifest_hash=$(
        {
            printf '%s\n' 'format=1' 'product=pratica'
            printf 'release_id=%s\n' "$inspected_id"
            printf 'version=%s\n' "$inspected_version"
            printf 'binary_sha256=%s\n' "$computed_binary_hash"
            printf 'deployer_sha256=%s\n' "$computed_deployer_hash"
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
    || fail 'unable to read the Pratica candidate version'
case "$candidate_version" in
    'pratica '*) version=${candidate_version#pratica } ;;
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

STATE_DIR="$install_home/Library/Application Support/Pratica"
INSTALL_DIR="$STATE_DIR/install"
RELEASES_DIR="$INSTALL_DIR/releases"
CURRENT_LINK="$INSTALL_DIR/current"
PREVIOUS_LINK="$INSTALL_DIR/previous"
UPDATE_LOCK="$INSTALL_DIR/.update-lock"
CLI_DIR="$install_home/.local/bin"
CLI_PATH="$CLI_DIR/pratica"
CHANCERY_STATE="$install_home/Library/Application Support/Chancery"
CHANCERY_PROVIDERS="$CHANCERY_STATE/providers"
CHANCERY_LINK="$CHANCERY_PROVIDERS/pratica"
EXPECTED_CLI="$INSTALL_DIR/current/bin/pratica"
EXPECTED_CHANCERY="$INSTALL_DIR/current/share/chancery/pratica"

for path in "$STATE_DIR" "$INSTALL_DIR" "$RELEASES_DIR" \
    "$CHANCERY_STATE" "$CHANCERY_PROVIDERS"; do
    [ ! -L "$path" ] || fail "refusing symbolic-link directory: $path"
    [ ! -e "$path" ] || [ -d "$path" ] \
        || fail "directory path is occupied by a non-directory: $path"
    install -d -m 0700 "$path"
done
[ ! -L "$CLI_DIR" ] || fail "refusing symbolic-link directory: $CLI_DIR"
[ ! -e "$CLI_DIR" ] || [ -d "$CLI_DIR" ] \
    || fail "directory path is occupied by a non-directory: $CLI_DIR"
install -d -m 0755 "$CLI_DIR"

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
        if [ -n "$old_current" ]; then
            atomic_symlink "$old_current" "$CURRENT_LINK"
        else
            rm -f "$CURRENT_LINK"
        fi
        if [ -n "$old_previous" ]; then
            atomic_symlink "$old_previous" "$PREVIOUS_LINK"
        else
            rm -f "$PREVIOUS_LINK"
        fi
        if [ -n "$old_cli" ]; then
            atomic_symlink "$old_cli" "$CLI_PATH"
        else
            rm -f "$CLI_PATH"
        fi
        if [ -n "$old_provider" ]; then
            atomic_symlink "$old_provider" "$CHANCERY_LINK"
        else
            rm -f "$CHANCERY_LINK"
        fi
    fi
    [ -z "$temporary_release" ] || rm -rf "$temporary_release"
    [ "$lock_created" -eq 0 ] \
        || rmdir "$UPDATE_LOCK" >/dev/null 2>&1 || true
    exit "$status"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

mkdir "$UPDATE_LOCK" 2>/dev/null \
    || fail "another Pratica deployment is active: $UPDATE_LOCK"
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
    fail 'installed selectors have no current Pratica release'
fi

binary_hash=$(shasum -a 256 "$binary_path" | awk '{print $1}')
deployer_hash=$(shasum -a 256 "$SOURCE_DEPLOYER" | awk '{print $1}')
chancery_hash=$(bundle_hash "$SOURCE_CHANCERY")
release_id=$(printf '%s\n' "$binary_hash" "$deployer_hash" "$chancery_hash" \
    | shasum -a 256 | awk '{print $1}')
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
    install -m 0755 "$binary_path" "$temporary_release/bin/pratica"
    install -m 0755 "$SOURCE_DEPLOYER" \
        "$temporary_release/package/deploy-user.sh"
    cp -R "$SOURCE_CHANCERY" \
        "$temporary_release/share/chancery/pratica"
    {
        printf '%s\n' 'format=1' 'product=pratica'
        printf 'release_id=%s\n' "$release_id"
        printf 'version=%s\n' "$version"
        printf 'binary_sha256=%s\n' "$binary_hash"
        printf 'deployer_sha256=%s\n' "$deployer_hash"
        printf 'chancery_sha256=%s\n' "$chancery_hash"
    } >"$temporary_release/manifest.txt"
    chmod 0444 "$temporary_release/manifest.txt"
    mv "$temporary_release" "$release_path"
    temporary_release=
    validate_installed_release "releases/$release_id"
fi

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
    || fail 'installed Pratica version check failed'
[ "$installed_version" = "pratica $version" ] \
    || fail "installed Pratica reported an unexpected version: $installed_version"
HOME="$install_home" "$CLI_PATH" --help >/dev/null \
    || fail 'installed Pratica help check failed'
[ -f "$CHANCERY_LINK/provider.json" ] \
    || fail 'installed Pratica Chancery provider is unavailable'
validate_installed_release "$(readlink "$CURRENT_LINK")"

committed=1
printf 'Installed Pratica release %s\n' "$release_id"
printf 'Command: %s\n' "$CLI_PATH"
printf 'Chancery provider: %s\n' "$CHANCERY_LINK"
printf '%s\n' 'Database: untouched by deployment; run `pratica init` separately'
