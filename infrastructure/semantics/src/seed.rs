use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};

use crate::domain::{GroundingSource, SemanticEffect, concept_id_for, normalize_label};
use crate::error::io;
use crate::store::Store;
use crate::{Error, Result};

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SeedEntry {
    pub label: String,
    pub meaning: String,
}

pub fn seed_one(
    store: &Store,
    project_id: &str,
    label: &str,
    meaning: &str,
    grounding: Option<&str>,
) -> Result<u64> {
    require_empty(store, project_id)?;
    let digest = digest_bytes(format!("{label}\0{meaning}").as_bytes());
    let mut effects = vec![SemanticEffect::Define {
        concept_id: concept_id_for(1),
        label: label.to_owned(),
        meaning: meaning.to_owned(),
    }];
    if let Some(statement) = grounding {
        effects.push(SemanticEffect::Ground {
            concept_id: concept_id_for(1),
            source: GroundingSource::Seed {
                source_label: "explicit-cli-seed".to_owned(),
                digest,
            },
            statement: statement.to_owned(),
        });
    }
    store.commit_revision(project_id, 0, "Explicit repository seed", None, &effects)
}

pub fn seed_markdown(store: &Store, project_id: &str, source_path: &Path) -> Result<u64> {
    require_empty(store, project_id)?;
    let project = store.project(project_id)?;
    let project_root = Path::new(&project.current_path)
        .canonicalize()
        .map_err(|source| io(PathBuf::from(&project.current_path), source))?;
    let canonical_source = source_path
        .canonicalize()
        .map_err(|source| io(source_path, source))?;
    let relative = canonical_source.strip_prefix(&project_root).map_err(|_| {
        Error::domain(
            "seed_source_outside_project",
            "seed source must be inside the registered project root",
        )
    })?;
    if relative.as_os_str().is_empty() {
        return Err(Error::domain(
            "seed_source_invalid",
            "seed source must be a regular file below the project root",
        ));
    }
    let metadata =
        fs::symlink_metadata(&canonical_source).map_err(|source| io(&canonical_source, source))?;
    if !metadata.file_type().is_file() {
        return Err(Error::domain(
            "seed_source_invalid",
            "seed source must be a regular file",
        ));
    }
    let bytes = fs::read(&canonical_source).map_err(|source| io(&canonical_source, source))?;
    let markdown = std::str::from_utf8(&bytes)
        .map_err(|_| Error::domain("seed_source_not_utf8", "seed Markdown must be UTF-8"))?;
    let entries = parse_definition_list(markdown)?;
    let source_label = relative
        .to_str()
        .ok_or_else(|| Error::domain("seed_source_not_utf8", "seed source label must be UTF-8"))?
        .replace(std::path::MAIN_SEPARATOR, "/");
    let digest = digest_bytes(&bytes);
    let mut effects = Vec::with_capacity(entries.len() * 2);
    for (index, entry) in entries.iter().enumerate() {
        let number = u64::try_from(index)
            .map_err(|_| Error::domain("seed_too_large", "too many seed entries"))?
            + 1;
        effects.push(SemanticEffect::Define {
            concept_id: concept_id_for(number),
            label: entry.label.clone(),
            meaning: entry.meaning.clone(),
        });
    }
    for (index, entry) in entries.iter().enumerate() {
        let number = u64::try_from(index)
            .map_err(|_| Error::domain("seed_too_large", "too many seed entries"))?
            + 1;
        effects.push(SemanticEffect::Ground {
            concept_id: concept_id_for(number),
            source: GroundingSource::Seed {
                source_label: source_label.clone(),
                digest: digest.clone(),
            },
            statement: entry.meaning.clone(),
        });
    }
    store.commit_revision(
        project_id,
        0,
        &format!("Seed repository from {source_label}"),
        None,
        &effects,
    )
}

pub fn parse_definition_list(markdown: &str) -> Result<Vec<SeedEntry>> {
    let lines = markdown.lines().collect::<Vec<_>>();
    let mut entries = Vec::new();
    let mut seen = BTreeSet::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index].trim();
        let Some(label) = definition_label(line) else {
            index += 1;
            continue;
        };
        index += 1;
        while index < lines.len() && lines[index].trim().is_empty() {
            index += 1;
        }
        if index >= lines.len() || !lines[index].trim_start().starts_with(':') {
            return Err(Error::domain(
                "seed_definition_missing",
                format!("definition-list term {label:?} has no following ':' definition"),
            ));
        }
        let first = lines[index]
            .trim_start()
            .strip_prefix(':')
            .unwrap_or_default()
            .trim();
        let mut definition_lines = vec![first.to_owned()];
        index += 1;
        while index < lines.len() {
            let raw = lines[index];
            if raw.trim().is_empty() {
                let mut lookahead = index + 1;
                while lookahead < lines.len() && lines[lookahead].trim().is_empty() {
                    lookahead += 1;
                }
                if lookahead < lines.len() && is_indented(lines[lookahead]) {
                    index = lookahead;
                    continue;
                }
                break;
            }
            if !is_indented(raw) {
                break;
            }
            definition_lines.push(raw.trim().to_owned());
            index += 1;
        }
        let meaning = definition_lines.join(" ").trim().to_owned();
        if label.trim().is_empty() || meaning.is_empty() {
            return Err(Error::domain(
                "seed_definition_empty",
                "seed terms and definitions must not be empty",
            ));
        }
        let normalized = normalize_label(label);
        if !seen.insert(normalized.clone()) {
            return Err(Error::domain(
                "seed_term_duplicate",
                format!("duplicate normalized seed term {normalized:?}"),
            ));
        }
        entries.push(SeedEntry {
            label: label.trim().to_owned(),
            meaning,
        });
    }
    if entries.is_empty() {
        return Err(Error::domain(
            "seed_empty",
            "seed Markdown contains no definition-list entries",
        ));
    }
    Ok(entries)
}

fn definition_label(line: &str) -> Option<&str> {
    line.strip_prefix("**")
        .and_then(|value| value.strip_suffix("**"))
        .map(str::trim)
}

fn is_indented(line: &str) -> bool {
    line.starts_with(' ') || line.starts_with('\t')
}

fn require_empty(store: &Store, project_id: &str) -> Result<()> {
    let repository = store.repository(project_id, None)?;
    if repository.revision != 0 {
        return Err(Error::domain(
            "seed_repository_not_empty",
            format!(
                "project {project_id} is already at revision {}",
                repository.revision
            ),
        ));
    }
    Ok(())
}

fn digest_bytes(bytes: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(bytes);
    format!("{:x}", digest.finalize())
}

#[cfg(test)]
mod tests {
    use super::parse_definition_list;

    #[test]
    fn parses_definition_list_in_document_order() {
        let entries = parse_definition_list(
            "# Vocabulary\n\n**Concern**\n: An open question.\n\n**Decision**\n: A durable settlement.\n",
        )
        .expect("valid definition list");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].label, "Concern");
        assert_eq!(entries[1].meaning, "A durable settlement.");
    }

    #[test]
    fn rejects_normalized_duplicates() {
        let error = parse_definition_list("**Concern**\n: One.\n\n**  CONCERN  **\n: Two.\n")
            .expect_err("duplicate should fail");
        assert_eq!(error.code(), "seed_term_duplicate");
    }

    #[test]
    fn stops_definition_before_following_section_and_table() {
        let entries = parse_definition_list(
            "**Concern**\n: An open question\n  with a continuation.\n\n## Other\n\n| A | B |\n|---|---|\n",
        )
        .expect("valid definition list");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].meaning, "An open question with a continuation.");
    }
}
