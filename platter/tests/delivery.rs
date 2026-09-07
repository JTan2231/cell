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
