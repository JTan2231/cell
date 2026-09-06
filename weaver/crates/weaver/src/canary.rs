use crate::error::{AppResult, WeaverError};
use crate::project::{Project, STAGES};
use crate::state::{RunStatus, StateStore};
use nucleus_client::NucleusClient;
use nucleus_core::JobState;
use serde_json::{Value, json};
use std::path::Path;

const CANARY_ID: &str = "weaver-deployment-canary/1\n";

pub(crate) async fn run(directory: &Path) -> AppResult<Value> {
    prepare_fixture(directory).map_err(|error| WeaverError::runtime(error.to_string()))?;
    let project = Project::resolve(directory, "canary", true)?;
    let store = StateStore::open(directory.join("state"))?;
    if !store.root().join("current.json").exists() {
        store.enqueue(project.repo_root.clone(), project.slug.clone())?;
    }
    let current = store.read_current(None)?;
    if current.repo_root != project.repo_root || current.narrative != project.slug {
        return Err(WeaverError::runtime(
            "canary current workflow belongs to another repository",
        ));
    }
    if !current.status.is_terminal() {
        crate::pipeline::run_worker(&store).await?;
    }
    let current = store.read_current(Some(&current.run_id))?;
    if current.status != RunStatus::Succeeded
        || current.next_stage != 5
        || crate::validator::check(&project)? != crate::validator::Verdict::Pass
    {
        return Err(WeaverError::runtime(
            "Weaver deployment canary did not produce five verified outputs",
        ));
    }
    let client = NucleusClient::for_current_user()
        .map_err(|error| WeaverError::runtime(error.to_string()))?;
    let mut jobs = Vec::new();
    for stage in STAGES {
        let job_id = crate::nucleus::stage_job_id(&current.run_id, stage);
        let job = client
            .get_job(&job_id)
            .await
            .map_err(|error| WeaverError::runtime(error.to_string()))?;
        let output = job
            .attempts
            .last()
            .and_then(|attempt| attempt.output.as_ref());
        let bytes = std::fs::read(project.output_path(stage))
            .map_err(|error| WeaverError::runtime(error.to_string()))?;
        if job.summary.state != JobState::Completed
            || job.summary.requester.program != "weaver"
            || job.summary.requester.id != current.run_id
            || output.is_none_or(|output| output.final_message.as_bytes() != bytes)
        {
            return Err(WeaverError::runtime(
                "Weaver canary output and runtime correlation did not match",
            ));
        }
        jobs.push(job_id);
    }
    Ok(
        json!({ "protocol_version": 1, "verified": true, "directory": directory, "run_id": current.run_id, "job_ids": jobs, "outputs_verified": 5 }),
    )
}

fn prepare_fixture(directory: &Path) -> std::io::Result<()> {
    prepare_root(directory)?;
    for (name, content) in [
        (
            "AGENTS.md",
            "This is an isolated synthetic Weaver deployment canary. No real people or events are represented. Follow the exact stage output instruction.\n",
        ),
        (
            "narratives/README.md",
            "A synthetic narrative used only to verify the installed workflow.\n",
        ),
        (
            "narratives/canary/basis.md",
            "# Basis\n\n## Canonical sources\n\n- [Synthetic evidence](../../evidence/source.md)\n\n## Boundaries\n\nFictional verification fixture only.\n",
        ),
        (
            "narratives/canary/brief.md",
            "Verify that a fictional gardener made a checklist. This is synthetic, never a claim about a real person.\n",
        ),
        (
            "evidence/source.md",
            "A fictional gardener wrote a watering checklist for a fictional greenhouse.\n",
        ),
        (
            "workflow/narrative/common.md",
            "Return only the exact Markdown specified by the current stage. No fences, preamble, or additional newline. This fixture tests stage persistence and validation.\n",
        ),
        (
            "workflow/narrative/voice.md",
            "Use the exact synthetic text supplied by the stage.\n",
        ),
        (
            "workflow/narrative/stories.md",
            "Return exactly this Markdown:\n# Stories\n\n<a id=\"one-story\"></a>\n\n## One story\n\nA fictional gardener wrote a watering checklist.",
        ),
        (
            "workflow/narrative/themes.md",
            "Return exactly this Markdown:\n# Themes\n\n[Preparation](../01-stories/output.md#one-story)",
        ),
        (
            "workflow/narrative/compose.md",
            "Return exactly this Markdown:\n# A synthetic narrative\n\n[One fictional checklist](../01-stories/output.md#one-story)",
        ),
        (
            "workflow/narrative/review.md",
            "Return exactly this Markdown:\nVerdict: PASS\n\nThe synthetic narrative matches its evidence.",
        ),
        (
            "workflow/narrative/finalize.md",
            "The review passed. Copy the complete embedded 03-draft/output.md exactly, byte for byte, without extra text.\n",
        ),
    ] {
        fixture_file(directory, name, content)?;
    }
    Ok(())
}

fn fixture_file(root: &std::path::Path, relative: &str, text: &str) -> std::io::Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        let mut checked = root.to_path_buf();
        for component in parent
            .strip_prefix(root)
            .map_err(std::io::Error::other)?
            .components()
        {
            checked.push(component);
            match std::fs::create_dir(&checked) {
                Ok(()) => {}
                Err(error)
                    if error.kind() == std::io::ErrorKind::AlreadyExists
                        && std::fs::symlink_metadata(&checked)?.file_type().is_dir() => {}
                Err(error) => return Err(error),
            }
        }
    }
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
    {
        Ok(mut file) => {
            file.write_all(text.as_bytes())?;
            file.sync_all()
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let metadata = std::fs::symlink_metadata(&path)?;
            if !metadata.file_type().is_file() || std::fs::read_to_string(&path)? != text {
                return Err(std::io::Error::other(
                    "canary fixture is foreign or changed",
                ));
            }
            Ok(())
        }
        Err(error) => Err(error),
    }
}

fn prepare_root(root: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt as _;
    if !root.is_absolute() {
        return Err(std::io::Error::other("canary directory must be absolute"));
    }
    match std::fs::DirBuilder::new().mode(0o700).create(root) {
        Ok(()) => fixture_file(root, ".deployment-canary", CANARY_ID),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            if !std::fs::symlink_metadata(root)?.file_type().is_dir()
                || !std::fs::symlink_metadata(root.join(".deployment-canary"))?
                    .file_type()
                    .is_file()
                || std::fs::read_to_string(root.join(".deployment-canary"))? != CANARY_ID
            {
                return Err(std::io::Error::other(
                    "canary directory is not owned by this canary",
                ));
            }
            Ok(())
        }
        Err(error) => Err(error),
    }
}
