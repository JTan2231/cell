use std::ffi::OsStr;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Value, json};

use crate::cli::{
    AssessArgs, Cli, Command, ConcernAddArgs, ConcernAssessArgs, ConcernCommand, ConcernListArgs,
    DesignAcceptArgs, DesignCommand, DesignCorrectArgs, DesignProposeArgs, DesignRejectArgs,
    EmailCommand, EmailSendArgs, ListArgs, MigrateArgs, NewArgs, NoteAddArgs, NoteCommand,
    RoutingAcceptArgs, RoutingCommand, RoutingRejectArgs, SearchArgs, SituationCommand,
};
use crate::config::Config;
use crate::email::EmailPreview;
use crate::error::{AppError, AppResult};
use crate::model::{ModelQuality, TodoId, TodoSummary, TodoView};
use crate::reconciliation_store::{
    AssessmentBase, DecisionSource, DesignView, RoutingProposalView, SituationAssessmentView,
};
use crate::render::{CommandOutput, terminal_text};
use crate::{db, digest, email, reconciliation, reconciliation_store, todo_store};

const MAX_PAGE_LIMIT: u32 = 1_000;

#[derive(Debug, Clone)]
pub(crate) struct ResolvedNewArgs {
    pub(crate) direction: String,
    pub(crate) source: PathBuf,
    pub(crate) quality: Option<ModelQuality>,
    pub(crate) model: Option<String>,
}

pub(crate) fn database_path(
    explicit: Option<&PathBuf>,
    config: &Config,
) -> Result<PathBuf, AppError> {
    let environment = std::env::var_os("TODO_DATABASE");
    resolve_database_path(
        explicit.map(PathBuf::as_path),
        environment.as_deref(),
        config.database.as_deref(),
    )
}

fn resolve_database_path(
    explicit: Option<&Path>,
    environment: Option<&OsStr>,
    configured: Option<&Path>,
) -> Result<PathBuf, AppError> {
    explicit
        .map(Path::to_path_buf)
        .or_else(|| {
            environment
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
        .or_else(|| configured.map(Path::to_path_buf))
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| {
            AppError::invalid(
                "database_not_configured",
                "no database is configured; pass --database, set TODO_DATABASE, or select a configuration that defines database",
            )
        })
}

pub(crate) fn run(cli: &Cli, config: &Config, database: &Path) -> AppResult<CommandOutput> {
    match &cli.command {
        Command::Init => initialize(database),
        Command::Migrate(args) => migrate(database, args),
        Command::Concern(command) => match command {
            ConcernCommand::Add(args) => add_concern(database, args),
            ConcernCommand::List(args) => list_concerns(database, args),
            ConcernCommand::Show(args) => show_concern(database, args.id),
            ConcernCommand::Assess(args) => assess_concern(database, config, args),
        },
        Command::Routing(command) => match command {
            RoutingCommand::Show(args) => show_routing(database, args.id),
            RoutingCommand::Accept(args) => accept_routing(database, args),
            RoutingCommand::Reject(args) => reject_routing(database, args),
        },
        Command::Assess(args) => assess_situation(database, config, args),
        Command::Situation(command) => match command {
            SituationCommand::Show(args) => show_situation(database, args.id),
        },
        Command::Design(command) => match command {
            DesignCommand::Propose(args) => propose_design(database, config, args),
            DesignCommand::Show(args) => show_design(database, args.id),
            DesignCommand::Correct(args) => correct_design(database, config, args),
            DesignCommand::Accept(args) => accept_design(database, args),
            DesignCommand::Reject(args) => reject_design(database, args),
        },
        Command::New(args) => new_concern(database, config, args),
        Command::List(args) => list_todos(database, args),
        Command::Search(args) => search_todos(database, args),
        Command::Show(args) => show_todo(database, args.id),
        Command::Note(command) => match command {
            NoteCommand::Add(args) => add_note(database, args),
        },
        Command::Email(command) => match command {
            EmailCommand::Preview => preview_email(database, config),
            EmailCommand::Send(args) => send_email(database, config, args),
        },
        Command::Done(args) => done(database, args.id),
        Command::Reopen(args) => reopen(database, args.id),
    }
}

fn migrate(database: &Path, args: &MigrateArgs) -> AppResult<CommandOutput> {
    let outcome = db::migrate(database, &args.backup)?;
    let human = if outcome.migrated {
        format!(
            "Migrated Todo database from v{} to v{}; backup: {}",
            outcome.from_version,
            outcome.to_version,
            args.backup.display()
        )
    } else {
        format!("Todo database is already at v{}", outcome.to_version)
    };
    Ok(CommandOutput::new(to_value(&outcome)?, human).mutation())
}

fn new_concern(database: &Path, config: &Config, args: &NewArgs) -> AppResult<CommandOutput> {
    let resolved = resolve_new_args(args)?;
    let concern = {
        let mut connection = db::open_write(database)?;
        reconciliation_store::capture_concern(
            &mut connection,
            &resolved.direction,
            &resolved.source,
        )?
    };
    match reconciliation::route_concern(
        database,
        config,
        concern.id,
        resolved.quality,
        resolved.model.as_deref(),
    ) {
        Ok(stage) => {
            let mut output = CommandOutput::new(
                json!({ "concern": concern, "routing": stage.artifact }),
                format!(
                    "Captured {} and recorded pending routing {}",
                    concern.id, stage.artifact.id
                ),
            )
            .mutation();
            if let Some(diagnostic) = stage.diagnostic {
                output.diagnostics = format!("todo: {diagnostic}");
            }
            Ok(output)
        }
        Err(error) => {
            let mut output = CommandOutput::new(
                json!({ "concern": concern, "routing": Value::Null }),
                format!("Captured {}; routing still needs assessment", concern.id),
            )
            .mutation();
            output.diagnostics =
                format!("todo: the concern was retained, but routing research failed: {error}");
            Ok(output)
        }
    }
}

fn add_concern(database: &Path, args: &ConcernAddArgs) -> AppResult<CommandOutput> {
    validate_nonblank("concern", &args.direction)?;
    let source = resolve_source(&args.source)?;
    let mut connection = db::open_write(database)?;
    let concern = reconciliation_store::capture_concern(&mut connection, &args.direction, &source)?;
    Ok(CommandOutput::new(
        json!({ "concern": concern }),
        format!("Captured {}", concern.id),
    )
    .mutation())
}

fn list_concerns(database: &Path, args: &ConcernListArgs) -> AppResult<CommandOutput> {
    validate_limit(args.limit)?;
    let connection = db::open_read(database)?;
    let concerns = reconciliation_store::list_concerns(&connection, args.all, args.limit)?;
    let human = if concerns.is_empty() {
        if args.all {
            "No concerns".to_owned()
        } else {
            "No pending concerns".to_owned()
        }
    } else {
        concerns
            .iter()
            .map(|concern| {
                format!(
                    "{}\t{:?}\t{}",
                    concern.id,
                    concern.status,
                    terminal_text(&concern.body, false)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    Ok(CommandOutput::new(json!({ "concerns": concerns }), human))
}

fn show_concern(database: &Path, id: crate::model::ConcernId) -> AppResult<CommandOutput> {
    let connection = db::open_read(database)?;
    let concern = reconciliation_store::get_concern(&connection, id)?;
    let routing = reconciliation_store::list_routing_for_concern(&connection, id)?;
    let mut human = format!(
        "{} [{:?}]\nCreated: {}\nSource: {}\n\n{}",
        concern.id,
        concern.status,
        terminal_text(&concern.created_at, false),
        terminal_text(&concern.source_path, false),
        terminal_text(&concern.body, true),
    );
    if !routing.is_empty() {
        human.push_str("\n\nRouting history");
        for proposal in &routing {
            let _ = write!(
                human,
                "\n{} [{}] {}",
                proposal.id,
                terminal_text(&proposal.decision, false),
                terminal_text(&proposal.action, false),
            );
        }
    }
    Ok(CommandOutput::new(
        json!({ "concern": concern, "routing": routing }),
        human,
    ))
}

fn assess_concern(
    database: &Path,
    config: &Config,
    args: &ConcernAssessArgs,
) -> AppResult<CommandOutput> {
    validate_research(args.research.model.as_deref())?;
    let stage = reconciliation::route_concern(
        database,
        config,
        args.id,
        args.research.quality,
        args.research.model.as_deref(),
    )?;
    let mut output = CommandOutput::new(
        json!({ "routing": stage.artifact }),
        format!("Recorded pending routing {}", stage.artifact.id),
    )
    .mutation();
    if let Some(diagnostic) = stage.diagnostic {
        output.diagnostics = format!("todo: {diagnostic}");
    }
    Ok(output)
}

fn show_routing(database: &Path, id: crate::model::RoutingProposalId) -> AppResult<CommandOutput> {
    let connection = db::open_read(database)?;
    let routing = reconciliation_store::get_routing(&connection, id)?;
    let human = render_routing(&routing);
    Ok(CommandOutput::new(json!({ "routing": routing }), human))
}

fn accept_routing(database: &Path, args: &RoutingAcceptArgs) -> AppResult<CommandOutput> {
    let source = decision_source(&args.source)?;
    let mut connection = db::open_write(database)?;
    let decision = reconciliation_store::authorize_routing(&mut connection, args.id, &source)?;
    let human = if decision.changed {
        match decision.todo_id {
            Some(todo) => format!("Accepted {}; current umbrella: {todo}", args.id),
            None => format!("Accepted {}", args.id),
        }
    } else {
        format!("{} was already accepted", args.id)
    };
    Ok(CommandOutput::new(to_value(&decision)?, human).mutation())
}

fn reject_routing(database: &Path, args: &RoutingRejectArgs) -> AppResult<CommandOutput> {
    let source = decision_source(&args.source)?;
    let reason = read_text_argument(&args.reason)?;
    validate_nonblank("rejection reason", &reason)?;
    let mut connection = db::open_write(database)?;
    let decision =
        reconciliation_store::reject_routing(&mut connection, args.id, &source, &reason)?;
    let human = if decision.changed {
        format!("Rejected {}", args.id)
    } else {
        format!("{} was already rejected with that decision", args.id)
    };
    Ok(CommandOutput::new(to_value(&decision)?, human).mutation())
}

fn assess_situation(
    database: &Path,
    config: &Config,
    args: &AssessArgs,
) -> AppResult<CommandOutput> {
    validate_research(args.research.model.as_deref())?;
    let stage = reconciliation::assess_todo(
        database,
        config,
        args.id,
        args.research.quality,
        args.research.model.as_deref(),
    )?;
    let mut output = CommandOutput::new(
        json!({ "assessment": stage.artifact }),
        format!(
            "Recorded situation assessment {} [{}]",
            stage.artifact.id, stage.artifact.disposition
        ),
    )
    .mutation();
    if let Some(diagnostic) = stage.diagnostic {
        output.diagnostics = format!("todo: {diagnostic}");
    }
    Ok(output)
}

fn show_situation(
    database: &Path,
    id: crate::model::SituationAssessmentId,
) -> AppResult<CommandOutput> {
    let connection = db::open_read(database)?;
    let assessment = reconciliation_store::get_assessment(&connection, id)?;
    let human = render_situation(&assessment);
    Ok(CommandOutput::new(
        json!({ "assessment": assessment }),
        human,
    ))
}

fn propose_design(
    database: &Path,
    config: &Config,
    args: &DesignProposeArgs,
) -> AppResult<CommandOutput> {
    validate_research(args.research.model.as_deref())?;
    let stage = reconciliation::propose_design(
        database,
        config,
        args.todo,
        args.research.quality,
        args.research.model.as_deref(),
    )?;
    Ok(design_stage_output(stage))
}

fn correct_design(
    database: &Path,
    config: &Config,
    args: &DesignCorrectArgs,
) -> AppResult<CommandOutput> {
    validate_research(args.research.model.as_deref())?;
    let feedback = read_text_argument(&args.feedback)?;
    let stage = reconciliation::correct_design(
        database,
        config,
        args.id,
        &feedback,
        args.research.quality,
        args.research.model.as_deref(),
    )?;
    Ok(design_stage_output(stage))
}

fn design_stage_output(
    stage: reconciliation::StageOutput<reconciliation_store::DesignView>,
) -> CommandOutput {
    let mut output = CommandOutput::new(
        json!({ "design": stage.artifact }),
        format!(
            "Recorded design {} [{}]",
            stage.artifact.id, stage.artifact.state
        ),
    )
    .mutation();
    if let Some(diagnostic) = stage.diagnostic {
        output.diagnostics = format!("todo: {diagnostic}");
    }
    output
}

fn show_design(database: &Path, id: crate::model::DesignId) -> AppResult<CommandOutput> {
    let connection = db::open_read(database)?;
    let design = reconciliation_store::get_design(&connection, id)?;
    let human = render_design(&design);
    Ok(CommandOutput::new(json!({ "design": design }), human))
}

fn accept_design(database: &Path, args: &DesignAcceptArgs) -> AppResult<CommandOutput> {
    let source = decision_source(&args.source)?;
    let mut connection = db::open_write(database)?;
    let (design, changed) =
        reconciliation_store::authorize_design(&mut connection, args.id, &source)?;
    let human = if changed {
        format!("Accepted {}", args.id)
    } else {
        format!("{} was already accepted", args.id)
    };
    Ok(CommandOutput::new(json!({ "design": design, "changed": changed }), human).mutation())
}

fn reject_design(database: &Path, args: &DesignRejectArgs) -> AppResult<CommandOutput> {
    let source = decision_source(&args.source)?;
    let reason = read_text_argument(&args.reason)?;
    validate_nonblank("rejection reason", &reason)?;
    let mut connection = db::open_write(database)?;
    let (design, changed) =
        reconciliation_store::reject_design(&mut connection, args.id, &source, &reason)?;
    let human = if changed {
        format!("Rejected {}", args.id)
    } else {
        format!("{} was already rejected with that decision", args.id)
    };
    Ok(CommandOutput::new(json!({ "design": design, "changed": changed }), human).mutation())
}

fn preview_email(database: &Path, config: &Config) -> AppResult<CommandOutput> {
    let preview = build_email(database, config)?;
    let human = render_email_preview(&preview);
    Ok(CommandOutput::new(to_value(&preview)?, human))
}

fn send_email(database: &Path, config: &Config, args: &EmailSendArgs) -> AppResult<CommandOutput> {
    let preview = build_email(database, config)?;
    let result = email::send(&preview, args.scheduled)?;
    let human = format!(
        "Sent the Todo daily digest with {} items needing attention and {} open todos to {} ({})",
        preview.attention_count, preview.todo_count, preview.to, result.email_id
    );
    Ok(CommandOutput::new(
        json!({
            "email_id": result.email_id,
            "idempotency_key": result.idempotency_key,
            "scheduled": args.scheduled,
            "to": preview.to,
            "attention_count": preview.attention_count,
            "pending_concern_count": preview.pending_concern_count,
            "todo_count": preview.todo_count,
        }),
        human,
    )
    .mutation())
}

fn build_email(database: &Path, config: &Config) -> AppResult<EmailPreview> {
    let email_config = config.email.as_ref().ok_or_else(|| {
        AppError::invalid(
            "email_not_configured",
            "email is not configured; add an [email] section with from and to",
        )
    })?;
    let mut connection = db::open_read(database)?;
    let digest = digest::load(&mut connection)?;
    Ok(EmailPreview::new(email_config, &digest))
}

fn render_email_preview(preview: &EmailPreview) -> String {
    format!(
        "From: {}\nTo: {}\nSubject: {}\n\n{}",
        terminal_text(&preview.from, false),
        terminal_text(&preview.to, false),
        terminal_text(&preview.subject, false),
        preview.text
    )
}

fn initialize(database: &Path) -> AppResult<CommandOutput> {
    db::init(database)?;
    Ok(CommandOutput::new(
        json!({ "database": database.display().to_string() }),
        format!("Initialized Todo database {}", database.display()),
    )
    .mutation())
}

fn list_todos(database: &Path, args: &ListArgs) -> AppResult<CommandOutput> {
    validate_limit(args.limit)?;
    let connection = db::open_read(database)?;
    let todos = todo_store::list(&connection, args.all, args.limit)?;
    let human = render_summaries(
        &todos,
        if args.all {
            "No todos"
        } else {
            "No open todos"
        },
    );
    Ok(CommandOutput::new(json!({ "todos": todos }), human))
}

fn search_todos(database: &Path, args: &SearchArgs) -> AppResult<CommandOutput> {
    validate_limit(args.limit)?;
    if args.query.trim().is_empty() {
        return Err(AppError::invalid(
            "blank_search_query",
            "search query must not be blank",
        ));
    }
    let connection = db::open_read(database)?;
    let todos = todo_store::search(&connection, &args.query, args.all, args.limit)?;
    let human = render_summaries(&todos, "No matching todos");
    Ok(CommandOutput::new(
        json!({ "query": args.query, "todos": todos }),
        human,
    ))
}

fn show_todo(database: &Path, id: TodoId) -> AppResult<CommandOutput> {
    let connection = db::open_read(database)?;
    let view = todo_store::show(&connection, id)?;
    let human = render_todo(&view);
    Ok(CommandOutput::new(to_value(&view)?, human))
}

fn add_note(database: &Path, args: &NoteAddArgs) -> AppResult<CommandOutput> {
    let text = read_text_argument(&args.text)?;
    let mut connection = db::open_write(database)?;
    let note = todo_store::append_note(&mut connection, args.id, &text)?;
    Ok(CommandOutput::new(
        json!({ "todo": args.id, "working_note": note }),
        format!("Added working note to {}", args.id),
    )
    .mutation())
}

fn done(database: &Path, id: TodoId) -> AppResult<CommandOutput> {
    let mut connection = db::open_write(database)?;
    let transition = todo_store::mark_done(&mut connection, id)?;
    let human = if transition.changed {
        format!("Completed {id}")
    } else {
        format!("{id} is already done")
    };
    Ok(CommandOutput::new(
        json!({ "todo": transition.todo, "changed": transition.changed }),
        human,
    )
    .mutation())
}

fn reopen(database: &Path, id: TodoId) -> AppResult<CommandOutput> {
    let mut connection = db::open_write(database)?;
    let transition = todo_store::reopen(&mut connection, id)?;
    let human = if transition.changed {
        format!("Reopened {id}")
    } else {
        format!("{id} is already open")
    };
    Ok(CommandOutput::new(
        json!({ "todo": transition.todo, "changed": transition.changed }),
        human,
    )
    .mutation())
}

fn resolve_new_args(args: &NewArgs) -> AppResult<ResolvedNewArgs> {
    validate_nonblank("direction", &args.direction)?;
    validate_research(args.model.as_deref())?;
    let source = resolve_source(&args.source)?;
    Ok(ResolvedNewArgs {
        direction: args.direction.clone(),
        source,
        quality: args.quality,
        model: args.model.clone(),
    })
}

fn resolve_source(path: &Path) -> AppResult<PathBuf> {
    let source = fs::canonicalize(path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            AppError::not_found(
                "source_not_found",
                format!("source not found: {}", path.display()),
            )
        } else {
            AppError::invalid(
                "source_unreadable",
                format!("unable to resolve source {}: {error}", path.display()),
            )
        }
    })?;
    let metadata = fs::metadata(&source).map_err(|error| {
        AppError::invalid(
            "source_unreadable",
            format!("unable to inspect source {}: {error}", source.display()),
        )
    })?;
    if !metadata.is_file() {
        return Err(AppError::invalid(
            "invalid_source",
            format!("source is not a regular file: {}", source.display()),
        ));
    }
    fs::read_to_string(&source).map_err(|error| {
        AppError::invalid(
            "source_unreadable",
            format!(
                "source must be readable UTF-8 text {}: {error}",
                source.display()
            ),
        )
    })?;
    if source.to_str().is_none() {
        return Err(AppError::invalid(
            "invalid_source_path",
            "resolved source path must contain valid UTF-8",
        ));
    }
    Ok(source)
}

fn decision_source(path: &Path) -> AppResult<DecisionSource> {
    let source = resolve_source(path)?;
    Ok(DecisionSource {
        source_path: source.to_string_lossy().into_owned(),
        thread_id: None,
        turn_id: None,
    })
}

fn validate_research(model: Option<&str>) -> AppResult<()> {
    if model.is_some_and(|model| model.trim().is_empty()) {
        Err(AppError::invalid(
            "invalid_model",
            "model must not be blank",
        ))
    } else {
        Ok(())
    }
}

fn validate_nonblank(name: &str, value: &str) -> AppResult<()> {
    if value.trim().is_empty() {
        Err(AppError::invalid(
            "blank_text",
            format!("{name} must not be blank"),
        ))
    } else {
        Ok(())
    }
}

fn validate_limit(limit: u32) -> AppResult<()> {
    if limit == 0 || limit > MAX_PAGE_LIMIT {
        return Err(AppError::invalid(
            "invalid_limit",
            format!("limit must be between 1 and {MAX_PAGE_LIMIT}"),
        ));
    }
    Ok(())
}

fn read_text_argument(argument: &str) -> AppResult<String> {
    if argument != "-" {
        return Ok(argument.to_owned());
    }
    let mut text = String::new();
    io::stdin().read_to_string(&mut text).map_err(|error| {
        AppError::invalid(
            "stdin_read_failed",
            format!("unable to read text from standard input: {error}"),
        )
    })?;
    Ok(text)
}

fn render_routing(routing: &RoutingProposalView) -> String {
    let mut human = format!(
        "{} [{}] {}\nConcern: {}\nCreated: {}",
        routing.id,
        terminal_text(&routing.decision, false),
        terminal_text(&routing.action, false),
        routing.concern_id,
        terminal_text(&routing.created_at, false),
    );
    if routing.targets.is_empty() {
        human.push_str("\nTargets: none");
    } else {
        human.push_str("\nTargets:");
        for target in &routing.targets {
            let _ = write!(
                human,
                "\n- {} @ direction r{}",
                target.todo_id, target.direction_revision
            );
        }
    }
    if let Some(survivor) = routing.survivor_todo_id {
        let _ = write!(human, "\nSurvivor: {survivor}");
    }
    if let Some(title) = &routing.proposed_title {
        let _ = write!(human, "\nProposed title: {}", terminal_text(title, false));
    }
    if let Some(direction) = &routing.proposed_direction {
        push_text_section(&mut human, "Proposed direction", direction);
    }
    human.push_str("\n\nProposed boundaries");
    if routing.proposed_boundaries.is_empty() {
        human.push_str("\nNone");
    } else {
        for boundary in &routing.proposed_boundaries {
            let _ = write!(
                human,
                "\n- {} [{}; {}]\n  {}",
                terminal_text(&boundary.local_ref, false),
                terminal_text(&boundary.kind, false),
                terminal_text(&boundary.attribution, false),
                indented_text(&boundary.statement, "  "),
            );
            push_string_list(&mut human, "Source references", &boundary.source_refs, "  ");
        }
    }
    push_text_section(&mut human, "Rationale", &routing.rationale);
    push_string_section(&mut human, "Evidence", &routing.evidence_refs);
    push_string_section(&mut human, "Limitations", &routing.limitations);
    push_decision_provenance(
        &mut human,
        routing.decision_source_path.as_deref(),
        routing.decision_thread_id.as_deref(),
        routing.decision_turn_id.as_deref(),
        routing.decision_reason.as_deref(),
        routing.decided_at.as_deref(),
    );
    human
}

fn render_situation(assessment: &SituationAssessmentView) -> String {
    let mut human = format!(
        "{} [{}] {}\nTodo: {}\nDirection revision: r{}\nObserved: {}\nCurrent: {}\nConcern set: {}",
        assessment.id,
        terminal_text(&assessment.disposition, false),
        terminal_text(&assessment.subject_label, false),
        assessment.todo_id,
        assessment.direction_revision,
        terminal_text(&assessment.observed_at, false),
        yes_no(assessment.current),
        terminal_text(&assessment.concern_set_digest, false),
    );
    match assessment.notes_through_id {
        Some(note) => {
            let _ = write!(human, "\nNotes through: {note}");
        }
        None => human.push_str("\nNotes through: none"),
    }
    match assessment.based_on_design_id {
        Some(design) => {
            let _ = write!(human, "\nBased on design: {design}");
        }
        None => human.push_str("\nBased on design: none"),
    }
    push_string_section(&mut human, "Stale reasons", &assessment.stale_reasons);
    push_text_section(&mut human, "Summary", &assessment.summary);
    push_string_section(&mut human, "Identity references", &assessment.identity_refs);
    render_situation_components(
        &mut human,
        &assessment.bases,
        &assessment.findings,
        &assessment.jurisdictions,
        &assessment.direction_mappings,
        &assessment.unresolved,
    );
    human
}

fn render_situation_components(
    human: &mut String,
    bases: &[AssessmentBase],
    findings: &[Value],
    jurisdictions: &[Value],
    direction_mappings: &[Value],
    unresolved_items: &[Value],
) {
    render_assessment_bases(human, bases);
    render_assessment_findings(human, findings);
    render_assessment_jurisdictions(human, jurisdictions);
    render_assessment_mappings(human, direction_mappings);
    render_assessment_unresolved(human, unresolved_items);
}

fn render_assessment_bases(human: &mut String, bases: &[AssessmentBase]) {
    human.push_str("\n\nBases");
    if bases.is_empty() {
        human.push_str("\nNone");
    } else {
        for basis in bases {
            let _ = write!(
                human,
                "\n- {} [{}] {}\n  Revision: {}\n  Observed: {}",
                terminal_text(&basis.source_ref, false),
                terminal_text(&basis.kind, false),
                terminal_text(&basis.locator, false),
                terminal_text(&basis.revision, false),
                terminal_text(&basis.observed_at, false),
            );
        }
    }
}

fn render_assessment_findings(human: &mut String, findings: &[Value]) {
    human.push_str("\n\nFindings");
    if findings.is_empty() {
        human.push_str("\nNone");
    } else {
        for finding in findings {
            let _ = write!(
                human,
                "\n- {} [{}]\n  {}",
                json_text(finding, "ref"),
                json_text(finding, "kind"),
                json_block(finding, "claim", "  "),
            );
            push_json_string_list(human, "Evidence", finding, "evidence_refs", "  ");
        }
    }
}

fn render_assessment_jurisdictions(human: &mut String, jurisdictions: &[Value]) {
    human.push_str("\n\nJurisdictions");
    if jurisdictions.is_empty() {
        human.push_str("\nNone");
    } else {
        for jurisdiction in jurisdictions {
            let _ = write!(
                human,
                "\n- {}\n  Concern: {}",
                json_text(jurisdiction, "key"),
                json_block(jurisdiction, "concern", "  "),
            );
            let assignments = json_array(jurisdiction, "assignments");
            human.push_str("\n  Assignments");
            if assignments.is_empty() {
                human.push_str("\n  None");
            } else {
                for assignment in assignments {
                    render_assignment(human, assignment, "  ");
                }
            }
            push_json_string_list(human, "Evidence", jurisdiction, "evidence_refs", "  ");
        }
    }
}

fn render_assessment_mappings(human: &mut String, direction_mappings: &[Value]) {
    human.push_str("\n\nDirection mappings");
    if direction_mappings.is_empty() {
        human.push_str("\nNone");
    } else {
        for mapping in direction_mappings {
            let _ = write!(
                human,
                "\n- {} [{}]\n  {}",
                json_text(mapping, "boundary_ref"),
                json_text(mapping, "disposition"),
                json_block(mapping, "explanation", "  "),
            );
            push_json_string_list(human, "Finding references", mapping, "finding_refs", "  ");
        }
    }
}

fn render_assessment_unresolved(human: &mut String, unresolved_items: &[Value]) {
    human.push_str("\n\nUnresolved");
    if unresolved_items.is_empty() {
        human.push_str("\nNone");
    } else {
        for unresolved in unresolved_items {
            let _ = write!(
                human,
                "\n- {} [{}]\n  {}\n  Materiality: {}",
                json_text(unresolved, "ref"),
                json_text(unresolved, "kind"),
                json_block(unresolved, "description", "  "),
                json_block(unresolved, "materiality", "  "),
            );
            push_json_string_list(human, "Evidence", unresolved, "evidence_refs", "  ");
        }
    }
}

fn render_design(design: &DesignView) -> String {
    let mut human = format!(
        "{} [{}] revision {}\nTodo: {}\nDraft version: {}\nCreated: {}\nCurrent: {}",
        design.id,
        terminal_text(&design.state, false),
        design.revision,
        design.todo_id,
        design.draft_version,
        terminal_text(&design.created_at, false),
        yes_no(design.current),
    );
    match design.assessment_id {
        Some(assessment) => {
            let _ = write!(human, "\nAssessment: {assessment}");
        }
        None => human.push_str("\nAssessment: none"),
    }
    match design.based_on_design_id {
        Some(predecessor) => {
            let _ = write!(human, "\nPredecessor: {predecessor}");
        }
        None => human.push_str("\nPredecessor: none"),
    }
    if let Some(basis_ref) = &design.correction_basis_ref {
        let _ = write!(
            human,
            "\nCorrection basis: {}",
            terminal_text(basis_ref, false)
        );
    }
    if let Some(feedback) = &design.correction_feedback {
        push_text_section(&mut human, "Correction feedback", feedback);
    }
    push_string_section(&mut human, "Stale reasons", &design.stale_reasons);
    push_text_section(&mut human, "Summary", &design.summary);
    render_design_jurisdictions(&mut human, &design.jurisdiction_changes);
    render_design_clauses(&mut human, &design.clauses);
    render_design_choices(&mut human, &design.unresolved_choices);
    push_decision_provenance(
        &mut human,
        design.decision_source_path.as_deref(),
        design.decision_thread_id.as_deref(),
        design.decision_turn_id.as_deref(),
        design.decision_reason.as_deref(),
        design.decided_at.as_deref(),
    );
    human
}

fn render_design_jurisdictions(human: &mut String, changes: &[Value]) {
    human.push_str("\n\nJurisdiction changes");
    if changes.is_empty() {
        human.push_str("\nNone");
    } else {
        for change in changes {
            let _ = write!(
                human,
                "\n- {} / {} [{}] {} {}\n  Rationale: {}",
                json_text(change, "operation_id"),
                json_text(change, "local_ref"),
                json_text(change, "status"),
                json_text(change, "action"),
                json_text(change, "key"),
                json_block(change, "rationale", "  "),
            );
            render_assignments(
                human,
                "Expected assignments",
                json_array(change, "expected_assignments"),
                "  ",
            );
            render_assignments(
                human,
                "Proposed assignments",
                json_array(change, "proposed_assignments"),
                "  ",
            );
            push_json_string_list(human, "Bases", change, "basis_refs", "  ");
            render_drop(human, change, "  ");
        }
    }
}

fn render_design_clauses(human: &mut String, clauses: &[Value]) {
    human.push_str("\n\nClauses");
    if clauses.is_empty() {
        human.push_str("\nNone");
    } else {
        for clause in clauses {
            let _ = write!(
                human,
                "\n- {} / {} [{}] {} — {}\n  {}",
                json_text(clause, "operation_id"),
                json_text(clause, "local_ref"),
                json_text(clause, "status"),
                json_text(clause, "kind"),
                json_text(clause, "subject"),
                json_block(clause, "statement", "  "),
            );
            if let Some(jurisdiction) = json_optional_text(clause, "jurisdiction_ref") {
                let _ = write!(human, "\n  Jurisdiction: {jurisdiction}");
            }
            push_json_string_list(human, "Bases", clause, "basis_refs", "  ");
            render_drop(human, clause, "  ");
        }
    }
}

fn render_design_choices(human: &mut String, choices: &[Value]) {
    human.push_str("\n\nUnresolved choices");
    if choices.is_empty() {
        human.push_str("\nNone");
    } else {
        for choice in choices {
            let _ = write!(
                human,
                "\n- {} / {} [{}]\n  {}\n  Materiality: {}",
                json_text(choice, "operation_id"),
                json_text(choice, "local_ref"),
                json_text(choice, "status"),
                json_block(choice, "question", "  "),
                json_block(choice, "why_material", "  "),
            );
            push_json_string_list(human, "Bases", choice, "basis_refs", "  ");
            render_drop(human, choice, "  ");
        }
    }
}

fn push_text_section(output: &mut String, title: &str, value: &str) {
    let _ = write!(output, "\n\n{title}\n{}", indented_text(value, ""));
}

fn push_string_section(output: &mut String, title: &str, values: &[String]) {
    let _ = write!(output, "\n\n{title}");
    if values.is_empty() {
        output.push_str("\nNone");
    } else {
        for value in values {
            let _ = write!(output, "\n- {}", terminal_text(value, false));
        }
    }
}

fn push_string_list(output: &mut String, label: &str, values: &[String], indent: &str) {
    let _ = write!(output, "\n{indent}{label}:");
    if values.is_empty() {
        output.push_str(" none");
    } else {
        for value in values {
            let _ = write!(output, "\n{indent}  - {}", terminal_text(value, false));
        }
    }
}

fn push_json_string_list(output: &mut String, label: &str, value: &Value, key: &str, indent: &str) {
    let _ = write!(output, "\n{indent}{label}:");
    let values = json_array(value, key);
    if values.is_empty() {
        output.push_str(" none");
    } else {
        for item in values {
            let text = item.as_str().unwrap_or("<invalid>");
            let _ = write!(output, "\n{indent}  - {}", terminal_text(text, false));
        }
    }
}

fn render_assignments(output: &mut String, label: &str, values: &[Value], indent: &str) {
    let _ = write!(output, "\n{indent}{label}:");
    if values.is_empty() {
        output.push_str(" none");
    } else {
        for assignment in values {
            render_assignment(output, assignment, indent);
        }
    }
}

fn render_assignment(output: &mut String, assignment: &Value, indent: &str) {
    let _ = write!(
        output,
        "\n{indent}  - {} [{}]\n{indent}    {}",
        json_text(assignment, "party"),
        json_text(assignment, "role"),
        json_block(assignment, "responsibility", &format!("{indent}    ")),
    );
}

fn render_drop(output: &mut String, operation: &Value, indent: &str) {
    let Some(drop) = operation.get("drop").filter(|value| !value.is_null()) else {
        return;
    };
    let _ = write!(
        output,
        "\n{indent}Drop:\n{indent}  Reason: {}\n{indent}  At: {}",
        json_block(drop, "reason", &format!("{indent}  ")),
        json_text(drop, "dropped_at"),
    );
    push_json_string_list(output, "Bases", drop, "basis_refs", &format!("{indent}  "));
}

fn push_decision_provenance(
    output: &mut String,
    source: Option<&str>,
    thread: Option<&str>,
    turn: Option<&str>,
    reason: Option<&str>,
    decided_at: Option<&str>,
) {
    output.push_str("\n\nDecision provenance");
    if source.is_none()
        && thread.is_none()
        && turn.is_none()
        && reason.is_none()
        && decided_at.is_none()
    {
        output.push_str("\nNone");
        return;
    }
    if let Some(source) = source {
        let _ = write!(output, "\nSource: {}", terminal_text(source, false));
    }
    if let Some(thread) = thread {
        let _ = write!(output, "\nThread: {}", terminal_text(thread, false));
    }
    if let Some(turn) = turn {
        let _ = write!(output, "\nTurn: {}", terminal_text(turn, false));
    }
    if let Some(reason) = reason {
        let _ = write!(output, "\nReason: {}", indented_text(reason, ""));
    }
    if let Some(decided_at) = decided_at {
        let _ = write!(output, "\nDecided: {}", terminal_text(decided_at, false));
    }
}

fn json_text(value: &Value, key: &str) -> String {
    terminal_text(
        value
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or("<missing>"),
        false,
    )
}

fn json_optional_text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(|text| terminal_text(text, false))
}

fn json_block(value: &Value, key: &str, indent: &str) -> String {
    let text = value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("<missing>");
    indented_text(text, indent)
}

fn json_array<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn indented_text(value: &str, indent: &str) -> String {
    terminal_text(value, true).replace('\n', &format!("\n{indent}"))
}

const fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn render_summaries(todos: &[TodoSummary], empty: &str) -> String {
    if todos.is_empty() {
        return empty.to_owned();
    }
    todos
        .iter()
        .map(|todo| {
            format!(
                "{}\t{}\t{}",
                todo.id,
                todo.status,
                terminal_text(&todo.title, false)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_todo(view: &TodoView) -> String {
    let todo = &view.todo;
    let mut human = format!(
        "{} [{}] {}\nCreated: {}\nDirection r{}:\n{}",
        todo.id,
        todo.status,
        terminal_text(&todo.title, false),
        terminal_text(&todo.created_at, false),
        todo.direction_revision,
        terminal_text(&todo.direction, true),
    );
    if let Some(completed_at) = &todo.completed_at {
        let _ = write!(human, "\nCompleted: {}", terminal_text(completed_at, false));
    }
    if view.requested_id != todo.id {
        let path = view
            .resolution_path
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" -> ");
        let _ = write!(human, "\nResolved: {path}");
    }
    if !view.concerns.is_empty() {
        human.push_str("\n\nConcerns");
        for concern in &view.concerns {
            let _ = write!(
                human,
                "\n\n{} ({})\n{}\nSource: {}",
                concern.id,
                concern.attached_todo_id,
                terminal_text(&concern.body, true),
                terminal_text(&concern.source_path.display().to_string(), false),
            );
        }
    }
    if let Some(assessment) = &view.latest_assessment {
        let _ = write!(
            human,
            "\n\nLatest assessment {} [{}]\n{}",
            assessment.id,
            terminal_text(&assessment.disposition, false),
            terminal_text(&assessment.summary, true),
        );
    }
    if let Some(design) = &view.latest_design {
        let _ = write!(
            human,
            "\n\nLatest design {} [{}]\n{}",
            design.id,
            terminal_text(&design.state, false),
            terminal_text(&design.summary, true),
        );
    }
    if !view.working_notes.is_empty() {
        human.push_str("\n\nWorking notes");
        for note in &view.working_notes {
            let _ = write!(
                human,
                "\n\n{}\n{}",
                terminal_text(&note.created_at, false),
                terminal_text(&note.text, true)
            );
        }
    }
    human
}

fn to_value<T: Serialize>(value: &T) -> AppResult<Value> {
    serde_json::to_value(value).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::path::Path;

    use serde_json::json;

    use super::{
        render_design, render_routing, render_situation_components, resolve_database_path,
    };
    use crate::reconciliation_store::{
        AssessmentBase, DesignView, DirectionBoundary, RoutingProposalView, RoutingTargetView,
    };

    #[test]
    fn database_selection_has_explicit_environment_config_precedence()
    -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            resolve_database_path(
                Some(Path::new("explicit.db")),
                Some(OsStr::new("environment.db")),
                Some(Path::new("configured.db"))
            )?,
            Path::new("explicit.db")
        );
        assert_eq!(
            resolve_database_path(
                None,
                Some(OsStr::new("environment.db")),
                Some(Path::new("configured.db"))
            )?,
            Path::new("environment.db")
        );
        assert_eq!(
            resolve_database_path(None, Some(OsStr::new("")), Some(Path::new("configured.db")))?,
            Path::new("configured.db")
        );
        Ok(())
    }

    #[test]
    fn routing_render_includes_the_complete_review_surface() {
        let routing = RoutingProposalView {
            id: "r1".parse().unwrap_or_else(|error| panic!("{error}")),
            concern_id: "c2".parse().unwrap_or_else(|error| panic!("{error}")),
            action: "revise".to_owned(),
            targets: vec![RoutingTargetView {
                todo_id: "t3".parse().unwrap_or_else(|error| panic!("{error}")),
                direction_revision: 4,
            }],
            survivor_todo_id: None,
            proposed_title: Some("Revised title".to_owned()),
            proposed_direction: Some("Keep the boundary\nMake it explicit".to_owned()),
            proposed_boundaries: vec![DirectionBoundary {
                id: 1,
                local_ref: "b1".to_owned(),
                kind: "required".to_owned(),
                statement: "Todo owns the record".to_owned(),
                attribution: "explicit_user".to_owned(),
                source_refs: vec!["source:12".to_owned()],
            }],
            rationale: "The identity remains stable\u{1b}".to_owned(),
            evidence_refs: vec!["source:12".to_owned()],
            limitations: vec!["Deployment state was not inspected".to_owned()],
            decision: "authorized".to_owned(),
            decision_source_path: Some("/tmp/decision.jsonl".to_owned()),
            decision_thread_id: Some("thread-1".to_owned()),
            decision_turn_id: Some("turn-2".to_owned()),
            decision_reason: None,
            decided_at: Some("2026-08-28T12:00:00.000Z".to_owned()),
            created_at: "2026-08-28T11:00:00.000Z".to_owned(),
        };

        let rendered = render_routing(&routing);
        for expected in [
            "t3 @ direction r4",
            "Proposed title: Revised title",
            "Proposed direction",
            "Make it explicit",
            "b1 [required; explicit_user]",
            "Todo owns the record",
            "Source references:",
            "Evidence",
            "Limitations",
            "Deployment state was not inspected",
            "Decision provenance",
            "/tmp/decision.jsonl",
            "thread-1",
            "turn-2",
        ] {
            assert!(
                rendered.contains(expected),
                "missing {expected:?}: {rendered}"
            );
        }
        assert!(rendered.contains("\\u{1b}"));
        assert!(!rendered.contains('\u{1b}'));
    }

    #[test]
    fn situation_components_render_bases_and_every_grounded_collection() {
        let mut rendered = String::new();
        render_situation_components(
            &mut rendered,
            &[AssessmentBase {
                source_ref: "source:s-fixture".to_owned(),
                kind: "git".to_owned(),
                locator: "/workspace".to_owned(),
                revision: "abc123".to_owned(),
                observed_at: "2026-08-28T10:00:00.000Z".to_owned(),
            }],
            &[json!({
                "ref": "f1",
                "kind": "current_state",
                "claim": "The contract is present",
                "evidence_refs": ["source:s-fixture@line:1"]
            })],
            &[json!({
                "key": "j-runtime",
                "concern": "Runtime authority",
                "assignments": [
                    {"party": "Nucleus", "role": "owner", "responsibility": "Own runtime state"},
                    {"party": "Todo", "role": "consumer", "responsibility": "Read the result"}
                ],
                "evidence_refs": ["source:s-fixture@line:2"]
            })],
            &[json!({
                "boundary_ref": "b1",
                "disposition": "satisfied",
                "finding_refs": ["f1"],
                "explanation": "The current contract satisfies the boundary"
            })],
            &[json!({
                "ref": "u1",
                "kind": "evidence_gap",
                "description": "Installed state is unknown\u{1b}",
                "materiality": "It changes the currentness judgment",
                "evidence_refs": ["source:s-fixture@line:3"]
            })],
        );

        for expected in [
            "Bases",
            "source:s-fixture",
            "[git] /workspace",
            "Revision: abc123",
            "Findings",
            "f1 [current_state]",
            "source:s-fixture@line:1",
            "Jurisdictions",
            "j-runtime",
            "Nucleus [owner]",
            "Todo [consumer]",
            "Direction mappings",
            "b1 [satisfied]",
            "Finding references:",
            "Unresolved",
            "u1 [evidence_gap]",
            "It changes the currentness judgment",
        ] {
            assert!(
                rendered.contains(expected),
                "missing {expected:?}: {rendered}"
            );
        }
        assert!(rendered.contains("\\u{1b}"));
        assert!(!rendered.contains('\u{1b}'));
    }

    #[test]
    fn design_render_includes_lineage_operations_drops_and_decision() {
        let design = DesignView {
            id: "d5".parse().unwrap_or_else(|error| panic!("{error}")),
            todo_id: "t3".parse().unwrap_or_else(|error| panic!("{error}")),
            revision: 2,
            assessment_id: Some("a4".parse().unwrap_or_else(|error| panic!("{error}"))),
            based_on_design_id: Some("d2".parse().unwrap_or_else(|error| panic!("{error}"))),
            draft_version: 3,
            state: "rejected".to_owned(),
            summary: "A complete desired state".to_owned(),
            current: false,
            stale_reasons: vec!["situation assessment is no longer current".to_owned()],
            jurisdiction_changes: vec![json!({
                "operation_id": "op-1",
                "local_ref": "jc1",
                "key": "j-runtime",
                "action": "move",
                "rationale": "Move runtime authority",
                "status": "active",
                "expected_assignments": [
                    {"party": "Todo", "role": "owner", "responsibility": "Own runtime state"}
                ],
                "proposed_assignments": [
                    {"party": "Nucleus", "role": "owner", "responsibility": "Own runtime state"},
                    {"party": "Todo", "role": "consumer", "responsibility": "Read results"}
                ],
                "basis_refs": ["assessment:a4:jurisdiction:j-runtime"],
                "drop": null
            })],
            clauses: vec![json!({
                "operation_id": "op-2",
                "local_ref": "dc1",
                "kind": "boundary",
                "subject": "Runtime records",
                "statement": "Nucleus is authoritative",
                "jurisdiction_ref": "j-runtime",
                "status": "dropped",
                "basis_refs": ["assessment:a4:jurisdiction:j-runtime", "direction:b1"],
                "drop": {
                    "reason": "Superseded by a narrower clause",
                    "basis_refs": ["correction:12"],
                    "dropped_at": "2026-08-28T12:00:00.000Z"
                }
            })],
            unresolved_choices: vec![json!({
                "operation_id": "op-3",
                "local_ref": "choice1",
                "question": "Which retention period?",
                "why_material": "It changes storage obligations",
                "status": "active",
                "basis_refs": ["direction:body"],
                "drop": null
            })],
            correction_basis_ref: Some("correction:12".to_owned()),
            correction_feedback: Some("Preserve the explicit runtime boundary\u{1b}".to_owned()),
            decision_source_path: Some("/tmp/decision.jsonl".to_owned()),
            decision_thread_id: Some("thread-9".to_owned()),
            decision_turn_id: Some("turn-4".to_owned()),
            decision_reason: Some("Boundary needs correction".to_owned()),
            decided_at: Some("2026-08-28T13:00:00.000Z".to_owned()),
            created_at: "2026-08-28T11:00:00.000Z".to_owned(),
        };

        let rendered = render_design(&design);
        for expected in [
            "Assessment: a4",
            "Predecessor: d2",
            "Correction basis: correction:12",
            "Correction feedback",
            "Preserve the explicit runtime boundary",
            "Draft version: 3",
            "situation assessment is no longer current",
            "op-1 / jc1 [active] move j-runtime",
            "Expected assignments:",
            "Todo [owner]",
            "Proposed assignments:",
            "Nucleus [owner]",
            "assessment:a4:jurisdiction:j-runtime",
            "op-2 / dc1 [dropped] boundary — Runtime records",
            "Jurisdiction: j-runtime",
            "Drop:",
            "Superseded by a narrower clause",
            "correction:12",
            "op-3 / choice1 [active]",
            "Which retention period?",
            "Decision provenance",
            "/tmp/decision.jsonl",
            "Boundary needs correction",
        ] {
            assert!(
                rendered.contains(expected),
                "missing {expected:?}: {rendered}"
            );
        }
        assert!(rendered.contains("\\u{1b}"));
        assert!(!rendered.contains('\u{1b}'));
    }
}
