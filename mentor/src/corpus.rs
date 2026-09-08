use std::collections::HashSet;
use std::io::{Error, ErrorKind};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MAX_CORPUS_BYTES: usize = 8 * 1024 * 1024;
const MAX_DOCUMENT_BYTES: usize = 256 * 1024;
const MAX_PROBLEMS: usize = 1_000;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Problem {
    pub id: String,
    pub title: String,
    pub markdown: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rubric {
    pub version: String,
    pub markdown: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Corpus {
    pub schema_version: u32,
    pub source_product: String,
    pub source_version: String,
    pub rubric: Rubric,
    pub problems: Vec<Problem>,
}

impl Corpus {
    /// Read the embedded authored problem collection.
    ///
    /// # Errors
    /// Rejects an invalid or unsupported embedded corpus.
    pub fn bundled() -> crate::Result<Self> {
        Self::from_json(include_str!("../content/corpus.json"))
    }

    /// Decode and validate one bounded corpus document.
    ///
    /// # Errors
    /// Rejects invalid JSON, unsupported metadata, duplicate problem IDs,
    /// invalid problem or rubric content, and exceeded size limits.
    pub fn from_json(json: &str) -> crate::Result<Self> {
        if json.len() > MAX_CORPUS_BYTES {
            return Err(invalid("corpus exceeds the 8 MiB limit"));
        }
        let corpus: Self = serde_json::from_str(json)?;
        corpus.validate()?;
        Ok(corpus)
    }

    /// Identify the validated corpus by its canonical JSON bytes.
    ///
    /// # Errors
    /// Returns corpus validation or JSON encoding errors.
    pub fn digest(&self) -> crate::Result<String> {
        self.validate()?;
        let bytes = serde_json::to_vec(self)?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }

    #[must_use]
    pub fn problem(&self, id: &str) -> Option<&Problem> {
        self.problems.iter().find(|problem| problem.id == id)
    }

    fn validate(&self) -> crate::Result<()> {
        if self.schema_version != 1 {
            return Err(invalid("unsupported corpus schema version"));
        }
        if self.source_product != "mentor" {
            return Err(invalid("corpus source product must be mentor"));
        }
        if !valid_version(&self.source_version) {
            return Err(invalid("corpus source version must be a semantic version"));
        }
        if self.problems.is_empty() || self.problems.len() > MAX_PROBLEMS {
            return Err(invalid("corpus must contain between 1 and 1000 problems"));
        }

        validate_rubric(&self.rubric)?;
        let mut ids = HashSet::new();
        for problem in &self.problems {
            if problem.id.is_empty()
                || problem.id.len() > 200
                || !problem
                    .id
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            {
                return Err(invalid("problem IDs must be lowercase ASCII slugs"));
            }
            if !ids.insert(problem.id.as_str()) {
                return Err(invalid(format!("duplicate problem ID: {}", problem.id)));
            }
            if problem.title.trim() != problem.title
                || problem.title.is_empty()
                || problem.title.len() > 512
                || problem.title.chars().any(char::is_control)
            {
                return Err(invalid(format!("invalid title for problem {}", problem.id)));
            }
            validate_document(&problem.markdown, &problem.id)?;
            let titles: Vec<_> = problem
                .markdown
                .lines()
                .filter_map(level_one_title)
                .collect();
            if titles.as_slice() != [problem.title.as_str()] {
                return Err(invalid(format!(
                    "problem {} must have one heading matching its title",
                    problem.id
                )));
            }
            if !problem
                .markdown
                .lines()
                .any(|line| level_one_title(line).is_none() && !line.trim().is_empty())
            {
                return Err(invalid(format!("problem {} has no body", problem.id)));
            }
        }
        Ok(())
    }
}

fn validate_document(markdown: &str, label: &str) -> crate::Result<()> {
    if markdown.trim().is_empty()
        || markdown.len() > MAX_DOCUMENT_BYTES
        || markdown.contains('\r')
        || markdown.contains('\0')
    {
        return Err(invalid(format!(
            "{label} must be nonempty Markdown with LF line endings, no NUL, and at most 256 KiB"
        )));
    }
    Ok(())
}

fn level_one_title(line: &str) -> Option<&str> {
    let rest = line.strip_prefix('#')?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    Some(rest.trim())
}

fn valid_version(version: &str) -> bool {
    if version.len() > 100 {
        return false;
    }
    let (core, suffix) = match version.split_once('-') {
        Some((core, suffix)) => (core, Some(suffix)),
        None => (version, None),
    };
    let parts: Vec<_> = core.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        && suffix.is_none_or(|suffix| {
            !suffix.is_empty()
                && suffix
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-')
        })
}

fn validate_rubric(rubric: &Rubric) -> crate::Result<()> {
    validate_document(&rubric.markdown, "rubric")?;
    if !valid_version(&rubric.version) {
        return Err(invalid("rubric version must be a semantic version"));
    }
    let titles: Vec<_> = rubric
        .markdown
        .lines()
        .filter_map(level_one_title)
        .collect();
    if titles.as_slice() != ["Mentor system-design evaluation contract"] {
        return Err(invalid("rubric must have its canonical title"));
    }
    let versions: Vec<_> = rubric
        .markdown
        .lines()
        .filter_map(|line| line.strip_prefix("- **Contract version:** "))
        .map(str::trim)
        .collect();
    if versions.as_slice() != [rubric.version.as_str()] {
        return Err(invalid("rubric metadata and Markdown version must match"));
    }
    let dimensions: Vec<_> = rubric
        .markdown
        .lines()
        .filter_map(|line| line.strip_prefix("## Dimension "))
        .collect();
    if dimensions.len() != 8 {
        return Err(invalid("rubric must have eight ordered dimensions"));
    }
    let mut names = HashSet::new();
    for (index, dimension) in dimensions.iter().enumerate() {
        let prefix = format!("{}: ", index + 1);
        let Some(name) = dimension.strip_prefix(prefix.as_str()).map(str::trim) else {
            return Err(invalid("rubric dimensions must be numbered 1 through 8"));
        };
        if name.is_empty() || !names.insert(name) {
            return Err(invalid(
                "rubric dimension names must be nonempty and unique",
            ));
        }
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> Box<dyn std::error::Error + Send + Sync> {
    Error::new(ErrorKind::InvalidData, message.into()).into()
}
