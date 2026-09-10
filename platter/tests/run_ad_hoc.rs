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
            crm_executable: tools.join("unused-crm"),
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
