//! Offline delivery acceptance and recovery checks against a fake Email CLI.
use anyhow::{Context, Result};
use base64::Engine as _;
use platter::{
    Config,
    store::{Edition, PacketRecord, Store},
    workflow,
};
use std::{
    fs,
    os::unix::fs::PermissionsExt as _,
    path::{Path, PathBuf},
};

struct Fixture {
    directory: tempfile::TempDir,
    email: PathBuf,
    attachment: platter::store::Artifact,
    packet: PacketRecord,
    edition: Edition,
}

impl Fixture {
    fn new(response: &str) -> Result<Self> {
        let directory = tempfile::tempdir()?;
        let root = directory.path();
        let email = root.join("fake-email");
        // Only a canned test response enters this script; packet data is
        // delivered as process arguments and stdin, never shell code.
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
            cast_executable: root.join("unused-cast"),
            crm_executable: root.join("unused-crm"),
            email_executable: email.clone(),
            original_resume: root.join("unused-original-resume.json"),
        };
        let mut store = Store::open(root)?;
        store.set_setting("config", &config)?;
        let packet = PacketRecord {
            id: "packet-one".into(),
            opportunity: "ashby:example:role-one".into(),
            job_id: "cast-one".into(),
            company: "Example".into(),
            title: "Engineer".into(),
            status: "ready".into(),
            directory: root.join("packet-one").to_string_lossy().into_owned(),
        };
        store.insert(&packet)?;
        let attachment = store.put_artifact(
            Some(&packet.id),
            "resume-pdf",
            "resume.pdf",
            "application/pdf",
            b"%PDF-1.4\nfrozen private resume fixture\n",
        )?;
        let edition = Edition {
            day: "2026-09-06".into(),
            status: "frozen".into(),
            subject: "Your jobs — 2026-09-06".into(),
            body: "Example Engineer is a credible fit, with compensation still unknown.\n".into(),
            packet_ids: vec![packet.id.clone()],
            attachments: vec![attachment.id.clone()],
            attachment_sha256: vec![attachment.sha256.clone()],
            idempotency_key: "platter/2026-09-06/test-occurrence".into(),
            receipt: None,
        };
        store.freeze(&edition)?;
        Ok(Self {
            directory,
            email,
            attachment,
            packet,
            edition,
        })
    }

    fn root(&self) -> &Path {
        self.directory.path()
    }

    fn email_output(&self, suffix: &str) -> PathBuf {
        self.email.with_file_name(format!("fake-email.{suffix}"))
    }

    fn current_edition(&self) -> Result<Edition> {
        Store::open(self.root())?
            .edition(&self.edition.day)?
            .context("frozen edition disappeared")
    }

    fn packet_status(&self) -> Result<String> {
        Ok(Store::open(self.root())?
            .packet(&self.packet.opportunity)?
            .context("packet disappeared")?
            .status)
    }

    fn candidates(&self, jobs: &[serde_json::Value]) -> Result<()> {
        let mut settings = workflow::config(self.root())?;
        settings.cast_executable = self.root().join("candidate-cast");
        fs::write(
            &settings.cast_executable,
            "#!/bin/sh\n/bin/cat \"$0.json\"\n",
        )?;
        fs::set_permissions(&settings.cast_executable, fs::Permissions::from_mode(0o700))?;
        platter::write_json(
            &self.root().join("candidate-cast.json"),
            &serde_json::json!({
                "schema_version":1,"snapshot_revision":1,"captured_at":"2026-09-07T23:00:00Z",
                "companies":[],"source_health":[],"coverage":[],"jobs":jobs
            }),
        )?;
        Store::open(self.root())?.set_setting("config", &settings)
    }

    fn cached_posting(
        &self,
        id: &str,
        listed: bool,
    ) -> Result<(serde_json::Value, serde_json::Value)> {
        let mut job = candidate(id, &format!("https://jobs.ashbyhq.com/{id}/role"));
        job["source_key"] = serde_json::json!(format!("ashby:{id}:role"));
        let posting = serde_json::json!({
            "jobUrl":job["url"], "isListed":listed,
            "descriptionHtml":"Build and operate reliable infrastructure for a growing organization. ".repeat(5)
        });
        platter::write_json(
            &self.root().join(format!("ashby-cache/{id}.json")),
            &serde_json::json!({"retrieved_at":cast::now(),"response":{"jobs":[posting]}}),
        )?;
        Ok((job, posting))
    }

    fn ready_candidate(&self, id: &str, listed: bool) -> Result<PacketRecord> {
        let (job, posting) = self.cached_posting(id, listed)?;
        let packet = PacketRecord {
            id: format!("packet-{id}"),
            opportunity: format!("ashby:{id}:role"),
            job_id: id.into(),
            company: "Example".into(),
            title: "Engineer".into(),
            status: "ready".into(),
            directory: String::new(),
        };
        let store = Store::open(self.root())?;
        store.insert_run(
            &packet,
            &serde_json::json!({
                "job":job,"company":"Example","career":[],"template_artifact":"unused-template",
                "posting":{"url":job["url"],"retrieved_at":cast::now(),"text":posting.to_string()}
            }),
        )?;
        store.put_content(&packet.id, "brief", &serde_json::json!({
            "paragraph":"Why it works: Strong infrastructure fit.\n\nRole: Build reliable systems.",
            "pursue":true
        }))?;
        store.put_content(
            &packet.id,
            "resume-content",
            &serde_json::json!({
                "jackson_bullets":["Built reliable systems"],"evidence":[]
            }),
        )?;
        store.put_artifact(
            Some(&packet.id),
            "resume-pdf",
            "resume.pdf",
            "application/pdf",
            b"%PDF-fixture",
        )?;
        Ok(packet)
    }
}

fn candidate(id: &str, url: &str) -> serde_json::Value {
    serde_json::json!({
        "id":id,"revision":1,"company_id":"example","source_id":"fixture",
        "source_key":"fixture","title":"Engineer","url":url,
        "first_seen_at":"2026-09-07T23:00:00Z","last_seen_at":"2026-09-07T23:00:00Z",
        "availability":"listed","missing_complete_snapshots":0,"evidence":[],
        "compensation":[],"geographic_eligibility":[],"parser_version":"fixture"
    })
}

#[test]
fn accepted_frozen_edition_sends_once_and_reuses_exact_receipt() -> Result<()> {
    let fixture = Fixture::new("Accepted receipt-1")?;
    let first = workflow::send(fixture.root(), &fixture.edition.day)?;
    let second = workflow::send(fixture.root(), &fixture.edition.day)?;
    assert_eq!(first.status, "sent");
    assert_eq!(first.receipt.as_deref(), Some("Accepted receipt-1"));
    assert_eq!(second.receipt, first.receipt);
    assert_eq!(second.idempotency_key, fixture.edition.idempotency_key);
    assert_eq!(fixture.current_edition()?.status, "sent");
    assert_eq!(fixture.packet_status()?, "ready");
    assert!(!Store::open(fixture.root())?.is_eligible(&fixture.packet.opportunity)?);
    assert_eq!(
        fs::read_to_string(fixture.email_output("calls"))?,
        "invoked\n"
    );
    let payload: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.email_output("body"))?)?;
    assert_eq!(payload["body"], fixture.edition.body);
    assert_eq!(
        payload["attachments"][0]["filename"],
        fixture.attachment.filename
    );
    let content = payload["attachments"][0]["content"]
        .as_str()
        .context("missing attachment bytes")?;
    assert_eq!(
        base64::engine::general_purpose::STANDARD.decode(content)?,
        fixture.attachment.content
    );
    let arguments = fs::read_to_string(fixture.email_output("arguments"))?;
    let expected = format!(
        "--payload-stdin\n--idempotency-key\n{}\n{}\n-\n",
        fixture.edition.idempotency_key, fixture.edition.subject,
    );
    assert_eq!(arguments, expected);
    Ok(())
}

#[test]
fn retained_attachment_corruption_is_rejected_before_external_invocation() -> Result<()> {
    let fixture = Fixture::new("Accepted receipt-1")?;
    let connection = rusqlite::Connection::open(fixture.root().join(platter::store::DATABASE))?;
    assert!(
        connection
            .execute(
                "UPDATE artifacts SET content=?1 WHERE id=?2",
                rusqlite::params![b"changed".as_slice(), fixture.attachment.id]
            )
            .is_err()
    );
    // Simulate damaged storage after proving normal writes cannot change an artifact.
    connection.execute_batch("DROP TRIGGER immutable_artifact")?;
    connection.execute(
        "UPDATE artifacts SET content=?1 WHERE id=?2",
        rusqlite::params![b"changed".as_slice(), fixture.attachment.id],
    )?;
    drop(connection);
    let error = workflow::send(fixture.root(), &fixture.edition.day)
        .err()
        .context("changed attachment unexpectedly submitted")?;
    assert!(
        error
            .to_string()
            .contains("retained artifact integrity mismatch")
    );
    assert!(!fixture.email_output("calls").exists());
    let connection = rusqlite::Connection::open(fixture.root().join(platter::store::DATABASE))?;
    let status: String = connection.query_row(
        "SELECT status FROM editions WHERE id=?1",
        [&fixture.edition.day],
        |r| r.get(0),
    )?;
    assert_eq!(status, "frozen");
    assert_eq!(fixture.packet_status()?, "ready");
    assert!(!Store::open(fixture.root())?.is_eligible(&fixture.packet.opportunity)?);
    Ok(())
}

#[test]
fn ambiguous_receipt_remains_unresolved_and_cannot_send_again() -> Result<()> {
    let fixture = Fixture::new("provider accepted something but returned an unrecognized receipt")?;
    let first_error = workflow::send(fixture.root(), &fixture.edition.day)
        .err()
        .context("ambiguous receipt unexpectedly accepted")?;
    assert!(
        first_error
            .to_string()
            .contains("unrecognized Email receipt")
    );
    let unresolved = fixture.current_edition()?;
    assert_eq!(unresolved.status, "sending");
    assert!(unresolved.receipt.is_none());
    assert_eq!(unresolved.idempotency_key, fixture.edition.idempotency_key);
    assert_eq!(fixture.packet_status()?, "ready");
    assert!(!Store::open(fixture.root())?.is_eligible(&fixture.packet.opportunity)?);
    let retry_error = workflow::send(fixture.root(), &fixture.edition.day)
        .err()
        .context("ambiguous occurrence unexpectedly retried")?;
    assert!(
        retry_error
            .to_string()
            .contains("send outcome is unresolved")
    );
    assert_eq!(
        fs::read_to_string(fixture.email_output("calls"))?,
        "invoked\n"
    );
    assert_eq!(fixture.current_edition()?.status, "sending");
    Ok(())
}

#[tokio::test]
async fn daily_sends_the_local_date_once_without_preparing_after_freeze() -> Result<()> {
    let fixture = Fixture::new("Accepted daily-receipt")?;
    // September 7 UTC is still September 6 in the configured Chicago zone.
    let now = "2026-09-07T02:00:00Z".parse()?;
    let first = workflow::run_daily(fixture.root(), now)
        .await?
        .context("daily edition missing")?;
    let second = workflow::run_daily(fixture.root(), now)
        .await?
        .context("accepted edition missing")?;
    assert_eq!(first.day, "2026-09-06");
    assert_eq!(first.status, "sent");
    assert_eq!(first.body, fixture.edition.body);
    assert_eq!(first.attachments, fixture.edition.attachments);
    assert_eq!(second.receipt, first.receipt);
    assert_eq!(first.idempotency_key, fixture.edition.idempotency_key);
    assert_eq!(
        fs::read_to_string(fixture.email_output("calls"))?,
        "invoked\n"
    );
    // Cast and CRM do not exist in this fixture, so either preparation would fail.
    Ok(())
}

#[tokio::test]
async fn daily_holds_uncertain_sends_without_preparing_or_resending() -> Result<()> {
    let fixture = Fixture::new("ambiguous provider response")?;
    let now = "2026-09-06T23:00:00Z".parse()?;
    assert!(workflow::run_daily(fixture.root(), now).await.is_err());
    let error = workflow::run_daily(fixture.root(), now)
        .await
        .err()
        .context("uncertain daily send unexpectedly retried")?;
    assert!(error.to_string().contains("send outcome is unresolved"));
    assert_eq!(fixture.current_edition()?.status, "sending");
    assert_eq!(
        fs::read_to_string(fixture.email_output("calls"))?,
        "invoked\n"
    );
    Ok(())
}

#[tokio::test]
async fn daily_empty_pool_creates_no_edition_or_email() -> Result<()> {
    let fixture = Fixture::new("Accepted unexpected")?;
    let mut settings = workflow::config(fixture.root())?;
    settings.cast_executable = fixture.root().join("empty-cast");
    fs::write(
        &settings.cast_executable,
        "#!/bin/sh\n/bin/cat \"$0.json\"\n",
    )?;
    fs::set_permissions(&settings.cast_executable, fs::Permissions::from_mode(0o700))?;
    platter::write_json(
        &fixture.root().join("empty-cast.json"),
        &serde_json::json!({
            "schema_version": 1, "snapshot_revision": 1,
            "captured_at": "2026-09-07T23:00:00Z",
            "companies": [], "jobs": [], "source_health": [], "coverage": []
        }),
    )?;
    Store::open(fixture.root())?.set_setting("config", &settings)?;
    let result = workflow::run_daily(fixture.root(), "2026-09-07T23:00:00Z".parse()?).await?;
    assert!(result.is_none());
    assert!(
        Store::open(fixture.root())?
            .edition("2026-09-07")?
            .is_none()
    );
    assert!(!fixture.email_output("calls").exists());
    assert_eq!(fixture.current_edition()?.status, "frozen");
    Ok(())
}

#[tokio::test]
async fn daily_preparation_failure_does_not_send_or_create_an_edition() -> Result<()> {
    let fixture = Fixture::new("Accepted unexpected")?;
    assert!(
        workflow::run_daily(fixture.root(), "2026-09-07T23:00:00Z".parse()?)
            .await
            .is_err()
    );
    assert!(
        Store::open(fixture.root())?
            .edition("2026-09-07")?
            .is_none()
    );
    assert!(!fixture.email_output("calls").exists());
    assert_eq!(fixture.current_edition()?.status, "frozen");
    Ok(())
}

#[tokio::test]
async fn daily_unavailable_candidates_are_ineligible_without_an_edition() -> Result<()> {
    let fixture = Fixture::new("Accepted unexpected")?;
    fixture.candidates(&[
        candidate("first", "http://example.com/first"),
        candidate("second", "http://example.com/second"),
    ])?;
    // HTTP is rejected locally at the posting retrieval boundary.
    for _ in 0..2 {
        assert!(
            workflow::run_daily(fixture.root(), "2026-09-07T23:00:00Z".parse()?)
                .await?
                .is_none()
        );
    }
    let store = Store::open(fixture.root())?;
    assert!(!store.is_eligible("http://example.com/first")?);
    assert!(!store.is_eligible("http://example.com/second")?);
    assert_eq!(store.jobs()?.len(), 3);
    assert_eq!(store.list()?.len(), 1); // No incomplete run with missing inputs.
    store.set_eligible("first", true)?;
    let error = workflow::prepare(fixture.root(), "first", None)
        .await
        .err()
        .context("unavailable posting unexpectedly prepared")?;
    assert!(format!("{error:#}").contains("posting must use public HTTPS"));
    assert!(!store.is_eligible("http://example.com/first")?);
    assert!(
        Store::open(fixture.root())?
            .edition("2026-09-07")?
            .is_none()
    );
    assert!(!fixture.email_output("calls").exists());
    assert_eq!(fixture.current_edition()?.status, "frozen");
    Ok(())
}

#[tokio::test]
async fn daily_skips_unavailable_candidates_and_sends_the_ready_packet_once() -> Result<()> {
    let fixture = Fixture::new("Accepted replacement")?;
    let packet = fixture.ready_candidate("available", true)?;
    fixture.candidates(&[
        candidate("first", "http://example.com/first"),
        candidate("second", "http://example.com/second"),
    ])?;
    let now = "2026-09-07T23:00:00Z".parse()?;
    let edition = workflow::run_daily(fixture.root(), now)
        .await?
        .context("missing edition")?;
    assert_eq!(edition.status, "sent");
    assert_eq!(edition.packet_ids, vec![packet.id]);
    let store = Store::open(fixture.root())?;
    assert!(!store.is_eligible("http://example.com/first")?);
    assert!(!store.is_eligible("http://example.com/second")?);
    assert_eq!(
        workflow::run_daily(fixture.root(), now)
            .await?
            .context("accepted edition disappeared")?
            .receipt,
        edition.receipt
    );
    assert_eq!(
        fs::read_to_string(fixture.email_output("calls"))?,
        "invoked\n"
    );
    Ok(())
}

#[tokio::test]
async fn daily_excludes_unavailable_prepared_postings_and_preserves_their_artifacts() -> Result<()>
{
    let fixture = Fixture::new("Accepted replacement")?;
    let unavailable = fixture.ready_candidate("unavailable", false)?;
    let available = fixture.ready_candidate("available", true)?;
    fixture.candidates(&[])?;
    let edition = workflow::run_daily(fixture.root(), "2026-09-07T23:00:00Z".parse()?)
        .await?
        .context("missing edition")?;
    assert_eq!(edition.packet_ids, vec![available.id]);
    let store = Store::open(fixture.root())?;
    assert!(!store.is_eligible(&unavailable.opportunity)?);
    assert_eq!(store.run(&unavailable.id)?.status, "deferred");
    assert!(store.run_artifact(&unavailable.id, "resume-pdf")?.is_some());
    // Excluded postings are not fetched again, even in a later invocation.
    fs::remove_file(fixture.root().join("ashby-cache/unavailable.json"))?;
    assert!(
        workflow::prepare_daily(fixture.root(), None)
            .await?
            .is_empty()
    );
    Ok(())
}

#[tokio::test]
async fn daily_tries_more_candidates_when_final_freshness_empties_the_pool() -> Result<()> {
    let fixture = Fixture::new("Accepted unexpected")?;
    let mut packets = vec![];
    for id in ["closing-one", "closing-two", "closing-three"] {
        packets.push(fixture.ready_candidate(id, true)?);
        let cache = fixture.root().join(format!("ashby-cache/{id}.json"));
        let mut unlisted: serde_json::Value = serde_json::from_slice(&fs::read(&cache)?)?;
        unlisted["response"]["jobs"][0]["isListed"] = serde_json::json!(false);
        platter::write_json(
            &fixture.root().join(format!("candidate-cast.{id}")),
            &unlisted,
        )?;
    }
    fixture.candidates(&[candidate("replacement", "http://example.com/replacement")])?;
    let settings = workflow::config(fixture.root())?;
    // Cast export falls between initial refresh and final preview. Close the
    // ready posting then, after preparation has already filled its pool.
    fs::write(
        &settings.cast_executable,
        "#!/bin/sh\nset -eu\nprintf 'invoked\\n' >> \"$0.calls\"\nfor id in closing-one closing-two closing-three; do\n/bin/cp \"$0.$id\" \"$(dirname \"$0\")/ashby-cache/$id.json\"\ndone\n/bin/cat \"$0.json\"\n",
    )?;
    assert!(
        workflow::run_daily(fixture.root(), "2026-09-07T23:00:00Z".parse()?)
            .await?
            .is_none()
    );
    let store = Store::open(fixture.root())?;
    for packet in packets {
        assert!(!store.is_eligible(&packet.opportunity)?);
    }
    assert!(!store.is_eligible("http://example.com/replacement")?);
    assert_eq!(
        fs::read_to_string(fixture.root().join("candidate-cast.calls"))?,
        "invoked\ninvoked\n"
    );
    assert!(!fixture.email_output("calls").exists());
    Ok(())
}

#[tokio::test]
async fn daily_reaches_the_next_candidate_but_still_stops_on_career_capture_failure() -> Result<()>
{
    let fixture = Fixture::new("Accepted unexpected")?;
    let (next, _) = fixture.cached_posting("second", true)?;
    fixture.candidates(&[candidate("first", "http://example.com/first"), next])?;
    assert!(
        workflow::run_daily(fixture.root(), "2026-09-07T23:00:00Z".parse()?)
            .await
            .is_err()
    );
    let store = Store::open(fixture.root())?;
    assert!(!store.is_eligible("http://example.com/first")?);
    assert!(store.is_eligible("ashby:second:role")?);
    assert!(store.edition("2026-09-07")?.is_none());
    assert!(!fixture.email_output("calls").exists());
    Ok(())
}
