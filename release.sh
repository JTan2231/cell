#!/bin/sh

set -eu

usage() {
    printf '%s\n' 'Usage: ./release.sh --patch|--minor|--major'
}

fail() {
    printf 'release.sh: %s\n' "$1" >&2
    exit 1
}

[ "$#" -eq 1 ] || {
    usage >&2
    exit 2
}

case "$1" in
    --patch) bump=patch ;;
    --minor) bump=minor ;;
    --major) bump=major ;;
    *)
        usage >&2
        exit 2
        ;;
esac

SCRIPT_DIR=$(CDPATH='' cd "$(dirname "$0")" && pwd)
cd "$SCRIPT_DIR"
manifest_path=crates/todo/Cargo.toml

for tool in awk git grep; do
    command -v "$tool" >/dev/null 2>&1 || fail "required tool not found: $tool"
done

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
command -v cargo >/dev/null 2>&1 || fail 'required tool not found: cargo'

[ -f Cargo.toml ] || fail 'workspace Cargo.toml not found'
[ -f Cargo.lock ] || fail 'Cargo.lock not found'
[ -f "$manifest_path" ] || fail "package manifest not found: $manifest_path"

[ -z "$(git status --porcelain --untracked-files=all)" ] \
    || fail 'the worktree must be completely clean'
branch=$(git symbolic-ref --quiet --short HEAD) \
    || fail 'HEAD must be attached to the main branch'
[ "$branch" = main ] || fail "releases must be made from main, not $branch"
git remote get-url origin >/dev/null 2>&1 \
    || fail 'the origin remote is not configured'
git var GIT_AUTHOR_IDENT >/dev/null 2>&1 \
    || fail 'Git author identity is not configured'

cargo metadata --locked --offline --no-deps --format-version 1 >/dev/null \
    || fail 'Cargo.toml and Cargo.lock are not synchronized'

current_version=$(awk '
    $0 == "[package]" { in_package = 1; next }
    in_package && /^\[/ { exit }
    in_package && /^[[:space:]]*version[[:space:]]*=/ {
        version = $0
        sub(/^[^=]*=[[:space:]]*"/, "", version)
        sub(/"[[:space:]]*$/, "", version)
        print version
        exit
    }
' "$manifest_path")

printf '%s\n' "$current_version" \
    | grep -Eq '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$' \
    || fail "package version must be numeric MAJOR.MINOR.PATCH; found $current_version"

major=${current_version%%.*}
remainder=${current_version#*.}
minor=${remainder%%.*}
patch=${remainder#*.}
case "$bump" in
    patch) patch=$((patch + 1)) ;;
    minor)
        minor=$((minor + 1))
        patch=0
        ;;
    major)
        major=$((major + 1))
        minor=0
        patch=0
        ;;
esac

new_version="$major.$minor.$patch"
tag="v$new_version"
local_revision=$(git rev-parse HEAD)

if git rev-parse --verify --quiet "refs/tags/$tag" >/dev/null; then
    fail "tag already exists locally: $tag"
fi
remote_tag=$(git ls-remote --tags origin "refs/tags/$tag" "refs/tags/$tag^{}") \
    || fail 'unable to inspect tags on origin'
[ -z "$remote_tag" ] || fail "tag already exists on origin: $tag"

remote_main=$(git ls-remote --heads origin refs/heads/main) \
    || fail 'unable to inspect main on origin'
if [ -n "$remote_main" ]; then
    remote_revision=$(printf '%s\n' "$remote_main" | awk 'NR == 1 { print $1 }')
    [ "$remote_revision" = "$local_revision" ] \
        || fail 'local main must exactly match origin/main before release'
else
    remote_refs=$(git ls-remote origin) || fail 'unable to inspect origin'
    [ -z "$remote_refs" ] \
        || fail 'origin has refs but no main branch; refusing initial publication'
fi

manifest_tmp="$manifest_path.release.$$"
rollback_version_files=false
cleanup() {
    status=$?
    trap - 0 1 2 15
    set +e
    rm -f "$manifest_tmp"
    if [ "$rollback_version_files" = true ]; then
        git restore --staged --worktree -- "$manifest_path" Cargo.lock
        printf 'release.sh: restored %s and Cargo.lock after failure\n' \
            "$manifest_path" >&2
    fi
    exit "$status"
}
trap cleanup 0
trap 'exit 129' 1
trap 'exit 130' 2
trap 'exit 143' 15
rollback_version_files=true

awk -v old="$current_version" -v new="$new_version" '
    BEGIN { changed = 0 }
    $0 == "[package]" { in_package = 1; print; next }
    in_package && /^\[/ { in_package = 0 }
    in_package && $0 == "version = \"" old "\"" {
        print "version = \"" new "\""
        changed++
        next
    }
    { print }
    END { if (changed != 1) exit 1 }
' "$manifest_path" >"$manifest_tmp" \
    || fail "unable to update the package version in $manifest_path"
mv "$manifest_tmp" "$manifest_path"

cargo update --workspace --offline || fail 'unable to refresh Cargo.lock'
cargo metadata --locked --offline --no-deps --format-version 1 >/dev/null \
    || fail 'the bumped manifest and lockfile are not synchronized'

./ci.sh

reported_version=$(target/release/todo --version) \
    || fail 'unable to read the release binary version'
[ "$reported_version" = "todo $new_version" ] \
    || fail "release binary reported an unexpected version: $reported_version"

[ -z "$(git diff --cached --name-only)" ] \
    || fail 'the index changed while running release checks'
[ -z "$(git ls-files --others --exclude-standard)" ] \
    || fail 'untracked files appeared while running release checks'
changed_files=$(git diff --name-only)
expected_files=$(printf '%s\n' Cargo.lock "$manifest_path")
[ "$changed_files" = "$expected_files" ] \
    || fail "files other than $manifest_path and Cargo.lock changed during release checks"
git diff --check

git add -- "$manifest_path" Cargo.lock
git diff --cached --check
git commit -m "Release $tag"

rollback_version_files=false
trap - 0 1 2 15

[ -z "$(git status --porcelain --untracked-files=all)" ] \
    || fail 'the worktree changed while creating the release commit'

if ! git tag -a "$tag" -m "Todo $tag"; then
    printf '%s\n' \
        "release.sh: tagging failed; the local release commit was preserved" \
        "release.sh: retry with: git tag -a $tag -m 'Todo $tag'" >&2
    exit 1
fi

if ! git push --atomic --set-upstream origin \
    HEAD:refs/heads/main \
    "refs/tags/$tag:refs/tags/$tag"
then
    printf '%s\n' \
        "release.sh: push failed; local release commit and tag $tag were preserved" \
        "release.sh: retry with: git push --atomic --set-upstream origin HEAD:refs/heads/main refs/tags/$tag:refs/tags/$tag" >&2
    exit 1
fi

printf 'Released Todo %s\n' "$new_version"
