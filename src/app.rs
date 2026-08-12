use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, TransactionBehavior};
use serde::Serialize;
use serde_json::{Value, json};

use crate::cli::{
    Cli, Command, IngestArgs, NodeAddArgs, NodeCommand, NodeDeleteArgs, NodeEditArgs, TreeCommand,
};
use crate::db;
use crate::error::{AppError, AppResult};
use crate::index;
use crate::ingest;
use crate::model::{
    LibraryStats, MatchReason, MutationOutput, Node, ResultExplanation, SearchOutput, TreeEntry,
    TreeSummary, ValidationSeverity,
};
use crate::render::{CommandOutput, color_enabled, render_snippet, render_terminal_text};
use crate::search::{self, Options as SearchOptions};
use crate::tree::{self, NewNode, NodeChanges, SourceFields};
use crate::validate;

/// Resolve the library path using the documented option/environment/default order.
#[must_use]
pub fn library_path(explicit: Option<&PathBuf>) -> PathBuf {
    explicit.cloned().unwrap_or_else(|| {
        std::env::var_os("ANNALS_LIBRARY")
            .map_or_else(|| PathBuf::from("./annals.db"), PathBuf::from)
    })
}

/// Execute one parsed CLI command.
#[allow(clippy::too_many_lines)]
pub fn run(cli: &Cli, path: &Path) -> AppResult<CommandOutput> {
    match &cli.command {
        Command::Init => initialize(path),
        Command::Stats => stats(path),
        Command::Validate => validate_library(path),
        Command::Backup(arguments) => backup(path, &arguments.output),
        Command::Reindex => reindex(path),
        Command::Ingest(arguments) => ingest_tree(path, arguments),
        Command::Tree(command) => match command {
            TreeCommand::Create(arguments) => {
                let body = read_body(arguments.body.as_ref(), arguments.body_file.as_deref())?
                    .unwrap_or_default();
                create_tree(path, &arguments.title, &body)
            }
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
        let _cleanup_result = fs::remove_file(path);
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
    let revision = tree::library_revision(&connection)?;
    let root_count = count(
        &connection,
        "SELECT COUNT(*) FROM nodes WHERE parent_id IS NULL",
    )?;
    let node_count = count(&connection, "SELECT COUNT(*) FROM nodes")?;
    let source_count = count(&connection, "SELECT COUNT(*) FROM sources")?;
    let indexed_unit_count = count(&connection, "SELECT COUNT(*) FROM search_units")?;
    let database_size_bytes =
        fs::metadata(path)
            .map(|metadata| metadata.len())
            .map_err(|error| {
                AppError::unexpected(
                    "database_metadata_failed",
                    format!("unable to read metadata for {}: {error}", path.display()),
                )
            })?;
    let index_current = index::status(&connection)?.is_current();
    let stats = LibraryStats {
        revision,
        root_count,
        node_count,
        source_count,
        indexed_unit_count,
        database_size_bytes,
        index_current,
    };
    let human = format!(
        "Revision: {}\nRoots: {}\nNodes: {}\nSources: {}\nSearch units: {}\nDatabase size: {} bytes\nIndex current: {}",
        stats.revision,
        stats.root_count,
        stats.node_count,
        stats.source_count,
        stats.indexed_unit_count,
        stats.database_size_bytes,
        stats.index_current
    );
    Ok(CommandOutput::new(to_value(&stats)?, human))
}

#[allow(clippy::format_push_string)]
fn validate_library(path: &Path) -> Result<CommandOutput, AppError> {
    let connection = db::open_validation(path)?;
    let report = validate::validate(&connection)?;
    let mut report_text = if report.valid {
        "Library is valid".to_owned()
    } else {
        "Library is invalid".to_owned()
    };
    for issue in &report.issues {
        let severity = match issue.severity {
            ValidationSeverity::Warning => "warning",
            ValidationSeverity::Error => "error",
        };
        report_text.push_str(&format!("\n{severity} [{}]: {}", issue.code, issue.message));
    }
    if !report.valid {
        return Err(AppError::database("validation_failed", report_text));
    }
    let warnings = report
        .issues
        .iter()
        .filter(|issue| issue.severity == ValidationSeverity::Warning)
        .map(|issue| format!("warning [{}]: {}", issue.code, issue.message))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(CommandOutput::new(to_value(&report)?, "Library is valid").with_diagnostics(warnings))
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
    let data = json!({ "indexed_nodes": stats.nodes, "indexed_units": stats.units });
    Ok(CommandOutput::new(
        data,
        format!("Indexed {} nodes into {} units", stats.nodes, stats.units),
    )
    .mutation())
}

fn ingest_tree(path: &Path, arguments: &IngestArgs) -> Result<CommandOutput, AppError> {
    let document = read_ingestion(&arguments.input)?;
    let plan = ingest::parse_plan(&document)?;
    let mut connection = current_write_connection(path)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let output = ingest::apply(&transaction, plan)?;
    transaction.commit()?;
    let human = format!(
        "Applied ingestion revision {} -> {} ({} created, {} replaced, {} moved, {} deleted)",
        output.previous_revision,
        output.new_revision,
        output.created.len(),
        output.replaced_node_ids.len(),
        output.moved.len(),
        output.deleted_node_ids.len(),
    );
    Ok(CommandOutput::new(to_value(&output)?, human).mutation())
}

fn create_tree(path: &Path, title: &str, body: &str) -> Result<CommandOutput, AppError> {
    let mut connection = current_write_connection(path)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let node_id = tree::create_root(&transaction, title, body)?;
    index::rebuild_node(&transaction, node_id)?;
    tree::bump_library_revision(&transaction)?;
    transaction.commit()?;
    mutation_output(&[node_id], format!("Created tree {node_id}"))
}

fn list_trees(path: &Path) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let mut summaries = Vec::new();
    for root in tree::roots(&connection)? {
        let node_count = u64::try_from(tree::subtree_count(&connection, root.id)?)
            .map_err(|_| AppError::database("invalid_count", "tree node count is too large"))?;
        summaries.push(TreeSummary {
            root_id: root.id,
            title: root.title,
            node_count,
        });
    }
    let human = if summaries.is_empty() {
        "No trees".to_owned()
    } else {
        summaries
            .iter()
            .map(|tree| {
                format!(
                    "{}\t{}\t{} node{}",
                    tree.root_id,
                    render_terminal_text(&tree.title, false),
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
                "{}{} {} [{}]",
                "  ".repeat(*depth),
                node.kind,
                render_terminal_text(&node.title, false),
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
    let preview_count = preview_ids.len();
    let preview_path = tree::node_path(&connection, root_node_id)?;
    confirm(
        &format!("tree {root_node_id} at {preview_path:?} ({preview_count} nodes)"),
        yes,
        json_mode,
    )?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let affected_ids = tree::subtree_ids(&transaction, root_node_id)?;
    let count = affected_ids.len();
    if affected_ids != preview_ids && !yes {
        return Err(AppError::conflict(
            "subtree_changed",
            "the tree changed while deletion was being confirmed; inspect it and retry",
        ));
    }
    tree::delete_tree(&transaction, root_node_id)?;
    tree::bump_library_revision(&transaction)?;
    transaction.commit()?;
    mutation_output(
        &affected_ids,
        format!("Deleted tree {root_node_id} ({count} nodes)"),
    )
}

fn add_node(path: &Path, arguments: &NodeAddArgs) -> Result<CommandOutput, AppError> {
    let body =
        read_body(arguments.body.as_ref(), arguments.body_file.as_deref())?.unwrap_or_default();
    let source = SourceFields {
        locator: arguments.locator.clone(),
        media_type: arguments.media_type.clone(),
        checksum: arguments.checksum.clone(),
        captured_at: arguments.captured_at.clone(),
    };
    let new_node = NewNode {
        kind: arguments.kind,
        title: arguments.title.clone(),
        body,
        position: arguments.position,
        source,
    };
    let mut connection = current_write_connection(path)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let node_id = tree::add_node(&transaction, arguments.parent, &new_node)?;
    index::rebuild_node(&transaction, node_id)?;
    tree::bump_library_revision(&transaction)?;
    transaction.commit()?;
    mutation_output(
        &[node_id],
        format!("Created {} node {node_id}", arguments.kind),
    )
}

fn show_node(path: &Path, node_id: i64) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let node = tree::get_node(&connection, node_id)?;
    let human = render_node(&node);
    Ok(CommandOutput::new(to_value(&node)?, human))
}

fn children(path: &Path, node_id: i64) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let nodes = tree::children(&connection, node_id)?;
    let human = if nodes.is_empty() {
        "No children".to_owned()
    } else {
        nodes
            .iter()
            .map(|node| {
                format!(
                    "{}\t{}\t{}",
                    node.id,
                    node.kind,
                    render_terminal_text(&node.title, false)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    Ok(CommandOutput::new(to_value(&nodes)?, human))
}

fn edit_node(path: &Path, arguments: &NodeEditArgs) -> Result<CommandOutput, AppError> {
    if !arguments.has_changes() {
        return Err(AppError::invalid(
            "no_changes",
            "node edit requires at least one changed field",
        ));
    }
    let body = if arguments.clear_body {
        Some(String::new())
    } else {
        read_body(arguments.body.as_ref(), arguments.body_file.as_deref())?
    };
    let changes = NodeChanges {
        kind: arguments.kind,
        title: arguments.title.clone(),
        body,
        source: SourceFields {
            locator: arguments.locator.clone(),
            media_type: arguments.media_type.clone(),
            checksum: arguments.checksum.clone(),
            captured_at: arguments.captured_at.clone(),
        },
    };
    let mut connection = current_write_connection(path)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let outcome = tree::edit_node(&transaction, arguments.node_id, &changes)?;
    let affected_ids = if outcome.title_changed {
        index::rebuild_subtree(&transaction, arguments.node_id)?;
        tree::subtree_ids(&transaction, arguments.node_id)?
    } else {
        index::rebuild_node(&transaction, arguments.node_id)?;
        vec![arguments.node_id]
    };
    tree::bump_library_revision(&transaction)?;
    transaction.commit()?;
    mutation_output(&affected_ids, format!("Updated node {}", arguments.node_id))
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
    index::rebuild_subtree(&transaction, node_id)?;
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
    let count = preview_ids.len();
    let preview_path = tree::node_path(&connection, arguments.node_id)?;
    if count > 1 {
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
                "subtree {} at {preview_path:?} ({count} nodes)",
                arguments.node_id
            ),
            arguments.yes,
            json_mode,
        )?;
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let affected_ids = tree::subtree_ids(&transaction, arguments.node_id)?;
    let count = affected_ids.len();
    if affected_ids != preview_ids && !arguments.yes {
        return Err(AppError::conflict(
            "subtree_changed",
            "the subtree changed while deletion was being confirmed; inspect it and retry",
        ));
    }
    if count > 1 && !arguments.recursive {
        return Err(AppError::conflict(
            "recursive_delete_required",
            format!(
                "node {} has descendants; use --recursive",
                arguments.node_id
            ),
        ));
    }
    tree::delete_node(&transaction, arguments.node_id)?;
    tree::bump_library_revision(&transaction)?;
    transaction.commit()?;
    mutation_output(
        &affected_ids,
        format!("Deleted node {} ({count} nodes)", arguments.node_id),
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
            kind: arguments.kind,
            detail: arguments.detail,
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

fn read_body(inline: Option<&String>, file: Option<&Path>) -> Result<Option<String>, AppError> {
    if let Some(body) = inline {
        return Ok(Some(body.clone()));
    }
    let Some(path) = file else {
        return Ok(None);
    };
    let bytes = if path == Path::new("-") {
        let mut bytes = Vec::new();
        io::stdin().read_to_end(&mut bytes).map_err(|error| {
            AppError::unexpected(
                "body_read_failed",
                format!("unable to read the body from standard input: {error}"),
            )
        })?;
        bytes
    } else {
        fs::read(path).map_err(|error| {
            AppError::unexpected(
                "body_read_failed",
                format!("unable to read body file {}: {error}", path.display()),
            )
        })?
    };
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| AppError::invalid("body_not_utf8", "body input must be valid UTF-8 text"))
}

fn read_ingestion(path: &Path) -> Result<String, AppError> {
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
            "invalid_ingestion",
            "ingestion input must be valid UTF-8 JSON",
        )
    })
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
    write!(stderr, "Delete {description}? [y/N] ").map_err(AppError::from)?;
    stderr.flush().map_err(AppError::from)?;
    drop(stderr);
    let mut response = String::new();
    io::stdin()
        .read_line(&mut response)
        .map_err(AppError::from)?;
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
    let data = MutationOutput {
        node_ids: node_ids.to_vec(),
    };
    Ok(CommandOutput::new(to_value(&data)?, human).mutation())
}

fn to_value<T: Serialize>(value: &T) -> Result<Value, AppError> {
    serde_json::to_value(value).map_err(AppError::from)
}

fn render_node(node: &Node) -> String {
    let mut lines = vec![
        format!("{} {}", node.kind, node.id),
        format!("Title: {}", render_terminal_text(&node.title, false)),
        format!(
            "Parent: {}",
            node.parent_id
                .map_or_else(|| "root".to_owned(), |id| id.to_string())
        ),
        format!("Position: {}", node.position),
        format!("Created: {}", node.created_at),
        format!("Updated: {}", node.updated_at),
        "Body:".to_owned(),
        render_terminal_text(&node.body, true),
    ];
    if let Some(source) = &node.source {
        lines.push("Source metadata:".to_owned());
        lines.push(format!(
            "  Locator: {}",
            render_terminal_text(source.locator.as_deref().unwrap_or("-"), false)
        ));
        lines.push(format!(
            "  Media type: {}",
            render_terminal_text(source.media_type.as_deref().unwrap_or("-"), false)
        ));
        lines.push(format!(
            "  Checksum: {}",
            render_terminal_text(source.checksum.as_deref().unwrap_or("-"), false)
        ));
        lines.push(format!(
            "  Captured at: {}",
            render_terminal_text(source.captured_at.as_deref().unwrap_or("-"), false)
        ));
    }
    lines.join("\n")
}

fn render_search(output: &SearchOutput, color: bool, explain: bool) -> String {
    if output.results.is_empty() {
        let mut rendered = "No matches".to_owned();
        if explain && let Some(explanation) = &output.explanation {
            rendered.push('\n');
            rendered.push_str(&render_search_explanation(explanation));
        }
        return rendered;
    }
    let mut groups = Vec::new();
    for result in &output.results {
        let mut lines = vec![format!(
            "{}. {} {} — {}",
            result.rank,
            result.kind,
            result.node_id,
            render_terminal_text(&result.title, false)
        )];
        lines.push(
            result
                .breadcrumb
                .iter()
                .map(|item| render_terminal_text(&item.title, false))
                .collect::<Vec<_>>()
                .join(" > "),
        );
        if let Some(snippet) = &result.snippet {
            lines.push(render_snippet(snippet, color));
        }
        if explain {
            lines.push(format!(
                "matches: {}",
                render_match_reasons(&result.match_reasons)
            ));
            if let Some(explanation) = &result.explanation {
                lines.push(format!(
                    "explain: {}",
                    render_result_explanation(explanation)
                ));
            }
        }
        for related in &result.related_hits {
            lines.push(format!(
                "  related: {} {} — {}",
                related.kind,
                related.node_id,
                render_terminal_text(&related.title, false)
            ));
            if let Some(snippet) = &related.snippet {
                lines.push(format!("    {}", render_snippet(snippet, color)));
            }
            if explain {
                lines.push(format!(
                    "    matches: {}",
                    render_match_reasons(&related.match_reasons)
                ));
                if let Some(explanation) = &related.explanation {
                    lines.push(format!(
                        "    explain: {}",
                        render_result_explanation(explanation)
                    ));
                }
            }
        }
        groups.push(lines.join("\n"));
    }
    if explain && let Some(explanation) = &output.explanation {
        groups.push(render_search_explanation(explanation));
    }
    groups.join("\n\n")
}

fn render_search_explanation(explanation: &crate::model::SearchExplanation) -> String {
    format!(
        "search explain: exact_candidates={} after_and={} or_fallback={} after_or={} \
         prefix_fallback={} after_prefix={} groups={} returned={}",
        explanation.exact_candidates,
        explanation.after_and_candidates,
        explanation.or_fallback_used,
        explanation.after_or_candidates,
        explanation.prefix_fallback_used,
        explanation.after_prefix_candidates,
        explanation.groups_after_collapse,
        explanation.returned_results
    )
}

fn render_result_explanation(explanation: &ResultExplanation) -> String {
    format!(
        "unit={} bm25={} lexical_rank={} pass={} exact={} direct={:.6} support={:.6} \
         support_source={} chain_group={} grouping={} branch={} diversity={} final_position={}",
        display_optional(explanation.primary_unit_id),
        explanation
            .raw_bm25
            .map_or_else(|| "-".to_owned(), |value| format!("{value:.6}")),
        display_optional(explanation.lexical_rank),
        explanation.retrieval_pass.as_deref().unwrap_or("-"),
        explanation.exact_class,
        explanation.direct_score,
        explanation.support_score,
        display_optional(explanation.support_source_node_id),
        explanation.chain_group_node_id,
        explanation.grouping_reason,
        explanation.branch_key,
        explanation.diversity_reason,
        display_optional(explanation.final_position)
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
            MatchReason::ExactTitle => "exact_title",
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
        for related in &mut result.related_hits {
            if let Some(snippet) = &mut related.snippet {
                *snippet = render_snippet(snippet, false);
            }
        }
    }
}
