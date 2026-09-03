use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, Read as _};
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Component, Path, PathBuf};

use sha2::{Digest as _, Sha256};

use crate::error::{Context as _, Error, Result};
use crate::model::{LaunchImage, Manifest, Schedule};
use crate::paths::{Layout, current_uid};

const MAX_DURATION_SECONDS: u64 = 31_536_000;

pub(crate) fn load(path: &Path, layout: &Layout) -> Result<(Manifest, String)> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .context("manifest_unreadable", format!("open {}", path.display()))?;
    let metadata = file
        .metadata()
        .context("manifest_unreadable", format!("inspect {}", path.display()))?;
    if !metadata.is_file() {
        return Err(Error::new(
            "manifest_unreadable",
            format!("{} must be a regular non-symlink file", path.display()),
        ));
    }
    if metadata.len() > 1024 * 1024 {
        return Err(Error::new(
            "manifest_too_large",
            "a definition manifest may not exceed 1 MiB",
        ));
    }
    require_current_user_owner(&metadata, "manifest")?;
    if metadata.mode() & 0o022 != 0 {
        return Err(Error::new(
            "manifest_permissions_unsafe",
            "definition manifest must not be group- or world-writable",
        ));
    }
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .context("manifest_unreadable", format!("read {}", path.display()))?;
    if bytes.len() > 1024 * 1024 {
        return Err(Error::new(
            "manifest_too_large",
            "a definition manifest may not exceed 1 MiB",
        ));
    }
    let source = String::from_utf8(bytes)
        .context("manifest_invalid", "manifest TOML must be valid UTF-8")?;
    let manifest: Manifest =
        toml::from_str(&source).context("manifest_invalid", "parse manifest TOML")?;
    validate(&manifest, layout)?;
    let digest = definition_digest(&manifest)?;
    Ok((manifest, digest))
}

pub(crate) fn definition_digest(manifest: &Manifest) -> Result<String> {
    let canonical =
        serde_json::to_vec(manifest).context("manifest_invalid", "serialize canonical manifest")?;
    Ok(hex::encode(Sha256::digest(canonical)))
}

#[allow(clippy::too_many_lines)]
pub(crate) fn validate(manifest: &Manifest, layout: &Layout) -> Result<()> {
    if manifest.schema_version != 1 {
        return Err(Error::new(
            "manifest_version_unsupported",
            format!(
                "schema_version {} is unsupported; expected 1",
                manifest.schema_version
            ),
        ));
    }
    validate_key(&manifest.key)?;
    validate_digest("release_id", &manifest.release_id)?;
    if manifest
        .timeout_seconds
        .is_some_and(|seconds| seconds == 0 || seconds > MAX_DURATION_SECONDS)
    {
        return Err(Error::new(
            "manifest_invalid",
            "timeout_seconds must be between 1 and 31536000",
        ));
    }
    match manifest.schedule {
        Schedule::Interval { seconds, .. } if seconds == 0 || seconds > MAX_DURATION_SECONDS => {
            return Err(Error::new(
                "manifest_invalid",
                "an interval schedule must be between 1 and 31536000 seconds",
            ));
        }
        Schedule::LocalCalendar { hour, minute, .. } if hour > 23 || minute > 59 => {
            return Err(Error::new(
                "manifest_invalid",
                "a local-calendar schedule requires hour 0..23 and minute 0..59",
            ));
        }
        _ => {}
    }
    validate_arguments(&manifest.arguments)?;
    validate_environment(&manifest.environment)?;

    let release_root = exact_directory(Path::new(&manifest.release_root), "release_root")?;
    require_not_group_or_world_writable(&release_root, "release_root")?;
    if release_root.file_name().and_then(|name| name.to_str()) != Some(manifest.release_id.as_str())
    {
        return Err(Error::new(
            "release_identity_mismatch",
            "release_root must end with the declared release_id",
        ));
    }
    let cwd = exact_directory(Path::new(&manifest.cwd), "cwd")?;
    require_not_group_or_world_writable(&cwd, "cwd")?;

    match &manifest.launch {
        LaunchImage::Direct { program, sha256 } => {
            let program = exact_artifact(Path::new(program), "program", true)?;
            require_beneath(&program, &release_root, "program")?;
            require_native_image(&program, "program")?;
            validate_digest("program sha256", sha256)?;
            verify_hash(&program, sha256, "program")?;
        }
        LaunchImage::Interpreted {
            interpreter,
            interpreter_sha256,
            script,
            script_sha256,
        } => {
            let interpreter = exact_artifact(Path::new(interpreter), "interpreter", false)?;
            let script = exact_artifact(Path::new(script), "script", true)?;
            require_beneath(&script, &release_root, "script")?;
            if interpreter != Path::new("/bin/sh") {
                return Err(Error::new(
                    "interpreter_unsupported",
                    "definition schema one supports only the explicit /bin/sh interpreter profile",
                ));
            }
            require_root_owner(&interpreter, "system interpreter")?;
            require_native_image(&interpreter, "interpreter")?;
            validate_digest("interpreter sha256", interpreter_sha256)?;
            validate_digest("script sha256", script_sha256)?;
            verify_hash(&interpreter, interpreter_sha256, "interpreter")?;
            verify_hash(&script, script_sha256, "script")?;
        }
    }

    validate_output(Path::new(&manifest.output.stdout), "stdout")?;
    validate_output(Path::new(&manifest.output.stderr), "stderr")?;
    if manifest.output.stdout == manifest.output.stderr {
        return Err(Error::new(
            "manifest_invalid",
            "stdout and stderr paths must be distinct",
        ));
    }
    for (label, output) in [
        ("stdout", Path::new(&manifest.output.stdout)),
        ("stderr", Path::new(&manifest.output.stderr)),
    ] {
        if output.starts_with(&release_root)
            || output.starts_with(layout.state_root())
            || output.starts_with(layout.logs_root())
            || output.starts_with(layout.agents_root())
        {
            return Err(Error::new(
                "output_path_invalid",
                format!("{label} must not target product release bytes or Clockwork-owned state"),
            ));
        }
    }
    Ok(())
}

pub(crate) fn validate_key(key: &str) -> Result<()> {
    let mut components = key.split('/');
    let owner = components.next().unwrap_or_default();
    let name = components.next().unwrap_or_default();
    if components.next().is_some() || !valid_key_component(owner) || !valid_key_component(name) {
        return Err(Error::new(
            "key_invalid",
            "a key must be owner/name and each at-most-63-byte component must match [a-z][a-z0-9-]*",
        ));
    }
    Ok(())
}

pub(crate) fn validate_definition_digest(digest: &str) -> Result<()> {
    validate_digest("definition digest", digest)
}

fn valid_key_component(value: &str) -> bool {
    if value.len() > 63 {
        return false;
    }
    let mut characters = value.chars();
    matches!(characters.next(), Some('a'..='z'))
        && characters.all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
}

fn validate_digest(label: &str, value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(Error::new(
            "manifest_invalid",
            format!("{label} must be a 64-character lowercase SHA-256 digest"),
        ));
    }
    Ok(())
}

fn validate_arguments(arguments: &[String]) -> Result<()> {
    if arguments.iter().any(|argument| argument.contains('\0')) {
        return Err(Error::new(
            "manifest_invalid",
            "arguments must not contain NUL bytes",
        ));
    }
    if arguments.iter().any(|argument| argument == "-c") {
        return Err(Error::new(
            "manifest_invalid",
            "the shell command-string argument -c is unsupported",
        ));
    }
    Ok(())
}

fn validate_environment(environment: &BTreeMap<String, String>) -> Result<()> {
    for (name, value) in environment {
        let mut characters = name.chars();
        let valid = matches!(characters.next(), Some('A'..='Z' | 'a'..='z' | '_'))
            && characters.all(|character| character.is_ascii_alphanumeric() || character == '_');
        if !valid || name.contains('\0') || value.contains('\0') {
            return Err(Error::new(
                "manifest_invalid",
                format!("environment entry {name:?} is not a literal POSIX environment entry"),
            ));
        }
        if looks_secret(name) {
            return Err(Error::new(
                "secret_environment_rejected",
                format!("environment entry {name:?} looks secret-bearing"),
            ));
        }
    }
    Ok(())
}

fn looks_secret(name: &str) -> bool {
    name.to_ascii_uppercase().split('_').any(|component| {
        matches!(
            component,
            "SECRET" | "TOKEN" | "PASSWORD" | "PASSWD" | "CREDENTIAL" | "CREDENTIALS" | "KEY"
        )
    })
}

fn exact_directory(path: &Path, label: &str) -> Result<PathBuf> {
    require_absolute_normal(path, label)?;
    require_no_symlink_components(path, label)?;
    require_trusted_ancestors(path, label)?;
    let metadata = fs::metadata(path).context(
        "manifest_path_invalid",
        format!("inspect {label} {}", path.display()),
    )?;
    if !metadata.is_dir() {
        return Err(Error::new(
            "manifest_path_invalid",
            format!("{label} {} must be a directory", path.display()),
        ));
    }
    require_current_user_owner(&metadata, label)?;
    let canonical =
        fs::canonicalize(path).context("manifest_path_invalid", format!("canonicalize {label}"))?;
    if canonical != path {
        return Err(Error::new(
            "manifest_path_invalid",
            format!("{label} must already be canonical"),
        ));
    }
    Ok(canonical)
}

fn exact_artifact(path: &Path, label: &str, require_current_owner: bool) -> Result<PathBuf> {
    require_absolute_normal(path, label)?;
    require_no_symlink_components(path, label)?;
    require_trusted_ancestors(path, label)?;
    let metadata = fs::symlink_metadata(path).context(
        "artifact_invalid",
        format!("inspect {label} {}", path.display()),
    )?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.nlink() != 1 {
        return Err(Error::new(
            "artifact_invalid",
            format!(
                "{label} {} must be a regular, non-symlink, non-hard-linked file",
                path.display()
            ),
        ));
    }
    if require_current_owner {
        require_current_user_owner(&metadata, label)?;
        if metadata.permissions().mode() & 0o100 == 0 {
            return Err(Error::new(
                "artifact_invalid",
                format!("{label} {} must be executable by its owner", path.display()),
            ));
        }
    } else if metadata.permissions().mode() & 0o001 == 0 {
        return Err(Error::new(
            "artifact_invalid",
            format!(
                "{label} {} must be executable by the current user",
                path.display()
            ),
        ));
    }
    require_not_group_or_world_writable(path, label)?;
    let canonical =
        fs::canonicalize(path).context("artifact_invalid", format!("canonicalize {label}"))?;
    if canonical != path {
        return Err(Error::new(
            "artifact_invalid",
            format!("{label} must already be canonical"),
        ));
    }
    Ok(canonical)
}

fn require_absolute_normal(path: &Path, label: &str) -> Result<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(Error::new(
            "manifest_path_invalid",
            format!("{label} must be an absolute normalized path"),
        ));
    }
    Ok(())
}

fn require_no_symlink_components(path: &Path, label: &str) -> Result<()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current).context(
            "manifest_path_invalid",
            format!("inspect {label} component {}", current.display()),
        )?;
        if metadata.file_type().is_symlink() {
            return Err(Error::new(
                "manifest_path_invalid",
                format!("{label} must not traverse symlinks: {}", current.display()),
            ));
        }
    }
    Ok(())
}

fn require_trusted_ancestors(path: &Path, label: &str) -> Result<()> {
    let current_uid = current_uid()?;
    let mut current = PathBuf::new();
    for component in path
        .components()
        .take(path.components().count().saturating_sub(1))
    {
        current.push(component.as_os_str());
        let metadata = fs::metadata(&current).context(
            "manifest_path_invalid",
            format!("inspect {label} ancestor {}", current.display()),
        )?;
        if !metadata.is_dir()
            || (metadata.uid() != 0 && metadata.uid() != current_uid)
            || metadata.mode() & 0o022 != 0
        {
            return Err(Error::new(
                "manifest_path_unsafe",
                format!(
                    "{label} ancestor {} must be a root- or current-user-owned non-writable directory",
                    current.display()
                ),
            ));
        }
    }
    Ok(())
}

fn require_not_group_or_world_writable(path: &Path, label: &str) -> Result<()> {
    let metadata = fs::metadata(path).context(
        "manifest_path_invalid",
        format!("inspect permissions for {label}"),
    )?;
    if metadata.mode() & 0o022 != 0 {
        return Err(Error::new(
            "artifact_permissions_unsafe",
            format!("{label} {} is group- or world-writable", path.display()),
        ));
    }
    Ok(())
}

fn require_beneath(path: &Path, root: &Path, label: &str) -> Result<()> {
    if !path.starts_with(root) || path == root {
        return Err(Error::new(
            "artifact_outside_release",
            format!(
                "{label} {} must be beneath {}",
                path.display(),
                root.display()
            ),
        ));
    }
    Ok(())
}

fn require_native_image(path: &Path, label: &str) -> Result<()> {
    let mut file = File::open(path).context(
        "artifact_unreadable",
        format!("open {label} {}", path.display()),
    )?;
    let mut magic = [0_u8; 4];
    file.read_exact(&mut magic).context(
        "artifact_invalid",
        format!("read native header for {label}"),
    )?;
    let native = matches!(
        magic,
        [0xfe, 0xed, 0xfa, 0xce | 0xcf]
            | [0xce | 0xcf, 0xfa, 0xed, 0xfe]
            | [0xca, 0xfe, 0xba, 0xbe | 0xbf]
            | [0xbe | 0xbf, 0xba, 0xfe, 0xca]
    );
    if !native {
        return Err(Error::new(
            "artifact_not_native",
            format!(
                "{label} {} must begin with recognized Mach-O or fat-binary magic",
                path.display()
            ),
        ));
    }
    Ok(())
}

fn verify_hash(path: &Path, expected: &str, label: &str) -> Result<()> {
    let file = File::open(path).context(
        "artifact_unreadable",
        format!("open {label} {}", path.display()),
    )?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    loop {
        let read = reader
            .read(&mut buffer)
            .context("artifact_unreadable", format!("hash {label}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let actual = hex::encode(hasher.finalize());
    if actual != expected {
        return Err(Error::new(
            "artifact_hash_mismatch",
            format!(
                "{label} {} does not match its registered SHA-256",
                path.display()
            ),
        ));
    }
    Ok(())
}

fn validate_output(path: &Path, label: &str) -> Result<()> {
    require_absolute_normal(path, label)?;
    let parent = path.parent().ok_or_else(|| {
        Error::new(
            "output_path_invalid",
            format!("{label} must have a parent directory"),
        )
    })?;
    let parent = exact_directory(parent, &format!("{label} parent"))?;
    require_not_group_or_world_writable(&parent, &format!("{label} parent"))?;
    let parent_metadata = fs::metadata(&parent).context(
        "output_path_invalid",
        format!("inspect {label} parent permissions"),
    )?;
    if parent_metadata.mode() & 0o300 != 0o300 {
        return Err(Error::new(
            "output_permissions_unsafe",
            format!("{label} parent must be writable and searchable by its owner"),
        ));
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(Error::new(
                    "output_path_invalid",
                    format!(
                        "{label} {} must be a regular non-symlink file",
                        path.display()
                    ),
                ));
            }
            if metadata.nlink() != 1 {
                return Err(Error::new(
                    "output_path_invalid",
                    format!("{label} {} must not be hard-linked", path.display()),
                ));
            }
            require_current_user_owner(&metadata, label)?;
            if metadata.mode() & 0o077 != 0 || metadata.mode() & 0o200 == 0 {
                return Err(Error::new(
                    "output_permissions_unsafe",
                    format!(
                        "{label} {} must be private and owner-writable",
                        path.display()
                    ),
                ));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(Error::new(
                "output_path_invalid",
                format!("inspect {label} {}: {error}", path.display()),
            ));
        }
    }
    Ok(())
}

fn require_current_user_owner(metadata: &fs::Metadata, label: &str) -> Result<()> {
    let uid = current_uid()?;
    if metadata.uid() != uid {
        return Err(Error::new(
            "artifact_owner_invalid",
            format!("{label} must be owned by the current user"),
        ));
    }
    Ok(())
}

fn require_root_owner(path: &Path, label: &str) -> Result<()> {
    let metadata =
        fs::metadata(path).context("artifact_invalid", format!("inspect {label} owner"))?;
    if metadata.uid() != 0 {
        return Err(Error::new(
            "artifact_owner_invalid",
            format!("{label} outside the product release must be owned by root"),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{looks_secret, validate_key};

    #[test]
    fn key_grammar_is_closed() {
        assert!(validate_key("annals/inbox").is_ok());
        assert!(validate_key("Annals/inbox").is_err());
        assert!(validate_key("annals/in_box").is_err());
        assert!(validate_key("annals/inbox/more").is_err());
    }

    #[test]
    fn secret_like_environment_names_are_rejected() {
        assert!(looks_secret("RESEND_API_KEY"));
        assert!(looks_secret("ACCESS_TOKEN"));
        assert!(!looks_secret("DECISIONS_DATABASE"));
    }
}
