#!/bin/sh

set -eu

PIPELINE_ROOT=$(CDPATH='' cd "$(dirname "$0")/.." && pwd)
export PIPELINE_ROOT
. "$PIPELINE_ROOT/pipeline/lib.sh"

release_fail() {
    printf 'release.sh: %s\n' "$1" >&2
    exit 1
}

release_usage() {
    printf '%s\n' "$RELEASE_USAGE"
}

release_metadata() {
    metadata_locked=$1
    metadata_no_deps=$2
    set -- cargo metadata --manifest-path "$PIPELINE_ROOT/Cargo.toml"
    if [ "$metadata_locked" = 1 ]; then
        set -- "$@" --locked
    fi
    set -- "$@" --offline
    if [ "$metadata_no_deps" = 1 ]; then
        set -- "$@" --no-deps
    fi
    set -- "$@" --format-version 1
    "$@" >/dev/null
}

release_remote_main_matches() {
    expected_revision=$1
    remote_main=$(git ls-remote --heads origin "refs/heads/$RELEASE_BRANCH") \
        || release_fail "unable to inspect $RELEASE_BRANCH on origin"
    if [ -n "$remote_main" ]; then
        remote_revision=$(printf '%s\n' "$remote_main" | awk 'NR == 1 { print $1 }')
        [ "$remote_revision" = "$expected_revision" ] \
            || release_fail "origin/$RELEASE_BRANCH changed while preparing the release"
    else
        remote_refs=$(git ls-remote origin) \
            || release_fail 'unable to inspect origin'
        [ -z "$remote_refs" ] \
            || release_fail "origin has refs but no $RELEASE_BRANCH branch; refusing initial publication"
    fi
}

release_remote_tag_absent() {
    checked_tag=$1
    remote_tag=$(git ls-remote --tags origin \
        "refs/tags/$checked_tag" "refs/tags/$checked_tag^{}") \
        || release_fail 'unable to inspect tags on origin'
    [ -z "$remote_tag" ] || release_fail "tag already exists on origin: $checked_tag"
}

release_provider_paths() {
    selected_unit=$1
    printf '%s\n' "$PROVIDERS" | awk -F '|' -v unit="$selected_unit" '
        $1 == unit { print $3 "/provider.json" }
    '
}

release_restore_files() {
    set -- "$version_manifest" "$lockfile_path"
    while IFS= read -r provider_manifest; do
        [ -n "$provider_manifest" ] || continue
        set -- "$@" "$provider_manifest"
    done <<EOF
$(release_provider_paths "$release_unit")
EOF
    git restore --staged --worktree -- "$@"
}

release_stage_files() {
    set -- "$version_manifest" "$lockfile_path"
    while IFS= read -r provider_manifest; do
        [ -n "$provider_manifest" ] || continue
        set -- "$@" "$provider_manifest"
    done <<EOF
$(release_provider_paths "$release_unit")
EOF
    git add -- "$@"
}

release_expected_files() {
    printf '%s\n' "$version_manifest" "$lockfile_path"
    release_provider_paths "$release_unit"
}

release_update_version() {
    old_version=$1
    replacement_version=$2
    source_kind=$3
    source_path=$4
    output_path=$5
    case "$source_kind" in
        package) source_heading='[package]' ;;
        workspace-package) source_heading='[workspace.package]' ;;
        *) release_fail "unsupported version source: $source_kind" ;;
    esac
    awk -v heading="$source_heading" -v old="$old_version" \
        -v new="$replacement_version" '
        BEGIN { changed = 0 }
        $0 == heading { in_section = 1; print; next }
        in_section && /^\[/ { in_section = 0 }
        in_section && $0 == "version = \"" old "\"" {
            print "version = \"" new "\""
            changed++
            next
        }
        { print }
        END { if (changed != 1) exit 1 }
    ' "$source_path" >"$output_path"
}

release_update_provider() {
    old_version=$1
    replacement_version=$2
    source_path=$3
    output_path=$4
    awk -v old="$old_version" -v new="$replacement_version" '
        BEGIN { changed = 0 }
        index($0, "\"release\": \"" old "\"") {
            sub("\"release\": \"" old "\"", "\"release\": \"" new "\"")
            changed++
        }
        { print }
        END { if (changed != 1) exit 1 }
    ' "$source_path" >"$output_path"
}

release_check_binaries() {
    while IFS='|' read -r unit binary_path command_name; do
        [ "$unit" = "$release_unit" ] || continue
        case "$binary_path" in
            target/*) absolute_binary=$(pipeline_target_file "${binary_path#target/}") ;;
            *) absolute_binary="$PIPELINE_ROOT/$binary_path" ;;
        esac
        reported_version=$("$absolute_binary" --version) \
            || release_fail "unable to read the $command_name release binary version"
        [ "$reported_version" = "$command_name $new_version" ] \
            || release_fail "$command_name reported an unexpected version: $reported_version"
    done <<EOF
$RELEASE_BINARY_CHECKS
EOF
}

release_lock_owned=false
release_lock_kind=
rollback_version_files=false
temporary_files=
release_lock_dir=

release_cleanup() {
    cleanup_status=$?
    trap - 0 1 2 15
    set +e
    old_ifs=$IFS
    IFS='
'
    for temporary_file in $temporary_files; do
        [ -n "$temporary_file" ] && rm -f "$temporary_file"
    done
    IFS=$old_ifs
    if [ "$rollback_version_files" = true ]; then
        release_restore_files
        printf '%s\n' 'release.sh: restored version files after failure' >&2
    fi
    if [ "$release_lock_owned" = true ]; then
        case "$release_lock_kind" in
            shlock) rm -f "$release_lock_dir" ;;
            mkdir)
                rm -f "$release_lock_dir/owner"
                rmdir "$release_lock_dir"
                ;;
        esac
    fi
    exit "$cleanup_status"
}

[ "$#" -ge 1 ] || pipeline_fail 'usage: pipeline/release.sh PRODUCT ARGS...'
product_id=$1
shift
pipeline_load_descriptor "$product_id"
pipeline_validate_descriptor

case "$#" in
    1)
        release_unit=$(pipeline_default_unit) \
            || { release_usage >&2; exit 2; }
        ;;
    2)
        [ "$RELEASE_ALLOW_EXPLICIT_UNIT" = 1 ] \
            || { release_usage >&2; exit 2; }
        release_unit=$1
        shift
        pipeline_unit_field "$release_unit" 1 >/dev/null 2>&1 \
            || { release_usage >&2; exit 2; }
        ;;
    *)
        release_usage >&2
        exit 2
        ;;
esac

case "$1" in
    --patch) bump=patch ;;
    --minor) bump=minor ;;
    --major) bump=major ;;
    *) release_usage >&2; exit 2 ;;
esac

release_name=$(pipeline_unit_field "$release_unit" 2)
version_kind=$(pipeline_unit_field "$release_unit" 3)
version_manifest=$(pipeline_unit_field "$release_unit" 4)
tag_prefix=$(pipeline_unit_field "$release_unit" 5)
lockfile_path=Cargo.lock

cd "$PIPELINE_ROOT"
for tool in awk git grep sort; do
    command -v "$tool" >/dev/null 2>&1 \
        || release_fail "required tool not found: $tool"
done
pipeline_bootstrap_cargo
command -v cargo >/dev/null 2>&1 \
    || release_fail 'required tool not found: cargo'

[ -f Cargo.toml ] || release_fail 'root workspace Cargo.toml not found'
[ -f "$lockfile_path" ] || release_fail 'root Cargo.lock not found'
[ -f "$version_manifest" ] \
    || release_fail "package manifest not found: $version_manifest"
while IFS= read -r provider_manifest; do
    [ -f "$provider_manifest" ] \
        || release_fail "provider manifest not found: $provider_manifest"
done <<EOF
$(release_provider_paths "$release_unit")
EOF

git rev-parse --is-inside-work-tree >/dev/null 2>&1 \
    || release_fail 'the workspace is not a Git worktree'
git_common_dir=$(git rev-parse --git-common-dir) \
    || release_fail 'unable to resolve the Git common directory'
case "$git_common_dir" in
    /*) ;;
    *) git_common_dir="$PIPELINE_ROOT/$git_common_dir" ;;
esac
git_common_dir=$(CDPATH='' cd "$git_common_dir" && pwd)
# Release verification must inspect the same storage-bounded target used by
# brokered product CI, regardless of a caller's inherited Cargo target.
CARGO_TARGET_DIR="$(dirname "$git_common_dir")/target"
export CARGO_TARGET_DIR
release_lock_dir="$git_common_dir/cell-release-publication.lock"
if command -v shlock >/dev/null 2>&1; then
    release_lock_kind=shlock
    shlock -p "$$" -f "$release_lock_dir" \
        || release_fail "another Cell release holds the publication lock: $release_lock_dir"
else
    # mkdir is a portable, fail-closed fallback. It deliberately does not
    # guess that an ownerless lock is stale.
    release_lock_kind=mkdir
    release_lock_dir="$release_lock_dir.d"
    mkdir "$release_lock_dir" 2>/dev/null \
        || release_fail "another Cell release holds the publication lock: $release_lock_dir"
fi
release_lock_owned=true
if [ "$release_lock_kind" = mkdir ]; then
    printf '%s\n' "pid=$$ product=$PRODUCT_ID unit=$release_unit" \
        >"$release_lock_dir/owner"
fi
trap release_cleanup 0
trap 'exit 129' 1
trap 'exit 130' 2
trap 'exit 143' 15

[ -z "$(git status --porcelain --untracked-files=all)" ] \
    || release_fail 'the worktree must be completely clean'
branch=$(git symbolic-ref --quiet --short HEAD) \
    || release_fail 'HEAD must be attached to the main branch'
[ "$branch" = "$RELEASE_BRANCH" ] \
    || release_fail "releases must be made from $RELEASE_BRANCH, not $branch"
git remote get-url origin >/dev/null 2>&1 \
    || release_fail 'the origin remote is not configured'
git var GIT_AUTHOR_IDENT >/dev/null 2>&1 \
    || release_fail 'Git author identity is not configured'
release_metadata 1 "$RELEASE_METADATA_NO_DEPS" \
    || release_fail 'Cargo.toml and Cargo.lock are not synchronized'

current_version=$(pipeline_read_version "$version_kind" "$version_manifest")
printf '%s\n' "$current_version" \
    | grep -Eq '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$' \
    || release_fail "package version must be numeric MAJOR.MINOR.PATCH; found $current_version"

while IFS= read -r provider_manifest; do
    provider_version=$(pipeline_provider_release "$provider_manifest")
    [ "$provider_version" = "$current_version" ] \
        || release_fail "provider release $provider_version does not match package version $current_version"
done <<EOF
$(release_provider_paths "$release_unit")
EOF

major=${current_version%%.*}
remainder=${current_version#*.}
minor=${remainder%%.*}
patch=${remainder#*.}
case "$bump" in
    patch) patch=$((patch + 1)) ;;
    minor) minor=$((minor + 1)); patch=0 ;;
    major) major=$((major + 1)); minor=0; patch=0 ;;
esac
new_version="$major.$minor.$patch"
tag="${tag_prefix}v$new_version"
local_revision=$(git rev-parse HEAD)

git rev-parse --verify --quiet "refs/tags/$tag" >/dev/null \
    && release_fail "tag already exists locally: $tag"
release_remote_tag_absent "$tag"
release_remote_main_matches "$local_revision"

manifest_tmp="$version_manifest.release.$$"
temporary_files=$manifest_tmp
rollback_version_files=true
release_update_version "$current_version" "$new_version" "$version_kind" \
    "$version_manifest" "$manifest_tmp" \
    || release_fail "unable to update $version_manifest"
mv "$manifest_tmp" "$version_manifest"

while IFS= read -r provider_manifest; do
    provider_tmp="$provider_manifest.release.$$"
    temporary_files="$temporary_files
$provider_tmp"
    release_update_provider "$current_version" "$new_version" \
        "$provider_manifest" "$provider_tmp" \
        || release_fail "unable to update $provider_manifest"
    mv "$provider_tmp" "$provider_manifest"
done <<EOF
$(release_provider_paths "$release_unit")
EOF

release_metadata 0 0 || release_fail 'unable to refresh root Cargo.lock'
release_metadata 1 "$RELEASE_METADATA_NO_DEPS" \
    || release_fail 'the bumped manifest and lockfile are not synchronized'

# A release intentionally created a new dirty candidate; it must not inherit an
# enclosing root plan's source binding.
unset CELL_CI_EXPECTED_SOURCE_KEY
"$PIPELINE_ROOT/$PRODUCT_DIR/ci.sh"
release_check_binaries

[ -z "$(git diff --cached --name-only)" ] \
    || release_fail 'the index changed while running release checks'
[ -z "$(git ls-files --others --exclude-standard)" ] \
    || release_fail 'untracked files appeared while running release checks'
changed_files=$(git diff --name-only | LC_ALL=C sort)
expected_files=$(release_expected_files | LC_ALL=C sort)
[ "$changed_files" = "$expected_files" ] \
    || release_fail 'files other than the release version files changed during release checks'
git diff --check

# The lock coordinates Cell release commands. Rechecking the remote here also
# catches publication by any process that does not participate in the lock.
release_remote_main_matches "$local_revision"
release_remote_tag_absent "$tag"
release_stage_files
git diff --cached --check
git commit -m "Release $tag"
rollback_version_files=false

[ -z "$(git status --porcelain --untracked-files=all)" ] \
    || release_fail 'the worktree changed while creating the release commit'
release_remote_main_matches "$local_revision"
release_remote_tag_absent "$tag"
git rev-parse --verify --quiet "refs/tags/$tag" >/dev/null \
    && release_fail "tag already exists locally: $tag"
if ! git tag -a "$tag" -m "$release_name v$new_version"; then
    release_fail "tagging failed; local release commit was preserved for $tag"
fi

release_remote_main_matches "$local_revision"
release_remote_tag_absent "$tag"
if ! git push --atomic --set-upstream origin \
    "HEAD:refs/heads/$RELEASE_BRANCH" "refs/tags/$tag:refs/tags/$tag"
then
    release_fail "push failed; local release commit and tag $tag were preserved"
fi

printf 'Released %s %s\n' "$release_name" "$new_version"
