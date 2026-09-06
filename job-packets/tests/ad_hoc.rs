//! Offline debug sends must not consume discovery budgets or ordinary jobs.
use anyhow::{Context, Result};
use job_packets::{
    Config, ad_hoc,
    store::{Edition, PacketRecord, Store},
};
use std::{
    fs,
    os::unix::fs::PermissionsExt as _,
    path::{Path, PathBuf},
};

struct Fixture {
    directory: tempfile::TempDir,
    email: PathBuf,
    baseline: Vec<u8>,
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
        job_packets::write_json(&root.join("config.json"), &config)?;
        let mut store = Store::open(root)?;
        let mut ids = Vec::new();
        for index in 1..=3 {
            let id = format!("packet-{index}");
            let packet = root.join("packets").join(&id);
            job_packets::private_dir(&packet)?;
            let resume = packet.join("resume.pdf");
            fs::write(&resume, b"%PDF-1.4\naccepted test resume\n")?;
            job_packets::write_json(
                &packet.join("inputs.json"),
                &serde_json::json!({
                    "company":"synthetic-source-label","job":{"title":format!("Engineer {index}"),"url":"https://example.invalid/retained-role"},
                    "posting":{"url":"https://example.invalid/retained-role","retrieved_at":"2026-09-06T21:00:00Z","text":serde_json::json!({"company_name":format!("Employer {index}")}).to_string()}
                }),
            )?;
            job_packets::write_json(
                &packet.join("brief-stage.json"),
                &serde_json::json!({"accepted":{"stage":"brief","result":{"paragraph":format!("Original brief {index} grounded in retained evidence."),"pursue":true}}}),
            )?;
            job_packets::write_json(
                &packet.join("resume-stage.json"),
                &serde_json::json!({"accepted":{"stage":"resume","result":{"jackson_bullets":["Supported Jackson work"],"evidence":[{"bullet_index":0,"career_entry_ids":["career-1"]}]}}}),
            )?;
            job_packets::write_json(
                &packet.join("artifacts.json"),
                &serde_json::json!({"resume_pdf":resume,"pages":1}),
            )?;
            store.insert(&PacketRecord {
                id: id.clone(),
                opportunity: format!("role-{index}"),
                job_id: format!("cast-{index}"),
                company: "synthetic-source-label".into(),
                title: format!("Engineer {index}"),
                status: "ready".into(),
                directory: packet.to_string_lossy().into_owned(),
            })?;
            ids.push(id);
        }
        // Retain real ordinary sent and reserved occurrences before the debug run.
        for (index, status) in [(0, "sent"), (1, "frozen")] {
            let mut edition = Edition {
                day: format!("2026-09-0{}", index + 1),
                status: "frozen".into(),
                subject: "Ordinary edition".into(),
                body: "Original ordinary body".into(),
                packet_ids: vec![ids[index].clone()],
                attachments: vec![],
                attachment_sha256: vec![],
                idempotency_key: format!("ordinary-{index}"),
                receipt: None,
            };
            store.freeze(&edition)?;
            if status == "sent" {
                edition.status = "sent".into();
                edition.receipt = Some("Accepted ordinary".into());
                store.save_edition(&edition)?;
            }
        }
        drop(store);
        let baseline = fs::read(root.join("packets.sqlite3"))?;
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
        assert_eq!(
            fs::read(self.root().join("packets.sqlite3"))?,
            self.baseline
        );
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
fn three_packet_debug_preview_and_repeated_send_leave_ordinary_state_byte_identical() -> Result<()>
{
    let fixture = Fixture::new("Accepted receipt-1")?;
    let original_brief = fs::read(fixture.root().join("packets/packet-1/brief-stage.json"))?;
    let overrides = fixture.root().join("reviewed-briefs.json");
    job_packets::write_json(
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
    assert!(edition.subject.starts_with("[TEST]"));
    assert!(edition.body.contains("1. Employer 1 — Engineer 1"));
    assert!(
        edition
            .body
            .contains("Reviewed brief with the corrected compensation wording.")
    );
    assert!(!edition.body.contains("synthetic-source-label"));
    assert!(edition.attachments[0].ends_with("1-Employer-1-resume.pdf"));
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
        fs::read(fixture.root().join("packets/packet-1/brief-stage.json"))?,
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
    assert!(error.to_string().contains("outcome is ambiguous"));
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
    fs::write(&edition.attachments[0], b"tampered")?;
    assert!(ad_hoc::send(fixture.root(), "2026-09-06", "frozen", Some(&fixture.email)).is_err());
    assert!(!fixture.root().join("fake-email.calls").exists());
    ad_hoc::preview(fixture.root(), "2026-09-06", "payload", &fixture.ids, None)?;
    let occurrence_path = fixture.root().join("ad-hoc/payload/occurrence.json");
    let mut occurrence: serde_json::Value = serde_json::from_slice(&fs::read(&occurrence_path)?)?;
    occurrence["edition"]["body"] = serde_json::json!("changed after freezing");
    job_packets::write_json(&occurrence_path, &occurrence)?;
    assert!(
        ad_hoc::send(
            fixture.root(),
            "2026-09-06",
            "payload",
            Some(&fixture.email)
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
