use std::env;
use std::fmt::Write as _;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use crate::capture::{MAX_INPUT_BYTES_U64, parse_capture, query_terms};
use crate::cli::{Cli, Command, EpisodeCommand};
use crate::error::{AppError, AppResult, Context as _};
use crate::model::{EpisodeListItem, RevisionView, SearchResult};
use crate::projection::{graph, render_graph_human, render_report_markdown, report};
use crate::store::Store;

pub struct CommandOutput {
    pub data: Value,
    pub human: String,
}

#[allow(clippy::too_many_lines)]
pub(crate) fn run(cli: &Cli) -> AppResult<CommandOutput> {
    let database = resolve_database(cli.database.as_deref())?;
    match &cli.command {
        Command::Init => {
            let result = Store::init(&database)?;
            Ok(CommandOutput {
                data: json!({
                    "type": "init",
                    "database": database,
                    "schema_version": result.schema_version,
                    "created": result.created,
                }),
                human: if result.created {
                    format!(
                        "Initialized Geste schema {} at {}",
                        result.schema_version,
                        database.display()
                    )
                } else {
                    format!(
                        "Geste schema {} is current at {}",
                        result.schema_version,
                        database.display()
                    )
                },
            })
        }
        Command::Doctor => {
            let result = Store::doctor(&database)?;
            Ok(CommandOutput {
                data: json!({
                    "type": "doctor",
                    "database": database,
                    "schema_version": result.schema_version,
                    "foreign_keys": result.foreign_keys,
                    "integrity": result.integrity,
                    "permissions": result.permissions,
                }),
                human: format!(
                    "ready: schema {}, foreign keys {}, integrity {}, permissions {} ({})",
                    result.schema_version,
                    result.foreign_keys,
                    result.integrity,
                    result.permissions,
                    database.display()
                ),
            })
        }
        Command::Search(args) => {
            validate_limit(args.limit)?;
            let terms = query_terms(&args.query)?;
            let store = Store::open_read(&database)?;
            let results = store.search(&terms, args.limit)?;
            Ok(CommandOutput {
                data: json!({
                    "type": "search_results",
                    "query_terms": terms,
                    "results": results,
                }),
                human: render_search(&results),
            })
        }
        Command::Episode { command } => match command {
            EpisodeCommand::Create { input } => {
                let (capture, digest) = load_capture(input)?;
                let mut store = Store::open_write(&database)?;
                let revision = store.create_episode(&capture, &digest)?;
                Ok(CommandOutput {
                    data: json!({"type": "episode_created", "episode": revision}),
                    human: format!(
                        "Created {} revision {}",
                        revision.episode, revision.revision
                    ),
                })
            }
            EpisodeCommand::Revise {
                episode,
                input,
                base,
            } => {
                let (capture, digest) = load_capture(input)?;
                let mut store = Store::open_write(&database)?;
                let revision = store.revise_episode(episode, *base, &capture, &digest)?;
                Ok(CommandOutput {
                    data: json!({"type": "episode_revised", "episode": revision}),
                    human: format!(
                        "Appended {} revision {}",
                        revision.episode, revision.revision
                    ),
                })
            }
            EpisodeCommand::List { limit } => {
                validate_limit(*limit)?;
                let store = Store::open_read(&database)?;
                let episodes = store.list(*limit)?;
                Ok(CommandOutput {
                    data: json!({"type": "episode_list", "episodes": episodes}),
                    human: render_list(&episodes),
                })
            }
            EpisodeCommand::Show(args) => {
                let store = Store::open_read(&database)?;
                let revision = store.load_revision(&args.episode, args.at)?;
                Ok(CommandOutput {
                    data: json!({"type": "episode_revision", "episode": revision}),
                    human: render_show(&revision),
                })
            }
        },
        Command::Report(args) => {
            let store = Store::open_read(&database)?;
            let revision = store.load_revision(&args.episode, args.at)?;
            let report = report(revision);
            let human = render_report_markdown(&report);
            Ok(CommandOutput {
                data: serde_json::to_value(report).context(
                    "json_serialization_failed",
                    "unable to serialize episode report",
                )?,
                human,
            })
        }
        Command::Graph(args) => {
            let store = Store::open_read(&database)?;
            let revision = store.load_revision(&args.episode, args.at)?;
            let graph = graph(&revision);
            let human = render_graph_human(&graph);
            Ok(CommandOutput {
                data: serde_json::to_value(graph).context(
                    "json_serialization_failed",
                    "unable to serialize episode graph",
                )?,
                human,
            })
        }
    }
}

pub(crate) fn resolve_database(explicit: Option<&Path>) -> AppResult<PathBuf> {
    if let Some(path) = explicit {
        if path.as_os_str().is_empty() {
            return Err(AppError::usage(
                "database_path_required",
                "--database must not be empty",
            ));
        }
        return Ok(path.to_path_buf());
    }
    if let Some(path) = env::var_os("GESTE_DATABASE").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    let home = env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            AppError::new(
                "home_unavailable",
                "HOME is required when --database and GESTE_DATABASE are not set",
            )
        })?;
    Ok(PathBuf::from(home)
        .join("Library")
        .join("Application Support")
        .join("Geste")
        .join("geste.db"))
}

fn load_capture(path: &Path) -> AppResult<(crate::model::Capture, String)> {
    if path == Path::new("-") {
        let bytes = read_capture_bytes(
            std::io::stdin().lock(),
            "unable to read capture input from stdin",
        )?;
        return parse_capture_bytes(&bytes);
    }
    let metadata = fs::symlink_metadata(path).context(
        "capture_read_failed",
        format!("unable to inspect capture input {}", path.display()),
    )?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(AppError::usage(
            "capture_input_not_regular",
            format!("capture input must be a regular file: {}", path.display()),
        ));
    }
    let file = fs::File::open(path).context(
        "capture_read_failed",
        format!("unable to open capture input {}", path.display()),
    )?;
    let bytes = read_capture_bytes(
        file,
        &format!("unable to read capture input {}", path.display()),
    )?;
    parse_capture_bytes(&bytes)
}

fn read_capture_bytes(reader: impl Read, failure_message: &str) -> AppResult<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_INPUT_BYTES_U64 + 1)
        .read_to_end(&mut bytes)
        .context("capture_read_failed", failure_message)?;
    Ok(bytes)
}

fn parse_capture_bytes(bytes: &[u8]) -> AppResult<(crate::model::Capture, String)> {
    let capture = parse_capture(bytes)?;
    let digest = format!("{:x}", Sha256::digest(bytes));
    Ok((capture, digest))
}

fn validate_limit(limit: usize) -> AppResult<()> {
    if !(1..=1_000).contains(&limit) {
        return Err(AppError::usage(
            "invalid_limit",
            "limit must be from 1 through 1000",
        ));
    }
    Ok(())
}

fn render_search(results: &[SearchResult]) -> String {
    if results.is_empty() {
        return "No matching episodes.".to_owned();
    }
    results
        .iter()
        .map(|result| {
            format!(
                "{}@{} score {} — {}\n  shape: {}\n  matched: {} [{}]",
                result.episode,
                result.revision,
                result.score,
                safe(&result.title),
                safe(&result.shape),
                result.matched_terms.join(", "),
                result.matched_fields.join(", ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_list(episodes: &[EpisodeListItem]) -> String {
    if episodes.is_empty() {
        return "No episodes.".to_owned();
    }
    episodes
        .iter()
        .map(|episode| {
            format!(
                "{}@{} [{}] {} — {}",
                episode.episode,
                episode.revision,
                episode.outcome_status.as_str(),
                safe(&episode.title),
                safe(&episode.shape)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_show(revision: &RevisionView) -> String {
    let capture = &revision.capture;
    format!(
        "{} revision {}\nTitle: {}\nShape: {}\nOutcome: {} — {}\nApplicability: {}\nBasis cutoff: {}\nSources: {}\nGaps: {}",
        revision.episode,
        revision.revision,
        safe(&capture.title),
        safe(&capture.shape),
        capture.outcome.status.as_str(),
        safe(&capture.outcome.summary),
        safe(&capture.applicability),
        safe(&capture.basis_cutoff_at),
        capture.sources.len(),
        capture.gaps.len()
    )
}

fn safe(value: &str) -> String {
    let mut rendered = String::with_capacity(value.len());
    for character in value.chars() {
        if character == '\n' {
            rendered.push(character);
        } else if character.is_control() {
            let _ = write!(rendered, "\\u{{{:x}}}", u32::from(character));
        } else {
            rendered.push(character);
        }
    }
    rendered
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::resolve_database;

    #[test]
    fn explicit_database_is_used_verbatim() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            resolve_database(Some(Path::new("state/geste.db")))?,
            Path::new("state/geste.db")
        );
        Ok(())
    }
}
