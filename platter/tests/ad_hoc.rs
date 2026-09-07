//! Retained-material sends preserve jobs, accepted artifacts and earlier editions.
use anyhow::{Context, Result};
use platter::{
    Config, ad_hoc,
    store::{Edition, PacketRecord, Store},
};
use std::{
    fmt::Write as _,
    fs,
    os::unix::fs::PermissionsExt as _,
    path::{Path, PathBuf},
};

fn retained_snapshot(root: &Path) -> Result<String> {
    let connection = rusqlite::Connection::open_with_flags(
        root.join(platter::store::DATABASE),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let mut snapshot = String::new();
    for query in [
        "SELECT * FROM jobs ORDER BY opportunity",
        "SELECT * FROM runs ORDER BY id",
        "SELECT * FROM artifacts ORDER BY id",
        "SELECT * FROM editions WHERE id IN ('2026-09-01','2026-09-02') ORDER BY id",
        "SELECT * FROM edition_attachments WHERE edition_id IN ('2026-09-01','2026-09-02') ORDER BY edition_id,position",
    ] {
        let mut statement = connection.prepare(query)?;
        let columns = statement.column_count();
        let rows = statement
            .query_map([], |row| {
                (0..columns)
                    .map(|i| row.get::<_, rusqlite::types::Value>(i))
                    .collect::<rusqlite::Result<Vec<_>>>()
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        writeln!(snapshot, "{rows:?}")?;
    }
    Ok(snapshot)
}

struct Fixture {
    directory: tempfile::TempDir,
    email: PathBuf,
    baseline: String,
    ids: Vec<String>,
}
impl Fixture {
    fn new(response: &str) -> Result<Self> {
        let directory = tempfile::tempdir()?;
        let root = directory.path();
        let forbidden = root.join("forbidden-source");
        fs::write(
            &forbidden,
            "#!/bin/sh\nprintf 'CALLED' > \"$0.called\"\nexit 99\n",
        )?;
        fs::set_permissions(&forbidden, fs::Permissions::from_mode(0o700))?;
        let email = root.join("fake-email");
        fs::write(
            &email,
            format!(
                "#!/bin/sh\nset -eu\nprintf 'invoked\\n' >> \"$0.calls\"\nprintf '%s\\n' \"$@\" > \"$0.arguments\"\n/bin/cat > \"$0.body\"\nprintf '%s\\n' '{response}'\n"
            ),
        )?;
        fs::set_permissions(&email, fs::Permissions::from_mode(0o700))?;
        let config = Config {
            daily_count: 3,
            delivery_hour: 9,
            delivery_minute: 0,
            timezone: "America/Chicago".into(),
            cast_executable: forbidden.clone(),
            crm_executable: forbidden.clone(),
            email_executable: forbidden,
            original_resume: root.join("original.json"),
        };
        let mut store = Store::open(root)?;
        store.set_setting("config", &config)?;
        let mut ids = Vec::new();
        let mut pdfs = Vec::new();
        for index in 1..=3 {
            let id = format!("packet-{index}");
            let record = PacketRecord {
                id: id.clone(),
                opportunity: format!("role-{index}"),
                job_id: format!("cast-{index}"),
                company: "synthetic-source-label".into(),
                title: format!("Engineer {index}"),
                status: "ready".into(),
                directory: String::new(),
            };
            let inputs = serde_json::json!({
                "company":"synthetic-source-label", "career":[], "template_artifact":"fixture-template",
                "job":{
                    "id":format!("cast-{index}"), "revision":1, "company_id":"fixture-company",
                    "source_id":"fixture-source", "source_key":format!("role-{index}"),
                    "title":format!("Engineer {index}"), "url":"https://example.invalid/retained-role",
                    "first_seen_at":"2026-09-06T21:00:00Z", "last_seen_at":"2026-09-06T21:00:00Z",
                    "availability":"listed", "missing_complete_snapshots":0,
                    "evidence":[], "compensation":[], "geographic_eligibility":[], "parser_version":"fixture"
                },
                "posting":{"url":"https://example.invalid/retained-role","retrieved_at":"2026-09-06T21:00:00Z","text":serde_json::json!({"company_name":format!("Employer {index}")}).to_string()}
            });
            store.insert_run(&record, &inputs)?;
            store.put_content(&id, "brief", &serde_json::json!({"paragraph":format!("Original brief {index} grounded in retained evidence."),"pursue":true}))?;
            store.put_content(&id, "resume-content", &serde_json::json!({"jackson_bullets":["Supported Jackson work"],"evidence":[{"bullet_index":0,"career_entry_ids":["career-1"]}]}))?;
            pdfs.push(store.put_artifact(
                Some(&id),
                "resume-pdf",
                &format!("Employer-{index}-resume.pdf"),
                "application/pdf",
                b"%PDF-1.4\naccepted test resume\n",
            )?);
            ids.push(id);
        }
        // Preserve sent and frozen ordinary editions and their eligibility policy.
        for (index, status) in [(0, "sent"), (1, "frozen")] {
            let mut edition = Edition {
                day: format!("2026-09-0{}", index + 1),
                status: "frozen".into(),
                subject: "Ordinary edition".into(),
                body: "Original ordinary body".into(),
                packet_ids: vec![ids[index].clone()],
                attachments: vec![pdfs[index].id.clone()],
                attachment_sha256: vec![pdfs[index].sha256.clone()],
                idempotency_key: format!("ordinary-{index}"),
                receipt: None,
            };
            store.freeze(&edition)?;
            if status == "sent" {
                store.begin_send(&edition.idempotency_key)?;
                edition.status = "sent".into();
                edition.receipt = Some("Accepted ordinary".into());
                store.save_edition(&edition)?;
            }
        }
        drop(store);
        let baseline = retained_snapshot(root)?;
        Ok(Self {
            directory,
            email,
            baseline,
            ids,
        })
    }
    fn root(&self) -> &Path {
        self.directory.path()
    }
    fn assert_ordinary_unchanged(&self) -> Result<()> {
        assert_eq!(retained_snapshot(self.root())?, self.baseline);
        assert!(
            !self.root().join("forbidden-source.called").exists(),
            "source/API budget capability was invoked"
        );
        Ok(())
    }
    fn calls(&self) -> Result<String> {
        Ok(fs::read_to_string(self.root().join("fake-email.calls"))?)
    }
}

#[test]
fn retained_preview_and_repeated_send_preserve_existing_records() -> Result<()> {
    let fixture = Fixture::new("Accepted receipt-1")?;
    let original_brief = Store::open(fixture.root())?
        .run_artifact("packet-1", "brief")?
        .context("missing brief")?
        .content;
    let overrides = fixture.root().join("reviewed-briefs.json");
    platter::write_json(
        &overrides,
        &serde_json::json!({"packet-1":"Reviewed brief with the corrected compensation wording."}),
    )?;
    let edition = ad_hoc::preview(
        fixture.root(),
        "2026-09-06",
        "debug-one",
        &[],
        Some(&overrides),
    )?;
    assert_eq!(edition.packet_ids, fixture.ids);
    assert_eq!(edition.attachments.len(), 3);
    assert_eq!(edition.subject, "Your jobs — 2026-09-06");
    assert!(edition.body.contains("1. Employer 1 — Engineer 1"));
    assert!(
        edition
            .body
            .contains("Reviewed brief with the corrected compensation wording.")
    );
    assert!(!edition.body.contains("synthetic-source-label"));
    assert_eq!(
        Store::open(fixture.root())?
            .artifact(&edition.attachments[0])?
            .filename,
        "Employer-1-resume.pdf"
    );
    fixture.assert_ordinary_unchanged()?;
    let repeat = ad_hoc::preview(
        fixture.root(),
        "2026-09-06",
        "debug-one",
        &fixture.ids,
        Some(&overrides),
    )?;
    assert_eq!(repeat.idempotency_key, edition.idempotency_key);
    let first = ad_hoc::send(
        fixture.root(),
        "2026-09-06",
        "debug-one",
        Some(&fixture.email),
    )?;
    let second = ad_hoc::send(
        fixture.root(),
        "2026-09-06",
        "debug-one",
        Some(&fixture.email),
    )?;
    assert_eq!(first.status, "sent");
    assert_eq!(first.receipt, second.receipt);
    assert_eq!(fixture.calls()?, "invoked\n");
    fixture.assert_ordinary_unchanged()?;
    assert_eq!(
        Store::open(fixture.root())?
            .run_artifact("packet-1", "brief")?
            .context("missing brief")?
            .content,
        original_brief
    );
    let another = ad_hoc::preview(
        fixture.root(),
        "2026-09-06",
        "debug-two",
        &fixture.ids,
        None,
    )?;
    assert_ne!(another.idempotency_key, edition.idempotency_key);
    ad_hoc::send(
        fixture.root(),
        "2026-09-06",
        "debug-two",
        Some(&fixture.email),
    )?;
    assert_eq!(fixture.calls()?, "invoked\ninvoked\n");
    fixture.assert_ordinary_unchanged()
}

#[test]
fn uncertain_debug_delivery_cannot_retry_and_never_reserves_jobs() -> Result<()> {
    let fixture = Fixture::new("ambiguous provider output")?;
    ad_hoc::preview(
        fixture.root(),
        "2026-09-06",
        "uncertain",
        &fixture.ids,
        None,
    )?;
    assert!(
        ad_hoc::send(
            fixture.root(),
            "2026-09-06",
            "uncertain",
            Some(&fixture.email)
        )
        .is_err()
    );
    let error = ad_hoc::send(
        fixture.root(),
        "2026-09-06",
        "uncertain",
        Some(&fixture.email),
    )
    .err()
    .context("uncertain occurrence was retried")?;
    assert!(error.to_string().contains("outcome is unresolved"));
    assert_eq!(fixture.calls()?, "invoked\n");
    fixture.assert_ordinary_unchanged()
}

#[test]
fn changed_frozen_attachment_or_selection_is_rejected_before_email() -> Result<()> {
    let fixture = Fixture::new("Accepted receipt-1")?;
    let edition = ad_hoc::preview(fixture.root(), "2026-09-06", "frozen", &fixture.ids, None)?;
    assert!(
        ad_hoc::preview(
            fixture.root(),
            "2026-09-06",
            "frozen",
            &fixture.ids[..1],
            None
        )
        .is_err()
    );
    let connection = rusqlite::Connection::open(fixture.root().join(platter::store::DATABASE))?;
    assert!(
        connection
            .execute(
                "UPDATE artifacts SET content=?1 WHERE id=?2",
                rusqlite::params![b"tampered".as_slice(), edition.attachments[0]]
            )
            .is_err()
    );
    assert!(
        connection
            .execute(
                "UPDATE editions SET body='changed' WHERE id='ad-hoc/frozen'",
                []
            )
            .is_err()
    );
    assert!(
        connection
            .execute(
                "UPDATE edition_attachments SET position=4 WHERE edition_id='ad-hoc/frozen'",
                []
            )
            .is_err()
    );
    assert!(!fixture.root().join("fake-email.calls").exists());
    assert!(
        ad_hoc::preview(
            fixture.root(),
            "2026-09-06",
            "../escape",
            &fixture.ids,
            None
        )
        .is_err()
    );
    fixture.assert_ordinary_unchanged()
}
