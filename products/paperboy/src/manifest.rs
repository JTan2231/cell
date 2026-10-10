//! File-backed production definitions. Production runs retain no product state.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{Read as _, Write as _};
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _};
use std::path::Path;

const MAX_MANIFEST_BYTES: u64 = 1_048_576;
pub const EMPTY_MANIFEST: &str = "version = 1\n\n[productions]\n";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: u32,
    // Keep the Rust and JSON field compatible; productions is the preferred TOML key.
    #[serde(alias = "productions")]
    pub jobs: BTreeMap<String, Job>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Job {
    pub render: Vec<String>,
    pub subject: Option<String>,
    pub schedule: Schedule,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Schedule {
    Interval { seconds: u32 },
    LocalCalendar { hour: u8, minute: u8 },
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Self> {
        // Parser diagnostics can contain supplied TOML. Do not copy them into logs.
        let manifest: Self = toml::from_str(text)
            .map_err(|_| anyhow::anyhow!("invalid Paperboy production-definition TOML"))?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn load(path: &Path) -> Result<Self> {
        ensure!(
            path.is_absolute(),
            "production-definition path must be absolute"
        );
        ensure!(
            fs::metadata(path)?.is_file(),
            "production-definition file must be a regular file"
        );
        let file = fs::File::open(path).context("cannot open Paperboy production definitions")?;
        ensure!(
            file.metadata()?.is_file(),
            "production-definition file must be a regular file"
        );
        let mut bytes = Vec::new();
        file.take(MAX_MANIFEST_BYTES + 1).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 <= MAX_MANIFEST_BYTES,
            "production-definition file exceeds 1 MiB"
        );
        Self::parse(
            std::str::from_utf8(&bytes).context("production-definition file must be UTF-8")?,
        )
    }

    /// Create only an absent empty definition file. Existing definitions are preserved.
    pub fn initialize(path: &Path) -> Result<bool> {
        ensure!(
            path.is_absolute(),
            "production-definition path must be absolute"
        );
        let parent = path
            .parent()
            .context("production-definition parent is required")?;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)?;
        let result = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path);
        match result {
            Ok(mut file) => {
                file.write_all(EMPTY_MANIFEST.as_bytes())?;
                file.sync_all()?;
                Ok(true)
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                Self::load(path)?;
                Ok(false)
            }
            Err(error) => Err(error).context("cannot initialize Paperboy production definitions"),
        }
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1,
            "unsupported Paperboy production-definition version"
        );
        for (id, job) in &self.jobs {
            job.validate(id)
                .with_context(|| format!("invalid production {id}"))?;
        }
        Ok(())
    }

    pub fn job(&self, id: &str) -> Result<&Job> {
        self.jobs
            .get(id)
            .context("production is not present in the definition file")
    }
}

impl Job {
    #[must_use]
    pub fn subject<'a>(&'a self, id: &'a str) -> &'a str {
        self.subject.as_deref().unwrap_or(id)
    }

    pub fn validate(&self, id: &str) -> Result<()> {
        validate_id(id)?;
        validate_render(&self.render)?;
        validate_subject(self.subject(id))?;
        match self.schedule {
            Schedule::Interval { seconds } => {
                ensure!(
                    (1..=31_536_000).contains(&seconds),
                    "interval must be 1..31536000 seconds"
                );
            }
            Schedule::LocalCalendar { hour, minute } => {
                ensure!(
                    hour < 24 && minute < 60,
                    "invalid local-calendar hour or minute"
                );
            }
        }
        Ok(())
    }
}

pub fn validate_id(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty()
            && id.len() <= 63
            && id.as_bytes()[0].is_ascii_lowercase()
            && id
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'),
        "production ID must start with a lowercase ASCII letter and contain only lowercase letters, digits, or hyphens (at most 63 bytes)"
    );
    Ok(())
}

pub fn validate_render(render: &[String]) -> Result<()> {
    let executable = render
        .first()
        .context("render must be a nonempty argv array")?;
    ensure!(
        Path::new(executable).is_absolute(),
        "renderer executable must be absolute"
    );
    ensure!(
        render.iter().all(|arg| !arg.contains('\0')),
        "renderer arguments must not contain NUL"
    );
    ensure!(
        render.iter().all(|arg| arg != "-c"),
        "Clockwork does not admit -c arguments; put the program in a script file"
    );
    Ok(())
}

pub fn validate_subject(subject: &str) -> Result<()> {
    ensure!(!subject.trim().is_empty(), "subject must not be blank");
    ensure!(
        !subject.chars().any(char::is_control),
        "subject must not contain control characters"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = r#"
version = 1
[productions.decisions]
render = ["/private/report", "--daily"]
subject = "Daily decisions"
schedule = { kind = "local-calendar", hour = 9, minute = 0 }
[productions.usage]
render = ["/private/usage"]
schedule = { kind = "interval", seconds = 3600 }
"#;

    #[test]
    fn parses_both_schedules_and_defaults_subject_to_identity() {
        let manifest = Manifest::parse(EXAMPLE).unwrap();
        assert_eq!(manifest.job("usage").unwrap().subject("usage"), "usage");
        assert_eq!(
            manifest.job("decisions").unwrap().schedule,
            Schedule::LocalCalendar { hour: 9, minute: 0 }
        );
        assert_eq!(
            Manifest::parse(&toml::to_string(&manifest).unwrap()).unwrap(),
            manifest
        );
        assert!(Manifest::parse(EMPTY_MANIFEST).unwrap().jobs.is_empty());
    }

    #[test]
    fn rejects_unknown_fields_and_invalid_shapes_before_apply() {
        for text in [
            EXAMPLE.replace("version = 1", "version = 2"),
            EXAMPLE.replace("version = 1", "version = 1\nenabled = true"),
            EXAMPLE.replace("[productions.decisions]", "[productions.Decisions]"),
            EXAMPLE.replace("/private/report", "relative/report"),
            EXAMPLE.replace("hour = 9", "hour = 24"),
            EXAMPLE.replace("seconds = 3600", "seconds = 0"),
            EXAMPLE.replace("seconds = 3600", "seconds = 31536001"),
            EXAMPLE.replace("minute = 0", "minute = 0, timezone = 'UTC'"),
            EXAMPLE.replace("--daily", "-c"),
            EXAMPLE.replace("Daily decisions", ""),
        ] {
            assert!(Manifest::parse(&text).is_err());
        }
        assert!(validate_render(&[]).is_err());
        assert!(validate_id("../escape").is_err());
        assert!(validate_id(&"a".repeat(64)).is_err());
        assert!(validate_subject("Header\r\nOther").is_err());
    }

    #[test]
    fn accepts_legacy_definitions_without_merging_table_names() {
        let current = Manifest::parse(EXAMPLE).unwrap();
        assert_eq!(
            Manifest::parse(&EXAMPLE.replace("productions.", "jobs.")).unwrap(),
            current
        );
        for text in [
            "version = 1\n[productions]\n[jobs]\n".to_owned(),
            format!("{EXAMPLE}\n[jobs]\n"),
        ] {
            assert!(Manifest::parse(&text).is_err());
        }
        // Existing structured consumers and Rust fields retain their names.
        assert!(serde_json::to_value(current).unwrap().get("jobs").is_some());
    }
}
