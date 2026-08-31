use std::env;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::cli::{Cli, Command, ListArgs};
use crate::error::AppError;
use crate::model::{EntryKind, Issue, LoadedEntry, Mode, Registry};
use crate::registry::{load_bundle, load_registry, validate_internal_dependencies};
use crate::render::{CommandOutput, terminal_text};

pub(crate) fn run(cli: &Cli) -> Result<CommandOutput, AppError> {
    match &cli.command {
        Command::Validate(args) => Ok(validate_bundle(&args.bundle)),
        Command::List(args) => {
            let registry = load_registry(&registry_path(cli.registry.as_deref())?)?;
            Ok(list(&registry, args))
        }
        Command::Show(args) => {
            let registry = load_registry(&registry_path(cli.registry.as_deref())?)?;
            show(&registry, &args.id)
        }
        Command::Doctor => {
            let registry = load_registry(&registry_path(cli.registry.as_deref())?)?;
            Ok(doctor(&registry))
        }
    }
}

fn registry_path(selected: Option<&Path>) -> Result<PathBuf, AppError> {
    if let Some(selected) = selected {
        if !selected.is_absolute() {
            return Err(AppError::invalid(
                "invalid_registry_path",
                format!("registry path must be absolute: {}", selected.display()),
            ));
        }
        return Ok(selected.to_path_buf());
    }
    let home = env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            AppError::invalid(
                "home_unavailable",
                "HOME is required when --registry and CHANCERY_REGISTRY are unset",
            )
        })?;
    let home = PathBuf::from(home);
    if !home.is_absolute() {
        return Err(AppError::invalid(
            "home_unavailable",
            "HOME must be an absolute path",
        ));
    }
    Ok(home.join("Library/Application Support/Chancery/providers"))
}

fn list(registry: &Registry, args: &ListArgs) -> CommandOutput {
    let mut values = Vec::new();
    let mut human = String::from("Installed Chancery catalog\n");
    let mut count = 0;
    for mode in [Mode::Use, Mode::Operate, Mode::Develop] {
        if args.mode.is_some_and(|selected| selected != mode) {
            continue;
        }
        let mut mode_entries: Vec<_> = registry
            .entries()
            .filter(|(_, entry)| {
                entry.document.mode == mode
                    && args
                        .kind
                        .is_none_or(|selected| selected == entry.document.kind)
            })
            .collect();
        mode_entries.sort_by(|left, right| left.1.document.id.cmp(&right.1.document.id));
        if mode_entries.is_empty() {
            continue;
        }
        let _ = write!(human, "\n{}\n", mode_heading(mode));
        for (provider, entry) in mode_entries {
            let document = &entry.document;
            let availability = availability(entry);
            let compatibility = compatibility(entry);
            let readiness = readiness(entry);
            values.push(json!({
                "id": document.id,
                "title": document.title,
                "summary": document.summary,
                "kind": document.kind,
                "mode": document.mode,
                "provider": provider,
                "provider_release": provider.release,
                "contract_version": document.contract_version,
                "support": document.support,
                "availability": availability,
                "compatibility": compatibility,
                "readiness": readiness,
            }));
            let _ = write!(
                human,
                "\n{} — {}\n  {}\n  {} · {} · {} {} · {} · {} · {} · {}\n",
                terminal_text(&document.id, false),
                terminal_text(&document.title, false),
                terminal_text(&document.summary, false),
                document.kind.as_str(),
                document.mode.as_str(),
                terminal_text(&provider.name, false),
                terminal_text(&provider.release, false),
                document.support.as_str(),
                availability,
                compatibility,
                readiness
            );
            count += 1;
        }
    }
    if count == 0 {
        human.push_str("\nNo installed entries match the selected filters\n");
    }
    let _ = write!(
        human,
        "\n{} entr{}",
        count,
        if count == 1 { "y" } else { "ies" }
    );
    append_issues(&mut human, &registry.issues);
    CommandOutput::success(
        json!({
            "entries": values,
            "issues": registry.issues,
        }),
        human,
    )
}

fn mode_heading(mode: Mode) -> &'static str {
    match mode {
        Mode::Use => "USE — ordinary outcome work",
        Mode::Operate => "OPERATE — administration, diagnosis, and recovery",
        Mode::Develop => "DEVELOP — implementation and integration changes",
    }
}

fn show(registry: &Registry, id: &str) -> Result<CommandOutput, AppError> {
    let Some((provider, entry)) = registry
        .entries()
        .find(|(_, entry)| entry.document.id == id)
    else {
        return Err(AppError::invalid(
            "entry_not_found",
            format!("installed entry not found: {id}"),
        ));
    };
    let document = &entry.document;
    let availability = availability(entry);
    let compatibility = compatibility(entry);
    let readiness = readiness(entry);
    let mut human = format!(
        "{}\n{}\n\nKind:              {}\nMode:              {}\nOwner:             {}\nProvider release:  {}\nSupport:           {}\nAvailability:      {}\nCompatibility:     {}\nReadiness:         {}\nContract version:  {}\n",
        terminal_text(&document.id, false),
        terminal_text(&document.title, false),
        document.kind.as_str(),
        document.mode.as_str(),
        terminal_text(&provider.name, false),
        terminal_text(&provider.release, false),
        document.support.as_str(),
        availability,
        compatibility,
        readiness,
        document.contract_version
    );
    append_entry_details(&mut human, entry);
    append_issues(&mut human, &registry.issues);
    Ok(CommandOutput::success(
        json!({
            "provider": provider,
            "entry": document,
            "availability": availability,
            "compatibility": compatibility,
            "readiness": readiness,
            "dependency_statuses": entry.dependency_statuses,
            "manual": entry.manual_text,
            "issues": registry.issues,
        }),
        human,
    ))
}

fn append_entry_details(human: &mut String, entry: &LoadedEntry) {
    let document = &entry.document;
    if document.kind == EntryKind::Operation {
        if let Some(runtime) = &document.runtime {
            let _ = writeln!(
                human,
                "Runtime:           {}",
                terminal_text(runtime, false)
            );
        }
        if let Some(automation) = &document.automation {
            let _ = writeln!(
                human,
                "Automation:        {}",
                terminal_text(automation, false)
            );
        }
    }
    append_section(human, "USE WHEN", &document.use_when);
    append_section(human, "DO NOT USE WHEN", &document.do_not_use_when);
    append_paragraph(human, "OUTCOME", &document.outcome);
    if !document.interfaces.is_empty() {
        human.push_str("\nINTERFACES\n");
        for interface in &document.interfaces {
            let _ = writeln!(
                human,
                "\n  {}:\n    {}",
                terminal_text(&interface.label, false),
                terminal_text(&interface.invocation, false)
            );
        }
    }
    append_section(human, "EFFECTS", &document.effects);
    append_section(human, "AUTHORITY", &document.authority);
    append_section(human, "SUCCESS", &document.success);
    append_section(
        human,
        "FAILURE AND RECOVERY",
        &document.failure_and_recovery,
    );
    append_section(human, "PRIVACY", &document.privacy);
    append_section(human, "DOES NOT AUTHORIZE", &document.does_not_authorize);
    if document.kind == EntryKind::Operation {
        append_section(human, "OPERATION", &document.steps);
        append_section(human, "CHECKPOINTS", &document.checkpoints);
        append_section(human, "ADAPTATION", &document.adaptation);
        append_section(human, "STOP WHEN", &document.stop_when);
    }
    append_dependencies(human, entry);
    append_section(human, "SESSION SURFACES", &document.session_surfaces);
    human.push_str("\nDETAILS\n\n");
    human.push_str(&terminal_text(&entry.manual_text, true));
    if !human.ends_with('\n') {
        human.push('\n');
    }
}

fn append_dependencies(human: &mut String, entry: &LoadedEntry) {
    if entry.dependency_statuses.is_empty() {
        return;
    }
    human.push_str("\nDEPENDENCIES\n");
    for dependency in &entry.dependency_statuses {
        let installed = dependency
            .installed_contract
            .map_or_else(|| "not installed".to_owned(), |value| value.to_string());
        let _ = writeln!(
            human,
            "\n  {}: {} (required >= {}, < {}; installed {})",
            terminal_text(&dependency.id, false),
            dependency.state.as_str(),
            dependency.min_contract,
            dependency.max_contract_exclusive,
            installed
        );
    }
}

fn doctor(registry: &Registry) -> CommandOutput {
    let valid = registry.issues.is_empty();
    let provider_values: Vec<Value> = registry
        .providers
        .iter()
        .map(|provider| {
            json!({
                "id": provider.identity.id,
                "name": provider.identity.name,
                "release": provider.identity.release,
                "root": provider.root,
                "entries": provider.entries.len(),
            })
        })
        .collect();
    let mut human = format!("Chancery registry\n  root: {}\n\n", registry.root.display());
    for provider in &registry.providers {
        let _ = writeln!(
            human,
            "PASS  {}  {}  {} entr{}",
            terminal_text(&provider.identity.id, false),
            terminal_text(&provider.identity.release, false),
            provider.entries.len(),
            if provider.entries.len() == 1 {
                "y"
            } else {
                "ies"
            }
        );
    }
    let excluded = registry
        .scanned_providers
        .saturating_sub(registry.providers.len());
    let _ = write!(
        human,
        "\nProviders: {} valid, {} excluded\nEntries:   {}\nStatus:    {}",
        registry.providers.len(),
        excluded,
        registry.entry_count(),
        if valid { "valid" } else { "invalid" }
    );
    append_issues(&mut human, &registry.issues);
    CommandOutput::report(
        valid,
        json!({
            "valid": valid,
            "registry": registry.root,
            "providers": provider_values,
            "counts": {
                "scanned_providers": registry.scanned_providers,
                "valid_providers": registry.providers.len(),
                "excluded_providers": excluded,
                "entries": registry.entry_count(),
            },
            "issues": registry.issues,
        }),
        human,
        i32::from(!valid),
    )
}

fn validate_bundle(bundle: &Path) -> CommandOutput {
    match load_bundle(bundle) {
        Ok(provider) => {
            let issues = validate_internal_dependencies(&provider);
            if !issues.is_empty() {
                return invalid_bundle(bundle, &issues);
            }
            let human = format!(
                "PASS  {} {}\nBundle: {}\nEntries: {}\nExternal dependencies: not checked",
                terminal_text(&provider.identity.name, false),
                terminal_text(&provider.identity.release, false),
                provider.root.display(),
                provider.entries.len()
            );
            CommandOutput::success(
                json!({
                    "valid": true,
                    "provider": provider.identity,
                    "bundle": provider.root,
                    "entries": provider.entries.len(),
                    "external_dependencies": "not_checked",
                    "issues": [],
                }),
                human,
            )
        }
        Err(issues) => invalid_bundle(bundle, &issues),
    }
}

fn invalid_bundle(bundle: &Path, issues: &[Issue]) -> CommandOutput {
    let mut human = format!("FAIL  {}\nStatus: invalid", bundle.display());
    append_issues(&mut human, issues);
    CommandOutput::report(
        false,
        json!({
            "valid": false,
            "bundle": bundle,
            "external_dependencies": "not_checked",
            "issues": issues,
        }),
        human,
        1,
    )
}

fn availability(_: &LoadedEntry) -> &'static str {
    "installed"
}

fn compatibility(entry: &LoadedEntry) -> &'static str {
    if entry.compatible {
        "compatible"
    } else {
        "unavailable"
    }
}

fn readiness(entry: &LoadedEntry) -> &'static str {
    if entry.document.session_surfaces.is_empty() {
        "not_checked"
    } else {
        "session_dependent"
    }
}

fn append_paragraph(human: &mut String, heading: &str, paragraph: &str) {
    let _ = write!(
        human,
        "\n{heading}\n\n  {}\n",
        terminal_text(paragraph, false)
    );
}

fn append_section(human: &mut String, heading: &str, values: &[String]) {
    if values.is_empty() {
        return;
    }
    let _ = write!(human, "\n{heading}\n");
    for value in values {
        let _ = write!(human, "\n  - {}", terminal_text(value, false));
    }
    human.push('\n');
}

fn append_issues(human: &mut String, issues: &[Issue]) {
    if issues.is_empty() {
        return;
    }
    human.push_str("\n\nISSUES\n");
    for issue in issues {
        let context = issue
            .provider
            .as_deref()
            .or(issue.entry.as_deref())
            .unwrap_or("registry");
        let _ = write!(
            human,
            "\n  {} [{}]: {}",
            terminal_text(context, false),
            terminal_text(&issue.code, false),
            terminal_text(&issue.message, false)
        );
    }
    human.push('\n');
}
