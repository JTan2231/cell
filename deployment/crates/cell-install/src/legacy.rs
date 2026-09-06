//! Exact predecessor release evidence shared by product-owned legacy readers.

use crate::artifact::{current_uid, hash_bytes, inventory, valid_hash};
use crate::transaction::{PublicEntry, ReleaseInfo};
use crate::{Error, Result, file_digest};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

pub struct LegacySpec {
    pub format: &'static str,
    pub manifest: &'static str,
    pub metadata: &'static [&'static str],
    pub proofs: &'static [LegacyProof],
    pub providers: &'static [LegacyProvider],
    pub hash_path_lines: bool,
}

pub struct LegacyProof {
    pub key: &'static str,
    pub paths: &'static [&'static str],
}

pub struct LegacyProvider {
    pub key: &'static str,
    pub provider: &'static str,
    pub path: &'static str,
    pub version_key: &'static str,
}

/// Decode a regular predecessor manifest without inferring any missing fields.
///
/// # Errors
/// Rejects malformed JSON/text, duplicate text keys, and nonscalar JSON fields.
pub fn manifest(path: &Path) -> Result<BTreeMap<String, String>> {
    file_digest(path)?;
    let bytes = fs::read(path)?;
    if bytes.len() > 1024 * 1024 {
        return Err(Error::new("legacy manifest exceeds one MiB"));
    }
    if path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
    {
        let value: Value = serde_json::from_slice(&bytes)?;
        value
            .as_object()
            .ok_or_else(|| Error::new("invalid legacy manifest"))?
            .iter()
            .map(|(key, value)| {
                let text = value
                    .as_str()
                    .map(str::to_owned)
                    .or_else(|| value.as_u64().map(|v| v.to_string()))
                    .or_else(|| value.as_bool().map(|v| v.to_string()))
                    .ok_or_else(|| Error::new("invalid legacy manifest value"))?;
                Ok((key.clone(), text))
            })
            .collect()
    } else {
        let text =
            String::from_utf8(bytes).map_err(|_| Error::new("legacy manifest is not UTF-8"))?;
        let mut result = BTreeMap::new();
        for line in text.lines() {
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| Error::new("invalid legacy manifest line"))?;
            if key.is_empty() || result.insert(key.to_owned(), value.to_owned()).is_some() {
                return Err(Error::new("duplicate legacy manifest key"));
            }
        }
        Ok(result)
    }
}

/// Prove the predecessor's exact file inventory, modes, hashes and identity.
///
/// # Errors
/// Refuses unknown keys/files, symbolic/hard-linked artifacts, changed bytes,
/// unsafe owner/modes, and provider/version or release-identity disagreement.
#[allow(clippy::too_many_lines)] // Keep the exact predecessor proof in one auditable sequence.
pub fn verify(root: &Path, spec: &LegacySpec, public: Vec<PublicEntry>) -> Result<ReleaseInfo> {
    let values = manifest(&root.join(spec.manifest))?;
    let expected_keys: BTreeSet<_> = ["format", "release_id"]
        .into_iter()
        .chain(spec.metadata.iter().copied())
        .chain(spec.proofs.iter().map(|p| p.key))
        .chain(spec.providers.iter().map(|p| p.key))
        .collect();
    if values.keys().map(String::as_str).collect::<BTreeSet<_>>() != expected_keys
        || values.get("format").map(String::as_str) != Some(spec.format)
    {
        return Err(Error::new("unsupported legacy manifest fields or format"));
    }
    let (files, directories) = inventory(root)?;
    let uid = current_uid()?;
    for path in std::iter::once(root.to_path_buf())
        .chain(directories.iter().map(|p| root.join(p)))
        .chain(files.keys().map(|p| root.join(p)))
    {
        let info = fs::symlink_metadata(path)?;
        if info.uid() != uid || info.mode() & 0o7022 != 0 {
            return Err(Error::new("unsafe legacy release ownership or modes"));
        }
    }
    let mut expected_files = BTreeSet::from([spec.manifest.to_owned()]);
    let mut identity = String::new();
    for proof in spec.proofs {
        let digest = values
            .get(proof.key)
            .ok_or_else(|| Error::new("legacy proof missing"))?;
        if !valid_hash(digest) || proof.paths.is_empty() {
            return Err(Error::new("invalid legacy proof"));
        }
        for path in proof.paths {
            if files.get(*path).map(|entry| &entry.sha256) != Some(digest) {
                return Err(Error::new("legacy artifact differs from manifest"));
            }
            expected_files.insert((*path).to_owned());
        }
        identity.push_str(digest);
        identity.push('\n');
    }
    let mut versions = BTreeMap::new();
    for provider in spec.providers {
        let prefix = format!("{}/", provider.path);
        let mut tree = String::new();
        for (path, entry) in files.iter().filter(|(path, _)| path.starts_with(&prefix)) {
            let relative = format!("./{}", &path[prefix.len()..]);
            if spec.hash_path_lines {
                tree.push_str("path=");
                tree.push_str(&relative);
                tree.push('\n');
            }
            tree.push_str(&entry.sha256);
            tree.push_str("  ");
            tree.push_str(&relative);
            tree.push('\n');
            expected_files.insert(path.clone());
        }
        let digest = hash_bytes(tree.as_bytes());
        if values.get(provider.key) != Some(&digest) {
            return Err(Error::new("legacy provider differs from manifest"));
        }
        let data: Value =
            serde_json::from_slice(&fs::read(root.join(provider.path).join("provider.json"))?)?;
        let version = if provider.version_key.is_empty() {
            data.pointer("/provider/release").and_then(Value::as_str)
        } else {
            values.get(provider.version_key).map(String::as_str)
        }
        .ok_or_else(|| Error::new("legacy provider version missing"))?;
        if data.pointer("/provider/id").and_then(Value::as_str) != Some(provider.provider)
            || data.pointer("/provider/release").and_then(Value::as_str) != Some(version)
        {
            return Err(Error::new("legacy provider identity or version differs"));
        }
        versions.insert(provider.provider.to_owned(), version.to_owned());
        identity.push_str(&digest);
        identity.push('\n');
    }
    let release_id = hash_bytes(identity.as_bytes());
    if values.get("release_id") != Some(&release_id)
        || root.file_name().and_then(|p| p.to_str()) != Some(release_id.as_str())
        || files.keys().cloned().collect::<BTreeSet<_>>() != expected_files
    {
        return Err(Error::new(
            "legacy release identity or exact inventory differs",
        ));
    }
    let mut expected_directories = BTreeSet::new();
    for file in &expected_files {
        let mut parent = Path::new(file).parent();
        while let Some(path) = parent {
            if path.as_os_str().is_empty() {
                break;
            }
            expected_directories.insert(path.to_string_lossy().into_owned());
            parent = path.parent();
        }
    }
    if directories != expected_directories {
        return Err(Error::new("legacy release has unmanifested directories"));
    }
    Ok(ReleaseInfo {
        release_id,
        format: spec.format.to_owned(),
        versions,
        files,
        public,
    })
}
