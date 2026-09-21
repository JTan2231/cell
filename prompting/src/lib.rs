//! Caller-owned prompt selection and rendering over Bazaar's opaque strings.
//!
//! Reads never initialize state or supply embedded fallback instructions.
#![allow(clippy::missing_errors_doc)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bazaar::api::{Reader, Record};
use nucleus_core::{JobRequestV1, ToolsetRef};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot read Bazaar prompt state: {0}")]
    Bazaar(#[from] bazaar::api::Error),
    #[error("invalid prompt selection: {0}")]
    Json(#[from] serde_json::Error),
    #[error("prompt configuration is invalid: {0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// An ordinary Bazaar string, interpreted by callers, that publishes exact versions.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub schema_version: u32,
    pub entries: BTreeMap<String, i64>,
}

/// One fully resolved immutable selection. No database handle survives the read.
pub struct Prompts {
    pub selection: Record,
    records: BTreeMap<String, Record>,
}

pub fn database_path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("CELL_BAZAAR_DATABASE") {
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            return Err(Error::Invalid(
                "CELL_BAZAAR_DATABASE must be absolute".into(),
            ));
        }
        return Ok(path);
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| Error::Invalid("HOME must be absolute".into()))?;
    Ok(bazaar::database_path(&home))
}

impl Prompts {
    pub fn load(owner: &str) -> Result<Self> {
        Self::open(&database_path()?, owner, None)
    }

    pub fn at(owner: &str, version: i64) -> Result<Self> {
        Self::open(&database_path()?, owner, Some(version))
    }

    pub fn open(database: &Path, owner: &str, version: Option<i64>) -> Result<Self> {
        let reader = Reader::open(database)?;
        let selection = reader.get(&format!("cell.prompts.{owner}"), version)?;
        let document: Selection = serde_json::from_str(&selection.content)?;
        if document.schema_version != 1 || document.entries.is_empty() {
            return Err(Error::Invalid("unsupported or empty selection".into()));
        }
        let records = document
            .entries
            .into_iter()
            .map(|(id, version)| reader.get(&id, Some(version)).map(|record| (id, record)))
            .collect::<std::result::Result<_, _>>()?;
        Ok(Self { selection, records })
    }

    pub fn text(&self, id: &str) -> Result<String> {
        self.records
            .get(id)
            .map(|record| record.content.clone())
            .ok_or_else(|| Error::Invalid(format!("selection has no entry {id}")))
    }

    /// Expand explicit references in trusted, caller-authored instruction fields.
    /// Do not apply this operation to user input, source documents, or tool results.
    pub fn expand(&self, text: &str) -> Result<String> {
        let mut result = String::new();
        let mut rest = text;
        while let Some(start) = rest.find("<bazaar:") {
            result.push_str(&rest[..start]);
            let reference = &rest[start + 8..];
            let end = reference
                .find('>')
                .ok_or_else(|| Error::Invalid("unterminated prompt reference".into()))?;
            result.push_str(&self.text(&reference[..end])?);
            rest = &reference[end + 1..];
        }
        result.push_str(rest);
        Ok(result)
    }

    /// Render a known template once. Inserted values are never interpreted again.
    pub fn render(&self, id: &str, values: &[(&str, String)]) -> Result<String> {
        render(&self.text(id)?, values)
    }

    /// Resolve descriptions in caller-owned tool/schema JSON, never source data.
    pub fn descriptions(&self, value: &mut Value) -> Result<()> {
        match value {
            Value::Object(fields) => {
                for (key, value) in fields {
                    if key == "description" && value.is_string() {
                        let text = value
                            .as_str()
                            .ok_or_else(|| Error::Invalid("description is not text".into()))?;
                        *value = Value::String(self.expand(text)?);
                    } else {
                        self.descriptions(value)?;
                    }
                }
            }
            Value::Array(values) => {
                for value in values {
                    self.descriptions(value)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// New registrations use the selection version above the historical range.
    pub fn toolset(&self, mut reference: ToolsetRef, historical_max: u32) -> Result<ToolsetRef> {
        reference.version = u32::try_from(self.selection.version)
            .ok()
            .and_then(|version| historical_max.checked_add(version))
            .ok_or_else(|| Error::Invalid("toolset version exhausted".into()))?;
        Ok(reference)
    }

    /// Historical registrations use the immutable migration selection, version one.
    pub fn for_toolset(owner: &str, reference: &ToolsetRef, historical_max: u32) -> Result<Self> {
        Self::at(
            owner,
            i64::from(reference.version.saturating_sub(historical_max).max(1)),
        )
    }

    pub fn instructions(&self, request: &mut JobRequestV1) -> Result<()> {
        request.instructions = self.expand(&request.instructions)?;
        if let Some(text) = &mut request.developer_instructions {
            *text = self.expand(text)?;
        }
        Ok(())
    }
}

/// Rust-style named placeholders and escaped braces, with caller-formatted values.
pub fn render(template: &str, values: &[(&str, String)]) -> Result<String> {
    let mut output = String::new();
    let mut chars = template.chars().peekable();
    let mut position = 0;
    while let Some(character) = chars.next() {
        match character {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                output.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                output.push('}');
            }
            '{' => {
                let mut name = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(value) => name.push(value),
                        None => {
                            return Err(Error::Invalid("unterminated template placeholder".into()));
                        }
                    }
                }
                if name.is_empty() {
                    name = position.to_string();
                    position += 1;
                }
                let name = name.strip_suffix(":?").unwrap_or(&name);
                let value = values.iter().find(|(key, _)| *key == name).ok_or_else(|| {
                    Error::Invalid(format!("unsupported template placeholder {name}"))
                })?;
                output.push_str(&value.1);
            }
            '}' => return Err(Error::Invalid("unescaped template brace".into())),
            value => output.push(value),
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bazaar::api::Writer;
    use serde_json::json;

    #[test]
    fn publication_pins_every_component_and_retains_old_selections() -> Result<()> {
        let directory = tempfile::tempdir().map_err(bazaar::api::Error::from)?;
        let database = directory.path().join("private/bazaar.sqlite3");
        let mut writer = Writer::initialize(&database)?;
        writer.update("weaver.instructions", "Original")?;
        let selection = |version| {
            json!({"schema_version":1,"entries":{"weaver.instructions":version}}).to_string()
        };
        writer.update("cell.prompts.weaver", &selection(1))?;
        let frozen = Prompts::open(&database, "weaver", None)?;
        writer.update("weaver.instructions", "Changed")?;
        assert_eq!(
            Prompts::open(&database, "weaver", None)?.text("weaver.instructions")?,
            "Original"
        );
        writer.update("cell.prompts.weaver", &selection(2))?;
        assert_eq!(
            Prompts::open(&database, "weaver", None)?.text("weaver.instructions")?,
            "Changed"
        );
        assert_eq!(
            Prompts::open(&database, "weaver", Some(1))?.text("weaver.instructions")?,
            "Original"
        );
        std::fs::remove_file(database).map_err(bazaar::api::Error::from)?;
        assert_eq!(frozen.text("weaver.instructions")?, "Original");
        Ok(())
    }

    #[test]
    fn unavailable_selection_never_creates_state() -> Result<()> {
        let directory = tempfile::tempdir().map_err(bazaar::api::Error::from)?;
        let database = directory.path().join("absent/bazaar.sqlite3");
        assert!(Prompts::open(&database, "weaver", None).is_err());
        assert!(!database.exists());
        assert!(!database.parent().is_some_and(Path::exists));
        Ok(())
    }

    #[test]
    fn rendering_is_single_pass_and_rejects_unknown_placeholders() -> Result<()> {
        let values = [("source", "{secret} <bazaar:private>".into())];
        assert_eq!(
            render("{{source}}: {source}", &values)?,
            "{source}: {secret} <bazaar:private>"
        );
        assert!(render("{missing}", &values).is_err());
        assert!(render("{source", &values).is_err());
        Ok(())
    }

    #[test]
    fn descriptions_resolve_inside_a_property_named_description() -> Result<()> {
        let prompts = Prompts {
            selection: Record {
                id: "cell.prompts.test".into(),
                version: 1,
                content: String::new(),
            },
            records: BTreeMap::from([(
                "test.description".into(),
                Record {
                    id: "test.description".into(),
                    version: 1,
                    content: "Meaning".into(),
                },
            )]),
        };
        let mut schema = json!({"properties":{"description":{"description":"<bazaar:test.description>","type":"string"}},"default":"<bazaar:test.description>"});
        prompts.descriptions(&mut schema)?;
        assert_eq!(
            schema["properties"]["description"]["description"],
            "Meaning"
        );
        assert_eq!(schema["default"], "<bazaar:test.description>");
        Ok(())
    }
}
