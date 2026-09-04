#![allow(dead_code)]

use std::collections::BTreeSet;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Read as _;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use time::OffsetDateTime;

use crate::agent_contracts::{PageRequest, SourceReadRequest, SourceSearchRequest};

pub(crate) const MANIFEST_SCHEMA_VERSION: u32 = 1;
pub(crate) const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
pub(crate) const MAX_SOURCE_BYTES: u64 = 4 * 1024 * 1024;
pub(crate) const MAX_CATALOG_BYTES: u64 = 32 * 1024 * 1024;
pub(crate) const MAX_SOURCES: usize = 4_000;
pub(crate) const FILE_VERIFIER_VERSION: &str = "pratica-files-v1";
const CATALOG_PAGE_SIZE: usize = 100;
const READ_PAGE_CHARACTERS: usize = 24_000;
const SEARCH_PAGE_MATCHES: usize = 40;
const SEARCH_SNIPPET_CHARACTERS: usize = 2_000;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceManifest {
    pub(crate) schema_version: u32,
    pub(crate) scope: String,
    pub(crate) version: u32,
    pub(crate) party: String,
    pub(crate) title: String,
    pub(crate) charter_markdown: String,
    pub(crate) sources: Vec<ManifestSource>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManifestSource {
    pub(crate) id: String,
    pub(crate) kind: String,
    pub(crate) path: PathBuf,
    #[serde(default)]
    pub(crate) revision: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct FrozenSourceCatalog {
    pub(crate) schema_version: u32,
    pub(crate) verifier_version: String,
    pub(crate) observed_at: i64,
    pub(crate) scope: String,
    pub(crate) version: u32,
    pub(crate) party: String,
    pub(crate) title: String,
    pub(crate) charter_markdown: String,
    pub(crate) charter_sha256: String,
    pub(crate) manifest_path: PathBuf,
    pub(crate) catalog_sha256: String,
    pub(crate) sources: Vec<FrozenSource>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct FrozenSource {
    pub(crate) id: String,
    pub(crate) kind: String,
    pub(crate) locator: String,
    pub(crate) origin_path: PathBuf,
    pub(crate) revision: Option<String>,
    pub(crate) content: Vec<u8>,
    pub(crate) content_sha256: String,
    pub(crate) observed_at: i64,
}

impl FrozenSource {
    pub(crate) fn byte_length(&self) -> usize {
        self.content.len()
    }

    fn text(&self) -> Result<&str, CatalogError> {
        std::str::from_utf8(&self.content).map_err(|error| {
            CatalogError::new(
                "source_snapshot_invalid",
                format!(
                    "frozen source {} is no longer valid UTF-8: {error}",
                    self.id
                ),
            )
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CatalogToolOutput {
    pub(crate) value: Value,
    pub(crate) evidence_refs: Vec<String>,
}

impl CatalogToolOutput {
    #[allow(clippy::needless_pass_by_value)]
    fn data(value: Value) -> Self {
        Self {
            value: json!({"ok": true, "data": value}),
            evidence_refs: Vec::new(),
        }
    }

    #[allow(clippy::needless_pass_by_value)]
    fn evidence(value: Value, evidence_refs: Vec<String>) -> Self {
        Self {
            value: json!({"ok": true, "data": value}),
            evidence_refs,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CatalogError {
    code: &'static str,
    message: String,
}

impl CatalogError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub(crate) const fn code(&self) -> &'static str {
        self.code
    }

    pub(crate) fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for CatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for CatalogError {}

impl FrozenSourceCatalog {
    pub(crate) fn load(manifest_path: &Path) -> Result<Self, CatalogError> {
        let manifest_metadata = fs::symlink_metadata(manifest_path).map_err(|error| {
            CatalogError::new(
                "manifest_unreadable",
                format!("unable to inspect {}: {error}", manifest_path.display()),
            )
        })?;
        if manifest_metadata.file_type().is_symlink() || !manifest_metadata.is_file() {
            return Err(CatalogError::new(
                "manifest_not_regular",
                "source manifest must be a nonsymlink regular file",
            ));
        }
        if manifest_metadata.len() > MAX_MANIFEST_BYTES {
            return Err(CatalogError::new(
                "manifest_too_large",
                format!("source manifest may contain at most {MAX_MANIFEST_BYTES} bytes"),
            ));
        }
        let manifest_bytes = read_bounded(manifest_path, MAX_MANIFEST_BYTES).map_err(|error| {
            CatalogError::new(
                "manifest_unreadable",
                format!("unable to read {}: {error}", manifest_path.display()),
            )
        })?;
        let manifest = decode_manifest(&manifest_bytes)?;

        let canonical_manifest = fs::canonicalize(manifest_path).map_err(|error| {
            CatalogError::new(
                "manifest_unreadable",
                format!("unable to resolve {}: {error}", manifest_path.display()),
            )
        })?;
        let parent = canonical_manifest.parent().ok_or_else(|| {
            CatalogError::new(
                "manifest_path_invalid",
                "source manifest has no containing directory",
            )
        })?;
        let canonical_parent = fs::canonicalize(parent).map_err(|error| {
            CatalogError::new(
                "manifest_path_invalid",
                format!("unable to resolve manifest directory: {error}"),
            )
        })?;
        Self::freeze_manifest(manifest, &canonical_parent, canonical_manifest)
    }

    pub(crate) fn load_from_reader<R: std::io::Read>(
        reader: R,
        source_root: &Path,
    ) -> Result<Self, CatalogError> {
        let canonical_root = canonical_source_root(source_root)?;
        let manifest_bytes = read_reader_bounded(reader, MAX_MANIFEST_BYTES).map_err(|error| {
            CatalogError::new(
                "manifest_unreadable",
                format!("unable to read source manifest from standard input: {error}"),
            )
        })?;
        let manifest = decode_manifest(&manifest_bytes)?;
        Self::freeze_manifest(manifest, &canonical_root, PathBuf::new())
    }

    pub(crate) fn load_from_bytes(
        manifest_bytes: &[u8],
        source_root: &Path,
    ) -> Result<Self, CatalogError> {
        let canonical_root = canonical_source_root(source_root)?;
        let manifest = decode_manifest(manifest_bytes)?;
        Self::freeze_manifest(manifest, &canonical_root, PathBuf::new())
    }

    #[allow(clippy::too_many_lines)]
    fn freeze_manifest(
        manifest: SourceManifest,
        source_base: &Path,
        manifest_path: PathBuf,
    ) -> Result<Self, CatalogError> {
        let mut ids = BTreeSet::new();
        let mut paths = BTreeSet::new();
        let mut sources = Vec::with_capacity(manifest.sources.len());
        let mut total_bytes = 0_u64;
        for source in &manifest.sources {
            if !ids.insert(source.id.as_str()) {
                return Err(CatalogError::new(
                    "source_id_duplicate",
                    format!("source ID is duplicated: {}", source.id),
                ));
            }
            let (origin_path, locator) = resolve_source_path(source_base, &source.path)?;
            if !paths.insert(origin_path.clone()) {
                return Err(CatalogError::new(
                    "source_path_duplicate",
                    format!("source path is listed more than once: {locator}"),
                ));
            }
            reject_sensitive_path(&source.path)?;
            let metadata = fs::symlink_metadata(&origin_path).map_err(|error| {
                CatalogError::new(
                    "source_unreadable",
                    format!("unable to inspect {locator}: {error}"),
                )
            })?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(CatalogError::new(
                    "source_not_regular",
                    format!("source must be a nonsymlink regular file: {locator}"),
                ));
            }
            if metadata.len() > MAX_SOURCE_BYTES {
                return Err(CatalogError::new(
                    "source_too_large",
                    format!("source exceeds {MAX_SOURCE_BYTES} bytes: {locator}"),
                ));
            }
            let content = read_bounded(&origin_path, MAX_SOURCE_BYTES).map_err(|error| {
                CatalogError::new(
                    "source_unreadable",
                    format!("unable to read {locator}: {error}"),
                )
            })?;
            if content.len() > usize::try_from(MAX_SOURCE_BYTES).unwrap_or(usize::MAX) {
                return Err(CatalogError::new(
                    "source_too_large",
                    format!("source exceeds {MAX_SOURCE_BYTES} bytes: {locator}"),
                ));
            }
            if u64::try_from(content.len()).unwrap_or(u64::MAX) != metadata.len() {
                return Err(CatalogError::new(
                    "source_changed",
                    format!("source changed while it was being frozen: {locator}"),
                ));
            }
            total_bytes = total_bytes
                .checked_add(u64::try_from(content.len()).unwrap_or(u64::MAX))
                .ok_or_else(|| {
                    CatalogError::new("catalog_too_large", "source catalog byte count overflowed")
                })?;
            if total_bytes > MAX_CATALOG_BYTES {
                return Err(CatalogError::new(
                    "catalog_too_large",
                    format!("source catalog may contain at most {MAX_CATALOG_BYTES} bytes"),
                ));
            }
            checked_utf8(&content, &format!("source {locator}"))?;
            let observed_at = OffsetDateTime::now_utc().unix_timestamp();
            sources.push(FrozenSource {
                id: source.id.clone(),
                kind: source.kind.clone(),
                locator,
                origin_path,
                revision: source.revision.clone(),
                content_sha256: digest(&content),
                content,
                observed_at,
            });
        }

        let observed_at = OffsetDateTime::now_utc().unix_timestamp();
        let charter_sha256 = digest(manifest.charter_markdown.as_bytes());
        let catalog_sha256 =
            catalog_digest(&manifest, FILE_VERIFIER_VERSION, &charter_sha256, &sources)?;
        Ok(Self {
            schema_version: manifest.schema_version,
            verifier_version: FILE_VERIFIER_VERSION.to_owned(),
            observed_at,
            scope: manifest.scope,
            version: manifest.version,
            party: manifest.party,
            title: manifest.title,
            charter_markdown: manifest.charter_markdown,
            charter_sha256,
            manifest_path,
            catalog_sha256,
            sources,
        })
    }

    pub(crate) fn verify_sources_current(&self) -> Result<(), CatalogError> {
        for source in &self.sources {
            let metadata = fs::symlink_metadata(&source.origin_path).map_err(|error| {
                CatalogError::new(
                    "basis_unavailable",
                    format!("unable to inspect {}: {error}", source.locator),
                )
            })?;
            if metadata.file_type().is_symlink()
                || !metadata.is_file()
                || metadata.len() != u64::try_from(source.content.len()).unwrap_or(u64::MAX)
            {
                return Err(CatalogError::new(
                    "basis_stale",
                    format!("source identity changed: {}", source.locator),
                ));
            }
            let content = read_bounded(&source.origin_path, MAX_SOURCE_BYTES).map_err(|error| {
                CatalogError::new(
                    "basis_unavailable",
                    format!("unable to read {}: {error}", source.locator),
                )
            })?;
            if content.len() > usize::try_from(MAX_SOURCE_BYTES).unwrap_or(usize::MAX)
                || content.len() != source.content.len()
                || digest(&content) != source.content_sha256
            {
                return Err(CatalogError::new(
                    "basis_stale",
                    format!("source content changed: {}", source.locator),
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn recompute_catalog_sha256(&self) -> Result<String, CatalogError> {
        let manifest = SourceManifest {
            schema_version: self.schema_version,
            scope: self.scope.clone(),
            version: self.version,
            party: self.party.clone(),
            title: self.title.clone(),
            charter_markdown: self.charter_markdown.clone(),
            sources: self
                .sources
                .iter()
                .map(|source| ManifestSource {
                    id: source.id.clone(),
                    kind: source.kind.clone(),
                    path: PathBuf::from(&source.locator),
                    revision: source.revision.clone(),
                })
                .collect(),
        };
        catalog_digest(
            &manifest,
            &self.verifier_version,
            &self.charter_sha256,
            &self.sources,
        )
    }

    pub(crate) fn catalog_page(
        &self,
        request: &PageRequest,
    ) -> Result<CatalogToolOutput, CatalogError> {
        let offset = parse_cursor(request.cursor.as_deref())?;
        if offset > self.sources.len() {
            return Err(invalid_cursor());
        }
        let end = (offset + CATALOG_PAGE_SIZE).min(self.sources.len());
        let sources = self.sources[offset..end]
            .iter()
            .map(|source| {
                json!({
                    "source_id": source.id,
                    "kind": source.kind,
                    "locator": source.locator,
                    "revision": source.revision,
                    "content_sha256": source.content_sha256,
                    "byte_length": source.content.len(),
                })
            })
            .collect::<Vec<_>>();
        Ok(CatalogToolOutput::data(json!({
            "scope": self.scope,
            "scope_version": self.version,
            "party": self.party,
            "title": self.title,
            "catalog_sha256": self.catalog_sha256,
            "sources": sources,
            "next_cursor": next_cursor(end, self.sources.len()),
        })))
    }

    pub(crate) fn read(
        &self,
        request: &SourceReadRequest,
    ) -> Result<CatalogToolOutput, CatalogError> {
        validate_source_id(&request.source_id)?;
        let offset = parse_cursor(request.cursor.as_deref())?;
        let source = self.source(&request.source_id)?;
        let text = source.text()?;
        let total = text.chars().count();
        if offset > total {
            return Err(invalid_cursor());
        }
        let content = text
            .chars()
            .skip(offset)
            .take(READ_PAGE_CHARACTERS)
            .collect::<String>();
        let end = offset + content.chars().count();
        let evidence_ref = format!("source:{}@chars:{offset}-{end}", source.id);
        Ok(CatalogToolOutput::evidence(
            json!({
                "source_id": source.id,
                "kind": source.kind,
                "locator": source.locator,
                "revision": source.revision,
                "content_sha256": source.content_sha256,
                "content": content,
                "evidence_ref": evidence_ref,
                "next_cursor": next_cursor(end, total),
            }),
            vec![evidence_ref],
        ))
    }

    pub(crate) fn search(
        &self,
        request: &SourceSearchRequest,
    ) -> Result<CatalogToolOutput, CatalogError> {
        validate_source_id(&request.source_id)?;
        if request.query.is_empty() || request.query.len() > 1_000 {
            return Err(CatalogError::new(
                "invalid_query",
                "source query must contain between 1 and 1000 UTF-8 bytes",
            ));
        }
        let source = self.source(&request.source_id)?;
        let text = source.text()?;
        let lines = text.lines().collect::<Vec<_>>();
        let offset = parse_cursor(request.cursor.as_deref())?;
        if offset > lines.len() {
            return Err(invalid_cursor());
        }
        let query = request.query.to_lowercase();
        let mut matches = Vec::new();
        let mut evidence_refs = Vec::new();
        let mut next = lines.len();
        for (index, line) in lines.iter().enumerate().skip(offset) {
            if line.to_lowercase().contains(&query) {
                let evidence_ref = format!("source:{}@line:{}", source.id, index + 1);
                matches.push(json!({
                    "source_id": source.id,
                    "locator": source.locator,
                    "line": index + 1,
                    "text": line.chars().take(SEARCH_SNIPPET_CHARACTERS).collect::<String>(),
                    "evidence_ref": evidence_ref,
                }));
                evidence_refs.push(evidence_ref);
                if matches.len() == SEARCH_PAGE_MATCHES {
                    next = index + 1;
                    break;
                }
            }
        }
        Ok(CatalogToolOutput::evidence(
            json!({
                "source_id": source.id,
                "query": request.query,
                "matches": matches,
                "next_cursor": next_cursor(next, lines.len()),
            }),
            evidence_refs,
        ))
    }

    fn source(&self, id: &str) -> Result<&FrozenSource, CatalogError> {
        self.sources
            .iter()
            .find(|source| source.id == id)
            .ok_or_else(|| {
                CatalogError::new(
                    "source_not_in_snapshot",
                    format!("source ID is not part of this frozen catalog: {id}"),
                )
            })
    }
}

fn decode_manifest(manifest_bytes: &[u8]) -> Result<SourceManifest, CatalogError> {
    if manifest_bytes.len() > usize::try_from(MAX_MANIFEST_BYTES).unwrap_or(usize::MAX) {
        return Err(CatalogError::new(
            "manifest_too_large",
            format!("source manifest may contain at most {MAX_MANIFEST_BYTES} bytes"),
        ));
    }
    let manifest_text = checked_utf8(manifest_bytes, "source manifest")?;
    let manifest = toml::from_str(manifest_text).map_err(|error| {
        CatalogError::new(
            "manifest_invalid",
            format!("unable to decode source manifest: {error}"),
        )
    })?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

fn canonical_source_root(source_root: &Path) -> Result<PathBuf, CatalogError> {
    if !source_root.is_absolute() {
        return Err(CatalogError::new(
            "source_root_relative",
            "source root must be an absolute directory",
        ));
    }
    let canonical = fs::canonicalize(source_root).map_err(|error| {
        CatalogError::new(
            "source_root_unreadable",
            format!(
                "unable to resolve source root {}: {error}",
                source_root.display()
            ),
        )
    })?;
    let metadata = fs::symlink_metadata(&canonical).map_err(|error| {
        CatalogError::new(
            "source_root_unreadable",
            format!(
                "unable to inspect source root {}: {error}",
                source_root.display()
            ),
        )
    })?;
    if !metadata.is_dir() {
        return Err(CatalogError::new(
            "source_root_not_directory",
            format!("source root must be a directory: {}", source_root.display()),
        ));
    }
    Ok(canonical)
}

fn validate_manifest(manifest: &SourceManifest) -> Result<(), CatalogError> {
    if manifest.schema_version != MANIFEST_SCHEMA_VERSION {
        return Err(CatalogError::new(
            "manifest_schema_unsupported",
            format!("source manifest schema_version must be {MANIFEST_SCHEMA_VERSION}"),
        ));
    }
    if manifest.version == 0 {
        return Err(CatalogError::new(
            "manifest_version_invalid",
            "source manifest version must be greater than zero",
        ));
    }
    bounded_nonblank("scope", &manifest.scope, 128)?;
    bounded_nonblank("party", &manifest.party, 128)?;
    bounded_nonblank("title", &manifest.title, 1_024)?;
    if manifest.charter_markdown.is_empty()
        || manifest.charter_markdown.len()
            > usize::try_from(MAX_MANIFEST_BYTES).unwrap_or(usize::MAX)
    {
        return Err(CatalogError::new(
            "manifest_charter_invalid",
            "charter_markdown must be nonempty and no larger than the manifest limit",
        ));
    }
    if manifest.sources.is_empty() || manifest.sources.len() > MAX_SOURCES {
        return Err(CatalogError::new(
            "manifest_sources_invalid",
            format!("source manifest must contain between 1 and {MAX_SOURCES} sources"),
        ));
    }
    for source in &manifest.sources {
        validate_source_id(&source.id)?;
        bounded_nonblank("source kind", &source.kind, 128)?;
        if let Some(revision) = &source.revision {
            bounded_nonblank("source revision", revision, 512)?;
        }
    }
    Ok(())
}

fn validate_source_id(value: &str) -> Result<(), CatalogError> {
    if value.is_empty()
        || value.len() > 128
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
        })
        || !value
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphanumeric())
    {
        return Err(CatalogError::new(
            "source_id_invalid",
            "source IDs must be 1-128 ASCII letters, digits, dots, underscores, or hyphens and start with a letter or digit",
        ));
    }
    Ok(())
}

fn bounded_nonblank(name: &str, value: &str, maximum: usize) -> Result<(), CatalogError> {
    if value.trim().is_empty() || value.len() > maximum {
        return Err(CatalogError::new(
            "manifest_field_invalid",
            format!("{name} must be nonblank and contain at most {maximum} UTF-8 bytes"),
        ));
    }
    Ok(())
}

fn resolve_source_path(base: &Path, relative: &Path) -> Result<(PathBuf, String), CatalogError> {
    if relative.as_os_str().is_empty() || relative.is_absolute() {
        return Err(CatalogError::new(
            "source_path_invalid",
            "source paths must be nonempty and relative to the manifest",
        ));
    }
    let mut current = base.to_path_buf();
    for component in relative.components() {
        match component {
            Component::Normal(name) => {
                current.push(name);
                let metadata = fs::symlink_metadata(&current).map_err(|error| {
                    CatalogError::new(
                        "source_unreadable",
                        format!("unable to inspect {}: {error}", relative.display()),
                    )
                })?;
                if metadata.file_type().is_symlink() {
                    return Err(CatalogError::new(
                        "source_symlink_forbidden",
                        format!(
                            "source paths may not traverse symlinks: {}",
                            relative.display()
                        ),
                    ));
                }
            }
            Component::ParentDir => {
                if !current.pop() {
                    return Err(CatalogError::new(
                        "source_path_invalid",
                        "source path traverses above the filesystem root",
                    ));
                }
            }
            Component::CurDir => {}
            Component::RootDir | Component::Prefix(_) => {
                return Err(CatalogError::new(
                    "source_path_invalid",
                    "source paths must remain relative to the manifest",
                ));
            }
        }
    }
    let canonical = fs::canonicalize(&current).map_err(|error| {
        CatalogError::new(
            "source_unreadable",
            format!("unable to resolve {}: {error}", relative.display()),
        )
    })?;
    let locator = relative
        .to_str()
        .ok_or_else(|| {
            CatalogError::new(
                "source_path_not_utf8",
                "source paths must be representable as UTF-8",
            )
        })?
        .to_owned();
    Ok((canonical, locator))
}

fn reject_sensitive_path(path: &Path) -> Result<(), CatalogError> {
    for component in path.components() {
        let Component::Normal(name) = component else {
            continue;
        };
        let name = name.to_string_lossy().to_ascii_lowercase();
        if name.starts_with('.')
            || matches!(
                name.as_str(),
                "target"
                    | "node_modules"
                    | "vendor"
                    | "secrets"
                    | "secret"
                    | "credentials"
                    | "credential"
                    | "id_rsa"
                    | "id_dsa"
                    | "id_ecdsa"
                    | "id_ed25519"
                    | "passwd"
                    | "shadow"
            )
            || name
                .split(|character: char| !character.is_ascii_alphanumeric())
                .any(|part| {
                    matches!(
                        part,
                        "secret"
                            | "secrets"
                            | "credential"
                            | "credentials"
                            | "password"
                            | "passwords"
                            | "privatekey"
                    )
                })
        {
            return Err(CatalogError::new(
                "source_sensitive_path",
                format!("source path is excluded as sensitive: {}", path.display()),
            ));
        }
    }
    if let Some(extension) = path.extension().and_then(|value| value.to_str())
        && matches!(
            extension.to_ascii_lowercase().as_str(),
            "db" | "db3"
                | "sqlite"
                | "sqlite3"
                | "wal"
                | "shm"
                | "log"
                | "pem"
                | "key"
                | "p12"
                | "pfx"
                | "der"
                | "png"
                | "jpg"
                | "jpeg"
                | "gif"
                | "webp"
                | "bmp"
                | "ico"
                | "pdf"
                | "zip"
                | "gz"
                | "xz"
                | "bz2"
                | "7z"
                | "tar"
                | "wasm"
                | "o"
                | "a"
                | "so"
                | "dylib"
                | "dll"
                | "exe"
                | "bin"
                | "class"
                | "jar"
                | "pyc"
                | "woff"
                | "woff2"
                | "ttf"
                | "otf"
                | "mp3"
                | "mp4"
                | "mov"
                | "wav"
                | "flac"
        )
    {
        return Err(CatalogError::new(
            "source_type_forbidden",
            format!("source file type is excluded: {}", path.display()),
        ));
    }
    Ok(())
}

fn checked_utf8<'a>(bytes: &'a [u8], label: &str) -> Result<&'a str, CatalogError> {
    if bytes.iter().any(|byte| {
        *byte == 0 || (*byte < b'\t') || (*byte > b'\r' && *byte < b' ') || *byte == 0x7f
    }) {
        return Err(CatalogError::new(
            "source_binary_forbidden",
            format!("{label} contains binary control bytes"),
        ));
    }
    std::str::from_utf8(bytes).map_err(|error| {
        CatalogError::new(
            "source_not_utf8",
            format!("{label} must be exact UTF-8 text: {error}"),
        )
    })
}

fn catalog_digest(
    manifest: &SourceManifest,
    verifier_version: &str,
    charter_sha256: &str,
    sources: &[FrozenSource],
) -> Result<String, CatalogError> {
    let projection = json!({
        "schema_version": manifest.schema_version,
        "verifier_version": verifier_version,
        "scope": manifest.scope,
        "version": manifest.version,
        "party": manifest.party,
        "title": manifest.title,
        "charter_sha256": charter_sha256,
        "sources": sources.iter().map(|source| json!({
            "id": source.id,
            "kind": source.kind,
            "locator": source.locator,
            "revision": source.revision,
            "content_sha256": source.content_sha256,
            "byte_length": source.content.len(),
        })).collect::<Vec<_>>()
    });
    serde_json::to_vec(&projection)
        .map(|bytes| digest(&bytes))
        .map_err(|error| {
            CatalogError::new(
                "catalog_digest_failed",
                format!("unable to encode canonical source catalog: {error}"),
            )
        })
}

fn digest(bytes: &[u8]) -> String {
    let value = Sha256::digest(bytes);
    format!("{value:x}")
}

fn read_bounded(path: &Path, maximum: u64) -> std::io::Result<Vec<u8>> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW);
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "bounded source must be a regular file",
        ));
    }
    if metadata.len() > maximum {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "bounded source exceeds its byte limit",
        ));
    }
    read_reader_bounded(file, maximum)
}

fn read_reader_bounded<R: std::io::Read>(reader: R, maximum: u64) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take(maximum.saturating_add(1))
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn parse_cursor(cursor: Option<&str>) -> Result<usize, CatalogError> {
    match cursor {
        None => Ok(0),
        Some(value) if !value.is_empty() && value.len() <= 20 => {
            value.parse().map_err(|_| invalid_cursor())
        }
        Some(_) => Err(invalid_cursor()),
    }
}

fn next_cursor(position: usize, total: usize) -> Value {
    if position < total {
        Value::String(position.to_string())
    } else {
        Value::Null
    }
}

fn invalid_cursor() -> CatalogError {
    CatalogError::new(
        "invalid_cursor",
        "cursor does not belong to this bounded source result",
    )
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Cursor;
    use std::path::Path;

    use super::{FrozenSourceCatalog, MAX_MANIFEST_BYTES};
    use crate::agent_contracts::{PageRequest, SourceReadRequest, SourceSearchRequest};

    type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

    fn fixture() -> TestResult<(tempfile::TempDir, std::path::PathBuf)> {
        let directory = tempfile::tempdir()?;
        fs::write(
            directory.path().join("contract.md"),
            "# Existing contract\r\n\r\nKeep exact λ bytes.  \r\n",
        )?;
        fs::write(
            directory.path().join("implementation.rs"),
            "pub fn existing_behavior() {}\n// exact evidence\n",
        )?;
        let manifest = directory.path().join("steward.toml");
        fs::write(
            &manifest,
            r#"schema_version = 1
scope = "crm-authority"
version = 1
party = "authority-steward"
title = "CRM authority"
charter_markdown = "Rectify authority expectations."

[[sources]]
id = "contract"
kind = "contract"
path = "contract.md"
revision = "v1"

[[sources]]
id = "implementation"
kind = "source"
path = "implementation.rs"
"#,
        )?;
        Ok((directory, manifest))
    }

    #[test]
    fn freezes_exact_bounded_sources_and_serves_evidence() -> TestResult {
        let (_directory, manifest) = fixture()?;
        let catalog = FrozenSourceCatalog::load(&manifest)?;
        assert_eq!(catalog.scope, "crm-authority");
        assert_eq!(catalog.version, 1);
        assert_eq!(catalog.sources.len(), 2);
        assert_eq!(catalog.catalog_sha256.len(), 64);
        assert!(
            catalog
                .catalog_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        );
        assert!(catalog.verify_sources_current().is_ok());

        let page = catalog.catalog_page(&PageRequest { cursor: None })?;
        assert_eq!(
            page.value["data"]["sources"].as_array().map(Vec::len),
            Some(2)
        );
        let read = catalog.read(&SourceReadRequest {
            source_id: "contract".to_owned(),
            cursor: None,
        })?;
        assert_eq!(
            read.value["data"]["content"],
            "# Existing contract\r\n\r\nKeep exact λ bytes.  \r\n"
        );
        assert_eq!(read.evidence_refs.len(), 1);
        let search = catalog.search(&SourceSearchRequest {
            source_id: "implementation".to_owned(),
            query: "evidence".to_owned(),
            cursor: None,
        })?;
        assert_eq!(
            search.value["data"]["matches"].as_array().map(Vec::len),
            Some(1)
        );
        assert_eq!(search.evidence_refs, vec!["source:implementation@line:2"]);
        Ok(())
    }

    #[test]
    fn reader_and_bytes_manifests_use_the_explicit_source_root() -> TestResult {
        let (directory, manifest) = fixture()?;
        let manifest_bytes = fs::read(&manifest)?;
        let from_file = FrozenSourceCatalog::load(&manifest)?;
        let from_reader = FrozenSourceCatalog::load_from_reader(
            Cursor::new(manifest_bytes.clone()),
            directory.path(),
        )?;
        let from_bytes = FrozenSourceCatalog::load_from_bytes(&manifest_bytes, directory.path())?;

        for catalog in [&from_reader, &from_bytes] {
            assert!(catalog.manifest_path.as_os_str().is_empty());
            assert_eq!(catalog.catalog_sha256, from_file.catalog_sha256);
            assert_eq!(catalog.charter_sha256, from_file.charter_sha256);
            assert_eq!(catalog.sources.len(), from_file.sources.len());
            for (source, file_source) in catalog.sources.iter().zip(&from_file.sources) {
                assert_eq!(source.id, file_source.id);
                assert_eq!(source.kind, file_source.kind);
                assert_eq!(source.locator, file_source.locator);
                assert_eq!(source.origin_path, file_source.origin_path);
                assert_eq!(source.revision, file_source.revision);
                assert_eq!(source.content, file_source.content);
                assert_eq!(source.content_sha256, file_source.content_sha256);
            }
        }
        Ok(())
    }

    #[test]
    fn reader_manifest_requires_a_bounded_absolute_existing_directory() -> TestResult {
        let (directory, manifest) = fixture()?;
        let manifest_bytes = fs::read(&manifest)?;

        let Err(error) =
            FrozenSourceCatalog::load_from_bytes(&manifest_bytes, Path::new("relative-root"))
        else {
            panic!("relative source root was accepted");
        };
        assert_eq!(error.code(), "source_root_relative");

        let missing = directory.path().join("missing");
        let Err(error) = FrozenSourceCatalog::load_from_bytes(&manifest_bytes, &missing) else {
            panic!("missing source root was accepted");
        };
        assert_eq!(error.code(), "source_root_unreadable");

        let regular_file = directory.path().join("not-a-directory");
        fs::write(&regular_file, "not a directory")?;
        let Err(error) = FrozenSourceCatalog::load_from_bytes(&manifest_bytes, &regular_file)
        else {
            panic!("regular file source root was accepted");
        };
        assert_eq!(error.code(), "source_root_not_directory");

        let oversized = vec![b'x'; usize::try_from(MAX_MANIFEST_BYTES)? + 1];
        let Err(error) =
            FrozenSourceCatalog::load_from_reader(Cursor::new(oversized), directory.path())
        else {
            panic!("oversized reader manifest was accepted");
        };
        assert_eq!(error.code(), "manifest_too_large");
        Ok(())
    }

    #[test]
    fn later_source_change_marks_the_basis_stale_without_changing_snapshot() -> TestResult {
        let (directory, manifest) = fixture()?;
        let catalog = FrozenSourceCatalog::load(&manifest)?;
        let original = catalog.sources[0].content.clone();
        fs::write(directory.path().join("contract.md"), "changed\n")?;
        let Err(error) = catalog.verify_sources_current() else {
            panic!("changed source was accepted as current");
        };
        assert_eq!(error.code(), "basis_stale");
        assert_eq!(catalog.sources[0].content, original);
        Ok(())
    }

    #[test]
    fn admits_explicit_parent_relative_sources_without_copying_them() -> TestResult {
        let root = tempfile::tempdir()?;
        let manifests = root.path().join("manifests");
        let product = root.path().join("product");
        fs::create_dir(&manifests)?;
        fs::create_dir(&product)?;
        fs::write(product.join("contract.md"), "# Actual product contract\n")?;
        let manifest = manifests.join("steward.toml");
        fs::write(
            &manifest,
            r#"schema_version = 1
scope = "cross-product"
version = 1
party = "steward"
title = "Cross-product steward"
charter_markdown = "Use the actual source."

[[sources]]
id = "contract"
kind = "contract"
path = "../product/contract.md"
"#,
        )?;

        let manifest_bytes = fs::read(&manifest)?;
        for catalog in [
            FrozenSourceCatalog::load(&manifest)?,
            FrozenSourceCatalog::load_from_bytes(&manifest_bytes, &manifests)?,
        ] {
            assert_eq!(catalog.sources[0].locator, "../product/contract.md");
            assert_eq!(
                catalog.sources[0].origin_path,
                fs::canonicalize(product.join("contract.md"))?
            );
            assert_eq!(catalog.sources[0].content, b"# Actual product contract\n");
        }
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinks_sensitive_files_and_binary_sources() -> TestResult {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir()?;
        fs::write(directory.path().join("actual.md"), "safe\n")?;
        symlink("actual.md", directory.path().join("alias.md"))?;
        assert!(super::read_bounded(&directory.path().join("alias.md"), 1_024).is_err());
        let manifest = |path: &str| {
            format!(
                "schema_version=1\nscope=\"s\"\nversion=1\nparty=\"p\"\ntitle=\"t\"\ncharter_markdown=\"c\"\n[[sources]]\nid=\"one\"\nkind=\"source\"\npath=\"{path}\"\n"
            )
        };
        let descriptor = directory.path().join("manifest.toml");
        fs::write(&descriptor, manifest("alias.md"))?;
        let Err(error) = FrozenSourceCatalog::load(&descriptor) else {
            panic!("symlink source was accepted");
        };
        assert_eq!(error.code(), "source_symlink_forbidden");

        fs::write(directory.path().join("credentials.txt"), "not admitted")?;
        fs::write(&descriptor, manifest("credentials.txt"))?;
        let Err(error) = FrozenSourceCatalog::load(&descriptor) else {
            panic!("sensitive source was accepted");
        };
        assert_eq!(error.code(), "source_sensitive_path");

        fs::write(directory.path().join("bytes.txt"), b"text\0binary")?;
        fs::write(&descriptor, manifest("bytes.txt"))?;
        let Err(error) = FrozenSourceCatalog::load(&descriptor) else {
            panic!("binary source was accepted");
        };
        assert_eq!(error.code(), "source_binary_forbidden");
        Ok(())
    }
}
