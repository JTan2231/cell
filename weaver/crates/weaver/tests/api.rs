use weaver::api::{Cancellation, Run, RunStatus, STAGES, Submission};

#[test]
fn client_selects_isolated_state_and_distinguishes_missing_run()
-> Result<(), Box<dyn std::error::Error>> {
    let temporary = tempfile::tempdir()?;
    let state = temporary.path().join("Weaver State");
    let client = weaver::api::Client::new(env!("CARGO_BIN_EXE_weaver")).with_state_dir(&state);
    client.begin_maintenance(0)?;
    assert!(state.is_dir());
    assert!(
        client
            .status(None)
            .is_err_and(|error| { error.to_string().contains("no current workflow") })
    );
    client.end_maintenance()?;
    Ok(())
}

#[test]
fn existing_status_text_preserves_multiline_detail() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = "Run: r-1\nNarrative: narratives/example\nState: failed\nCompleted stages: 2/5\nNucleus job: j-1\nDetail: provider failed\nState: this is part of the detail\n";
    let run: Run = fixture.parse()?;
    assert_eq!(run.status, RunStatus::Failed);
    assert_eq!(run.completed_stages, 2);
    assert_eq!(
        run.detail.as_deref(),
        Some("provider failed\nState: this is part of the detail")
    );
    assert_eq!(run.to_string(), fixture);
    Ok(())
}

#[test]
fn receipts_and_artifact_layout_preserve_existing_interface() {
    assert_eq!(
        Submission {
            run_id: "r-1".into(),
            narrative: "example".into()
        }
        .to_string(),
        "weaver: submitted r-1: narratives/example"
    );
    assert_eq!(
        Cancellation {
            run_id: "r-1".into()
        }
        .to_string(),
        "weaver: cancellation requested: r-1"
    );
    assert_eq!(
        STAGES.map(|stage| stage.directory),
        [
            "01-stories",
            "02-themes",
            "03-draft",
            "04-review",
            "05-final"
        ]
    );
    assert!(
        "Run: r-1\nNarrative: narratives/example\nState: unknown\nCompleted stages: 0/5\n"
            .parse::<Run>()
            .is_err()
    );
}
