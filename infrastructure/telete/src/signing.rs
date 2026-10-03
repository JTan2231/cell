use crate::paths::{self, Paths};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs;
use std::io::Read;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SigningPolicy {
    pub schema: u32,
    pub macos: MacSigning,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MacSigning {
    pub profile: String,
    pub certificate_sha1: String,
    pub keychain: PathBuf,
    pub identifier_namespace: String,
}

pub(crate) fn home() -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is unavailable")?;
    ensure!(home.is_absolute(), "HOME must be absolute");
    Ok(home)
}

fn uid() -> Result<u32> {
    let output = Command::new("/usr/bin/id").arg("-u").output()?;
    ensure!(output.status.success(), "cannot identify the current user");
    Ok(String::from_utf8(output.stdout)?.trim().parse()?)
}

fn name(value: &str) -> bool {
    value.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

impl SigningPolicy {
    pub(crate) fn normalize(mut self) -> Result<Self> {
        ensure!(
            self.schema == 1 && self.macos.profile == "local",
            "unsupported signing policy"
        );
        ensure!(
            self.macos.certificate_sha1.len() == 40
                && self
                    .macos
                    .certificate_sha1
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit()),
            "certificate fingerprint must contain 40 hexadecimal digits"
        );
        self.macos.certificate_sha1.make_ascii_lowercase();
        let namespace = &self.macos.identifier_namespace;
        ensure!(
            namespace.contains('.') && namespace.split('.').all(name),
            "invalid permanent signing namespace"
        );
        ensure!(
            self.macos.keychain.is_absolute()
                && !self
                    .macos
                    .keychain
                    .to_string_lossy()
                    .contains(['\0', '\n', '\r']),
            "invalid Keychain path"
        );
        Ok(self)
    }
}

fn read(path: &Path) -> Result<SigningPolicy> {
    let info = fs::symlink_metadata(path)
        .with_context(|| format!("signing selection unavailable: {}", path.display()))?;
    ensure!(
        info.is_file()
            && info.nlink() == 1
            && info.uid() == uid()?
            && info.mode() & 0o777 == 0o600
            && info.len() <= 65536,
        "signing selection must be an owned private regular file"
    );
    serde_json::from_slice::<SigningPolicy>(&fs::read(path)?)?.normalize()
}

pub(crate) fn current() -> Result<SigningPolicy> {
    read(&home()?.join("Library/Application Support/Cell/signing.json"))
}

pub(crate) fn selected(paths: &Paths) -> Result<SigningPolicy> {
    let path = paths.root.join("signing.json");
    match fs::symlink_metadata(&path) {
        Ok(_) => read(&path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => current(),
        Err(error) => Err(error.into()),
    }
}

// These commands contain only public certificate selection. Never print a key
// export, password, or an entire command line in signing diagnostics.
fn tool(program: &str, args: &[String]) -> Result<String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("signing tool unavailable")?;
    let mut stdout = child.stdout.take().context("signing stdout unavailable")?;
    let mut stderr = child.stderr.take().context("signing stderr unavailable")?;
    let output = std::thread::spawn(move || -> std::io::Result<String> {
        let mut text = String::new();
        stdout.read_to_string(&mut text)?;
        Ok(text)
    });
    let diagnostic = std::thread::spawn(move || -> std::io::Result<String> {
        let mut text = String::new();
        stderr.read_to_string(&mut text)?;
        Ok(text)
    });
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() >= Duration::from_secs(30) {
            child.kill()?;
            child.wait()?;
            bail!("signing key access timed out; unlock and authorize the selected Keychain");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let output = output
        .join()
        .map_err(|_| anyhow::anyhow!("signing stdout reader failed"))??;
    let diagnostic = diagnostic
        .join()
        .map_err(|_| anyhow::anyhow!("signing stderr reader failed"))??;
    ensure!(
        status.success(),
        "signing tool failed: {}",
        diagnostic
            .chars()
            .rev()
            .take(1500)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>()
    );
    Ok(output)
}

pub(crate) fn preflight(policy: &SigningPolicy) -> Result<()> {
    ensure!(cfg!(target_os = "macos"), "native signing requires macOS");
    let policy = policy.clone().normalize()?;
    let keychain = &policy.macos.keychain;
    let info = fs::symlink_metadata(keychain).context("selected Keychain is unavailable")?;
    ensure!(
        info.is_file() && info.uid() == uid()?,
        "Keychain must be an owned regular file"
    );
    let output = tool(
        "/usr/bin/security",
        &[
            "find-identity".into(),
            "-v".into(),
            "-p".into(),
            "codesigning".into(),
            keychain.display().to_string(),
        ],
    )?;
    ensure!(
        output
            .split(|c: char| !c.is_ascii_hexdigit())
            .any(|value| value.eq_ignore_ascii_case(&policy.macos.certificate_sha1)),
        "selected certificate or private key is missing, expired, or unavailable"
    );
    tool(
        "/usr/bin/codesign",
        &[
            "--dryrun".into(),
            "--detached".into(),
            "/dev/null".into(),
            "--force".into(),
            "--sign".into(),
            policy.macos.certificate_sha1.clone(),
            "--keychain".into(),
            keychain.display().to_string(),
            "--identifier".into(),
            format!("{}.preflight", policy.macos.identifier_namespace),
            "--timestamp=none".into(),
            "/usr/bin/true".into(),
        ],
    )?;
    Ok(())
}

pub(crate) fn identifier(
    policy: &SigningPolicy,
    product: &str,
    executable: &str,
) -> Result<String> {
    ensure!(
        name(product) && name(executable),
        "invalid permanent executable identity"
    );
    Ok(format!(
        "{}.{}.{}",
        policy.macos.identifier_namespace, product, executable
    ))
}

pub(crate) fn sign(
    path: &Path,
    policy: &SigningPolicy,
    product: &str,
    executable: &str,
) -> Result<()> {
    let identity = identifier(policy, product, executable)?;
    let requirement = format!(
        "=designated => identifier \"{identity}\" and certificate leaf = H\"{}\"",
        policy.macos.certificate_sha1
    );
    tool(
        "/usr/bin/codesign",
        &[
            "--force".into(),
            "--sign".into(),
            policy.macos.certificate_sha1.clone(),
            "--keychain".into(),
            policy.macos.keychain.display().to_string(),
            "--identifier".into(),
            identity,
            "--requirements".into(),
            requirement,
            "--timestamp=none".into(),
            path.display().to_string(),
        ],
    )?;
    Ok(())
}

pub(crate) fn status(paths: &Paths) -> Result<Value> {
    let policy = selected(paths)?;
    preflight(&policy)?;
    Ok(
        json!({"policy":policy,"ready":true,"selection":if paths.root.join("signing.json").exists(){"telete"}else{"host"}}),
    )
}

pub(crate) fn configure(
    paths: &Paths,
    fingerprint: &str,
    keychain: &Path,
    namespace: &str,
) -> Result<SigningPolicy> {
    let policy = SigningPolicy {
        schema: 1,
        macos: MacSigning {
            profile: "local".into(),
            certificate_sha1: fingerprint.into(),
            keychain: keychain.into(),
            identifier_namespace: namespace.into(),
        },
    }
    .normalize()?;
    let _admission = paths::lock(&paths.root.join("admission.lock"), false)?;
    let _worker = paths::lock(&paths.root.join("worker.lock"), false)?;
    let _deployment = paths::lock(&paths.root.join("deployments/deployment.lock"), false)?;
    crate::manager::require_quiescent(paths)?;
    ensure!(
        !paths.root.join("deployments/active.json").exists(),
        "settle interrupted Telete deployment before changing signing selection"
    );
    preflight(&policy)?;
    let path = paths.root.join("signing.json");
    if path.try_exists()? {
        read(&path)?;
    }
    paths::atomic_json(&path, &policy)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(policy)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    fn policy() -> SigningPolicy {
        SigningPolicy {
            schema: 1,
            macos: MacSigning {
                profile: "local".into(),
                certificate_sha1: "AA".repeat(20),
                keychain: "/private/login.keychain-db".into(),
                identifier_namespace: "local.cell".into(),
            },
        }
    }
    #[test]
    fn policy_normalizes_exact_identity_without_changing_namespace() {
        let policy = policy().normalize().unwrap();
        assert_eq!(policy.macos.certificate_sha1, "aa".repeat(20));
        assert_eq!(
            identifier(&policy, "krisis", "krisis-install").unwrap(),
            "local.cell.krisis.krisis-install"
        );
    }
    #[test]
    fn configuration_rejects_unsupported_profiles_and_nonliteral_identities() {
        let mut invalid = policy();
        invalid.macos.profile = "ad-hoc".into();
        assert!(invalid.normalize().is_err());
        let mut invalid = policy();
        invalid.macos.identifier_namespace = "local.cell..x".into();
        assert!(invalid.normalize().is_err());
        let mut invalid = policy();
        invalid.macos.certificate_sha1 = "Cell Local Signing".into();
        assert!(invalid.normalize().is_err());
        let mut invalid = policy();
        invalid.macos.keychain = "relative".into();
        assert!(invalid.normalize().is_err());
    }
}
