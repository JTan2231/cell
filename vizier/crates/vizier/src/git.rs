use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output, Stdio};

use crate::error::{AppError, AppResult};
use crate::model::PathScope;

const ZERO_OID: &str = "0000000000000000000000000000000000000000";

pub fn canonical_repository(path: &Path) -> AppResult<PathBuf> {
    if !path.is_absolute() {
        return Err(AppError::new(
            "repository_not_absolute",
            "repository path must be absolute",
        ));
    }
    let canonical = path.canonicalize().map_err(|error| {
        AppError::new(
            "repository_unavailable",
            format!("cannot open repository {}: {error}", path.display()),
        )
    })?;
    let inside = git_output(&canonical, ["rev-parse", "--is-inside-work-tree"])?;
    if text(&inside)?.trim() != "true" {
        return Err(AppError::new(
            "repository_invalid",
            format!("{} is not a Git worktree", canonical.display()),
        ));
    }
    Ok(canonical)
}

pub fn resolve_commit(repository: &Path, revision: &str) -> AppResult<String> {
    validate_revision(revision)?;
    let spec = format!("{revision}^{{commit}}");
    let output = git_output(repository, ["rev-parse", "--verify", spec.as_str()])?;
    let oid = text(&output)?.trim().to_owned();
    if oid.len() < 40 || !oid.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(AppError::new(
            "source_revision_invalid",
            "Git returned an invalid source commit identity",
        ));
    }
    Ok(oid)
}

pub fn require_git() -> AppResult<String> {
    let output = Command::new("git")
        .arg("--version")
        .output()
        .map_err(|error| AppError::new("git_unavailable", error.to_string()))?;
    if !output.status.success() {
        return Err(AppError::new("git_unavailable", "git --version failed"));
    }
    Ok(text(&output)?.trim().to_owned())
}

pub fn prepare_worktree(
    repository: &Path,
    state_root: &Path,
    run_id: &str,
    attempt_id: &str,
    base_commit: &str,
) -> AppResult<PathBuf> {
    validate_internal_id(run_id)?;
    validate_internal_id(attempt_id)?;
    let root = state_root.join("worktrees").join(run_id);
    private_directory(&root)?;
    let path = root.join(attempt_id);
    if path.exists() {
        let actual = resolve_commit(&path, "HEAD")?;
        if actual != base_commit {
            return Err(AppError::new(
                "worktree_base_conflict",
                format!("existing worktree {} has unexpected HEAD", path.display()),
            ));
        }
        return Ok(path);
    }
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(["worktree", "add", "--detach"])
        .arg(&path)
        .arg(base_commit)
        .output()?;
    require_success(output, "git_worktree_failed")?;
    Ok(path)
}

pub fn remove_worktree(repository: &Path, path: &Path) -> AppResult<()> {
    if !path.exists() {
        return Ok(());
    }
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(["worktree", "remove", "--force"])
        .arg(path)
        .output()?;
    require_success(output, "git_worktree_remove_failed")?;
    Ok(())
}

pub fn quarantine_worktree(
    repository: &Path,
    state_root: &Path,
    run_id: &str,
    attempt_id: &str,
    path: &Path,
) -> AppResult<Option<PathBuf>> {
    if !path.exists() {
        return Ok(None);
    }
    validate_internal_id(run_id)?;
    validate_internal_id(attempt_id)?;
    let root = state_root.join("quarantine").join(run_id);
    private_directory(&root)?;
    let destination = root.join(attempt_id);
    if destination.exists() {
        return Err(AppError::new(
            "quarantine_conflict",
            format!("quarantine {} already exists", destination.display()),
        ));
    }
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(["worktree", "move"])
        .arg(path)
        .arg(&destination)
        .output()?;
    require_success(output, "git_worktree_quarantine_failed")?;
    Ok(Some(destination))
}

pub fn snapshot_worktree(
    repository: &Path,
    state_root: &Path,
    worktree: &Path,
    base_commit: &str,
    scopes: &[PathScope],
    ref_name: &str,
    message: &str,
) -> AppResult<String> {
    if let Some(existing) = optional_ref(repository, ref_name)? {
        return Ok(existing);
    }
    let head = resolve_commit(worktree, "HEAD")?;
    if head != base_commit {
        return Err(AppError::new(
            "writer_moved_head",
            "writer changed Git HEAD; Vizier writers may only change worktree files",
        ));
    }
    validate_scopes(scopes)?;
    for path in changed_paths(worktree)? {
        if !scopes.iter().any(|scope| scope_contains(scope, &path)) {
            return Err(AppError::new(
                "packet_scope_violation",
                format!("changed path {path} is outside the packet's permitted scopes"),
            ));
        }
    }
    let index_root = state_root.join("indexes");
    private_directory(&index_root)?;
    let index = index_root.join(format!("index-{}", uuid::Uuid::now_v7()));
    let read = git_with_index(worktree, &index, ["read-tree", base_commit])?;
    require_success(read, "git_snapshot_failed")?;
    let add = git_with_index(worktree, &index, ["add", "-A", "--", "."])?;
    require_success(add, "git_snapshot_failed")?;
    let tree_output = git_with_index(worktree, &index, ["write-tree"])?;
    let tree = text(&require_success(tree_output, "git_snapshot_failed")?)?
        .trim()
        .to_owned();
    let _ = fs::remove_file(&index);
    let commit = commit_tree(repository, &tree, &[base_commit], message)?;
    create_immutable_ref(repository, ref_name, &commit)?;
    Ok(commit)
}

pub fn compose_commits(
    repository: &Path,
    run_id: &str,
    base_commit: &str,
    commits: &[String],
    ref_name: &str,
) -> AppResult<String> {
    if let Some(existing) = optional_ref(repository, ref_name)? {
        return Ok(existing);
    }
    let mut current = base_commit.to_owned();
    for (index, commit) in commits.iter().enumerate() {
        if is_ancestor(repository, commit, &current)? {
            continue;
        }
        let output = Command::new("git")
            .arg("-C")
            .arg(repository)
            .args(["merge-tree", "--write-tree"])
            .arg(&current)
            .arg(commit)
            .output()?;
        let output = require_success(output, "candidate_integration_conflict")?;
        let stdout = text(&output)?;
        let tree = stdout.lines().next().unwrap_or_default().trim();
        if tree.is_empty() {
            return Err(AppError::new(
                "candidate_integration_failed",
                "git merge-tree returned no integrated tree",
            ));
        }
        current = commit_tree(
            repository,
            tree,
            &[&current, commit],
            &format!("Vizier run {run_id} composition step {}", index + 1),
        )?;
    }
    create_immutable_ref(repository, ref_name, &current)?;
    Ok(current)
}

pub fn publish_final_ref(repository: &Path, run_id: &str, commit: &str) -> AppResult<String> {
    validate_internal_id(run_id)?;
    let reference = format!("refs/vizier/runs/{run_id}/result");
    create_immutable_ref(repository, &reference, commit)?;
    Ok(reference)
}

pub fn ensure_clean(worktree: &Path) -> AppResult<()> {
    let output = git_output(
        worktree,
        ["status", "--porcelain=v1", "--untracked-files=all"],
    )?;
    if !output.stdout.is_empty() {
        return Err(AppError::new(
            "read_only_candidate_mutated",
            "a reviewer or gate mutated its disposable candidate checkout",
        ));
    }
    Ok(())
}

pub fn ensure_exact_clean(worktree: &Path, expected_commit: &str) -> AppResult<()> {
    let actual = resolve_commit(worktree, "HEAD")?;
    if actual != expected_commit {
        return Err(AppError::new(
            "read_only_candidate_moved",
            "a reviewer or gate changed the exact candidate HEAD",
        ));
    }
    ensure_clean(worktree)
}

#[must_use]
pub fn scopes_overlap(left: &[PathScope], right: &[PathScope]) -> bool {
    left.iter().any(|a| {
        right.iter().any(|b| {
            scope_contains(a, &b.path)
                || scope_contains(b, &a.path)
                || (a.path == "." && a.recursive)
                || (b.path == "." && b.recursive)
        })
    })
}

pub fn validate_scopes(scopes: &[PathScope]) -> AppResult<()> {
    if scopes.is_empty() {
        return Err(AppError::new(
            "packet_scope_empty",
            "every work packet needs at least one path scope",
        ));
    }
    for scope in scopes {
        validate_relative_path(&scope.path)?;
    }
    Ok(())
}

fn changed_paths(worktree: &Path) -> AppResult<BTreeSet<String>> {
    let tracked = git_output(worktree, ["diff", "--name-only", "-z", "HEAD", "--"])?;
    let untracked = git_output(
        worktree,
        ["ls-files", "--others", "--exclude-standard", "-z", "--"],
    )?;
    let mut paths = BTreeSet::new();
    for bytes in [tracked.stdout, untracked.stdout] {
        for path in bytes
            .split(|byte| *byte == 0)
            .filter(|path| !path.is_empty())
        {
            let path = std::str::from_utf8(path).map_err(|_| {
                AppError::new(
                    "git_path_not_utf8",
                    "changed paths must be valid UTF-8 for scope enforcement",
                )
            })?;
            paths.insert(path.to_owned());
        }
    }
    Ok(paths)
}

fn scope_contains(scope: &PathScope, path: &str) -> bool {
    if scope.path == "." && scope.recursive {
        return true;
    }
    path == scope.path
        || (scope.recursive
            && path
                .strip_prefix(&scope.path)
                .is_some_and(|suffix| suffix.starts_with('/')))
}

fn validate_relative_path(value: &str) -> AppResult<()> {
    let path = Path::new(value);
    if value.is_empty() || path.is_absolute() || value.contains('\0') {
        return Err(AppError::new(
            "packet_scope_invalid",
            format!("invalid relative packet path scope {value:?}"),
        ));
    }
    if value == "." {
        return Ok(());
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(AppError::new(
            "packet_scope_invalid",
            format!("packet path scope {value:?} escapes the repository"),
        ));
    }
    let canonical = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/");
    if canonical != value {
        return Err(AppError::new(
            "packet_scope_invalid",
            format!("packet path scope {value:?} is not in canonical repository-relative form"),
        ));
    }
    Ok(())
}

fn validate_revision(revision: &str) -> AppResult<()> {
    if revision.is_empty() || revision.starts_with('-') || revision.contains('\0') {
        return Err(AppError::new(
            "source_revision_invalid",
            "source revision is empty or option-like",
        ));
    }
    Ok(())
}

fn validate_internal_id(value: &str) -> AppResult<()> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(AppError::new(
            "internal_id_invalid",
            format!("unsafe internal identity {value:?}"),
        ));
    }
    Ok(())
}

fn optional_ref(repository: &Path, reference: &str) -> AppResult<Option<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(["rev-parse", "--verify"])
        .arg(format!("{reference}^{{commit}}"))
        .output()?;
    if output.status.success() {
        Ok(Some(text(&output)?.trim().to_owned()))
    } else {
        Ok(None)
    }
}

fn create_immutable_ref(repository: &Path, reference: &str, commit: &str) -> AppResult<()> {
    if let Some(existing) = optional_ref(repository, reference)? {
        if existing == commit {
            return Ok(());
        }
        return Err(AppError::new(
            "immutable_ref_conflict",
            format!("immutable candidate ref {reference} already names another commit"),
        ));
    }
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(["update-ref", reference, commit, ZERO_OID])
        .output()?;
    require_success(output, "immutable_ref_create_failed")?;
    Ok(())
}

fn is_ancestor(repository: &Path, ancestor: &str, descendant: &str) -> AppResult<bool> {
    let status = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(["merge-base", "--is-ancestor", ancestor, descendant])
        .status()?;
    match status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(AppError::new(
            "git_ancestry_failed",
            "git merge-base could not compare candidates",
        )),
    }
}

fn commit_tree(
    repository: &Path,
    tree: &str,
    parents: &[&str],
    message: &str,
) -> AppResult<String> {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(repository)
        .args(["commit-tree", tree]);
    for parent in parents {
        command.args(["-p", parent]);
    }
    command
        .args(["-m", message])
        .env("GIT_AUTHOR_NAME", "Vizier")
        .env("GIT_AUTHOR_EMAIL", "vizier@localhost")
        .env("GIT_COMMITTER_NAME", "Vizier")
        .env("GIT_COMMITTER_EMAIL", "vizier@localhost")
        .stdin(Stdio::null());
    let output = command.output()?;
    Ok(text(&require_success(output, "git_commit_tree_failed")?)?
        .trim()
        .to_owned())
}

fn git_with_index<I, S>(worktree: &Path, index: &Path, arguments: I) -> AppResult<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Ok(Command::new("git")
        .arg("-C")
        .arg(worktree)
        .args(arguments)
        .env("GIT_INDEX_FILE", index)
        .output()?)
}

fn git_output<I, S>(repository: &Path, arguments: I) -> AppResult<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(arguments)
        .output()?;
    require_success(output, "git_command_failed")
}

fn require_success(output: Output, code: &'static str) -> AppResult<Output> {
    if output.status.success() {
        return Ok(output);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    Err(AppError::new(
        code,
        format!("{}{}", stderr.trim(), stdout.trim()),
    ))
}

fn text(output: &Output) -> AppResult<&str> {
    std::str::from_utf8(&output.stdout)
        .map_err(|_| AppError::new("git_output_invalid_utf8", "Git output was not valid UTF-8"))
}

fn private_directory(path: &Path) -> AppResult<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Command;

    use super::{
        ensure_exact_clean, prepare_worktree, resolve_commit, scopes_overlap, snapshot_worktree,
        validate_scopes,
    };
    use crate::model::PathScope;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn git(path: &std::path::Path, arguments: &[&str]) -> TestResult {
        let status = Command::new("git")
            .arg("-C")
            .arg(path)
            .args(arguments)
            .status()?;
        if !status.success() {
            return Err(std::io::Error::other(format!(
                "git command {arguments:?} exited {status}"
            ))
            .into());
        }
        Ok(())
    }

    #[test]
    fn snapshots_only_scoped_changes_without_moving_head() -> TestResult {
        let directory = tempfile::tempdir()?;
        let repository = directory.path().join("repo");
        fs::create_dir(&repository)?;
        git(&repository, &["init", "-q"])?;
        git(&repository, &["config", "user.name", "Test"])?;
        git(&repository, &["config", "user.email", "test@example.com"])?;
        fs::write(repository.join("allowed.txt"), "before\n")?;
        git(&repository, &["add", "."])?;
        git(&repository, &["commit", "-qm", "initial"])?;
        let base = resolve_commit(&repository, "HEAD")?;
        let state = directory.path().join("state");
        let worktree = prepare_worktree(&repository, &state, "run-test", "attempt-test", &base)?;
        fs::write(worktree.join("allowed.txt"), "after\n")?;
        let candidate = snapshot_worktree(
            &repository,
            &state,
            &worktree,
            &base,
            &[PathScope {
                path: "allowed.txt".to_owned(),
                recursive: false,
            }],
            "refs/vizier/runs/run-test/packets/p1/round/0",
            "test candidate",
        )?;
        assert_ne!(candidate, base);
        assert_eq!(resolve_commit(&worktree, "HEAD")?, base);
        let repeated = snapshot_worktree(
            &repository,
            &state,
            &worktree,
            &base,
            &[PathScope {
                path: "allowed.txt".to_owned(),
                recursive: false,
            }],
            "refs/vizier/runs/run-test/packets/p1/round/0",
            "test candidate",
        )?;
        assert_eq!(repeated, candidate);
        Ok(())
    }

    #[test]
    fn path_scopes_are_canonical_before_overlap_checks() {
        for invalid in ["a/", "a//b", "a/./b", "../a", "/a"] {
            assert!(
                validate_scopes(&[PathScope {
                    path: invalid.to_owned(),
                    recursive: true
                }])
                .is_err(),
                "{invalid} must be rejected"
            );
        }
        let parent = [PathScope {
            path: "a".to_owned(),
            recursive: true,
        }];
        let child = [PathScope {
            path: "a/b".to_owned(),
            recursive: false,
        }];
        assert!(scopes_overlap(&parent, &child));
    }

    #[test]
    fn clean_but_moved_review_head_is_rejected() -> TestResult {
        let directory = tempfile::tempdir()?;
        let repository = directory.path().join("repo");
        fs::create_dir(&repository)?;
        git(&repository, &["init", "-q"])?;
        git(&repository, &["config", "user.name", "Test"])?;
        git(&repository, &["config", "user.email", "test@example.com"])?;
        fs::write(repository.join("file.txt"), "before\n")?;
        git(&repository, &["add", "."])?;
        git(&repository, &["commit", "-qm", "initial"])?;
        let base = resolve_commit(&repository, "HEAD")?;
        let worktree = prepare_worktree(
            &repository,
            &directory.path().join("state"),
            "run-test",
            "review-test",
            &base,
        )?;
        fs::write(worktree.join("file.txt"), "after\n")?;
        git(&worktree, &["add", "."])?;
        git(&worktree, &["commit", "-qm", "unauthorized review commit"])?;
        assert!(ensure_exact_clean(&worktree, &base).is_err());
        Ok(())
    }
}
