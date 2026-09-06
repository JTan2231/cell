use decisions::api::{Client, StopHookInput};

#[test]
fn provider_clients_preserve_observation_and_frozen_lifecycle_boundaries()
-> Result<(), Box<dyn std::error::Error>> {
    let temporary = tempfile::tempdir()?;
    let database = temporary.path().join("krisis.db");
    let client = Client::new(env!("CARGO_BIN_EXE_krisis")).with_database(&database);
    let first = client.activate(Some(1))?;
    let replay = client.activate(Some(2))?;
    assert!(first.created);
    assert!(!replay.created);
    assert_eq!(first.observer_baseline_at, replay.observer_baseline_at);
    let hook = StopHookInput {
        session_id: "fixture-session".to_owned(),
        turn_id: "fixture-turn".to_owned(),
        hook_event_name: "Stop".to_owned(),
    };
    client.ingest(&hook)?;
    client.ingest(&hook)?;
    assert_eq!(client.status(None)?.queued, 1);
    let legacy =
        krisis_api::lifecycle::Client::new(env!("CARGO_BIN_EXE_krisis")).with_database(database);
    let watermark = legacy.watermark()?;
    let page = legacy.read_after(&watermark.cursor, 100)?;
    assert_eq!(page.next_cursor, watermark.cursor);
    assert!(page.events.is_empty());
    assert!(legacy.read_after(&watermark.cursor, 0).is_err());
    assert!(client.show("missing").is_err());
    Ok(())
}
