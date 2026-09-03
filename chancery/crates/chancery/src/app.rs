use std::collections::BTreeSet;
use std::env;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Value, json};

use crate::cli::{Cli, Command, ListArgs, ResolveArgs};
use crate::error::AppError;
use crate::model::{
    ClaimStatus, EntryKind, Issue, LoadedEntry, Mode, PromiseFacet, ProviderBundle, Registry,
};
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
        Command::Resolve(args) => {
            let registry = load_registry(&registry_path(cli.registry.as_deref())?)?;
            resolve(&registry, args)
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
    let Some((provider, entry)) = registry.find_entry(id) else {
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
        terminal_text(&provider.identity.name, false),
        terminal_text(&provider.identity.release, false),
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
            "provider": provider.identity,
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

#[derive(Debug, Serialize)]
struct ResolutionGap {
    code: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    entry: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    facet: Option<String>,
}

struct ResolutionSummary<'a> {
    status: &'a str,
    declaration_status: &'a str,
    dependency_closure_status: &'a str,
}

impl ResolutionGap {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            entry: None,
            facet: None,
        }
    }

    #[must_use]
    fn entry(mut self, entry: impl Into<String>) -> Self {
        self.entry = Some(entry.into());
        self
    }

    #[must_use]
    fn facet(mut self, facet: PromiseFacet) -> Self {
        self.facet = Some(facet.as_str().to_owned());
        self
    }
}

const NORMALIZED_PROMISE_FACETS: [PromiseFacet; 12] = [
    PromiseFacet::Consumers,
    PromiseFacet::Preconditions,
    PromiseFacet::Inputs,
    PromiseFacet::Outputs,
    PromiseFacet::DataSemantics,
    PromiseFacet::IdentityAndUnits,
    PromiseFacet::CompletenessAndFreshness,
    PromiseFacet::Access,
    PromiseFacet::LifecycleAndConsistency,
    PromiseFacet::OperationalLimits,
    PromiseFacet::CompatibilityAndEvolution,
    PromiseFacet::Reliances,
];

fn resolve(registry: &Registry, args: &ResolveArgs) -> Result<CommandOutput, AppError> {
    validate_contract_range(args)?;
    let Some((root_provider, root_entry)) = registry.find_entry(&args.id) else {
        return Err(AppError::invalid(
            "entry_not_found",
            format!("installed entry not found: {}", args.id),
        ));
    };

    let closure = dependency_closure(registry, root_entry);

    let mut gaps = Vec::new();
    let mut declarations_complete = collect_contract_gaps(root_provider, root_entry, &mut gaps);
    for (provider, entry) in &closure {
        declarations_complete &= collect_contract_gaps(provider, entry, &mut gaps);
    }

    let (required, unsatisfied) = check_facet_requirements(args, root_entry, &mut gaps);

    let contract_matches = contract_matches(root_entry, args);
    if !contract_matches {
        gaps.push(
            ResolutionGap::new(
                "contract_incompatible",
                format!(
                    "installed contract {} for {} is outside the requested version bounds",
                    root_entry.document.contract_version, root_entry.document.id
                ),
            )
            .entry(&root_entry.document.id),
        );
    }

    let status = if !contract_matches {
        "contract_incompatible"
    } else if !root_entry.compatible {
        "dependency_unavailable"
    } else if !declarations_complete || !unsatisfied.is_empty() {
        "incomplete_declaration"
    } else {
        "resolved_not_ready"
    };
    let documentation_resolved = status == "resolved_not_ready";
    let dependency_closure_status = if root_entry.compatible {
        "complete"
    } else {
        "unavailable"
    };
    let declaration_status = if declarations_complete {
        "complete"
    } else {
        "incomplete"
    };
    let root_value = contract_dossier(root_provider, root_entry);
    let closure_values: Vec<_> = closure
        .iter()
        .map(|(provider, entry)| contract_dossier(provider, entry))
        .collect();
    let required_values: Vec<_> = required.iter().map(|facet| facet.as_str()).collect();
    let unsatisfied_values: Vec<_> = unsatisfied.iter().map(|facet| facet.as_str()).collect();

    let human = resolution_human(
        registry,
        root_provider,
        root_entry,
        &closure,
        &gaps,
        &ResolutionSummary {
            status,
            declaration_status,
            dependency_closure_status,
        },
    );

    Ok(CommandOutput::report(
        documentation_resolved,
        json!({
            "requested_id": args.id,
            "status": status,
            "contract_requirement": {
                "min_contract": args.min_contract,
                "max_contract_exclusive": args.max_contract_exclusive,
                "satisfied": contract_matches,
            },
            "facet_requirements": {
                "required": required_values,
                "unsatisfied": unsatisfied_values,
            },
            "declaration_status": declaration_status,
            "dependency_closure_status": dependency_closure_status,
            "readiness": readiness(root_entry),
            "root": root_value,
            "dependency_closure": closure_values,
            "gaps": gaps,
            "issues": registry.issues,
        }),
        human,
        i32::from(!documentation_resolved),
    ))
}

fn dependency_closure<'a>(
    registry: &'a Registry,
    root: &LoadedEntry,
) -> Vec<(&'a ProviderBundle, &'a LoadedEntry)> {
    let mut pending: BTreeSet<String> = root
        .document
        .dependencies
        .iter()
        .map(|dependency| dependency.id.clone())
        .collect();
    let mut visited = BTreeSet::new();
    let mut closure = Vec::new();
    while let Some(id) = pending.pop_first() {
        if id == root.document.id || !visited.insert(id.clone()) {
            continue;
        }
        let Some((provider, entry)) = registry.find_entry(&id) else {
            continue;
        };
        pending.extend(
            entry
                .document
                .dependencies
                .iter()
                .map(|dependency| dependency.id.clone()),
        );
        closure.push((provider, entry));
    }
    closure.sort_by(|left, right| left.1.document.id.cmp(&right.1.document.id));
    closure
}

fn check_facet_requirements(
    args: &ResolveArgs,
    root: &LoadedEntry,
    gaps: &mut Vec<ResolutionGap>,
) -> (BTreeSet<PromiseFacet>, Vec<PromiseFacet>) {
    let required: BTreeSet<_> = args.require.iter().copied().collect();
    let unsatisfied: Vec<_> = required
        .iter()
        .copied()
        .filter(|facet| !facet_has_declared_claim(root, *facet))
        .collect();
    for facet in &unsatisfied {
        gaps.push(
            ResolutionGap::new(
                "required_facet_unsatisfied",
                format!(
                    "{} has no declared positive claim for required facet {}",
                    root.document.id,
                    facet.as_str()
                ),
            )
            .entry(&root.document.id)
            .facet(*facet),
        );
    }
    (required, unsatisfied)
}

fn resolution_human(
    registry: &Registry,
    root_provider: &ProviderBundle,
    root_entry: &LoadedEntry,
    closure: &[(&ProviderBundle, &LoadedEntry)],
    gaps: &[ResolutionGap],
    summary: &ResolutionSummary<'_>,
) -> String {
    let mut human = format!(
        "{}\nResolved outward promise\n\nStatus:              {}\nDocumentation:       {}\nDependency closure:  {}\nReadiness:           {}\n",
        terminal_text(&root_entry.document.id, false),
        summary.status,
        summary.declaration_status,
        summary.dependency_closure_status,
        readiness(root_entry),
    );
    append_resolved_contract_human(&mut human, root_provider, root_entry, "ROOT CONTRACT");
    if closure.is_empty() {
        human.push_str("\nDEPENDENCY CONTRACTS\n\n  None.\n");
    } else {
        for (provider, entry) in closure {
            append_resolved_contract_human(&mut human, provider, entry, "DEPENDENCY CONTRACT");
        }
    }
    append_resolution_gaps(&mut human, gaps);
    append_issues(&mut human, &registry.issues);
    human
}

fn validate_contract_range(args: &ResolveArgs) -> Result<(), AppError> {
    if args.min_contract == Some(0) {
        return Err(AppError::invalid(
            "invalid_contract_range",
            "--min-contract must be positive",
        ));
    }
    if args
        .max_contract_exclusive
        .is_some_and(|maximum| maximum <= 1)
    {
        return Err(AppError::invalid(
            "invalid_contract_range",
            "--max-contract-exclusive must be greater than 1",
        ));
    }
    if let (Some(minimum), Some(maximum)) = (args.min_contract, args.max_contract_exclusive)
        && maximum <= minimum
    {
        return Err(AppError::invalid(
            "invalid_contract_range",
            "--max-contract-exclusive must be greater than --min-contract",
        ));
    }
    Ok(())
}

fn contract_matches(entry: &LoadedEntry, args: &ResolveArgs) -> bool {
    args.min_contract
        .is_none_or(|minimum| entry.document.contract_version >= minimum)
        && args
            .max_contract_exclusive
            .is_none_or(|maximum| entry.document.contract_version < maximum)
}

fn contract_dossier(provider: &ProviderBundle, entry: &LoadedEntry) -> Value {
    json!({
        "provider": provider.identity,
        "provider_schema_version": provider.schema_version,
        "provider_promise_scope": provider.promise_scope,
        "entry": entry.document,
        "facet_coverage": facet_coverage(entry),
        "availability": availability(entry),
        "compatibility": compatibility(entry),
        "readiness": readiness(entry),
        "dependency_statuses": entry.dependency_statuses,
        "manual": entry.manual_text,
        "basis": {
            "provider_manifest": {
                "path": "provider.json",
                "sha256": provider.manifest_sha256,
            },
            "entry_contract": {
                "path": entry.source_path,
                "sha256": entry.source_sha256,
            },
            "manual": {
                "path": entry.document.manual,
                "sha256": entry.manual_sha256,
            }
        }
    })
}

fn facet_coverage(entry: &LoadedEntry) -> Value {
    let mut coverage = serde_json::Map::new();
    for facet in PromiseFacet::ALL {
        let statuses = facet_claim_statuses(entry, facet);
        let state = aggregate_claim_status(&statuses);
        let claim_statuses: Vec<_> = statuses.iter().map(|status| status.as_str()).collect();
        coverage.insert(
            facet.as_str().to_owned(),
            json!({
                "state": state,
                "claim_statuses": claim_statuses,
            }),
        );
    }
    Value::Object(coverage)
}

fn facet_claim_statuses(entry: &LoadedEntry, facet: PromiseFacet) -> BTreeSet<ClaimStatus> {
    let mut statuses = BTreeSet::new();
    match facet {
        PromiseFacet::Applicability
        | PromiseFacet::Outcome
        | PromiseFacet::Effects
        | PromiseFacet::Authority
        | PromiseFacet::Success
        | PromiseFacet::FailureAndRecovery
        | PromiseFacet::Privacy
        | PromiseFacet::Exclusions => {
            statuses.insert(ClaimStatus::Declared);
        }
        PromiseFacet::Interfaces => {
            if entry.document.interfaces.is_empty() && entry.document.session_surfaces.is_empty() {
                return statuses;
            }
            statuses.insert(ClaimStatus::Declared);
        }
        PromiseFacet::Dependencies => {
            statuses.insert(if entry.document.dependencies.is_empty() {
                ClaimStatus::NotApplicable
            } else {
                ClaimStatus::Declared
            });
        }
        normalized => {
            let Some(promise) = &entry.document.promise else {
                return statuses;
            };
            match normalized {
                PromiseFacet::Consumers => {
                    statuses.extend(promise.consumers.iter().map(|claim| claim.status));
                }
                PromiseFacet::Preconditions => {
                    statuses.extend(promise.preconditions.iter().map(|claim| claim.status));
                }
                PromiseFacet::Inputs => {
                    statuses.extend(promise.inputs.iter().map(|claim| claim.status));
                }
                PromiseFacet::Outputs => {
                    statuses.extend(promise.outputs.iter().map(|claim| claim.status));
                }
                PromiseFacet::DataSemantics => {
                    statuses.extend(promise.data_semantics.iter().map(|claim| claim.status));
                }
                PromiseFacet::IdentityAndUnits => {
                    statuses.extend(promise.identity_and_units.iter().map(|claim| claim.status));
                }
                PromiseFacet::CompletenessAndFreshness => {
                    statuses.extend(
                        promise
                            .completeness_and_freshness
                            .iter()
                            .map(|claim| claim.status),
                    );
                }
                PromiseFacet::Access => {
                    statuses.extend(promise.access.iter().map(|claim| claim.status));
                }
                PromiseFacet::LifecycleAndConsistency => {
                    statuses.extend(
                        promise
                            .lifecycle_and_consistency
                            .iter()
                            .map(|claim| claim.status),
                    );
                }
                PromiseFacet::OperationalLimits => {
                    statuses.extend(promise.operational_limits.iter().map(|claim| claim.status));
                }
                PromiseFacet::CompatibilityAndEvolution => {
                    statuses.extend(
                        promise
                            .compatibility_and_evolution
                            .iter()
                            .map(|claim| claim.status),
                    );
                }
                PromiseFacet::Reliances => {
                    statuses.extend(promise.reliances.iter().map(|claim| claim.status));
                }
                _ => {}
            }
        }
    }
    statuses
}

fn aggregate_claim_status(statuses: &BTreeSet<ClaimStatus>) -> &'static str {
    if statuses.is_empty() {
        "undeclared"
    } else if statuses.len() > 1 {
        "mixed"
    } else {
        match statuses.first().copied() {
            Some(ClaimStatus::Declared) => "declared",
            Some(ClaimStatus::Unsupported) => "unsupported",
            Some(ClaimStatus::Unspecified) => "unspecified",
            Some(ClaimStatus::NotApplicable) => "not_applicable",
            None => "undeclared",
        }
    }
}

fn facet_has_declared_claim(entry: &LoadedEntry, facet: PromiseFacet) -> bool {
    facet_claim_statuses(entry, facet).contains(&ClaimStatus::Declared)
}

fn collect_contract_gaps(
    provider: &ProviderBundle,
    entry: &LoadedEntry,
    gaps: &mut Vec<ResolutionGap>,
) -> bool {
    let mut complete = true;
    match &provider.promise_scope {
        Some(scope)
            if scope.inventory.completeness == crate::model::InventoryCompleteness::Partial =>
        {
            complete = false;
            gaps.push(
                ResolutionGap::new(
                    "provider_inventory_partial",
                    format!(
                        "provider {} declares only partial inventory completeness",
                        provider.identity.id
                    ),
                )
                .entry(&entry.document.id),
            );
        }
        Some(_) => {}
        None => {
            complete = false;
            gaps.push(
                ResolutionGap::new(
                    "provider_scope_undeclared",
                    format!(
                        "provider {} schema {} has no normalized promise scope",
                        provider.identity.id, provider.schema_version
                    ),
                )
                .entry(&entry.document.id),
            );
        }
    }

    let Some(promise) = &entry.document.promise else {
        complete = false;
        for facet in NORMALIZED_PROMISE_FACETS {
            gaps.push(
                ResolutionGap::new(
                    "facet_undeclared",
                    format!(
                        "{} does not publish normalized claims for {}",
                        entry.document.id,
                        facet.as_str()
                    ),
                )
                .entry(&entry.document.id)
                .facet(facet),
            );
        }
        append_dependency_gaps(entry, gaps);
        return complete;
    };

    for facet in NORMALIZED_PROMISE_FACETS {
        for statement in unspecified_claims(promise, facet) {
            gaps.push(
                ResolutionGap::new("promise_unspecified", statement)
                    .entry(&entry.document.id)
                    .facet(facet),
            );
        }
    }
    for reliance in &promise.reliances {
        if reliance.status == ClaimStatus::Declared && reliance.contract.is_none() {
            complete = false;
            gaps.push(
                ResolutionGap::new(
                    "uncontracted_reliance",
                    format!(
                        "{} relies on {} without a dedicated installed contract: {}",
                        entry.document.id,
                        reliance.target.as_deref().unwrap_or("an unnamed system"),
                        reliance.statement
                    ),
                )
                .entry(&entry.document.id)
                .facet(PromiseFacet::Reliances),
            );
        }
    }
    append_dependency_gaps(entry, gaps);
    complete
}

fn unspecified_claims(promise: &crate::model::EntryPromise, facet: PromiseFacet) -> Vec<String> {
    let claims: &[crate::model::PromiseClaim] = match facet {
        PromiseFacet::Consumers => &promise.consumers,
        PromiseFacet::Preconditions => &promise.preconditions,
        PromiseFacet::Inputs => &promise.inputs,
        PromiseFacet::Outputs => &promise.outputs,
        PromiseFacet::DataSemantics => &promise.data_semantics,
        PromiseFacet::IdentityAndUnits => &promise.identity_and_units,
        PromiseFacet::CompletenessAndFreshness => &promise.completeness_and_freshness,
        PromiseFacet::Access => &promise.access,
        PromiseFacet::LifecycleAndConsistency => &promise.lifecycle_and_consistency,
        PromiseFacet::OperationalLimits => &promise.operational_limits,
        PromiseFacet::CompatibilityAndEvolution => &promise.compatibility_and_evolution,
        PromiseFacet::Reliances => {
            return promise
                .reliances
                .iter()
                .filter(|claim| claim.status == ClaimStatus::Unspecified)
                .map(|claim| claim.statement.clone())
                .collect();
        }
        _ => return Vec::new(),
    };
    claims
        .iter()
        .filter(|claim| claim.status == ClaimStatus::Unspecified)
        .map(|claim| claim.statement.clone())
        .collect()
}

fn append_dependency_gaps(entry: &LoadedEntry, gaps: &mut Vec<ResolutionGap>) {
    for dependency in &entry.dependency_statuses {
        if dependency.state != crate::model::DependencyState::Compatible {
            gaps.push(
                ResolutionGap::new(
                    "dependency_unavailable",
                    format!(
                        "{} requires {} >= {}, < {}, but its installed state is {}",
                        entry.document.id,
                        dependency.id,
                        dependency.min_contract,
                        dependency.max_contract_exclusive,
                        dependency.state.as_str()
                    ),
                )
                .entry(&entry.document.id),
            );
        }
    }
}

fn append_resolved_contract_human(
    human: &mut String,
    provider: &ProviderBundle,
    entry: &LoadedEntry,
    heading: &str,
) {
    let _ = write!(
        human,
        "\n{heading}\n\n{} · {} {} · contract {} · schema {}\nAvailability: {} · Compatibility: {} · Readiness: {}\n",
        terminal_text(&entry.document.id, false),
        terminal_text(&provider.identity.name, false),
        terminal_text(&provider.identity.release, false),
        entry.document.contract_version,
        provider.schema_version,
        availability(entry),
        compatibility(entry),
        readiness(entry),
    );
    append_provider_scope(human, provider);
    human.push_str("\nFACET COVERAGE\n");
    for facet in PromiseFacet::ALL {
        let statuses = facet_claim_statuses(entry, facet);
        let _ = writeln!(
            human,
            "\n  {}: {}",
            facet.as_str(),
            aggregate_claim_status(&statuses)
        );
    }
    append_entry_details(human, entry);
    let _ = writeln!(
        human,
        "\nBASIS\n\n  provider.json  {}\n  {}  {}\n  {}  {}",
        provider.manifest_sha256,
        terminal_text(&entry.source_path, false),
        entry.source_sha256,
        terminal_text(&entry.document.manual, false),
        entry.manual_sha256,
    );
}

fn append_provider_scope(human: &mut String, provider: &ProviderBundle) {
    let Some(scope) = &provider.promise_scope else {
        let _ = writeln!(
            human,
            "\nPROVIDER PROMISE SCOPE\n\n  Undeclared in provider schema {}.",
            provider.schema_version
        );
        return;
    };
    let _ = writeln!(
        human,
        "\nPROVIDER PROMISE SCOPE\n\n  Inventory completeness: {}",
        scope.inventory.completeness.as_str()
    );
    append_section(human, "AUTHORITATIVE FOR", &scope.authoritative_for);
    append_section(human, "NOT AUTHORITATIVE FOR", &scope.not_authoritative_for);
    append_section(human, "INVENTORY COVERS", &scope.inventory.covers);
    append_section(human, "INVENTORY EXCLUDES", &scope.inventory.excludes);
    append_section(
        human,
        "SHARED ACCESS AND TRUST",
        &scope.shared_access_and_trust,
    );
    append_section(
        human,
        "SHARED PRIVACY AND RETENTION",
        &scope.shared_privacy_and_retention,
    );
    append_section(
        human,
        "COMPATIBILITY AND RETIREMENT",
        &scope.compatibility_and_retirement,
    );
    append_section(human, "OPERATIONAL LIMITS", &scope.operational_limits);
}

fn append_resolution_gaps(human: &mut String, gaps: &[ResolutionGap]) {
    if gaps.is_empty() {
        human.push_str("\nRESOLUTION GAPS\n\n  None.\n");
        return;
    }
    human.push_str("\nRESOLUTION GAPS\n");
    for gap in gaps {
        let context = gap.entry.as_deref().unwrap_or("resolution");
        let facet = gap
            .facet
            .as_deref()
            .map_or_else(String::new, |facet| format!("/{facet}"));
        let _ = write!(
            human,
            "\n  - {} [{}{}]: {}",
            gap.code,
            terminal_text(context, false),
            facet,
            terminal_text(&gap.message, false)
        );
    }
    human.push('\n');
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
    append_promise(human, entry);
    human.push_str("\nDETAILS\n\n");
    human.push_str(&terminal_text(&entry.manual_text, true));
    if !human.ends_with('\n') {
        human.push('\n');
    }
}

fn append_promise(human: &mut String, entry: &LoadedEntry) {
    let Some(promise) = &entry.document.promise else {
        return;
    };
    human.push_str("\nNORMALIZED PROMISE\n");
    for (label, claims) in [
        ("consumers", promise.consumers.as_slice()),
        ("preconditions", promise.preconditions.as_slice()),
        ("inputs", promise.inputs.as_slice()),
        ("outputs", promise.outputs.as_slice()),
        ("data_semantics", promise.data_semantics.as_slice()),
        ("identity_and_units", promise.identity_and_units.as_slice()),
        (
            "completeness_and_freshness",
            promise.completeness_and_freshness.as_slice(),
        ),
        ("access", promise.access.as_slice()),
        (
            "lifecycle_and_consistency",
            promise.lifecycle_and_consistency.as_slice(),
        ),
        ("operational_limits", promise.operational_limits.as_slice()),
        (
            "compatibility_and_evolution",
            promise.compatibility_and_evolution.as_slice(),
        ),
    ] {
        let _ = write!(human, "\n  {label}\n");
        for claim in claims {
            let _ = write!(
                human,
                "\n    - {}: {}",
                claim.status.as_str(),
                terminal_text(&claim.statement, false)
            );
        }
        human.push('\n');
    }
    human.push_str("\n  reliances\n");
    for claim in &promise.reliances {
        let metadata = if claim.status == ClaimStatus::Declared {
            format!(
                " [{} / {}{}]",
                claim.target.as_deref().unwrap_or("missing target"),
                claim.kind.map_or("missing kind", |kind| kind.as_str()),
                claim
                    .contract
                    .as_deref()
                    .map_or_else(String::new, |contract| format!(" / {contract}")),
            )
        } else {
            String::new()
        };
        let _ = write!(
            human,
            "\n    - {}{}: {}",
            claim.status.as_str(),
            metadata,
            terminal_text(&claim.statement, false)
        );
    }
    human.push('\n');
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
