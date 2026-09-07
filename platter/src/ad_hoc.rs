//! Explicit debug deliveries isolated from ordinary packet and edition state.
use std::{
    collections::BTreeMap,
    fs,
    io::Write as _,
    os::unix::fs::PermissionsExt as _,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    agent::{Brief, StageResult},
    source::Posting,
    store::{Edition, PacketRecord},
    workflow,
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Occurrence {
    version: u32,
    run_id: String,
    edition: Edition,
    payload_sha256: String,
    briefs: Vec<BriefBasis>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BriefBasis {
    packet_id: String,
    accepted: Brief,
    reviewed_override: Option<String>,
    inputs_sha256: String,
    brief_stage_sha256: String,
    resume_stage_sha256: String,
}

#[derive(Deserialize)]
struct Captured {
    company: String,
    job: CapturedJob,
    posting: Posting,
}
#[derive(Deserialize)]
struct CapturedJob {
    title: String,
    url: String,
}
#[derive(Deserialize)]
struct AcceptedStage {
    accepted: Option<StageResult>,
}
#[derive(Deserialize)]
struct Artifacts {
    resume_pdf: PathBuf,
    pages: usize,
}

/// Freeze a debug occurrence from already accepted retained materials only.
/// No discovery, profile, posting, preparation, or ordinary-state writes occur.
#[allow(clippy::too_many_lines)] // One retained-input freeze, with no source or ordinary-state effects.
pub fn preview(
    root: &Path,
    day: &str,
    run_id: &str,
    packet_ids: &[String],
    brief_overrides: Option<&Path>,
) -> Result<Edition> {
    validate_identity(day, run_id)?;
    let directory = root.join("ad-hoc").join(run_id);
    let path = directory.join("occurrence.json");
    let overrides: BTreeMap<String, String> = if let Some(path) = brief_overrides {
        ensure!(path.is_absolute(), "brief overrides path must be absolute");
        serde_json::from_slice(&fs::read(path)?)?
    } else {
        BTreeMap::new()
    };
    for paragraph in overrides.values() {
        validate_paragraph(paragraph)?;
    }
    if path.exists() {
        let occurrence = load(&path, day, run_id)?;
        ensure!(
            packet_ids.is_empty() || packet_ids == occurrence.edition.packet_ids,
            "ad hoc packet selection is immutable; use a new run ID"
        );
        if brief_overrides.is_some() {
            let retained: BTreeMap<_, _> = occurrence
                .briefs
                .iter()
                .filter_map(|basis| {
                    basis
                        .reviewed_override
                        .as_ref()
                        .map(|paragraph| (basis.packet_id.clone(), paragraph.clone()))
                })
                .collect();
            ensure!(
                retained == overrides,
                "ad hoc brief overrides are immutable; use a new run ID"
            );
        }
        verify_attachments(&occurrence.edition)?;
        return Ok(occurrence.edition);
    }
    let records = retained_packets(root)?;
    let selected: Vec<_> = if packet_ids.is_empty() {
        records
            .iter()
            .filter(|record| complete_files(Path::new(&record.directory)))
            .take(3)
            .collect()
    } else {
        ensure!(
            (1..=3).contains(&packet_ids.len()),
            "select one through three complete packets"
        );
        let mut selected = Vec::new();
        for id in packet_ids {
            ensure!(
                !selected
                    .iter()
                    .any(|record: &&PacketRecord| &record.id == id),
                "duplicate selected packet"
            );
            selected.push(
                records
                    .iter()
                    .find(|record| &record.id == id)
                    .context("selected packet not found")?,
            );
        }
        selected
    };
    ensure!(
        !selected.is_empty(),
        "no complete retained packets; prepare packets separately first"
    );
    ensure!(
        overrides
            .keys()
            .all(|id| selected.iter().any(|record| &record.id == id)),
        "brief override references an unselected packet"
    );
    crate::private_dir(&directory)?;
    let mut edition = Edition {
        day: day.into(),
        status: "frozen".into(),
        subject: format!("[TEST] Your jobs — {day}"),
        body: String::new(),
        packet_ids: Vec::new(),
        attachments: Vec::new(),
        attachment_sha256: Vec::new(),
        idempotency_key: format!("platter/ad-hoc/{run_id}/{}", uuid::Uuid::now_v7()),
        receipt: None,
    };
    let mut briefs = Vec::new();
    for (index, record) in selected.into_iter().enumerate() {
        use std::fmt::Write as _;
        let packet = Path::new(&record.directory);
        let inputs_bytes = fs::read(packet.join("inputs.json"))?;
        let captured: Captured = serde_json::from_slice(&inputs_bytes)?;
        let brief_bytes = fs::read(packet.join("brief-stage.json"))?;
        let resume_bytes = fs::read(packet.join("resume-stage.json"))?;
        let accepted: AcceptedStage = serde_json::from_slice(&brief_bytes)?;
        let Some(StageResult::Brief(brief)) = accepted.accepted else {
            anyhow::bail!("packet has no accepted brief")
        };
        ensure!(brief.pursue, "packet was not selected for pursuit");
        let accepted: AcceptedStage = serde_json::from_slice(&resume_bytes)?;
        ensure!(
            matches!(accepted.accepted, Some(StageResult::Resume(_))),
            "packet has no accepted resume"
        );
        let artifacts: Artifacts =
            serde_json::from_slice(&fs::read(packet.join("artifacts.json"))?)?;
        ensure!(
            artifacts.pages == 1,
            "retained resume has not passed its one-page check"
        );
        ensure!(
            fs::symlink_metadata(&artifacts.resume_pdf)?
                .file_type()
                .is_file(),
            "retained resume must be a regular file"
        );
        let pdf = fs::read(&artifacts.resume_pdf)?;
        ensure!(
            pdf.starts_with(b"%PDF-"),
            "retained attachment is not a PDF"
        );
        let employer = employer_name(&captured);
        let attachment = directory.join(format!(
            "{}-{}-resume.pdf",
            index + 1,
            filename_component(&employer)
        ));
        fs::write(&attachment, &pdf)?;
        fs::set_permissions(&attachment, fs::Permissions::from_mode(0o600))?;
        fs::File::open(&attachment)?.sync_all()?;
        let reviewed_override = overrides.get(&record.id).cloned();
        let paragraph = reviewed_override.as_deref().unwrap_or(&brief.paragraph);
        crate::agent::validate_brief_text(paragraph)?;
        write!(
            edition.body,
            "{}. {} — {}\n{}\n\n{}\n\n",
            index + 1,
            employer,
            captured.job.title,
            captured.job.url,
            paragraph
        )?;
        edition.packet_ids.push(record.id.clone());
        edition
            .attachments
            .push(attachment.to_string_lossy().into_owned());
        edition.attachment_sha256.push(digest(&pdf));
        briefs.push(BriefBasis {
            packet_id: record.id.clone(),
            accepted: brief,
            reviewed_override,
            inputs_sha256: digest(&inputs_bytes),
            brief_stage_sha256: digest(&brief_bytes),
            resume_stage_sha256: digest(&resume_bytes),
        });
    }
    let occurrence = Occurrence {
        version: 1,
        run_id: run_id.into(),
        payload_sha256: payload_digest(&edition)?,
        edition,
        briefs,
    };
    crate::write_json(&path, &occurrence)?;
    Ok(occurrence.edition)
}

/// Submit one frozen debug occurrence without consuming ordinary jobs.
pub fn send(
    root: &Path,
    day: &str,
    run_id: &str,
    email_executable: Option<&Path>,
) -> Result<Edition> {
    validate_identity(day, run_id)?;
    let path = root.join("ad-hoc").join(run_id).join("occurrence.json");
    let mut occurrence = load(&path, day, run_id)?;
    if occurrence.edition.status == "sent" {
        return Ok(occurrence.edition);
    }
    ensure!(
        occurrence.edition.status == "frozen",
        "ad hoc send outcome is ambiguous; inspect provider acceptance before another send"
    );
    verify_attachments(&occurrence.edition)?;
    let executable = if let Some(path) = email_executable {
        path.to_owned()
    } else {
        workflow::config(root)?.email_executable
    };
    ensure!(
        executable.is_absolute(),
        "Email executable must be absolute"
    );
    let mut command = std::process::Command::new(executable);
    command
        .arg("--idempotency-key")
        .arg(&occurrence.edition.idempotency_key);
    for attachment in &occurrence.edition.attachments {
        command.arg("--attach").arg(attachment);
    }
    command
        .arg(&occurrence.edition.subject)
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    occurrence.edition.status = "sending".into();
    crate::write_json(&path, &occurrence)?;
    let mut child = command.spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(occurrence.edition.body.as_bytes())?;
    }
    let output = child.wait_with_output()?;
    ensure!(
        output.status.success(),
        "Email submission failed or is ambiguous: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt = String::from_utf8(output.stdout)?;
    ensure!(
        receipt
            .trim()
            .strip_prefix("Accepted ")
            .is_some_and(|id| !id.is_empty() && !id.contains(char::is_whitespace)),
        "unrecognized Email receipt; ad hoc occurrence remains unresolved"
    );
    occurrence.edition.status = "sent".into();
    occurrence.edition.receipt = Some(receipt.trim().into());
    crate::write_json(&path, &occurrence)?;
    Ok(occurrence.edition)
}

fn retained_packets(root: &Path) -> Result<Vec<PacketRecord>> {
    let connection = Connection::open_with_flags(
        root.join("packets.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    ensure!(
        connection.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))? == 1,
        "unsupported Platter schema"
    );
    let mut statement = connection.prepare(
        "SELECT id,opportunity,job_id,company,title,status,directory FROM packets ORDER BY id",
    )?;
    Ok(statement
        .query_map([], |row| {
            Ok(PacketRecord {
                id: row.get(0)?,
                opportunity: row.get(1)?,
                job_id: row.get(2)?,
                company: row.get(3)?,
                title: row.get(4)?,
                status: row.get(5)?,
                directory: row.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?)
}
fn complete_files(packet: &Path) -> bool {
    [
        "inputs.json",
        "brief-stage.json",
        "resume-stage.json",
        "artifacts.json",
    ]
    .iter()
    .all(|file| packet.join(file).is_file())
}
fn validate_identity(day: &str, run_id: &str) -> Result<()> {
    let parsed = chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d")?;
    ensure!(
        parsed.format("%Y-%m-%d").to_string() == day,
        "date must use YYYY-MM-DD"
    );
    ensure!(
        !run_id.is_empty()
            && run_id.len() <= 80
            && run_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')),
        "ad hoc run ID must contain 1 to 80 ASCII letters, digits, hyphens or underscores"
    );
    Ok(())
}
fn validate_paragraph(paragraph: &str) -> Result<()> {
    ensure!(
        !paragraph.trim().is_empty()
            && !paragraph.contains(['\n', '\r'])
            && paragraph.split_whitespace().count() <= 150,
        "brief must be one nonempty paragraph of at most 150 words"
    );
    Ok(())
}
fn employer_name(captured: &Captured) -> String {
    let posting: serde_json::Value =
        serde_json::from_str(&captured.posting.text).unwrap_or_default();
    posting
        .get("company_name")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            posting
                .pointer("/hiringOrganization/name")
                .and_then(serde_json::Value::as_str)
        })
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(&captured.company)
        .trim()
        .to_owned()
}
fn filename_component(value: &str) -> String {
    let name: String = value
        .chars()
        .take(60)
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let name = name.trim_matches('-');
    if name.is_empty() {
        "employer".into()
    } else {
        name.into()
    }
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn payload_digest(edition: &Edition) -> Result<String> {
    Ok(digest(&serde_json::to_vec(
        &serde_json::json!({"day":edition.day,"subject":edition.subject,"body":edition.body,
        "packet_ids":edition.packet_ids,"attachments":edition.attachments,"attachment_sha256":edition.attachment_sha256,"idempotency_key":edition.idempotency_key}),
    )?))
}
fn load(path: &Path, day: &str, run_id: &str) -> Result<Occurrence> {
    let value: Occurrence = serde_json::from_slice(
        &fs::read(path).context("preview the ad hoc occurrence before sending")?,
    )?;
    ensure!(
        value.version == 1 && value.run_id == run_id && value.edition.day == day,
        "ad hoc occurrence identity conflict"
    );
    ensure!(
        value.payload_sha256 == payload_digest(&value.edition)?,
        "frozen ad hoc payload changed"
    );
    Ok(value)
}
fn verify_attachments(edition: &Edition) -> Result<()> {
    ensure!(
        (1..=3).contains(&edition.attachments.len())
            && edition.attachments.len() == edition.attachment_sha256.len(),
        "frozen attachment digests are absent"
    );
    for (path, expected) in edition.attachments.iter().zip(&edition.attachment_sha256) {
        ensure!(
            fs::symlink_metadata(path)?.file_type().is_file(),
            "frozen attachment must be a regular file"
        );
        ensure!(
            digest(&fs::read(path)?) == *expected,
            "frozen attachment changed after preview"
        );
    }
    Ok(())
}
