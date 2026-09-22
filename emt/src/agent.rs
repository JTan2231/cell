//! Ordinary Nucleus agents discover and operate Cell through installed interfaces.
use crate::store::{Config, Exchange, Store};
use crate::{Result, fail};
use nucleus_client::{ClientError, NucleusClient};
use nucleus_core::{
    AbsolutePath, AgentInvocationV1, BuiltinToolsV1, HarnessCapability, JobId, JobRequestV1,
    JobState, JobV1, ModelId, ReasoningEffort, Requester, TimeoutSeconds, WorkspaceAccess,
};
use rusqlite::params;
use serde_json::json;
use std::path::Path;
use std::time::Duration;
use tokio::time::timeout;

const HTTP_TIMEOUT: Duration = Duration::from_secs(15);

pub enum Progress {
    Waiting,
    Complete,
    Failed,
    QuotaExhausted,
    QuotaExpired,
}

pub fn prepare(root: &Path, store: &Store, exchange: &Exchange, config: &Config) -> Result<String> {
    let clockwork = clockwork::api::Client::new(&config.clockwork_executable);
    let incident = clockwork.incident(&exchange.incident_id)?;
    let notification = clockwork.notification(&exchange.incident_id)?;
    store.connection.execute(
        "UPDATE incidents SET clockwork_json=?2,basic_email_json=?3 WHERE id=?1",
        params![
            exchange.incident_id,
            serde_json::to_string(&incident)?,
            serde_json::to_string(&notification)?
        ],
    )?;
    let definition=incident.definition_digest.as_deref().map(|digest| {
        clockwork.definition(digest).map_or_else(
            |_|json!({"digest":digest,"read_error":"Read the selected definition through Clockwork."}),
            |record|json!(record))
    });
    let correspondence=store.exchanges(&exchange.incident_id)?.into_iter()
        .filter(|item|item.id!=exchange.id)
        .map(|item|json!({"incoming_email":item.incoming_json,"outgoing_email":item.mail_json,
            "email_submission":item.send_state,"nucleus_job_id":item.nucleus_job_id,"state":item.state})).collect::<Vec<_>>();
    let prompts = cell_prompts::Prompts::load("emt")?;
    let task = if exchange.kind == "diagnosis" {
        "<bazaar:emt.diagnosis.instructions>"
    } else {
        "<bazaar:emt.intervention.instructions>"
    };
    let executable = std::env::current_exe()?.canonicalize()?;
    let instructions = prompts.render(
        "emt.responder.instructions",
        &[
            ("task", prompts.expand(task)?),
            (
                "chancery",
                crate::home()?
                    .join(".local/bin/chancery")
                    .display()
                    .to_string(),
            ),
            ("cell", config.cell_root.display().to_string()),
            ("emt", shell_quote(&executable.to_string_lossy())),
            ("exchange", exchange.id.clone()),
            ("deadline", exchange.deadline_at.to_string()),
        ],
    )?;
    let prompt = json!({
        "purpose":exchange.kind,"incident":incident,"selected_definition":definition,
        "clockwork_basic_notification":notification,"correspondence":correspondence,
        "current_received_email":exchange.incoming_json,"emt_state_root":root,
    })
    .to_string();
    let invocation = responder_invocation(config, &exchange.kind);
    let mut request = JobRequestV1::new(
        JobId::new(&exchange.nucleus_job_id),
        if exchange.kind == "diagnosis" {
            "EMT incident diagnosis"
        } else {
            "EMT email intervention"
        },
        Requester {
            program: "emt".into(),
            id: exchange.id.clone(),
        },
        instructions,
        prompt,
        invocation,
    );
    request.developer_instructions = Some(prompts.text("emt.responder.developer-instructions")?);
    request.validate()?;
    let encoded = serde_json::to_string(&request)?;
    store.connection.execute(
        "UPDATE exchanges SET request_json=?2,request_digest=?3,state='running' WHERE id=?1",
        params![exchange.id, encoded, request.request_digest()?],
    )?;
    Ok(encoded)
}

fn responder_invocation(config: &Config, kind: &str) -> AgentInvocationV1 {
    let mut invocation = AgentInvocationV1::new(
        "codex",
        ModelId::new(&config.model),
        AbsolutePath::new(&config.agent_cwd),
        WorkspaceAccess::Unrestricted,
        BuiltinToolsV1 {
            local_execution: true,
            web_search: false,
        },
        TimeoutSeconds::new(if kind == "diagnosis" { 300 } else { 900 }),
    );
    invocation.reasoning_effort = Some(ReasoningEffort::Medium);
    invocation
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn verify(exchange: &Exchange, job: &JobV1) -> Result<()> {
    if job.request.id.as_str() != exchange.nucleus_job_id
        || job.summary.id != job.request.id
        || job.request.requester.program != "emt"
        || job.request.requester.id != exchange.id
        || job.summary.requester != job.request.requester
        || exchange.request_digest.as_deref() != Some(job.request.request_digest()?.as_str())
        || job.summary.request_digest != job.request.request_digest()?
    {
        return Err(fail("Nucleus job does not match this EMT exchange"));
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
pub async fn advance(store: &Store, exchange: &Exchange, quota_blocked: bool) -> Result<Progress> {
    let client = NucleusClient::for_current_user()?;
    let id = JobId::new(&exchange.nucleus_job_id);
    let job = match timeout(HTTP_TIMEOUT, client.get_job(&id)).await? {
        Ok(job) => Some(job),
        Err(ClientError::Api { status: 404, .. }) => None,
        Err(_) => return Err(fail("Nucleus job observation is unavailable")),
    };
    if let Some(job) = job {
        verify(exchange, &job)?;
        if !exchange.job_admitted {
            store.connection.execute(
                "UPDATE exchanges SET job_admitted=1,request_json=NULL WHERE id=?1",
                [&exchange.id],
            )?;
        }
        if !job.summary.state.is_terminal() {
            if crate::now() >= exchange.deadline_at {
                if job.summary.state == JobState::Accepted
                    && job
                        .quota
                        .as_ref()
                        .is_some_and(nucleus_core::QuotaStatusV1::is_blocked)
                {
                    store.connection.execute(
                        "UPDATE exchanges SET error='quota_deferred_expired' WHERE id=?1",
                        [&exchange.id],
                    )?;
                }
                timeout(HTTP_TIMEOUT, client.cancel_job(&id)).await??;
            }
            return Ok(Progress::Waiting);
        }
        let quota_expired: bool = store.connection.query_row(
            "SELECT COALESCE(error='quota_deferred_expired',0) FROM exchanges WHERE id=?1",
            [&exchange.id],
            |row| row.get(0),
        )?;
        if quota_expired && job.summary.state == JobState::Cancelled {
            return Ok(Progress::QuotaExpired);
        }
        return Ok(
            if job.attempts.last().is_some_and(|attempt| {
                attempt.terminal_reason == Some(nucleus_core::AttemptTerminalReason::QuotaExhausted)
            }) {
                Progress::QuotaExhausted
            } else if job.summary.state == JobState::Completed {
                Progress::Complete
            } else {
                Progress::Failed
            },
        );
    }
    if !exchange.job_admitted && quota_blocked && crate::now() >= exchange.deadline_at {
        return Ok(Progress::QuotaExpired);
    }
    if exchange.job_admitted || crate::now() >= exchange.deadline_at {
        return Ok(Progress::Failed);
    }
    let request: JobRequestV1 = serde_json::from_str(
        exchange
            .request_json
            .as_deref()
            .ok_or_else(|| fail("pending request is absent"))?,
    )?;
    if request.id != id
        || request.request_digest()?.as_str() != exchange.request_digest.as_deref().unwrap_or("")
    {
        return Err(fail("pending request identity mismatch"));
    }
    let health = timeout(HTTP_TIMEOUT, client.health_for_work()).await??;
    let required = [
        HarnessCapability::WorkspaceUnrestricted,
        HarnessCapability::BuiltinLocalExecution,
        HarnessCapability::BuiltinWebSearch,
        HarnessCapability::ExactModel,
        HarnessCapability::ReasoningEffort,
        HarnessCapability::DeveloperInstructions,
    ];
    if health.version != 1
        || health.status != "ok"
        || !health.accepting_jobs
        || !health.authentication.configured
        || !health.authentication.authenticated
        || !health.supported_protocol_versions.contains(&1)
        || !health.execution.is_some_and(|capacity| {
            capacity.max_active_jobs == 8
                && capacity.active_jobs <= 8
                && capacity.available_slots == 8 - capacity.active_jobs
        })
        || required
            .iter()
            .any(|capability| !health.capabilities.contains(capability))
    {
        return Err(fail("Nucleus is not ready for EMT local execution"));
    }
    let receipt = match timeout(HTTP_TIMEOUT, client.submit_job(&request)).await? {
        Ok(receipt) => receipt,
        Err(error @ ClientError::QuotaDeferred(_)) => return Err(error.into()),
        Err(ClientError::Validation(_)) => return Ok(Progress::Failed),
        Err(ClientError::Api { status, .. })
            if (400..500).contains(&status) && !matches!(status, 408 | 425 | 429) =>
        {
            return Ok(Progress::Failed);
        }
        Err(_) => {
            return Err(fail(
                "Nucleus admission is unresolved; preserve the same exchange",
            ));
        }
    };
    if receipt.version != 1
        || receipt.job_id != id
        || receipt.request_digest != request.request_digest()?
    {
        return Err(fail("Nucleus admission identity mismatch"));
    }
    store.connection.execute(
        "UPDATE exchanges SET job_admitted=1,request_json=NULL WHERE id=?1",
        [&exchange.id],
    )?;
    Ok(Progress::Waiting)
}

#[cfg(test)]
mod tests {
    use super::{Config, WorkspaceAccess, responder_invocation};

    #[test]
    fn both_responder_assignments_request_unrestricted_local_execution() {
        let config = Config {
            receiving_domain: "example.test".into(),
            email_executable: "/tmp/email".into(),
            clockwork_executable: "/tmp/clockwork".into(),
            cell_root: "/tmp/cell".into(),
            agent_cwd: "/tmp/agent".into(),
            model: "example-model".into(),
            paused: true,
            poll_after: None,
        };
        for (kind, seconds) in [("diagnosis", 300), ("reply", 900)] {
            let invocation = responder_invocation(&config, kind);
            assert_eq!(invocation.version, nucleus_core::INVOCATION_VERSION_V2);
            assert_eq!(invocation.workspace_access, WorkspaceAccess::Unrestricted);
            assert!(invocation.builtin_tools.local_execution);
            assert!(!invocation.builtin_tools.web_search);
            assert_eq!(invocation.cwd.as_path(), config.agent_cwd);
            assert_eq!(invocation.timeout_seconds.get(), seconds);
            invocation
                .validate()
                .unwrap_or_else(|error| panic!("{error}"));
        }
    }
}
