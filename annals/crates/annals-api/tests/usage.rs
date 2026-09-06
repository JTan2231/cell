use annals_api::usage::{Error, Library, read_receipts};

#[test]
fn read_only_open_does_not_create_a_library() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("absent.db");
    assert!(Library::open(&path).is_err());
    assert!(!path.exists());
    Ok(())
}

#[test]
fn partial_receipts_keep_unknown_fields_and_reject_ambiguous_attribution()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    for (state, job) in [("done", "one"), ("skipped", "two")] {
        let envelope = directory.path().join(state).join(job);
        std::fs::create_dir_all(&envelope)?;
        std::fs::write(
            envelope.join("job.json"),
            serde_json::to_vec(&serde_json::json!({
                "id": job, "ingestion_id": 7, "model_run_token": "shared-token", "unrelated": {"future": true}
            }))?,
        )?;
    }
    assert!(matches!(
        read_receipts(directory.path()),
        Err(Error::DuplicateModelRunReceipt { .. })
    ));
    Ok(())
}
