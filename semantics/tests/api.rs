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
        None,
    )?;
    assert_eq!(receipt.revision, 1);
    assert_eq!(client.search("fixture", "identity", None)?.len(), 1);
    assert_eq!(client.repository("fixture", Some(0))?.concepts.len(), 0);
    assert_eq!(client.log("fixture", 1, None)?.len(), 1);
    assert_eq!(client.diff("fixture", 0, 1)?.revisions.len(), 1);
    assert!(client.intake(None)?.annals_decision_accounts.is_empty());
    assert!(
        matches!(client.project("missing"), Err(CliError::Rejected { code, .. }) if code == "project_not_found")
    );
    Ok(())
}
