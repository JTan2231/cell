use geste::api::{
    Capture, Client, ClientError, Data, EpisodeCommand, Outcome, OutcomeStatus, ReadArgs, Request,
    SourceAnchor, SourceRole, decode_capture, decode_response,
};
use sha2::{Digest as _, Sha256};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn capture() -> Capture {
    Capture {
        schema_version: 1,
        title: "Synthetic episode".into(),
        shape: "contract boundary".into(),
        basis_cutoff_at: "2026-09-02T18:00:00Z".into(),
        recorded_by: "test".into(),
        situation: "An import needed a supported shape.".into(),
        response: "Imported the provider type.".into(),
        outcome: Outcome {
            status: OutcomeStatus::Solved,
            summary: "Decoded the current revision.".into(),
        },
        applicability: "Local consumer boundaries".into(),
        actions: vec![],
        lessons: vec![],
        settlements: vec![],
        tags: vec!["interface".into()],
        gaps: vec![],
        related_episodes: vec![],
        sources: vec![SourceAnchor {
            id: "context".into(),
            system: "conversations".into(),
            kind: "thread".into(),
            reference: "synthetic-thread".into(),
            revision: None,
            digest: None,
            observed_at: "2026-09-02T17:00:00Z".into(),
            role: SourceRole::Context,
            label: "Synthetic context".into(),
            supports: vec!["shape".into(), "situation".into()],
        }],
    }
}

#[test]
fn typed_capture_preserves_exact_input_and_historical_reads() -> TestResult {
    let temporary = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    let client =
        Client::new(env!("CARGO_BIN_EXE_geste")).with_database(temporary.path().join("geste.db"));
    client.execute(&Request::Init)?;
    let original = capture();
    let bytes = serde_json::to_vec_pretty(&original)?;
    assert_eq!(decode_capture(&bytes)?, original);
    let created = client.execute_with_input(
        &Request::Episode {
            command: EpisodeCommand::Create { input: "-".into() },
        },
        &bytes,
    )?;
    let Data::EpisodeCreated { episode } = created.data else {
        return Err(std::io::Error::other("expected created episode").into());
    };
    assert_eq!(episode.capture, original);
    assert_eq!(
        episode.submitted_sha256,
        format!("{:x}", Sha256::digest(&bytes))
    );
    let mut next = original;
    next.title = "Revised synthetic episode".into();
    let revise = Request::Episode {
        command: EpisodeCommand::Revise {
            episode: episode.episode.clone(),
            input: "-".into(),
            base: 1,
        },
    };
    let revision = client.execute_with_input(&revise, &serde_json::to_vec(&next)?)?;
    assert!(matches!(revision.data, Data::EpisodeRevised { episode } if episode.revision == 2));
    let stale = client.execute_with_input(&revise, &bytes);
    assert!(
        matches!(stale, Err(ClientError::Rejected(error)) if error.error.code == "stale_revision")
    );
    let historical = client.execute(&Request::Episode {
        command: EpisodeCommand::Show(ReadArgs {
            episode: episode.episode.clone(),
            at: Some(1),
        }),
    })?;
    assert_eq!(
        historical.data,
        Data::EpisodeRevision {
            episode: episode.clone()
        }
    );
    assert!(
        matches!(client.execute(&Request::Report(ReadArgs { episode: episode.episode.clone(), at: None }))?.data,
        Data::EpisodeReport { episode, .. } if episode.revision == 2)
    );
    assert!(matches!(
        client
            .execute(&Request::Graph(ReadArgs {
                episode: episode.episode,
                at: None
            }))?
            .data,
        Data::EpisodeGraph { revision: 2, .. }
    ));
    let input = temporary.path().join("capture 雪 café.json");
    std::fs::write(&input, &bytes)?;
    assert!(matches!(
        client
            .execute(&Request::Episode {
                command: EpisodeCommand::Create { input },
            })?
            .data,
        Data::EpisodeCreated { .. }
    ));
    Ok(())
}

#[test]
fn capture_and_response_versions_remain_distinct_and_strict() -> TestResult {
    let mut value = serde_json::to_value(capture())?;
    value["unexpected"] = true.into();
    assert_eq!(
        decode_capture(&serde_json::to_vec(&value)?)
            .err()
            .map(|error| error.code()),
        Some("invalid_capture_json")
    );
    assert!(
        decode_response::<Data>(
            br#"{"schema_version":2,"ok":true,"data":{"type":"episode_list","episodes":[]}}"#
        )
        .is_err()
    );
    assert!(
        decode_response::<Data>(
            br#"{"schema_version":1,"ok":false,"data":{},"error":{"code":"x","message":"x"}}"#
        )
        .is_err()
    );
    Ok(())
}
