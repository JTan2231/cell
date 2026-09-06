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

#[test]
fn profile_client_preserves_markdown_and_updates_only_selected_entry() -> TestResult {
    let temporary = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    let client =
        Client::new(env!("CARGO_BIN_EXE_crm")).with_database(temporary.path().join("crm.db"));
    client.execute(&Request::Init)?;
    let original = b"# Synthetic vignette\r\n\r\n- Contribution: implemented.\r\n- Outcome: **Needs check**.\r\n";
    let Data::ProfileEntry { entry } = client
        .execute_with_input(
            &Request::CreateProfileEntry {
                title: "Synthetic vignette".into(),
                input: "-".into(),
            },
            original,
        )?
        .data
    else {
        return Err(std::io::Error::other("expected profile entry").into());
    };
    assert_eq!(entry.body_md.as_bytes(), original);
    let shown = client.execute(&Request::ShowProfileEntry {
        entry: entry.id.clone(),
    })?;
    assert_eq!(
        shown.data,
        Data::ProfileEntry {
            entry: entry.clone()
        }
    );
    let updated = "# Corrected vignette\n\nOutcome remains **Needs check**. 雪\n";
    let Data::ProfileEntry { entry: edited } = client
        .execute_with_input(
            &Request::UpdateProfileEntry {
                entry: entry.id.clone(),
                title: "Corrected vignette".into(),
                input: "-".into(),
            },
            updated.as_bytes(),
        )?
        .data
    else {
        return Err(std::io::Error::other("expected updated profile entry").into());
    };
    assert_eq!(edited.id, entry.id);
    assert_eq!(edited.title, "Corrected vignette");
    assert_eq!(edited.body_md, updated);
    assert_eq!(
        client
            .execute(&Request::ListProfileEntries { limit: 20 })?
            .data,
        Data::ProfileList {
            entries: vec![edited]
        }
    );
    assert_eq!(
        client.execute(&Request::ListCases { limit: 20 })?.data,
        Data::CaseList { cases: vec![] }
    );
    assert!(matches!(
        client.execute(&Request::ShowProfileEntry {
            entry: "missing".into()
        }),
        Err(ClientError::Rejected(_))
    ));
    Ok(())
}

#[test]
fn migration_client_requires_explicit_upgrade_and_returns_backup() -> TestResult {
    let temporary = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    let database = temporary.path().join("crm.db");
    let old = rusqlite::Connection::open(&database)?;
    old.execute_batch(include_str!("fixtures/schema-v1.sql"))?;
    old.pragma_update(None, "user_version", 1)?;
    drop(old);
    let client = Client::new(env!("CARGO_BIN_EXE_crm")).with_database(&database);
    assert!(matches!(
        client.execute(&Request::Init),
        Err(ClientError::Rejected(_))
    ));
    let backup = temporary.path().join("before-profile.db");
    let result = client.execute(&Request::Migrate {
        backup: backup.clone(),
    })?;
    assert_eq!(
        result.data,
        Data::Migrated {
            database: database.clone(),
            backup: Some(backup.clone()),
            from_schema_version: 1,
            schema_version: 2,
            changed: true,
        }
    );
    assert!(backup.is_file());
    let unused_backup = temporary.path().join("unused.db");
    let again = client.execute(&Request::Migrate {
        backup: unused_backup.clone(),
    })?;
    assert_eq!(
        again.data,
        Data::Migrated {
            database,
            backup: None,
            from_schema_version: 2,
            schema_version: 2,
            changed: false,
        }
    );
    assert!(!unused_backup.exists());
    Ok(())
}
