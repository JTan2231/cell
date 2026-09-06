use todo::api::{
    Client, ClientError, ConcernAddArgs, ConcernArgs, ConcernCommand, ConcernId, ConcernListArgs,
    ConcernStatus, Data, Request, TodoId, decode_response,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn typed_client_captures_provenance_and_preserves_history_shape() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let client =
        Client::new(env!("CARGO_BIN_EXE_todo")).with_database(temporary.path().join("todo.db"));
    assert!(matches!(
        client.execute(&Request::Init)?.data,
        Data::Init { .. }
    ));
    let source = temporary.path().join("origin.md");
    std::fs::write(&source, "Synthetic originating need.\n")?;
    let captured = client.execute(&Request::Concern(ConcernCommand::Add(ConcernAddArgs {
        direction: "Retain the source boundary".into(),
        source: source.clone(),
    })))?;
    let Data::Captured { concern } = captured.data else {
        return Err(std::io::Error::other("expected captured concern").into());
    };
    assert_eq!(concern.status, ConcernStatus::Pending);
    assert_eq!(
        concern.source_path,
        std::fs::canonicalize(source)?.display().to_string()
    );
    let shown = client.execute(&Request::Concern(ConcernCommand::Show(ConcernArgs {
        id: concern.id,
    })))?;
    assert_eq!(
        shown.data,
        Data::ConcernHistory {
            concern: concern.clone(),
            routing: vec![]
        }
    );
    assert_eq!(
        client
            .execute(&Request::Concern(ConcernCommand::List(ConcernListArgs {
                all: false,
                limit: 20
            })))?
            .data,
        Data::Concerns {
            concerns: vec![concern]
        }
    );
    let duplicate = client.execute(&Request::Init);
    assert!(
        matches!(duplicate, Err(ClientError::Rejected(error)) if error.error.code == "database_exists")
    );
    Ok(())
}

#[test]
fn public_ids_and_overlapping_response_shapes_remain_distinct() -> TestResult {
    assert!("c1".parse::<TodoId>().is_err());
    assert!("t1".parse::<ConcernId>().is_err());
    let response = br#"{"ok":true,"data":{"query":"synthetic","todos":[]}}"#;
    let data: Data = decode_response(response)?;
    assert_eq!(
        data,
        Data::Search {
            query: "synthetic".into(),
            todos: vec![]
        }
    );
    assert!(
        decode_response::<Data>(
            br#"{"ok":true,"data":{"todos":[]},"error":{"code":"x","message":"x"}}"#
        )
        .is_err()
    );
    assert!(
        decode_response::<Data>(br#"{"ok":false,"data":{},"error":{"code":"x","message":"x"}}"#)
            .is_err()
    );
    Ok(())
}

#[test]
fn fixed_design_fields_roundtrip_without_losing_nulls_or_drop_provenance() -> TestResult {
    let value = serde_json::json!({
        "design": {
            "id": "d1", "todo_id": "t1", "revision": 1,
            "assessment_id": "a1", "draft_version": 2, "state": "ready",
            "summary": "Synthetic design", "current": true, "stale_reasons": [],
            "jurisdiction_changes": [], "unresolved_choices": [],
            "clauses": [{
                "operation_id": "op-1", "local_ref": "boundary",
                "kind": "boundary", "subject": "Imported records",
                "statement": "The provider owns the wire shape.",
                "jurisdiction_ref": null, "status": "dropped", "basis_refs": ["direction:body"],
                "drop": {"reason": "Superseded", "basis_refs": ["correction:1"], "dropped_at": "2026-09-02T18:00:00Z"}
            }],
            "created_at": "2026-09-02T17:00:00Z"
        },
        "changed": false
    });
    let data: Data = serde_json::from_value(value.clone())?;
    let Data::DesignDecision { design, changed } = &data else {
        return Err(std::io::Error::other("expected design decision").into());
    };
    assert!(!changed);
    assert_eq!(design.clauses[0].operation_id, "op-1");
    assert_eq!(
        design.clauses[0]
            .drop
            .as_ref()
            .map(|drop| drop.reason.as_str()),
        Some("Superseded")
    );
    assert_eq!(serde_json::to_value(data)?, value);
    Ok(())
}
