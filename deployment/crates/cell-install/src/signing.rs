//! The configured macOS signer is independent of product release selection.

use crate::artifact::{current_uid, regular, valid_name};
use crate::{Error, FileEntry, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::File;
use std::io::Read;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Configuration {
    schema: u32,
    macos: Policy,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    profile: String,
    certificate_sha1: String,
    keychain: PathBuf,
    identifier_namespace: String,
}

impl Policy {
    fn read(home: &Path, uid: u32) -> Result<Self> {
        if uid == 0 || home != operator_home(uid)? {
            return Err(Error::new(
                "Cell signing policy must use the selected operator's system home",
            ));
        }
        let path = home.join("Library/Application Support/Cell/signing.json");
        let metadata = regular(&path).map_err(|_| {
            Error::new("Cell signing configuration is missing or is not a regular private file")
        })?;
        if metadata.uid() != uid || metadata.permissions().mode() & 0o7777 != 0o600 {
            return Err(Error::new(
                "Cell signing configuration must be owned by the selected operator and mode 0600",
            ));
        }
        let mut bytes = Vec::new();
        File::open(path)?.take(65537).read_to_end(&mut bytes)?;
        if bytes.len() > 65536 {
            return Err(Error::new(
                "Cell signing configuration exceeds its size limit",
            ));
        }
        Self::decode(&bytes)
    }

    fn decode(bytes: &[u8]) -> Result<Self> {
        let configuration: Configuration = serde_json::from_slice(bytes)
            .map_err(|_| Error::new("invalid Cell signing configuration"))?;
        let mut policy = configuration.macos;
        if configuration.schema != 1
            || policy.profile != "local"
            || policy.certificate_sha1.len() != 40
            || !policy
                .certificate_sha1
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || !policy.keychain.is_absolute()
            || policy
                .keychain
                .to_str()
                .is_none_or(|path| path.contains(['\0', '\n', '\r']))
            || policy.identifier_namespace.split('.').count() < 2
            || !policy.identifier_namespace.split('.').all(valid_component)
        {
            return Err(Error::new(
                "unsupported or invalid Cell signing configuration",
            ));
        }
        policy.certificate_sha1.make_ascii_lowercase();
        Ok(policy)
    }

    fn identifier(&self, product: &str, artifact: &str) -> Result<String> {
        if !valid_component(product) || !valid_component(artifact) {
            return Err(Error::new(
                "invalid Cell signing product or artifact identity",
            ));
        }
        Ok(format!(
            "{}.{product}.{artifact}",
            self.identifier_namespace
        ))
    }

    fn arguments(&self, product: &str, artifact: &str, path: &Path) -> Result<Vec<OsString>> {
        let identifier = self.identifier(product, artifact)?;
        Ok(vec![
            "--verify".into(),
            "--strict".into(),
            "--all-architectures".into(),
            "--test-requirement".into(),
            format!(
                "=identifier \"{identifier}\" and certificate leaf = H\"{}\"",
                self.certificate_sha1
            )
            .into(),
            path.as_os_str().to_owned(),
        ])
    }

    fn verify(&self, product: &str, artifact: &str, path: &Path) -> Result<()> {
        self.verify_with(product, artifact, path, |arguments| {
            Ok(crate::command::run(
                Path::new("/usr/bin/codesign"),
                arguments,
                &BTreeMap::new(),
                Duration::from_secs(60),
            )?
            .status
            .success())
        })
    }

    fn verify_with(
        &self,
        product: &str,
        artifact: &str,
        path: &Path,
        run: impl FnOnce(&[OsString]) -> Result<bool>,
    ) -> Result<()> {
        if !run(&self.arguments(product, artifact, path)?)? {
            return Err(Error::new(format!(
                "Cell native artifact {product}/{artifact} does not have the configured signing certificate and identifier"
            )));
        }
        Ok(())
    }
}

fn valid_component(value: &str) -> bool {
    value
        .bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_lowercase())
        && valid_name(value)
}

fn operator_home(uid: u32) -> Result<PathBuf> {
    let output = crate::command::run(
        Path::new("/usr/bin/id"),
        &["-P".into(), uid.to_string().into()],
        &BTreeMap::new(),
        Duration::from_secs(10),
    )?;
    if !output.status.success() {
        return Err(Error::new("cannot resolve Cell signing operator home"));
    }
    passwd_home(&output.stdout, uid)
}

fn passwd_home(bytes: &[u8], uid: u32) -> Result<PathBuf> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| Error::new("invalid operator account record"))?;
    let fields: Vec<_> = text.trim_end().split(':').collect();
    if fields.len() != 10
        || fields[2].parse::<u32>().ok() != Some(uid)
        || !Path::new(fields[8]).is_absolute()
    {
        return Err(Error::new("invalid operator home in account record"));
    }
    Ok(PathBuf::from(fields[8]))
}

fn native(path: &Path) -> Result<bool> {
    regular(path)?;
    let mut magic = [0_u8; 4];
    let count = File::open(path)?.read(&mut magic)?;
    Ok(count == 4
        && matches!(
            magic,
            [0xfe, 0xed, 0xfa, 0xce | 0xcf]
                | [0xce | 0xcf, 0xfa, 0xed, 0xfe]
                | [0xca, 0xfe, 0xba, 0xbe | 0xbf]
                | [0xbe | 0xbf, 0xba, 0xfe, 0xca]
        ))
}

#[derive(Default)]
pub(crate) struct Verifier {
    policy: Option<Policy>,
}

fn enabled() -> bool {
    cfg!(target_os = "macos")
}

impl Verifier {
    pub(crate) fn identifier(
        &mut self,
        product: &str,
        artifact: &str,
        path: &Path,
    ) -> Result<Option<String>> {
        if !enabled() || !native(path)? {
            return Ok(None);
        }
        if self.policy.is_none() {
            let uid = current_uid()?;
            self.policy = Some(Policy::read(&operator_home(uid)?, uid)?);
        }
        self.policy
            .as_ref()
            .ok_or_else(|| Error::new("Cell signing policy is unavailable"))?
            .identifier(product, artifact)
            .map(Some)
    }

    pub(crate) fn verify(&mut self, product: &str, artifact: &str, path: &Path) -> Result<()> {
        if !enabled() || !native(path)? {
            return Ok(());
        }
        if self.policy.is_none() {
            let uid = current_uid()?;
            self.policy = Some(Policy::read(&operator_home(uid)?, uid)?);
        }
        self.policy
            .as_ref()
            .ok_or_else(|| Error::new("Cell signing policy is unavailable"))?
            .verify(product, artifact, path)
    }
}

/// Verify a supplied native executable before launching it. Scripts and non-macOS
/// artifacts retain their existing execution contract. This never signs a file.
///
/// # Errors
/// Rejects malformed native inputs, missing signing policy, invalid signatures,
/// or a certificate/identifier different from the explicitly configured policy.
pub fn verify_native(product: &str, artifact: &str, path: &Path) -> Result<()> {
    Verifier::default().verify(product, artifact, path)
}

/// Verify a native migration input using the explicitly selected non-root
/// operator's fixed signing policy, including when the caller is privileged.
///
/// # Errors
/// Rejects another user's home, unsafe configuration, or a signature that does
/// not match the selected operator's certificate and permanent code identity.
pub fn verify_native_for_user(
    product: &str,
    artifact: &str,
    path: &Path,
    home: &Path,
    uid: u32,
) -> Result<()> {
    if !enabled() || !native(path)? {
        return Ok(());
    }
    Policy::read(home, uid)?.verify(product, artifact, path)
}

pub(crate) fn artifact_key(product: &str, relative: &str, installer_alias: bool) -> Result<String> {
    if installer_alias || relative == "package/install" {
        return Ok(format!("{product}-install"));
    }
    let key = Path::new(relative)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| valid_name(name))
        .ok_or_else(|| Error::new("native artifact has no stable Cell signing key"))?;
    Ok(key.to_owned())
}

pub(crate) fn verify_source(
    verifier: &mut Verifier,
    product: &str,
    artifact: &Result<String>,
    path: &Path,
) -> Result<()> {
    if !enabled() || !native(path)? {
        return Ok(());
    }
    let artifact = artifact
        .as_ref()
        .map_err(|error| Error::new(&error.message))?;
    verifier.verify(product, artifact, path)
}

pub(crate) fn verify_files(
    product: &str,
    root: &Path,
    files: &BTreeMap<String, FileEntry>,
) -> Result<()> {
    let mut verifier = Verifier::default();
    let installer = files.get("package/install");
    for (relative, file) in files {
        let path = root.join(relative);
        if !enabled() || !native(&path)? {
            continue;
        }
        let key = recorded_key(product, relative, file, installer)?;
        if let Some(recorded) = &file.code_identifier
            && verifier.identifier(product, &key, &path)?.as_ref() != Some(recorded)
        {
            return Err(Error::new(
                "retained native code identifier differs from the configured policy",
            ));
        }
        verifier.verify(product, &key, &path)?;
    }
    Ok(())
}

pub(crate) fn recorded_key(
    product: &str,
    relative: &str,
    file: &FileEntry,
    installer: Option<&FileEntry>,
) -> Result<String> {
    if let Some(identifier) = &file.code_identifier {
        let key = identifier.rsplit('.').next().unwrap_or_default();
        if !valid_component(key)
            || !identifier.ends_with(&format!(".{product}.{key}"))
            || !identifier.split('.').all(valid_component)
        {
            return Err(Error::new("invalid retained native code identifier"));
        }
        return Ok(key.to_owned());
    }
    let alias = installer
        .is_some_and(|installer| !installer.sha256.is_empty() && installer.sha256 == file.sha256);
    artifact_key(product, relative, alias)
}

pub(crate) fn verify_recorded_file(
    product: &str,
    artifact: &Result<String>,
    path: &Path,
    recorded: &FileEntry,
) -> Result<()> {
    if regular(path)?.mode() & 0o7777 != recorded.mode {
        return Err(Error::new("recorded recovery artifact mode changed"));
    }
    if !enabled() || !native(path)? {
        return Ok(());
    }
    let artifact = artifact
        .as_ref()
        .map_err(|error| Error::new(&error.message))?;
    let identifier = if let Some(identifier) = &recorded.code_identifier {
        Some(identifier.clone())
    } else {
        let output = crate::command::run(
            Path::new("/usr/bin/codesign"),
            &[
                "--display".into(),
                "--verbose=4".into(),
                path.as_os_str().to_owned(),
            ],
            &BTreeMap::new(),
            Duration::from_secs(60),
        )?;
        if !output.status.success() {
            if String::from_utf8_lossy(&output.stderr).contains("code object is not signed at all")
            {
                return Ok(());
            }
            return Err(Error::new("cannot read retained native code identity"));
        }
        let text = String::from_utf8_lossy(&output.stderr);
        if text.lines().any(|line| line == "Signature=adhoc") {
            None
        } else {
            Some(
                text.lines()
                    .find_map(|line| line.strip_prefix("Identifier="))
                    .filter(|id| {
                        id.ends_with(&format!(".{product}.{artifact}"))
                            && id.split('.').all(valid_component)
                    })
                    .ok_or_else(|| {
                        Error::new(
                            "retained native code identity differs from the product artifact",
                        )
                    })?
                    .to_owned(),
            )
        }
    };
    let mut arguments: Vec<OsString> = vec![
        "--verify".into(),
        "--strict".into(),
        "--all-architectures".into(),
    ];
    if let Some(identifier) = identifier {
        arguments.extend([
            "--test-requirement".into(),
            format!("=identifier \"{identifier}\"").into(),
        ]);
    }
    arguments.push(path.as_os_str().to_owned());
    let output = crate::command::run(
        Path::new("/usr/bin/codesign"),
        &arguments,
        &BTreeMap::new(),
        Duration::from_secs(60),
    )?;
    if !output.status.success() {
        return Err(Error::new("retained native recovery signature is invalid"));
    }
    Ok(())
}

pub(crate) fn verify_recorded_files(
    product: &str,
    root: &Path,
    files: &BTreeMap<String, FileEntry>,
) -> Result<()> {
    let (mut actual, _) = crate::artifact::inventory(root)?;
    for metadata in ["manifest.json", "manifest.txt"] {
        if !files.contains_key(metadata) {
            actual.remove(metadata);
        }
    }
    if !crate::artifact::inventory_matches(&actual, files) {
        return Err(Error::new("recorded recovery inventory or modes changed"));
    }
    let installer = files.get("package/install");
    for (relative, recorded) in files {
        let key = recorded_key(product, relative, recorded, installer);
        let path = root.join(relative);
        if enabled() && native(&path)? {
            verify_recorded_file(product, &key, &path, recorded)?;
        } else if regular(&path)?.mode() & 0o7777 != recorded.mode {
            return Err(Error::new("recorded recovery artifact mode changed"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn configuration() -> serde_json::Value {
        json!({"schema":1,"macos":{"profile":"local","certificate_sha1":format!("{}ABCD", "ABCDEF".repeat(6)),"keychain":"/private/test/cell.keychain-db","identifier_namespace":"local.cell"}})
    }

    #[test]
    fn policy_pins_the_leaf_certificate_and_all_architectures() -> Result<()> {
        let policy = Policy::decode(&serde_json::to_vec(&configuration())?)?;
        let arguments = policy.arguments(
            "fixture",
            "fixture-install",
            Path::new("/private/stage/install"),
        )?;
        assert!(arguments.contains(&OsString::from("--all-architectures")));
        assert!(arguments.contains(&OsString::from(format!(
            "=identifier \"local.cell.fixture.fixture-install\" and certificate leaf = H\"{}abcd\"",
            "abcdef".repeat(6)
        ))));
        assert_eq!(
            arguments.last(),
            Some(&OsString::from("/private/stage/install"))
        );
        assert!(policy.identifier("fixture", "bad\"identity").is_err());
        Ok(())
    }

    #[test]
    fn malformed_policy_has_no_default_or_fallback() -> Result<()> {
        for (key, value) in [
            ("profile", json!("adhoc")),
            ("certificate_sha1", json!("auto")),
            ("keychain", json!("relative/keychain")),
            ("keychain", json!("/private/test/line\nbreak")),
            ("keychain", json!("/private/test/invalid\u{0}path")),
            ("identifier_namespace", json!("local.cell\" or true")),
            ("identifier_namespace", json!("cell")),
            ("identifier_namespace", json!("local.9cell")),
        ] {
            let mut configuration = configuration();
            configuration["macos"][key] = value;
            assert!(Policy::decode(&serde_json::to_vec(&configuration)?).is_err());
        }
        let mut configuration = configuration();
        configuration["schema"] = json!(2);
        assert!(Policy::decode(&serde_json::to_vec(&configuration)?).is_err());
        configuration = json!({"schema":1});
        assert!(Policy::decode(&serde_json::to_vec(&configuration)?).is_err());
        Ok(())
    }

    #[test]
    fn failed_signature_or_tool_result_is_not_accepted() -> Result<()> {
        let policy = Policy::decode(&serde_json::to_vec(&configuration())?)?;
        let path = Path::new("/private/stage/fixture");
        let error = policy
            .verify_with("fixture", "fixture", path, |_| Ok(false))
            .err()
            .ok_or_else(|| Error::new("wrong signer unexpectedly accepted"))?;
        assert!(
            error
                .message
                .contains("configured signing certificate and identifier")
        );
        assert!(
            policy
                .verify_with("fixture", "fixture", path, |_| Err(Error::new(
                    "tool unavailable"
                )))
                .is_err()
        );
        assert!(
            policy
                .verify_with("fixture", "fixture", path, |_| Ok(true))
                .is_ok()
        );
        Ok(())
    }

    #[test]
    fn installer_aliases_keep_source_identity() -> Result<()> {
        assert_eq!(
            artifact_key("fixture", "package/install", false)?,
            "fixture-install"
        );
        assert_eq!(
            artifact_key("fixture", "bin/frontend", true)?,
            "fixture-install"
        );
        assert_eq!(artifact_key("fixture", "libexec/worker", false)?, "worker");
        let recorded = FileEntry {
            sha256: String::new(),
            mode: 0o555,
            code_identifier: Some("previous.namespace.fixture.fixture-install".to_owned()),
        };
        assert_eq!(
            recorded_key("fixture", "bin/frontend", &recorded, None)?,
            "fixture-install"
        );
        let ordinary = FileEntry {
            code_identifier: None,
            ..recorded
        };
        assert_eq!(
            recorded_key("fixture", "bin/frontend", &ordinary, Some(&ordinary))?,
            "frontend"
        );
        Ok(())
    }

    #[test]
    fn account_home_requires_the_exact_system_uid_and_absolute_home() -> Result<()> {
        let record = b"fixture:*:501:20::0:0:Fixture:/Users/fixture:/bin/zsh\n";
        assert_eq!(passwd_home(record, 501)?, PathBuf::from("/Users/fixture"));
        assert!(passwd_home(record, 502).is_err());
        assert!(passwd_home(b"fixture:*:501:20::0:0:Fixture:relative:/bin/zsh", 501).is_err());
        assert!(passwd_home(b"malformed", 501).is_err());
        Ok(())
    }
}
