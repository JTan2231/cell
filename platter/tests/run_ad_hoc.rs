//! URL-selected delivery reuses retained packet, edition and send records.
use anyhow::Result;
use cast::models::Job;
use platter::{
    Config,
    store::{PacketRecord, Store},
};
use std::{
    fs,
    os::unix::fs::PermissionsExt as _,
    path::{Path, PathBuf},
    process::{Command, Output},
};

struct Fixture {
    directory: tempfile::TempDir,
    home: PathBuf,
    root: PathBuf,
    cast: PathBuf,
    email: PathBuf,
    url: String,
    packet: PacketRecord,
}

impl Fixture {
    #[allow(
        clippy::too_many_lines,
        reason = "Keep one complete private-state and executable fixture together"
    )]
    fn new() -> Result<Self> {
        let directory = tempfile::tempdir()?;
        let home = directory.path().join("home");
        let root = home.join(".local/share/platter");
        let tools = directory.path().join("tools");
        fs::create_dir_all(&tools)?;
        let cast = tools.join("cast");
        let email = tools.join("email");
        fs::write(
            &cast,
            "#!/bin/sh\nset -eu\nprintf '%s\\n' \"$*\" >> \"$0.calls\"\ncase \"$1:$2\" in\n  job:collect) /bin/cat \"$0.job\" ;;\n  export:--json) /bin/cat \"$0.snapshot\" ;;\n  *) exit 2 ;;\nesac\n",
        )?;
        fs::write(
            &email,
            "#!/bin/sh\nset -eu\nprintf 'invoked\\n' >> \"$0.calls\"\n/bin/cat > \"$0.payload\"\nprintf 'Accepted receipt-url-one\\n'\n",
        )?;
        fs::set_permissions(&cast, fs::Permissions::from_mode(0o700))?;
        fs::set_permissions(&email, fs::Permissions::from_mode(0o700))?;

        let url = "https://jobs.ashbyhq.com/acme/role-one".to_owned();
        let job = Job {
            id: "cast-role-one".into(),
            revision: 1,
            company_id: "company-acme".into(),
            source_id: "source-acme".into(),
            source_key: "ashby:acme:role-one".into(),
            title: "Infrastructure Engineer".into(),
            url: url.clone(),
            apply_url: None,
            location: Some("Remote".into()),
            remote: Some(true),
            employment_type: Some("FullTime".into()),
            source_published_at: Some("2026-09-01".into()),
            source_updated_at: None,
            source_internal_id: None,
            first_seen_at: "2026-09-10T00:00:00Z".into(),
            last_seen_at: "2026-09-10T00:00:00Z".into(),
            availability: "listed".into(),
            missing_complete_snapshots: 0,
            first_missing_at: None,
            description: Some("Build reliable infrastructure.".into()),
            evidence: vec![],
            compensation: vec![],
            geographic_eligibility: vec![],
            published_at_semantics: Some("last_published_at".into()),
            content_fingerprint: None,
            parser_version: "fixture".into(),
        };
        platter::write_json(
            &cast.with_extension("job"),
            &cast::models::JobSelection {
                schema_version: 1,
                job: job.clone(),
            },
        )?;
        platter::write_json(
            &cast.with_extension("snapshot"),
            &serde_json::json!({
                "schema_version":1,
                "snapshot_revision":1,
                "captured_at":"2026-09-10T00:00:00Z",
                "companies":[{
                    "id":"company-acme","revision":1,"name":"Acme","domain":null,
                    "website_url":null,"first_seen_at":"2026-09-10T00:00:00Z",
                    "last_seen_at":"2026-09-10T00:00:00Z","relevance_reasons":[],"evidence":[]
                }],
                "jobs":[job],"source_health":[],"coverage":[]
            }),
        )?;

        let config = Config {
            daily_count: 3,
            delivery_hour: 9,
            delivery_minute: 0,
            timezone: "America/Chicago".into(),
            cast_executable: cast.clone(),
            email_executable: email.clone(),
            original_resume: tools.join("unused-resume.tex"),
        };
        let store = Store::open(&root)?;
        store.set_setting("config", &config)?;
        let packet = PacketRecord {
            id: "packet-url-one".into(),
            opportunity: "ashby:acme:role-one".into(),
            job_id: "cast-role-one".into(),
            company: "Acme".into(),
            title: "Infrastructure Engineer".into(),
            status: "ready".into(),
            directory: String::new(),
        };
        let posting = serde_json::json!({
            "jobUrl":url,
            "title":"Infrastructure Engineer",
            "isListed":true,
            "descriptionHtml":"Build and operate reliable infrastructure for a growing product organization. ".repeat(5)
        });
        store.insert_run(
            &packet,
            &serde_json::json!({
                "job":job,
                "company":"Acme",
                "posting":{
                    "url":url,
                    "retrieved_at":cast::now(),
                    "text":posting.to_string()
                },
                "career":[],
                "template_artifact":"unused-template"
            }),
        )?;
        store.put_content(
            &packet.id,
            "brief",
            &serde_json::json!({
                "paragraph":"Why it works: Strong infrastructure fit.\n\nRole: Build reliable systems.",
                "pursue":true
            }),
        )?;
        store.put_content(
            &packet.id,
            "resume-content",
            &serde_json::json!({"jackson_bullets":["Built reliable systems"],"evidence":[]}),
        )?;
        store.put_artifact(
            Some(&packet.id),
            "resume-pdf",
            "Acme-resume.pdf",
            "application/pdf",
            b"%PDF-1.4\nretained fixture\n",
        )?;
        platter::write_json(
            &root.join("ashby-cache/acme.json"),
            &serde_json::json!({
                "retrieved_at":cast::now(),
                "response":{"jobs":[posting]}
            }),
        )?;
        Ok(Self {
            directory,
            home,
            root,
            cast,
            email,
            url,
            packet,
        })
    }

    fn run(&self, url: &str) -> Result<Output> {
        Ok(Command::new(env!("CARGO_BIN_EXE_platter"))
            .env("HOME", &self.home)
            .args([
                "--stop-after-seconds",
                "600",
                "run-ad-hoc",
                url,
                "--id",
                "url-one",
            ])
            .output()?)
    }

    fn calls(executable: &Path) -> Result<String> {
        Ok(fs::read_to_string(executable.with_extension("calls"))?)
    }
}

#[test]
fn url_selected_command_reuses_the_daily_packet_and_send_records() -> Result<()> {
    let fixture = Fixture::new()?;
    let first = fixture.run(&format!("{}/application", fixture.url))?;
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(String::from_utf8_lossy(&first.stdout).contains("sent (1 packet)"));
    let edition = Store::open(&fixture.root)?
        .edition("ad-hoc/url-one")?
        .ok_or_else(|| anyhow::anyhow!("URL-selected edition is missing"))?;
    assert_eq!(edition.packet_ids, vec![fixture.packet.id.clone()]);
    assert_eq!(edition.status, "sent");
    assert!(!Store::open(&fixture.root)?.is_eligible(&fixture.packet.opportunity)?);
    assert_eq!(Fixture::calls(&fixture.cast)?.lines().count(), 2);
    assert_eq!(Fixture::calls(&fixture.email)?, "invoked\n");

    let second = fixture.run(&fixture.url)?;
    assert!(second.status.success());
    assert_eq!(Fixture::calls(&fixture.cast)?.lines().count(), 2);
    assert_eq!(Fixture::calls(&fixture.email)?, "invoked\n");

    let changed = fixture.run("https://jobs.ashbyhq.com/acme/another-role")?;
    assert!(!changed.status.success());
    assert!(String::from_utf8_lossy(&changed.stderr).contains("different job URL"));
    assert_eq!(Fixture::calls(&fixture.cast)?.lines().count(), 2);
    assert_eq!(Fixture::calls(&fixture.email)?, "invoked\n");
    drop(fixture.directory);
    Ok(())
}

#[test]
#[allow(clippy::too_many_lines)] // One isolated CLI scenario covers capture, retry and preserved delivery.
fn regeneration_captures_a_new_packet_and_retries_without_reenabling_or_sending() -> Result<()> {
    use serde_json::{Value, json};
    let fixture = Fixture::new()?;
    let store = Store::open(&fixture.root)?;
    // Start with a sent packet and its retained receipt.
    assert!(fixture.run(&fixture.url)?.status.success());
    let original = serde_json::to_value(store.run(&fixture.packet.id)?)?;
    let edition = serde_json::to_value(store.edition("ad-hoc/url-one")?)?;
    let old_pdf = store
        .run_artifact(&fixture.packet.id, "resume-pdf")?
        .ok_or_else(|| anyhow::anyhow!("missing PDF"))?;
    let annals = fixture.home.join(".local/bin/annals");
    fs::create_dir_all(
        annals
            .parent()
            .ok_or_else(|| anyhow::anyhow!("missing parent"))?,
    )?;
    fs::write(
        &annals,
        "#!/bin/sh\nset -eu\nprintf 'read\\n' >> \"$0.calls\"\ncase \"$*\" in\n'--json library vita work list --limit=1000') /bin/cat \"$0.list\";;\n'--json library vita work show -- story') /bin/cat \"$0.story\";;\n*) exit 91;;\nesac\n",
    )?;
    fs::set_permissions(&annals, fs::Permissions::from_mode(0o700))?;
    let summary =
        json!({"work":"story","sha256":"unused","size_bytes":0,"first_retained_at":"unused"});
    fs::write(
        annals.with_extension("list"),
        json!({"ok":true,"data":{"schema_version":2,"items":[summary],"has_more":false}})
            .to_string(),
    )?;
    let mut work = summary;
    work["text"] = json!("Current career evidence");
    work["headings"] = json!([]);
    fs::write(
        annals.with_extension("story"),
        json!({"ok":true,"data":work}).to_string(),
    )?;
    // Capture commits before this deliberately missing template stops execution.
    store.set_setting("template", &"missing-template")?;
    let run = |job: &str, id: &str| -> Result<Output> {
        Ok(Command::new(env!("CARGO_BIN_EXE_platter"))
            .env_clear()
            .env("HOME", &fixture.home)
            .env("PATH", "/usr/bin:/bin")
            .env(
                "NUCLEUS_SOCKET",
                fixture.directory.path().join("absent.sock"),
            )
            .env("CHANCERY_USAGE_DISABLED", "1")
            .args(["regenerate", job, "--id", id])
            .output()?)
    };
    let first = run(&fixture.packet.job_id, "regeneration-one")?;
    assert!(!first.status.success());
    let records = store.list()?;
    assert_eq!(records.len(), 2);
    let new = records
        .iter()
        .find(|packet| packet.id != fixture.packet.id)
        .ok_or_else(|| anyhow::anyhow!("new packet missing"))?;
    let captured: Value = store.inputs(&new.id)?;
    assert_eq!(captured["regeneration_id"], "regeneration-one");
    assert_eq!(captured["career"][0]["markdown"], "Current career evidence");
    assert_eq!(
        captured["resume_editorial"],
        include_str!("../prompts/resume-editorial.md")
    );
    assert_eq!(
        captured["project_resources"],
        include_str!("../prompts/project-resources.md")
    );
    assert!(store.run_artifact(&new.id, "brief")?.is_none());
    assert!(store.execution(&new.id, "brief")?.is_none());
    let cast_calls = Fixture::calls(&fixture.cast)?;
    let career_calls = Fixture::calls(&annals)?;
    fs::remove_file(&fixture.cast)?;
    fs::remove_file(&annals)?;
    let retry = run(&fixture.packet.job_id, "regeneration-one")?;
    assert!(!retry.status.success());
    assert_eq!(retry.stderr, first.stderr);
    assert_eq!(store.list()?.len(), 2);
    assert_eq!(store.inputs::<Value>(&new.id)?, captured);
    let conflict = run("different-job", "regeneration-one")?;
    assert!(String::from_utf8_lossy(&conflict.stderr).contains("different job"));
    assert!(!run(&fixture.packet.job_id, "../bad")?.status.success());

    // A retained completed result is returned without any dependency calls.
    let new_brief = store.put_content(
        &new.id,
        "brief",
        &json!({"paragraph":"New brief", "pursue":true}),
    )?;
    let new_pdf = store.put_artifact(
        Some(&new.id),
        "resume-pdf",
        "new.pdf",
        "application/pdf",
        b"new PDF fixture",
    )?;
    store.status(&new.id, "ready")?;
    let complete = run(&fixture.packet.job_id, "regeneration-one")?;
    assert!(complete.status.success());
    let output = String::from_utf8(complete.stdout)?;
    assert!(output.contains(&format!("{}: ready", new.id)));
    assert!(output.contains(&new_brief.id) && output.contains(&new_pdf.id));
    assert_eq!(store.list()?.len(), 2);
    assert!(!store.is_eligible(&fixture.packet.opportunity)?);
    assert_eq!(
        serde_json::to_value(store.run(&fixture.packet.id)?)?,
        original
    );
    assert_eq!(
        serde_json::to_value(store.edition("ad-hoc/url-one")?)?,
        edition
    );
    assert_eq!(store.artifact(&old_pdf.id)?.content, old_pdf.content);
    assert_eq!(Fixture::calls(&fixture.cast)?, cast_calls);
    assert_eq!(Fixture::calls(&annals)?, career_calls);
    assert_eq!(Fixture::calls(&fixture.email)?, "invoked\n");
    platter::maintenance::gate(&fixture.home).hold("regeneration-test")?;
    assert!(
        !run(&fixture.packet.job_id, "regeneration-one")?
            .status
            .success()
    );
    Ok(())
}
