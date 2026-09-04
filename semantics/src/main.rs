#![allow(clippy::missing_errors_doc, clippy::too_many_lines)]
#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used))]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::str::FromStr as _;

use clap::{Args, Parser, Subcommand};
use conversations::{AppServerClient, ClientConfig, StderrPolicy};
use serde::Serialize;
use serde_json::json;

use semantics::account_worker::AccountWorker;
use semantics::adapters::{
    AnnalsDecisionFeedCli, AppServerConversationLocator, DecisionAccountPage,
    DecisionAccountSource, canonical_directory, require_participation_marker,
};
use semantics::domain::{IntakeStatus, ProjectStatus, validate_project_id};
use semantics::nucleus::NucleusReconciler;
use semantics::seed::{seed_markdown, seed_one};
use semantics::store::Store;
use semantics::{Error, Result};

#[derive(Debug, Parser)]
#[command(version, about = "Project-scoped authoritative semantic repositories")]
struct Cli {
    #[arg(long, env = "SEMANTICS_DATABASE")]
    database: Option<PathBuf>,
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Project(ProjectArgs),
    Repository(RepositoryArgs),
    Intake(IntakeArgs),
    Doctor,
}

#[derive(Debug, Args)]
struct ProjectArgs {
    #[command(subcommand)]
    command: ProjectCommand,
}

#[derive(Debug, Subcommand)]
enum ProjectCommand {
    Register {
        id: String,
        root: PathBuf,
        #[arg(long, hide = true)]
        activation_cursor: Option<String>,
        #[arg(long, hide = true, requires = "annals_library_id")]
        annals_activation_cursor: Option<String>,
        #[arg(long, hide = true, requires = "annals_activation_cursor")]
        annals_library_id: Option<String>,
    },
    List,
    Show {
        id: String,
    },
    Move {
        id: String,
        root: PathBuf,
    },
    Pause {
        id: String,
    },
    Resume {
        id: String,
    },
    Retire {
        id: String,
    },
    #[command(hide = true)]
    ActivateAnnals {
        #[arg(long)]
        final_decisions_watermark: String,
    },
}

#[derive(Debug, Args)]
struct RepositoryArgs {
    #[command(subcommand)]
    command: RepositoryCommand,
}

#[derive(Debug, Subcommand)]
enum RepositoryCommand {
    Show {
        project: String,
        #[arg(long)]
        revision: Option<u64>,
    },
    Search {
        project: String,
        query: String,
        #[arg(long)]
        revision: Option<u64>,
    },
    Log {
        project: String,
        #[arg(long, default_value_t = 1)]
        from: u64,
        #[arg(long)]
        to: Option<u64>,
    },
    Diff {
        project: String,
        from: u64,
        to: u64,
    },
    Seed {
        project: String,
        #[arg(long)]
        label: String,
        #[arg(long)]
        meaning: String,
        #[arg(long)]
        grounding: Option<String>,
    },
    SeedMarkdown {
        project: String,
        path: PathBuf,
    },
}

#[derive(Debug, Args)]
struct IntakeArgs {
    #[command(subcommand)]
    command: IntakeCommand,
}

#[derive(Debug, Subcommand)]
enum IntakeCommand {
    Status {
        #[arg(long)]
        status: Option<String>,
    },
    Assign {
        event_id: String,
        project: String,
    },
    Retry {
        event_id: String,
    },
    #[command(hide = true)]
    Run,
}

fn main() {
    let cli = Cli::parse();
    let json = cli.json;
    let scheduled_worker = matches!(
        &cli.command,
        Command::Intake(arguments) if matches!(&arguments.command, IntakeCommand::Run)
    );
    if let Err(error) = run(cli) {
        eprintln!("{}", render_error(&error, json, scheduled_worker));
        std::process::exit(1);
    }
}

fn render_error(error: &Error, json_output: bool, scheduled_worker: bool) -> String {
    if scheduled_worker && json_output {
        serde_json::to_string(&json!({
            "ok": false,
            "error": {
                "code": "semantics_worker_failed",
                "message": "Semantics worker stopped; inspect durable intake and dependency readiness"
            }
        }))
        .unwrap_or_else(|_| "{\"ok\":false}".to_owned())
    } else if scheduled_worker {
        "semantics: semantics_worker_failed: Semantics worker stopped; inspect durable intake and dependency readiness".to_owned()
    } else if json_output {
        serde_json::to_string(&json!({
            "ok": false,
            "error": {"code": error.code(), "message": error.to_string()}
        }))
        .unwrap_or_else(|_| "{\"ok\":false}".to_owned())
    } else {
        format!("semantics: {error}")
    }
}

fn run(cli: Cli) -> Result<()> {
    let database = cli.database.map_or_else(default_database, Ok)?;
    let store = Store::open(database)?;
    match cli.command {
        Command::Project(arguments) => project_command(&store, arguments.command, cli.json),
        Command::Repository(arguments) => repository_command(&store, arguments.command, cli.json),
        Command::Intake(arguments) => intake_command(&store, arguments.command, cli.json),
        Command::Doctor => doctor(&store, cli.json),
    }
}

fn project_command(store: &Store, command: ProjectCommand, compact: bool) -> Result<()> {
    match command {
        ProjectCommand::Register {
            id,
            root,
            activation_cursor,
            annals_activation_cursor,
            annals_library_id,
        } => {
            validate_project_id(&id)?;
            let root = canonical_directory(&root)?;
            require_participation_marker(&root, &id)?;
            let (library_id, cursor) = match (annals_library_id, annals_activation_cursor) {
                (Some(library_id), Some(cursor)) => (library_id, cursor),
                (None, None) => AnnalsDecisionFeedCli::for_current_user(
                    store.account_feed_library_id()?.as_deref(),
                )?
                .watermark()?,
                _ => {
                    return Err(Error::domain(
                        "annals_activation_incomplete",
                        "test/recovery Annals library and cursor overrides must be supplied together",
                    ));
                }
            };
            if activation_cursor.is_some() {
                return Err(Error::domain(
                    "legacy_activation_unsupported",
                    "new projects use Annals activation; the legacy Decisions cursor is preserved for existing databases only",
                ));
            }
            store.register_project_with_account_feed(&id, &root, &library_id, &cursor)?;
            print(&store.project_detail(&id)?, compact)
        }
        ProjectCommand::List => print(&store.list_projects()?, compact),
        ProjectCommand::Show { id } => print(&store.project_detail(&id)?, compact),
        ProjectCommand::Move { id, root } => {
            let root = canonical_directory(&root)?;
            require_participation_marker(&root, &id)?;
            store.move_project(&id, &root)?;
            print(&store.project_detail(&id)?, compact)
        }
        ProjectCommand::Pause { id } => {
            store.set_project_status(&id, ProjectStatus::Paused)?;
            print(&store.project_detail(&id)?, compact)
        }
        ProjectCommand::Resume { id } => {
            let project = store.project(&id)?;
            require_participation_marker(Path::new(&project.current_path), &id)?;
            store.set_project_status(&id, ProjectStatus::Active)?;
            print(&store.project_detail(&id)?, compact)
        }
        ProjectCommand::Retire { id } => {
            store.set_project_status(&id, ProjectStatus::Retired)?;
            print(&store.project_detail(&id)?, compact)
        }
        ProjectCommand::ActivateAnnals {
            final_decisions_watermark,
        } => {
            let correlations = store.legacy_cutover_snapshot(&final_decisions_watermark)?;
            NucleusReconciler::for_current_user().prove_legacy_cutover_ready(&correlations)?;
            let expected = store.account_feed_library_id()?;
            let (library_id, watermark) =
                AnnalsDecisionFeedCli::for_current_user(expected.as_deref())?.watermark()?;
            store.activate_account_feed_after_legacy_cutover(
                &library_id,
                &watermark,
                &final_decisions_watermark,
                &correlations,
            )?;
            print(
                &json!({"library_id": library_id, "activation_watermark": watermark}),
                compact,
            )
        }
    }
}

fn repository_command(store: &Store, command: RepositoryCommand, compact: bool) -> Result<()> {
    match command {
        RepositoryCommand::Show { project, revision } => {
            print(&store.repository(&project, revision)?, compact)
        }
        RepositoryCommand::Search {
            project,
            query,
            revision,
        } => {
            let repository = store.repository(&project, revision)?;
            let needle = query.to_lowercase();
            let hits = repository
                .concepts
                .values()
                .filter(|concept| {
                    concept.label.to_lowercase().contains(&needle)
                        || concept.meaning.to_lowercase().contains(&needle)
                })
                .collect::<Vec<_>>();
            print(&hits, compact)
        }
        RepositoryCommand::Log { project, from, to } => {
            print(&store.revisions(&project, from, to)?, compact)
        }
        RepositoryCommand::Diff { project, from, to } => {
            print(&store.diff(&project, from, to)?, compact)
        }
        RepositoryCommand::Seed {
            project,
            label,
            meaning,
            grounding,
        } => {
            let revision = seed_one(store, &project, &label, &meaning, grounding.as_deref())?;
            print(
                &json!({"project_id": project, "revision": revision}),
                compact,
            )
        }
        RepositoryCommand::SeedMarkdown { project, path } => {
            let revision = seed_markdown(store, &project, &path)?;
            print(
                &json!({"project_id": project, "revision": revision}),
                compact,
            )
        }
    }
}

fn intake_command(store: &Store, command: IntakeCommand, compact: bool) -> Result<()> {
    match command {
        IntakeCommand::Status { status } => {
            let status = status.as_deref().map(IntakeStatus::from_str).transpose()?;
            print(
                &json!({
                    "annals_decision_accounts": store.list_account_intake(status.clone())?,
                    "legacy_decisions": store.list_intake(status)?,
                }),
                compact,
            )
        }
        IntakeCommand::Assign { event_id, project } => {
            let target = store.project(&project)?;
            require_participation_marker(Path::new(&target.current_path), &project)?;
            if store.has_account_intake(&event_id)? {
                store.assign_account_intake(&event_id, &project)?;
                print(&store.account_intake(&event_id)?, compact)
            } else {
                store.assign_intake(&event_id, &project)?;
                print(&store.intake(&event_id)?, compact)
            }
        }
        IntakeCommand::Retry { event_id } => {
            if store.has_account_intake(&event_id)? {
                NucleusReconciler::for_current_user().retry_account_failed(store, &event_id)?;
                print(&store.account_intake(&event_id)?, compact)
            } else {
                NucleusReconciler::for_current_user().retry_failed(store, &event_id)?;
                print(&store.intake(&event_id)?, compact)
            }
        }
        IntakeCommand::Run => {
            let conversations = AppServerConversationLocator::for_current_user()?;
            let worker = AccountWorker::new(
                store,
                AnnalsDecisionFeedCli::for_current_user(
                    store.account_feed_library_id()?.as_deref(),
                )?,
                conversations,
            );
            print(&worker.run_once()?, compact)
        }
    }
}

#[derive(Debug, Serialize)]
struct DoctorCheck {
    name: &'static str,
    ok: bool,
    detail: String,
}

fn doctor(store: &Store, compact: bool) -> Result<()> {
    let mut checks = Vec::new();
    checks.push(check("database", || {
        let version = store.schema_version()?;
        Ok(format!("schema {version} at {}", store.path().display()))
    }));
    checks.push(check("participation_markers", || {
        for project in store
            .list_projects()?
            .into_iter()
            .filter(|project| project.status != ProjectStatus::Retired)
        {
            require_participation_marker(Path::new(&project.current_path), &project.id)?;
        }
        Ok("all active and paused projects have exact-root markers".to_owned())
    }));
    checks.push(check("annals_decision_feed", || {
        let expected = ready_account_feed_library(store)?;
        let mut annals = AnnalsDecisionFeedCli::for_current_user(expected.as_deref())?;
        prove_annals_feed_replay(store, &mut annals, expected.as_deref())
    }));
    checks.push(check("conversations_exact_cwd", || {
        let mut client = AppServerClient::spawn(ClientConfig {
            stderr_policy: StderrPolicy::Suppress,
            ..ClientConfig::default()
        })
        .map_err(|_| {
            Error::domain(
                "conversations_not_ready",
                "Conversations exact-cwd readiness check did not start",
            )
        })?;
        let report = client.doctor().map_err(|_| {
            Error::domain(
                "conversations_not_ready",
                "Conversations exact-cwd readiness check did not complete",
            )
        })?;
        if !report.ok {
            return Err(Error::domain(
                "conversations_not_ready",
                "Conversations doctor reported not ready",
            ));
        }
        Ok(format!("{} visible threads", report.visible_threads))
    }));
    checks.push(check("nucleus_reconciliation", || {
        NucleusReconciler::for_current_user()
            .doctor()
            .map_err(|_| {
                Error::domain(
                    "nucleus_not_ready",
                    "Nucleus reconciliation readiness check did not complete",
                )
            })?;
        Ok("health, capabilities, schemas, and immutable toolset verified".to_owned())
    }));
    let ok = checks.iter().all(|check| check.ok);
    print(&json!({"ok": ok, "checks": checks}), compact)?;
    if ok {
        Ok(())
    } else {
        Err(Error::domain(
            "doctor_failed",
            "one or more Semantics readiness checks failed",
        ))
    }
}

fn prove_annals_feed_replay(
    store: &Store,
    annals: &mut impl DecisionAccountSource,
    expected_library_id: Option<&str>,
) -> Result<String> {
    let (library_id, watermark) = annals.watermark()?;
    if expected_library_id.is_some_and(|expected| expected != library_id) {
        return Err(Error::domain(
            "annals_library_mismatch",
            "Annals feed library does not match the activated Semantics library",
        ));
    }
    let mut cursors = store
        .list_projects()?
        .into_iter()
        .filter(|project| project.status != ProjectStatus::Retired)
        .filter_map(|project| project.annals_scan_cursor)
        .collect::<BTreeSet<_>>();
    if cursors.is_empty() {
        cursors.insert(watermark.clone());
    }
    let mut pages_verified = 0_usize;
    for installed_cursor in &cursors {
        let mut cursor = installed_cursor.clone();
        let mut request_cursors = BTreeSet::from([cursor.clone()]);
        let mut event_cursors = BTreeSet::new();
        let mut event_ids = BTreeSet::new();
        let mut account_ids = BTreeSet::new();
        let mut reached_empty_page = false;
        for _ in 0..1_000 {
            let page = annals.read_page(&cursor, &watermark, 100)?;
            validate_doctor_page(&page, &library_id, &cursor, &watermark)?;
            let replay = annals.read_page(&cursor, &watermark, 100)?;
            validate_doctor_page(&replay, &library_id, &cursor, &watermark)?;
            if replay != page {
                return Err(Error::domain(
                    "annals_replay_mismatch",
                    "Annals changed a page replay at one fixed watermark",
                ));
            }
            pages_verified += 1;
            if page.events.is_empty() {
                reached_empty_page = true;
                break;
            }
            for event in &page.events {
                if !event_cursors.insert(event.cursor.clone())
                    || !event_ids.insert(event.event_id.clone())
                    || !account_ids.insert(event.account_id.clone())
                {
                    return Err(Error::domain(
                        "annals_page_order_invalid",
                        "Annals repeated a decision-account identity across fixed pages",
                    ));
                }
            }
            if !request_cursors.insert(page.next_cursor.clone()) {
                return Err(Error::domain(
                    "annals_page_cycle",
                    "Annals decision-feed pages formed a cursor cycle",
                ));
            }
            cursor = page.next_cursor;
        }
        if !reached_empty_page {
            return Err(Error::domain(
                "annals_page_bound_exceeded",
                "Annals fixed-watermark replay exceeded 1000 bounded pages",
            ));
        }
    }
    let activation = if expected_library_id.is_some() {
        "activated"
    } else {
        "activation pending; no active or paused projects"
    };
    Ok(format!(
        "library identity verified ({} bytes), {pages_verified} fixed-watermark page replay(s) verified from {} cursor(s), {activation}",
        library_id.len(),
        cursors.len()
    ))
}

fn validate_doctor_page(
    page: &DecisionAccountPage,
    library_id: &str,
    cursor: &str,
    watermark: &str,
) -> Result<()> {
    if page.library_id != library_id
        || page.request_cursor != cursor
        || page.watermark != watermark
        || page.events.len() > 100
    {
        return Err(Error::domain(
            "annals_page_mismatch",
            "Annals page did not echo the bounded fixed-watermark request",
        ));
    }
    let mut seen_cursors = BTreeSet::new();
    let mut seen_events = BTreeSet::new();
    let mut seen_accounts = BTreeSet::new();
    for event in &page.events {
        if event.library_id != library_id
            || event.cursor == cursor
            || !seen_cursors.insert(event.cursor.as_str())
            || !seen_events.insert(event.event_id.as_str())
            || !seen_accounts.insert(event.account_id.as_str())
        {
            return Err(Error::domain(
                "annals_page_order_invalid",
                "Annals returned duplicate or nonadvancing decision-feed ordering",
            ));
        }
    }
    let expected_next = page
        .events
        .last()
        .map_or(cursor, |event| event.cursor.as_str());
    if page.next_cursor != expected_next {
        return Err(Error::domain(
            "annals_next_cursor_mismatch",
            "Annals next cursor does not match the fixed page ordering",
        ));
    }
    Ok(())
}

fn ready_account_feed_library(store: &Store) -> Result<Option<String>> {
    let projects = store
        .list_projects()?
        .into_iter()
        .filter(|project| project.status != ProjectStatus::Retired)
        .collect::<Vec<_>>();
    let expected = store.account_feed_library_id()?;
    if projects.is_empty() {
        return Ok(expected);
    }
    let library_id = expected.ok_or_else(|| {
        Error::domain(
            "annals_feed_inactive",
            "activate the Annals decisions feed for every active or paused project before enabling the worker",
        )
    })?;
    if projects.iter().any(|project| {
        project.annals_library_id.as_deref() != Some(library_id.as_str())
            || project.annals_activation_cursor.is_none()
            || project.annals_scan_cursor.is_none()
    }) {
        return Err(Error::domain(
            "annals_feed_inactive",
            "every active or paused project must have the selected Annals decisions-library identity and cursors before enabling the worker",
        ));
    }
    Ok(Some(library_id))
}

fn check(name: &'static str, operation: impl FnOnce() -> Result<String>) -> DoctorCheck {
    match operation() {
        Ok(detail) => DoctorCheck {
            name,
            ok: true,
            detail,
        },
        Err(error) => DoctorCheck {
            name,
            ok: false,
            detail: error.to_string(),
        },
    }
}

fn print(value: &impl Serialize, compact: bool) -> Result<()> {
    let output = if compact {
        serde_json::to_string(value)?
    } else {
        serde_json::to_string_pretty(value)?
    };
    println!("{output}");
    Ok(())
}

fn default_database() -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| {
            Error::domain(
                "home_unavailable",
                "HOME must be an absolute path or --database must be supplied",
            )
        })?;
    Ok(home.join("Library/Application Support/Semantics/semantics.db"))
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::{prove_annals_feed_replay, ready_account_feed_library, render_error};
    use semantics::Error;
    use semantics::adapters::{DecisionAccountPage, DecisionAccountSource};
    use semantics::domain::{DecisionAccountAnchor, DecisionAccountEvent};
    use semantics::store::Store;
    use tempfile::TempDir;

    struct Feed {
        watermark: String,
        pages: VecDeque<DecisionAccountPage>,
    }

    impl DecisionAccountSource for Feed {
        fn watermark(&mut self) -> semantics::Result<(String, String)> {
            Ok((
                "0123456789abcdef0123456789abcdef".to_owned(),
                self.watermark.clone(),
            ))
        }

        fn read_page(
            &mut self,
            _cursor: &str,
            _watermark: &str,
            _limit: u16,
        ) -> semantics::Result<DecisionAccountPage> {
            self.pages
                .pop_front()
                .ok_or_else(|| Error::domain("fixture_empty", "missing page"))
        }
    }

    fn account(cursor: &str, ordinal: u8) -> DecisionAccountEvent {
        DecisionAccountEvent {
            library_id: "0123456789abcdef0123456789abcdef".to_owned(),
            cursor: cursor.to_owned(),
            event_id: format!("event-{ordinal}"),
            account_id: format!("account-{ordinal}"),
            account_schema_version: 1,
            statement: "statement".to_owned(),
            context: "context".to_owned(),
            action: "action".to_owned(),
            result: "result".to_owned(),
            occurred_at: i64::from(ordinal),
            occurred_at_precision: "second".to_owned(),
            authority: DecisionAccountAnchor {
                host_id: "host".to_owned(),
                thread_id: "thread".to_owned(),
                turn_id: format!("turn-{ordinal}"),
                item_id: format!("item-{ordinal}"),
                span_start: 0,
                span_end: 1,
            },
        }
    }

    fn page(
        request_cursor: &str,
        watermark: &str,
        events: Vec<DecisionAccountEvent>,
    ) -> DecisionAccountPage {
        let next_cursor = events
            .last()
            .map_or_else(|| request_cursor.to_owned(), |event| event.cursor.clone());
        DecisionAccountPage {
            library_id: "0123456789abcdef0123456789abcdef".to_owned(),
            request_cursor: request_cursor.to_owned(),
            next_cursor,
            watermark: watermark.to_owned(),
            events,
        }
    }

    #[test]
    fn account_feed_readiness_fails_closed_for_unactivated_projects() {
        let temporary = TempDir::new().expect("temporary directory");
        let store = Store::open(temporary.path().join("semantics.db")).expect("store");
        assert_eq!(
            ready_account_feed_library(&store).expect("empty repository readiness"),
            None
        );

        store
            .register_project("legacy", temporary.path(), "legacy-cursor")
            .expect("legacy project");
        let error = ready_account_feed_library(&store).expect_err("activation must be required");
        assert_eq!(error.code(), "annals_feed_inactive");

        let library_id = "0123456789abcdef0123456789abcdef";
        store
            .activate_account_feed(library_id, "afe1_0000")
            .expect("feed activation");
        assert_eq!(
            ready_account_feed_library(&store).expect("activated readiness"),
            Some(library_id.to_owned())
        );
    }

    #[test]
    fn scheduled_worker_errors_are_body_free() {
        let error = Error::domain(
            "dependency_failed",
            "PRIVATE account body at /private/project from thread-secret",
        );
        let output = render_error(&error, true, true);
        assert!(output.contains("semantics_worker_failed"));
        for private in ["PRIVATE", "/private/project", "thread-secret"] {
            assert!(!output.contains(private));
        }
    }

    #[test]
    fn doctor_requires_identical_fixed_watermark_page_replay() {
        let temporary = TempDir::new().expect("temporary directory");
        let store = Store::open(temporary.path().join("semantics.db")).expect("store");
        let first = page("a1", "a1", Vec::new());
        let changed = page("a1", "a1", vec![account("a2", 1)]);
        let mut feed = Feed {
            watermark: "a1".to_owned(),
            pages: VecDeque::from([first, changed]),
        };
        let error = prove_annals_feed_replay(&store, &mut feed, None)
            .expect_err("changed replay must fail");
        assert_eq!(error.code(), "annals_replay_mismatch");
    }

    #[test]
    fn doctor_replays_every_page_until_an_unchanged_empty_page() {
        let temporary = TempDir::new().expect("temporary directory");
        let store = Store::open(temporary.path().join("semantics.db")).expect("store");
        store
            .register_project("legacy", temporary.path(), "legacy-final")
            .expect("legacy project");
        store
            .activate_account_feed("0123456789abcdef0123456789abcdef", "a0")
            .expect("feed activation");
        let first = page("a0", "a3", vec![account("a1", 1), account("a2", 2)]);
        let second = page("a2", "a3", vec![account("a3", 3)]);
        let empty = page("a3", "a3", Vec::new());
        let mut feed = Feed {
            watermark: "a3".to_owned(),
            pages: VecDeque::from([
                first.clone(),
                first,
                second.clone(),
                second,
                empty.clone(),
                empty,
            ]),
        };
        let detail =
            prove_annals_feed_replay(&store, &mut feed, Some("0123456789abcdef0123456789abcdef"))
                .expect("multipage replay");
        assert!(detail.contains("3 fixed-watermark page replay(s)"));
        assert!(feed.pages.is_empty());
    }

    #[test]
    fn doctor_rejects_a_later_page_cycle() {
        let temporary = TempDir::new().expect("temporary directory");
        let store = Store::open(temporary.path().join("semantics.db")).expect("store");
        store
            .register_project("legacy", temporary.path(), "legacy-final")
            .expect("legacy project");
        store
            .activate_account_feed("0123456789abcdef0123456789abcdef", "a0")
            .expect("feed activation");
        let first = page("a0", "a9", vec![account("a1", 1)]);
        let cycle = page("a1", "a9", vec![account("a0", 2)]);
        let mut feed = Feed {
            watermark: "a9".to_owned(),
            pages: VecDeque::from([first.clone(), first, cycle.clone(), cycle]),
        };
        let error =
            prove_annals_feed_replay(&store, &mut feed, Some("0123456789abcdef0123456789abcdef"))
                .expect_err("cursor cycle");
        assert_eq!(error.code(), "annals_page_cycle");
    }

    #[test]
    fn doctor_rejects_changed_replay_on_a_later_page() {
        let temporary = TempDir::new().expect("temporary directory");
        let store = Store::open(temporary.path().join("semantics.db")).expect("store");
        store
            .register_project("legacy", temporary.path(), "legacy-final")
            .expect("legacy project");
        store
            .activate_account_feed("0123456789abcdef0123456789abcdef", "a0")
            .expect("feed activation");
        let first = page("a0", "a9", vec![account("a1", 1)]);
        let second = page("a1", "a9", vec![account("a2", 2)]);
        let mut changed = second.clone();
        changed.events[0].statement = "changed replay".to_owned();
        let mut feed = Feed {
            watermark: "a9".to_owned(),
            pages: VecDeque::from([first.clone(), first, second, changed]),
        };
        let error =
            prove_annals_feed_replay(&store, &mut feed, Some("0123456789abcdef0123456789abcdef"))
                .expect_err("later changed replay");
        assert_eq!(error.code(), "annals_replay_mismatch");
    }
}
