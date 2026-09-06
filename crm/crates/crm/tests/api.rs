use crm::api::{Client, ClientError, Data, Request, Stage, decode_response};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn client_owns_mutation_read_and_error_decoding() -> TestResult {
    let temporary = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    let client =
        Client::new(env!("CARGO_BIN_EXE_crm")).with_database(temporary.path().join("crm.db"));
    assert!(matches!(
        client.execute(&Request::Init)?.data,
        Data::Init { created: true, .. }
    ));
    let markdown = b"# Synthetic case\n\nExact caller notes.\n";
    let created = client.execute_with_input(
        &Request::CreateCase {
            title: "Synthetic case".into(),
            input: Some("-".into()),
            stage: Stage::Research,
        },
        markdown,
    )?;
    let Data::CaseCreated { case } = created.data else {
        return Err(std::io::Error::other("expected created case").into());
    };
    assert_eq!(case.markdown.as_bytes(), markdown);
    assert_eq!(case.stage, Stage::Research);
    assert!(!case.attention);
    let shown = client.execute(&Request::ShowCase {
        case: case.case_id.clone(),
        revision: Some(1),
    })?;
    assert_eq!(shown.data, Data::CaseRevision { case: case.clone() });
    assert_eq!(
        client
            .execute(&Request::CaseHistory {
                case: case.case_id.clone()
            })?
            .data,
        Data::CaseHistory {
            revisions: vec![case.clone()]
        }
    );
    let Data::CaseList { cases } = client.execute(&Request::ListCases { limit: 20 })?.data else {
        return Err(std::io::Error::other("expected case list").into());
    };
    assert_eq!(cases.len(), 1);
    let error = client.execute(&Request::ShowCase {
        case: "c99999".into(),
        revision: None,
    });
    assert!(matches!(error, Err(ClientError::Rejected(_))));
    let input = temporary.path().join("case 雪 café.md");
    std::fs::write(&input, markdown)?;
    assert!(matches!(
        client
            .execute(&Request::CreateCase {
                title: "Unicode filename".into(),
                input: Some(input),
                stage: Stage::Research,
            })?
            .data,
        Data::CaseCreated { .. }
    ));
    Ok(())
}

#[test]
fn response_decoder_refuses_contradictory_envelopes() {
    assert!(decode_response::<Data>(br#"{"ok":true,"data":{"type":"case_list","cases":[]},"error":{"code":"x","message":"x"}}"#).is_err());
    assert!(
        decode_response::<Data>(br#"{"ok":false,"data":{},"error":{"code":"x","message":"x"}}"#)
            .is_err()
    );
    assert!(
        decode_response::<Data>(br#"{"ok":"true","data":{"type":"case_list","cases":[]}}"#)
            .is_err()
    );
}
