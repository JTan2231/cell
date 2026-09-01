use std::error::Error as StdError;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};
use conversations::{
    AppServerClient, ArchiveScope, ClientConfig, CompletedFileChange, Conversation, ListOptions,
    Role, SearchHit, StderrPolicy, ThreadSummary, TurnActivity, TurnRef,
};
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(
    name = "conversations",
    version,
    about = "Explore local Codex conversation history through App Server"
)]
struct Cli {
    /// Codex CLI executable used to launch App Server.
    #[arg(
        long,
        global = true,
        env = "CONVERSATIONS_CODEX",
        default_value = "codex"
    )]
    codex: PathBuf,

    /// Stable identity included in exported conversation references.
    #[arg(long, global = true, env = "CONVERSATIONS_HOST_ID")]
    host_id: Option<String>,

    /// Select whether App Server diagnostics reach this process's stderr.
    #[arg(long, global = true, value_enum, default_value = "inherit")]
    app_server_stderr: CliStderrPolicy,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Check Codex/App Server connectivity and version compatibility.
    Doctor(OutputArgs),
    /// List stored tasks without loading message content.
    List(ListArgs),
    /// Show normalized user and assistant messages for one task.
    Show(ShowArgs),
    /// Show content-free activity metadata for one completed turn.
    Activity(ActivityArgs),
    /// Search task titles and normalized message text.
    Search(SearchArgs),
    /// Materialize a deduplicated user/assistant-only conversation corpus.
    Export(ExportArgs),
    /// Explicitly allow App Server to scan logs and repair metadata.
    Refresh(OutputArgs),
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
enum CliArchive {
    Active,
    Archived,
    #[default]
    All,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
enum CliStderrPolicy {
    #[default]
    Inherit,
    Suppress,
}

impl From<CliStderrPolicy> for StderrPolicy {
    fn from(value: CliStderrPolicy) -> Self {
        match value {
            CliStderrPolicy::Inherit => Self::Inherit,
            CliStderrPolicy::Suppress => Self::Suppress,
        }
    }
}

impl From<CliArchive> for ArchiveScope {
    fn from(value: CliArchive) -> Self {
        match value {
            CliArchive::Active => Self::Active,
            CliArchive::Archived => Self::Archived,
            CliArchive::All => Self::All,
        }
    }
}

#[derive(Clone, Debug, Default, Args)]
struct OutputArgs {
    /// Emit stable JSON rather than human-readable text.
    #[arg(long)]
    json: bool,
}

#[derive(Clone, Debug, Args)]
struct FilterArgs {
    /// Include Codex subagent tasks; root tasks are the default.
    #[arg(long)]
    include_subagents: bool,

    /// Include non-interactive `codex exec` tasks.
    #[arg(long)]
    include_exec: bool,

    /// Select active, archived, or both persisted task sets.
    #[arg(long, value_enum, default_value = "all")]
    archive: CliArchive,

    /// Match App Server's exact recorded working directory.
    #[arg(long)]
    cwd: Option<String>,

    /// Keep tasks updated at or after this Unix timestamp.
    #[arg(long)]
    updated_after: Option<i64>,
}

impl FilterArgs {
    fn options(&self) -> ListOptions {
        ListOptions {
            archive: self.archive.into(),
            include_subagents: self.include_subagents,
            include_exec: self.include_exec,
            cwd: self.cwd.clone(),
            updated_after: self.updated_after,
            ..ListOptions::default()
        }
    }
}

#[derive(Clone, Debug, Args)]
struct ListArgs {
    #[command(flatten)]
    filters: FilterArgs,

    /// Ask App Server to match this exact-case title fragment.
    #[arg(long)]
    title: Option<String>,

    /// Limit rows after active and archived results are merged.
    #[arg(long)]
    limit: Option<usize>,

    #[command(flatten)]
    output: OutputArgs,
}

#[derive(Clone, Debug, Args)]
struct ShowArgs {
    /// Codex thread identifier.
    thread_id: String,

    /// Restrict output to one turn identifier.
    #[arg(long)]
    turn: Option<String>,

    #[command(flatten)]
    output: OutputArgs,
}

#[derive(Clone, Debug, Args)]
struct ActivityArgs {
    /// App Server thread ID or session ID supplied by a Codex hook.
    session_hint: String,

    /// Exact Codex turn identifier.
    turn_id: String,

    #[command(flatten)]
    output: OutputArgs,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ActivityReport {
    reference: TurnRef,
    started_at: Option<i64>,
    completed_at: Option<i64>,
    status: String,
    user_messages: usize,
    assistant_messages: usize,
    completed_file_changes: Vec<CompletedFileChange>,
}

impl From<TurnActivity> for ActivityReport {
    fn from(activity: TurnActivity) -> Self {
        let user_messages = activity
            .turn
            .messages
            .iter()
            .filter(|message| message.role == Role::User)
            .count();
        let assistant_messages = activity.turn.messages.len() - user_messages;
        Self {
            reference: activity.turn.reference,
            started_at: activity.turn.started_at,
            completed_at: activity.turn.completed_at,
            status: activity.turn.status,
            user_messages,
            assistant_messages,
            completed_file_changes: activity.completed_file_changes,
        }
    }
}

#[derive(Clone, Debug, Args)]
struct SearchArgs {
    /// Case-insensitive message query; App Server title matching is also used.
    query: String,

    #[command(flatten)]
    filters: FilterArgs,

    /// Limit matching messages after fork/copy deduplication.
    #[arg(long)]
    limit: Option<usize>,

    /// Limit candidate tasks before loading their full histories.
    #[arg(long)]
    thread_limit: Option<usize>,

    #[command(flatten)]
    output: OutputArgs,
}

#[derive(Clone, Debug, Args)]
struct ExportArgs {
    #[command(flatten)]
    filters: FilterArgs,

    /// Limit tasks before loading their full histories.
    #[arg(long)]
    limit: Option<usize>,

    #[command(flatten)]
    output: OutputArgs,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("conversations: {error}");
            ExitCode::FAILURE
        }
    }
}

#[allow(clippy::too_many_lines)] // Keep the small command dispatcher in one readable match.
fn run() -> Result<(), Box<dyn StdError>> {
    let cli = Cli::parse();
    let mut config = ClientConfig {
        codex_path: cli.codex,
        stderr_policy: cli.app_server_stderr.into(),
        ..ClientConfig::default()
    };
    if let Some(host_id) = cli.host_id {
        if host_id.trim().is_empty() {
            return Err("--host-id must not be empty".into());
        }
        config.host_id = host_id;
    }
    let mut client = AppServerClient::spawn(config)?;
    match cli.command {
        Command::Doctor(args) => {
            let report = client.doctor()?;
            if args.json {
                write_json(&report)?;
            } else {
                println!("ok: {}", report.ok);
                println!("host: {}", report.host_id);
                println!("codex: {}", report.executable_version);
                println!("visible threads: {}", report.visible_threads);
                if let Some(user_agent) = report.app_server_user_agent {
                    println!("app server: {user_agent}");
                }
                for warning in report.warnings {
                    println!("warning: {warning}");
                }
            }
        }
        Command::List(args) => {
            let mut options = args.filters.options();
            options.title_query = args.title;
            options.limit = args.limit;
            let threads = client.list(&options)?;
            if args.output.json {
                write_json(&threads)?;
            } else {
                write_thread_list(&threads)?;
            }
        }
        Command::Show(args) => {
            let mut conversation = client.read_thread(&args.thread_id)?;
            if let Some(turn_id) = args.turn {
                conversation
                    .turns
                    .retain(|turn| turn.reference.turn_id == turn_id);
                if conversation.turns.is_empty() {
                    return Err(format!(
                        "turn {turn_id} was not found in thread {}",
                        args.thread_id
                    )
                    .into());
                }
            }
            if args.output.json {
                write_json(&conversation)?;
            } else {
                write_conversation(&conversation)?;
            }
        }
        Command::Activity(args) => {
            let report = ActivityReport::from(
                client.resolve_turn_activity(&args.session_hint, &args.turn_id)?,
            );
            if args.output.json {
                write_json(&report)?;
            } else {
                write_activity(&report)?;
            }
        }
        Command::Search(args) => {
            let mut options = args.filters.options();
            options.limit = args.thread_limit;
            let mut hits = client.search(&args.query, &options)?;
            truncate(&mut hits, args.limit);
            if args.output.json {
                write_json(&hits)?;
            } else {
                write_search_hits(&hits)?;
            }
        }
        Command::Export(args) => {
            let mut options = args.filters.options();
            options.limit = args.limit;
            let conversations = client.snapshot(&options)?;
            if args.output.json {
                write_json(&conversations)?;
            } else {
                let turns = conversations
                    .iter()
                    .map(|conversation| conversation.turns.len())
                    .sum::<usize>();
                let messages = conversations
                    .iter()
                    .flat_map(|conversation| &conversation.turns)
                    .map(|turn| turn.messages.len())
                    .sum::<usize>();
                println!(
                    "exported corpus: {} threads, {turns} turns, {messages} user/assistant messages",
                    conversations.len()
                );
                println!("use --json to write the normalized corpus to standard output");
            }
        }
        Command::Refresh(args) => {
            let report = client.refresh()?;
            if args.json {
                write_json(&report)?;
            } else {
                println!(
                    "refreshed metadata: {} active, {} archived, {} total",
                    report.active_threads, report.archived_threads, report.total_threads
                );
            }
        }
    }
    Ok(())
}

fn truncate<T>(values: &mut Vec<T>, limit: Option<usize>) {
    if let Some(limit) = limit {
        values.truncate(limit);
    }
}

fn write_json<T: Serialize>(value: &T) -> Result<(), Box<dyn StdError>> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    serde_json::to_writer_pretty(&mut output, value)?;
    writeln!(output)?;
    Ok(())
}

fn write_thread_list(threads: &[ThreadSummary]) -> io::Result<()> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    writeln!(output, "THREAD\tARCHIVE\tUPDATED\tSOURCE\tSTATUS\tTITLE")?;
    for thread in threads {
        writeln!(
            output,
            "{}\t{}\t{}\t{}\t{}\t{}",
            thread.reference.thread_id,
            if thread.archived {
                "archived"
            } else {
                "active"
            },
            timestamp(thread.updated_at.or(thread.created_at)),
            thread.source_kind,
            thread.runtime_status,
            one_line(thread.title())
        )?;
    }
    Ok(())
}

fn write_conversation(conversation: &Conversation) -> io::Result<()> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    writeln!(
        output,
        "thread: {}",
        conversation.thread.reference.thread_id
    )?;
    writeln!(output, "title: {}", one_line(conversation.thread.title()))?;
    writeln!(
        output,
        "archive: {}",
        if conversation.thread.archived {
            "archived"
        } else {
            "active"
        }
    )?;
    writeln!(
        output,
        "runtime status: {}",
        conversation.thread.runtime_status
    )?;
    for turn in &conversation.turns {
        writeln!(
            output,
            "\nturn {} started={} status={}",
            turn.reference.turn_id,
            timestamp(turn.started_at),
            turn.status
        )?;
        for message in &turn.messages {
            writeln!(
                output,
                "\n{} {}",
                role_name(message.role),
                message.reference.item_id
            )?;
            writeln!(output, "{}", message.text)?;
        }
    }
    Ok(())
}

fn write_search_hits(hits: &[SearchHit]) -> io::Result<()> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    writeln!(output, "THREAD\tTURN\tITEM\tROLE\tTEXT")?;
    for hit in hits {
        writeln!(
            output,
            "{}\t{}\t{}\t{}\t{}",
            hit.message.reference.thread_id,
            hit.message.reference.turn_id,
            hit.message.reference.item_id,
            role_name(hit.message.role),
            one_line(&hit.message.text)
        )?;
    }
    Ok(())
}

fn write_activity(report: &ActivityReport) -> io::Result<()> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    writeln!(output, "thread: {}", report.reference.thread_id)?;
    writeln!(output, "turn: {}", report.reference.turn_id)?;
    writeln!(output, "started: {}", timestamp(report.started_at))?;
    writeln!(output, "completed: {}", timestamp(report.completed_at))?;
    writeln!(output, "status: {}", report.status)?;
    writeln!(output, "user messages: {}", report.user_messages)?;
    writeln!(output, "assistant messages: {}", report.assistant_messages)?;
    writeln!(
        output,
        "completed file changes: {}",
        report.completed_file_changes.len()
    )?;
    for file_change in &report.completed_file_changes {
        writeln!(
            output,
            "fileChange {} changes={}",
            file_change.reference.item_id, file_change.change_count
        )?;
    }
    Ok(())
}

fn timestamp(value: Option<i64>) -> String {
    value.map_or_else(|| "unknown".to_owned(), |value| value.to_string())
}

fn role_name(role: Role) -> &'static str {
    match role {
        Role::User => "user",
        Role::Assistant => "assistant",
    }
}

fn one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}
