use semantics::api::{CliError, Client};
use semantics::store::Store;

#[test]
fn typed_client_reads_and_updates_through_the_real_cli() -> Result<(), Box<dyn std::error::Error>> {
    let temporary = tempfile::tempdir()?;
    let database = temporary.path().join("semantics.db");
    let root = temporary.path().join("project");
    std::fs::create_dir(&root)?;
    std::fs::write(root.join("AGENTS.md"), "Semantics-Project: fixture\n")?;
    // Fixture setup substitutes only the unavailable upstream activation read.
    Store::open(&database)?.register_project_with_account_feed(
        "fixture",
        &root,
        "0123456789abcdef0123456789abcdef",
        "opaque-activation",
    )?;
    let client = Client::new(env!("CARGO_BIN_EXE_semantics")).with_database(database);
    assert_eq!(client.projects()?.len(), 1);
    assert_eq!(client.project("fixture")?.project.id, "fixture");
    assert_eq!(client.repository("fixture", None)?.revision, 0);
    let receipt = client.seed(
        "fixture",
        "Stable identity",
        "Identity survives wording changes.",
        Some("Synthetic grounding retained in provenance."),
    )?;
    assert_eq!(receipt.revision, 1);
    assert_eq!(
        client.search("fixture", "identity", None)?.concepts.len(),
        1
    );
    assert_eq!(client.repository("fixture", Some(0))?.concepts.len(), 0);
    let current = client.repository("fixture", None)?;
    let encoded = serde_json::to_value(&current)?;
    assert!(encoded["concepts"]["c000001"].get("grounds").is_none());
    assert_eq!(
        client.repository_provenance("fixture", None)?.concepts["c000001"]
            .grounds
            .len(),
        1
    );
    assert_eq!(client.search("fixture", "missing", None)?.revision, 1);
    assert_eq!(client.log("fixture", 1, None)?.len(), 1);
    assert_eq!(client.diff("fixture", 0, 1)?.revisions.len(), 1);
    assert!(client.intake(None)?.annals_decision_accounts.is_empty());
    assert!(
        matches!(client.project("missing"), Err(CliError::Rejected { code, .. }) if code == "project_not_found")
    );
    Ok(())
}

#[test]
fn current_view_retains_retirement_replacement_and_complete_distinctions() {
    use semantics::api::{Concept, Distinction, Repository, RepositoryView};
    let meaning = "Meaning with significant qualifiers. ".repeat(100);
    let statement = "A distinct concept, not a synonym. ".repeat(100);
    let concept = Concept {
        id: "retired".into(),
        label: "Retired concept".into(),
        meaning: meaning.clone(),
        active: false,
        replacement_concept_id: Some("replacement".into()),
        created_revision: 1,
        changed_revision: 4,
        grounds: vec![],
        distinctions: vec![Distinction {
            revision: 3,
            other_concept_id: "other".into(),
            statement: statement.clone(),
        }],
    };
    let repository = Repository {
        project_id: "fixture".into(),
        revision: 4,
        concepts: [(concept.id.clone(), concept)].into(),
    };
    let view = RepositoryView::from(&repository);
    let summary = &view.concepts["retired"];
    assert_eq!(view.revision, 4);
    assert!(!summary.active);
    assert_eq!(
        summary.replacement_concept_id.as_deref(),
        Some("replacement")
    );
    assert_eq!(summary.meaning, meaning);
    assert_eq!(summary.distinctions[0].statement, statement);
}
