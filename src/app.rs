use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, TransactionBehavior};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::cli::{
    Cli, Command, IngestArgs, NodeAddArgs, NodeCommand, NodeDeleteArgs, NodeEditArgs, TreeCommand,
};
use crate::db;
use crate::error::{AppError, AppResult};
use crate::generation::{
    self, ADAPTER_NAME, ADAPTER_VERSION, GENERATED_TREE_SCHEMA_VERSION, GeneratedTree,
    PROMPT_VERSION, ResolutionPolicy,
};
use crate::index;
use crate::model::{
    LibraryStats, MatchReason, MutationOutput, Node, ResultExplanation, SearchOutput, TreeEntry,
    TreeSummary, ValidationSeverity,
};
use crate::model_runner::Runner;
use crate::render::{CommandOutput, color_enabled, render_snippet, render_terminal_text};
use crate::search::{self, Options as SearchOptions};
use crate::tree::{self, NewGenerationRun, NewNode, NodeChanges};
use crate::validate;

const MODEL: &str = "gpt-5.6-terra";
const REASONING_EFFORT: &str = "medium";

/// Resolve the library path using option, environment, then local default.
#[must_use]
pub fn library_path(explicit: Option<&PathBuf>) -> PathBuf {
    explicit.cloned().unwrap_or_else(|| {
        std::env::var_os("ANNALS_LIBRARY")
            .map_or_else(|| PathBuf::from("./annals.db"), PathBuf::from)
    })
}

/// Execute one parsed CLI command.
pub fn run(cli: &Cli, path: &Path) -> AppResult<CommandOutput> {
    match &cli.command {
        Command::Init => initialize(path),
        Command::Stats => stats(path),
        Command::Validate => validate_library(path),
        Command::Backup(arguments) => backup(path, &arguments.output),
        Command::Reindex => reindex(path),
        Command::Ingest(arguments) => ingest_tree(path, arguments, !cli.json),
        Command::Tree(command) => match command {
            TreeCommand::Create(arguments) => create_tree(path, &arguments.text),
            TreeCommand::List => list_trees(path),
            TreeCommand::Show(arguments) => {
                show_tree(path, arguments.root_node_id, arguments.depth)
            }
            TreeCommand::Delete(arguments) => {
                delete_tree(path, arguments.root_node_id, arguments.yes, cli.json)
            }
        },
        Command::Node(command) => match command {
            NodeCommand::Add(arguments) => add_node(path, arguments),
            NodeCommand::Show(arguments) => show_node(path, arguments.node_id),
            NodeCommand::Children(arguments) => children(path, arguments.node_id),
            NodeCommand::Edit(arguments) => edit_node(path, arguments),
            NodeCommand::Move(arguments) => move_node(
                path,
                arguments.node_id,
                arguments.parent,
                arguments.position,
            ),
            NodeCommand::Delete(arguments) => delete_node(path, arguments, cli.json),
        },
        Command::Search(arguments) => search_library(path, arguments, cli.no_color),
    }
}

fn initialize(path: &Path) -> Result<CommandOutput, AppError> {
    let mut connection = db::init(path)?;
    let initialization = (|| {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        index::rebuild_all(&transaction)?;
        transaction.commit()?;
        Ok::<(), AppError>(())
    })();
    if let Err(error) = initialization {
        drop(connection);
        let _ = fs::remove_file(path);
        return Err(error);
    }
    Ok(CommandOutput::new(
        json!({ "library": path.display().to_string() }),
        format!("Initialized library {}", path.display()),
    )
    .mutation())
}

fn stats(path: &Path) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let stats = LibraryStats {
        revision: tree::library_revision(&connection)?,
        root_count: count(
            &connection,
            "SELECT COUNT(*) FROM nodes WHERE parent_id IS NULL",
        )?,
        node_count: count(&connection, "SELECT COUNT(*) FROM nodes")?,
        raw_input_count: count(&connection, "SELECT COUNT(*) FROM raw_inputs")?,
        generation_run_count: count(&connection, "SELECT COUNT(*) FROM generation_runs")?,
        support_link_count: count(&connection, "SELECT COUNT(*) FROM node_support")?,
        indexed_unit_count: count(&connection, "SELECT COUNT(*) FROM search_units")?,
        database_size_bytes: fs::metadata(path).map(|metadata| metadata.len()).map_err(
            |error| {
                AppError::unexpected(
                    "database_metadata_failed",
                    format!("unable to read metadata for {}: {error}", path.display()),
                )
            },
        )?,
        index_current: index::status(&connection)?.is_current(),
    };
    let human = format!(
        "Revision: {}\nRoots: {}\nNodes: {}\nRaw inputs: {}\nGeneration runs: {}\nSupport links: {}\nSearch units: {}\nDatabase size: {} bytes\nIndex current: {}",
        stats.revision,
        stats.root_count,
        stats.node_count,
        stats.raw_input_count,
        stats.generation_run_count,
        stats.support_link_count,
        stats.indexed_unit_count,
        stats.database_size_bytes,
        stats.index_current,
    );
    Ok(CommandOutput::new(to_value(&stats)?, human))
}

fn validate_library(path: &Path) -> Result<CommandOutput, AppError> {
    let connection = db::open_validation(path)?;
    let report = validate::validate(&connection)?;
    let mut text = if report.valid {
        "Library is valid".to_owned()
    } else {
        "Library is invalid".to_owned()
    };
    for issue in &report.issues {
        let severity = match issue.severity {
            ValidationSeverity::Warning => "warning",
            ValidationSeverity::Error => "error",
        };
        let _ = write!(text, "\n{severity} [{}]: {}", issue.code, issue.message);
    }
    if !report.valid {
        return Err(AppError::database("validation_failed", text));
    }
    Ok(CommandOutput::new(to_value(&report)?, text))
}

fn backup(path: &Path, output: &Path) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    db::backup(&connection, output)?;
    Ok(CommandOutput::new(
        json!({ "output": output.display().to_string() }),
        format!("Backed up {} to {}", path.display(), output.display()),
    )
    .mutation())
}

fn reindex(path: &Path) -> Result<CommandOutput, AppError> {
    let mut connection = db::open_write(path)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let stats = index::rebuild_all(&transaction)?;
    transaction.commit()?;
    Ok(CommandOutput::new(
        json!({ "indexed_nodes": stats.nodes, "indexed_units": stats.units }),
        format!("Indexed {} nodes into {} units", stats.nodes, stats.units),
    )
    .mutation())
}

fn ingest_tree(
    path: &Path,
    arguments: &IngestArgs,
    forward_model_progress: bool,
) -> Result<CommandOutput, AppError> {
    ingest_tree_with_runner(path, arguments, &Runner::default(), forward_model_progress)
}

fn ingest_tree_with_runner(
    path: &Path,
    arguments: &IngestArgs,
    runner: &Runner,
    forward_model_progress: bool,
) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    index::require_current(&connection)?;
    drop(connection);
    let input = read_utf8(&arguments.input)?;
    let policy = ResolutionPolicy {
        node_budget: arguments.node_budget,
        max_depth: arguments.max_depth,
        max_children: arguments.max_children,
    };
    let units =
        generation::segment_raw_input(&input).map_err(|error| generation_input_error(&error))?;
    let prompt = generation::build_generation_prompt(&units, &policy)
        .map_err(|error| generation_input_error(&error))?;

    // Inference deliberately happens before the SQLite writer transaction.
    let proposal_text = runner.run(&prompt, forward_model_progress)?;
    let proposal = generation::parse_and_validate_generated_tree(&proposal_text, &units, &policy)
        .map_err(|error| generation_output_error(&error))?;
    persist_generated_tree(path, &input, &units, &policy, &proposal)
}

fn persist_generated_tree(
    path: &Path,
    input: &str,
    units: &[generation::RawUnit],
    policy: &ResolutionPolicy,
    proposal: &GeneratedTree,
) -> Result<CommandOutput, AppError> {
    let accepted_proposal_json = serde_json::to_string(proposal)?;
    let checksum = sha256_hex(input.as_bytes());
    let mut connection = current_write_connection(path)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let input_id = tree::insert_raw_input(&transaction, input, &checksum)?;
    let run_id = tree::insert_generation_run(
        &transaction,
        &NewGenerationRun {
            input_id,
            adapter_name: ADAPTER_NAME.to_owned(),
            adapter_version: ADAPTER_VERSION.to_owned(),
            model: MODEL.to_owned(),
            reasoning_effort: REASONING_EFFORT.to_owned(),
            prompt_version: PROMPT_VERSION.to_owned(),
            output_schema_version: GENERATED_TREE_SCHEMA_VERSION,
            node_budget: policy.node_budget,
            max_depth: policy.max_depth,
            max_children: policy.max_children,
            accepted_proposal_json,
        },
    )?;
    for unit in units {
        tree::insert_input_unit(
            &transaction,
            run_id,
            &unit.id,
            unit.start_byte,
            unit.end_byte,
        )?;
    }

    let mut resolved = HashMap::<&str, i64>::new();
    let mut node_ids = Vec::with_capacity(proposal.nodes.len());
    for generated in &proposal.nodes {
        let node_id = if let Some(parent_ref) = generated.parent_id.as_deref() {
            let parent_id = resolved.get(parent_ref).copied().ok_or_else(|| {
                AppError::invalid(
                    "invalid_generated_tree",
                    format!("generated parent {parent_ref} was not resolved"),
                )
            })?;
            tree::add_node(
                &transaction,
                parent_id,
                &NewNode {
                    text: generated.text.clone(),
                    position: None,
                    generation_run_id: Some(run_id),
                },
            )?
        } else {
            tree::create_root_for_run(&transaction, &generated.text, run_id)?
        };
        resolved.insert(&generated.id, node_id);
        node_ids.push(node_id);
        for unit_id in &generated.support_unit_ids {
            tree::insert_node_support(&transaction, node_id, run_id, unit_id)?;
        }
    }
    let root_id = *node_ids
        .first()
        .ok_or_else(|| AppError::invalid("invalid_generated_tree", "generated tree has no root"))?;
    tree::set_generation_root(&transaction, run_id, root_id)?;
    index::rebuild_all(&transaction)?;
    let revision = tree::bump_library_revision(&transaction)?;
    transaction.commit()?;

    let data = json!({
        "root_node_id": root_id,
        "node_ids": node_ids,
        "input_id": input_id,
        "generation_run_id": run_id,
        "revision": revision,
    });
    Ok(CommandOutput::new(
        data,
        format!(
            "Generated tree {root_id} with {} nodes from {} input units",
            proposal.nodes.len(),
            units.len()
        ),
    )
    .mutation())
}

fn generation_input_error(error: &generation::GenerationError) -> AppError {
    AppError::invalid("invalid_ingestion", error.to_string())
}

fn generation_output_error(error: &generation::GenerationError) -> AppError {
    AppError::invalid("invalid_model_output", error.to_string())
}

fn create_tree(path: &Path, text: &str) -> Result<CommandOutput, AppError> {
    let mut connection = current_write_connection(path)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let node_id = tree::create_root(&transaction, text)?;
    index::rebuild_all(&transaction)?;
    tree::bump_library_revision(&transaction)?;
    transaction.commit()?;
    mutation_output(&[node_id], format!("Created tree {node_id}"))
}

fn list_trees(path: &Path) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let summaries = tree::roots(&connection)?
        .into_iter()
        .map(|root| {
            Ok(TreeSummary {
                root_id: root.id,
                text: root.text,
                node_count: u64::try_from(tree::subtree_count(&connection, root.id)?).map_err(
                    |_| AppError::database("invalid_count", "tree node count is too large"),
                )?,
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    let human = if summaries.is_empty() {
        "No trees".to_owned()
    } else {
        summaries
            .iter()
            .map(|tree| {
                format!(
                    "{}\t{}\t{} node{}",
                    tree.root_id,
                    render_terminal_text(&tree.text, false),
                    tree.node_count,
                    if tree.node_count == 1 { "" } else { "s" }
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    Ok(CommandOutput::new(to_value(&summaries)?, human))
}

fn show_tree(
    path: &Path,
    root_node_id: i64,
    max_depth: Option<usize>,
) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let rows = tree::subtree_with_depth(&connection, root_node_id, max_depth)?;
    let entries = rows
        .iter()
        .map(|(node, depth)| {
            Ok(TreeEntry {
                node: node.clone(),
                depth: u64::try_from(*depth)
                    .map_err(|_| AppError::database("invalid_depth", "tree depth is too large"))?,
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    let human = rows
        .iter()
        .map(|(node, depth)| {
            format!(
                "{}{} [{}]",
                "  ".repeat(*depth),
                render_terminal_text(&node.text, false),
                node.id
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(CommandOutput::new(to_value(&entries)?, human))
}

fn delete_tree(
    path: &Path,
    root_node_id: i64,
    yes: bool,
    json_mode: bool,
) -> Result<CommandOutput, AppError> {
    let mut connection = current_write_connection(path)?;
    let root = tree::get_node(&connection, root_node_id)?;
    if root.parent_id.is_some() {
        return Err(AppError::not_found(
            "tree_not_found",
            format!("node {root_node_id} is not a tree root"),
        ));
    }
    let preview_ids = tree::subtree_ids(&connection, root_node_id)?;
    confirm(
        &format!("tree {root_node_id} ({} nodes)", preview_ids.len()),
        yes,
        json_mode,
    )?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let affected_ids = tree::subtree_ids(&transaction, root_node_id)?;
    if affected_ids != preview_ids && !yes {
        return Err(AppError::conflict(
            "subtree_changed",
            "the tree changed while deletion was being confirmed",
        ));
    }
    tree::delete_tree(&transaction, root_node_id)?;
    index::rebuild_all(&transaction)?;
    tree::bump_library_revision(&transaction)?;
    transaction.commit()?;
    mutation_output(
        &affected_ids,
        format!("Deleted tree {root_node_id} ({} nodes)", affected_ids.len()),
    )
}

fn add_node(path: &Path, arguments: &NodeAddArgs) -> Result<CommandOutput, AppError> {
    let mut connection = current_write_connection(path)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let node_id = tree::add_node(
        &transaction,
        arguments.parent,
        &NewNode {
            text: arguments.text.clone(),
            position: arguments.position,
            generation_run_id: None,
        },
    )?;
    index::rebuild_all(&transaction)?;
    tree::bump_library_revision(&transaction)?;
    transaction.commit()?;
    mutation_output(&[node_id], format!("Created node {node_id}"))
}

fn show_node(path: &Path, node_id: i64) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let node = tree::get_node(&connection, node_id)?;
    Ok(CommandOutput::new(to_value(&node)?, render_node(&node)))
}

fn children(path: &Path, node_id: i64) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let nodes = tree::children(&connection, node_id)?;
    let human = if nodes.is_empty() {
        "No children".to_owned()
    } else {
        nodes
            .iter()
            .map(|node| format!("{}\t{}", node.id, render_terminal_text(&node.text, false)))
            .collect::<Vec<_>>()
            .join("\n")
    };
    Ok(CommandOutput::new(to_value(&nodes)?, human))
}

fn edit_node(path: &Path, arguments: &NodeEditArgs) -> Result<CommandOutput, AppError> {
    let mut connection = current_write_connection(path)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tree::edit_node(
        &transaction,
        arguments.node_id,
        &NodeChanges {
            text: Some(arguments.text.clone()),
        },
    )?;
    index::rebuild_all(&transaction)?;
    tree::bump_library_revision(&transaction)?;
    transaction.commit()?;
    mutation_output(
        &[arguments.node_id],
        format!("Updated node {}", arguments.node_id),
    )
}

fn move_node(
    path: &Path,
    node_id: i64,
    parent_id: i64,
    position: Option<usize>,
) -> Result<CommandOutput, AppError> {
    let mut connection = current_write_connection(path)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let affected_ids = tree::subtree_ids(&transaction, node_id)?;
    tree::move_node(&transaction, node_id, parent_id, position)?;
    index::rebuild_all(&transaction)?;
    tree::bump_library_revision(&transaction)?;
    transaction.commit()?;
    mutation_output(
        &affected_ids,
        format!("Moved node {node_id} beneath node {parent_id}"),
    )
}

fn delete_node(
    path: &Path,
    arguments: &NodeDeleteArgs,
    json_mode: bool,
) -> Result<CommandOutput, AppError> {
    let mut connection = current_write_connection(path)?;
    let node = tree::get_node(&connection, arguments.node_id)?;
    if node.parent_id.is_none() {
        return Err(AppError::conflict(
            "root_delete_not_allowed",
            "root nodes must be deleted with `tree delete`",
        ));
    }
    let preview_ids = tree::subtree_ids(&connection, arguments.node_id)?;
    if preview_ids.len() > 1 {
        if !arguments.recursive {
            return Err(AppError::conflict(
                "recursive_delete_required",
                format!(
                    "node {} has descendants; use --recursive",
                    arguments.node_id
                ),
            ));
        }
        confirm(
            &format!(
                "subtree {} ({} nodes)",
                arguments.node_id,
                preview_ids.len()
            ),
            arguments.yes,
            json_mode,
        )?;
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let affected_ids = tree::subtree_ids(&transaction, arguments.node_id)?;
    if affected_ids != preview_ids && !arguments.yes {
        return Err(AppError::conflict(
            "subtree_changed",
            "the subtree changed while deletion was being confirmed",
        ));
    }
    tree::delete_node(&transaction, arguments.node_id)?;
    index::rebuild_all(&transaction)?;
    tree::bump_library_revision(&transaction)?;
    transaction.commit()?;
    mutation_output(
        &affected_ids,
        format!(
            "Deleted node {} ({} nodes)",
            arguments.node_id,
            affected_ids.len()
        ),
    )
}

fn search_library(
    path: &Path,
    arguments: &crate::cli::SearchArgs,
    no_color: bool,
) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let output = search::search(
        &connection,
        &arguments.query,
        SearchOptions {
            within: arguments.within,
            limit: arguments.limit,
            explain: arguments.explain,
        },
    )?;
    let human = render_search(&output, color_enabled(no_color), arguments.explain);
    let mut json_output = output.clone();
    clean_search_snippets(&mut json_output);
    Ok(CommandOutput::new(to_value(&json_output)?, human))
}

fn current_write_connection(path: &Path) -> Result<Connection, AppError> {
    let connection = db::open_write(path)?;
    index::require_current(&connection)?;
    Ok(connection)
}

fn count(connection: &Connection, sql: &str) -> Result<u64, AppError> {
    let value = connection.query_row(sql, [], |row| row.get::<_, i64>(0))?;
    u64::try_from(value)
        .map_err(|_| AppError::database("invalid_count", "database returned a negative count"))
}

fn read_utf8(path: &Path) -> Result<String, AppError> {
    let bytes = if path == Path::new("-") {
        let mut bytes = Vec::new();
        io::stdin().read_to_end(&mut bytes).map_err(|error| {
            AppError::unexpected(
                "ingestion_read_failed",
                format!("unable to read ingestion input from standard input: {error}"),
            )
        })?;
        bytes
    } else {
        fs::read(path).map_err(|error| {
            AppError::unexpected(
                "ingestion_read_failed",
                format!("unable to read ingestion input {}: {error}", path.display()),
            )
        })?
    };
    String::from_utf8(bytes).map_err(|_| {
        AppError::invalid(
            "ingestion_not_utf8",
            "ingestion input must be valid UTF-8 text",
        )
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn confirm(description: &str, yes: bool, json_mode: bool) -> Result<(), AppError> {
    if yes {
        return Ok(());
    }
    if json_mode || !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        return Err(AppError::conflict(
            "confirmation_required",
            format!("deleting {description} requires --yes in non-interactive use"),
        ));
    }
    let mut stderr = io::stderr().lock();
    write!(stderr, "Delete {description}? [y/N] ")?;
    stderr.flush()?;
    drop(stderr);
    let mut response = String::new();
    io::stdin().read_line(&mut response)?;
    if matches!(response.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        Ok(())
    } else {
        Err(AppError::conflict(
            "confirmation_declined",
            "deletion was not confirmed",
        ))
    }
}

fn mutation_output(node_ids: &[i64], human: String) -> Result<CommandOutput, AppError> {
    Ok(CommandOutput::new(
        to_value(&MutationOutput {
            node_ids: node_ids.to_vec(),
        })?,
        human,
    )
    .mutation())
}

fn to_value<T: Serialize>(value: &T) -> Result<Value, AppError> {
    serde_json::to_value(value).map_err(AppError::from)
}

fn render_node(node: &Node) -> String {
    format!(
        "Node {}\nText: {}\nParent: {}\nPosition: {}\nGeneration run: {}\nCreated: {}\nUpdated: {}",
        node.id,
        render_terminal_text(&node.text, false),
        node.parent_id
            .map_or_else(|| "root".to_owned(), |id| id.to_string()),
        node.position,
        node.generation_run_id
            .map_or_else(|| "manual".to_owned(), |id| id.to_string()),
        node.created_at,
        node.updated_at,
    )
}

fn render_search(output: &SearchOutput, color: bool, explain: bool) -> String {
    if output.results.is_empty() {
        return "No matches".to_owned();
    }
    let mut lines = Vec::new();
    for result in &output.results {
        lines.push(format!(
            "{}. {} [{}]",
            result.rank,
            render_terminal_text(&result.text, false),
            result.node_id
        ));
        lines.push(format!(
            "   Path: {}",
            result
                .breadcrumb
                .iter()
                .map(|item| render_terminal_text(&item.text, false))
                .collect::<Vec<_>>()
                .join(" / ")
        ));
        if let Some(snippet) = &result.snippet {
            lines.push(format!("   {}", render_snippet(snippet, color)));
        }
        lines.push(format!(
            "   Match: {}",
            render_match_reasons(&result.match_reasons)
        ));
        if explain && let Some(explanation) = &result.explanation {
            lines.push(format!(
                "   Explain: {}",
                render_result_explanation(explanation)
            ));
        }
    }
    if explain && let Some(explanation) = &output.explanation {
        lines.push(format!(
            "Query explain: exact={} and={} or={} prefix={} returned={}",
            explanation.exact_candidates,
            explanation.after_and_candidates,
            explanation.or_fallback_used,
            explanation.prefix_fallback_used,
            explanation.returned_results,
        ));
    }
    lines.join("\n")
}

fn render_result_explanation(explanation: &ResultExplanation) -> String {
    format!(
        "class={} direct={:.3} pass={} lexical_rank={} branch={} position={}",
        explanation.exact_class,
        explanation.direct_score,
        explanation.retrieval_pass.as_deref().unwrap_or("-"),
        display_optional(explanation.lexical_rank),
        explanation.branch_key,
        display_optional(explanation.final_position),
    )
}

fn display_optional<T: std::fmt::Display>(value: Option<T>) -> String {
    value.map_or_else(|| "-".to_owned(), |value| value.to_string())
}

fn render_match_reasons(reasons: &[MatchReason]) -> String {
    reasons
        .iter()
        .map(|reason| match reason {
            MatchReason::ExactId => "exact_id",
            MatchReason::ExactPath => "exact_path",
            MatchReason::ExactText => "exact_text",
            MatchReason::Phrase => "phrase",
            MatchReason::Lexical => "lexical",
            MatchReason::Prefix => "prefix",
            MatchReason::Typo => "typo",
            MatchReason::DescendantSupport => "descendant_support",
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn clean_search_snippets(output: &mut SearchOutput) {
    for result in &mut output.results {
        if let Some(snippet) = &mut result.snippet {
            *snippet = render_snippet(snippet, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::time::Duration;

    use rusqlite::Connection;
    use tempfile::tempdir;

    use super::{IngestArgs, ingest_tree_with_runner, sha256_hex};
    use crate::{db, generation, index, model_runner::Runner, validate};

    #[test]
    fn hashes_raw_input_as_lowercase_sha256() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn fake_model_ingestion_persists_grounding_and_provenance_atomically()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let library = directory.path().join("annals.db");
        initialize_library(&library)?;
        let input = directory.path().join("input.txt");
        fs::write(&input, "alpha evidence")?;
        let runner = fake_runner(
            directory.path(),
            r#"{"schema_version":1,"nodes":[{"id":"n0","parent_id":null,"text":"Alpha","support_unit_ids":["u000000"]}]}"#,
        )?;

        let output = ingest_tree_with_runner(
            &library,
            &IngestArgs {
                input,
                node_budget: 32,
                max_depth: 6,
                max_children: 6,
            },
            &runner,
            false,
        )?;

        assert_eq!(output.data["root_node_id"], 1);
        let connection = db::open_read(&library)?;
        assert_eq!(
            connection.query_row("SELECT text FROM raw_inputs", [], |row| row
                .get::<_, String>(0))?,
            "alpha evidence"
        );
        assert_eq!(
            connection.query_row("SELECT model FROM generation_runs", [], |row| row
                .get::<_, String>(0))?,
            "gpt-5.6-terra"
        );
        assert_eq!(
            connection.query_row("SELECT COUNT(*) FROM node_support", [], |row| row
                .get::<_, i64>(0))?,
            1
        );
        assert!(index::status(&connection)?.is_current());
        Ok(())
    }

    #[test]
    fn invalid_model_tree_leaves_no_canonical_rows() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let library = directory.path().join("annals.db");
        initialize_library(&library)?;
        let input = directory.path().join("input.txt");
        fs::write(&input, "alpha evidence")?;
        let runner = fake_runner(
            directory.path(),
            r#"{"schema_version":1,"nodes":[{"id":"n0","parent_id":null,"text":"Unsupported","support_unit_ids":[]}]}"#,
        )?;

        let result = ingest_tree_with_runner(
            &library,
            &IngestArgs {
                input,
                node_budget: 32,
                max_depth: 6,
                max_children: 6,
            },
            &runner,
            false,
        );
        let Err(error) = result else {
            return Err("an unsupported leaf was accepted".into());
        };

        assert_eq!(error.code(), "invalid_model_output");
        let connection = db::open_read(&library)?;
        assert_eq!(row_count(&connection, "nodes")?, 0);
        assert_eq!(row_count(&connection, "raw_inputs")?, 0);
        assert_eq!(row_count(&connection, "generation_runs")?, 0);
        Ok(())
    }

    #[test]
    fn validation_recomputes_the_recorded_raw_window_adapter()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let library = directory.path().join("annals.db");
        initialize_library(&library)?;
        let input = directory.path().join("input.txt");
        fs::write(&input, "a".repeat(generation::RAW_WINDOW_BYTES + 20))?;
        let runner = fake_runner(
            directory.path(),
            r#"{"schema_version":1,"nodes":[{"id":"n0","parent_id":null,"text":"Alpha","support_unit_ids":["u000000"]}]}"#,
        )?;
        ingest_tree_with_runner(
            &library,
            &IngestArgs {
                input,
                node_budget: 32,
                max_depth: 6,
                max_children: 6,
            },
            &runner,
            false,
        )?;

        let connection = Connection::open(&library)?;
        connection.execute(
            "UPDATE input_units SET end_byte = ?1 WHERE unit_id = 'u000000'",
            [i64::try_from(generation::RAW_WINDOW_BYTES - 1)?],
        )?;
        connection.execute(
            "UPDATE input_units SET start_byte = ?1 WHERE unit_id = 'u000001'",
            [i64::try_from(generation::RAW_WINDOW_BYTES - 1)?],
        )?;

        let report = validate::validate(&connection)?;
        assert!(!report.valid);
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.code == "adapter_output_mismatch")
        );
        Ok(())
    }

    fn initialize_library(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let mut connection = db::init(path)?;
        let transaction = connection.transaction()?;
        index::rebuild_all(&transaction)?;
        transaction.commit()?;
        Ok(())
    }

    fn fake_runner(directory: &Path, output: &str) -> Result<Runner, Box<dyn std::error::Error>> {
        let script = directory.join(format!("fake-{}.sh", directory.read_dir()?.count()));
        fs::write(
            &script,
            format!("#!/bin/sh\nset -eu\ncat >/dev/null\nprintf '%s' '{output}'\n"),
        )?;
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755))?;
        Ok(Runner::new(
            script,
            Duration::from_secs(1),
            1024 * 1024,
            1024,
        ))
    }

    fn row_count(connection: &Connection, table: &str) -> rusqlite::Result<i64> {
        connection.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
    }
}
