//! Product operations, maintenance, and the explicit Clockwork binding.
use anyhow::{Context, Result, bail, ensure};
use chrono::{Local, TimeZone};
use nucleus_client::NucleusClient;
use rusqlite::params;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

use crate::store::{Store, private_directory, runner_lock};

pub async fn maintenance(root: &Path, operation: &str, owner: Option<&str>) -> Result<Value> {
    let gate = crate::gate(root);
    match operation {
        "hold" => {
            gate.hold(owner.context("hold owner required")?)?;
        }
        "release" => {
            gate.release(owner.context("release owner required")?)?;
        }
        "status" | "drain" => {}
        _ => bail!("unsupported maintenance operation"),
    }
    let status = gate.status()?;
    let client = NucleusClient::for_current_user()?;
    let jobs = tokio::time::timeout(
        Duration::from_secs(30),
        crate::agent::nonterminal_jobs(&client),
    )
    .await;
    let jobs = match jobs {
        Ok(Ok(count)) => Some(count),
        _ => None,
    };
    Ok(
        json!({"maintenance":{"protocol_version":1,"holds":status.holds,"drained":status.drained && jobs==Some(0),"nonterminal_jobs":jobs}}),
    )
}

pub async fn doctor(root: &Path) -> Result<Value> {
    let owner = std::env::var("CELL_DEPLOYMENT_RUN_ID").ok();
    let gate = crate::gate(root);
    let _guard = if let Some(owner) = &owner {
        gate.enter_for(owner)?
    } else {
        gate.enter()?
    };
    let initialized = root.join(crate::store::DATABASE).exists();
    if initialized {
        let store = Store::open(root)?;
        let result: String = store
            .connection
            .query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        ensure!(result == "ok", "Paperboy database integrity check failed");
        store
            .connection
            .prepare("SELECT id,window_start,window_end,subject,body FROM briefs LIMIT 0")?;
        store
            .connection
            .prepare("SELECT request,tool_replies FROM agent_attempts LIMIT 0")?;
        store
            .connection
            .prepare("SELECT outcome,provider_message_id FROM email_attempts LIMIT 0")?;
    }
    let client = NucleusClient::for_current_user()?;
    crate::agent::readiness(&client, owner.as_deref()).await?;
    crate::agent::source_config()?;
    for command in ["email", "clockwork"] {
        let executable = crate::home()?.join(".local/bin").join(command);
        cell_install::command::checked(
            &executable,
            &["--version".into()],
            &BTreeMap::new(),
            Duration::from_secs(20),
        )?;
    }
    Ok(json!({"ready":true,"initialized":initialized,"schema_version":1}))
}

pub fn migrate(root: &Path, backup: &Path) -> Result<Value> {
    let owner =
        std::env::var("CELL_DEPLOYMENT_RUN_ID").context("migration requires a deployment owner")?;
    let _guard = crate::gate(root).enter_for(&owner)?;
    let _lock = runner_lock(root)?;
    let existed = root.join(crate::store::DATABASE).exists();
    let store = Store::initialize(root)?;
    if existed {
        store.backup(backup)?;
    }
    Ok(json!({"schema_version":1,"initialized":!existed,"backup_created":existed}))
}

pub async fn run(root: &Path, ad_hoc: bool, selected: Option<&str>, retry: bool) -> Result<Value> {
    let _admission = if selected.is_some() {
        crate::gate(root).recover()?
    } else {
        crate::gate(root).enter()?
    };
    let _lock = runner_lock(root)?;
    let mut store = Store::open(root)?;
    let brief = if let Some(id) = selected {
        store.brief(id)?
    } else {
        let now = Local::now();
        let end = if ad_hoc {
            now.timestamp()
        } else {
            let today = now
                .date_naive()
                .and_hms_opt(9, 0, 0)
                .context("invalid local morning")?;
            let today = Local
                .from_local_datetime(&today)
                .single()
                .context("ambiguous local morning")?;
            if now < today {
                let previous = now
                    .date_naive()
                    .pred_opt()
                    .context("previous date absent")?
                    .and_hms_opt(9, 0, 0)
                    .context("invalid previous morning")?;
                Local
                    .from_local_datetime(&previous)
                    .single()
                    .context("ambiguous previous morning")?
                    .timestamp()
            } else {
                today.timestamp()
            }
        };
        let occurrence = if ad_hoc {
            format!("ad-hoc/{}", uuid::Uuid::now_v7())
        } else {
            format!("daily/{end}")
        };
        let timezone = format!("system local time ({})", now.offset());
        store.create(&occurrence, end, &timezone)?
    };
    if let Some(receipt) = store.accepted_receipt(&brief.id)? {
        return Ok(
            json!({"brief_id":brief.id,"outcome":"accepted","provider_message_id":receipt,"already_accepted":true}),
        );
    }
    if brief.body.is_none() {
        crate::agent::summarize(&mut store, &brief, root, retry).await?;
    }
    send(&store, &store.brief(&brief.id)?).await
}

async fn send(store: &Store, brief: &crate::store::Brief) -> Result<Value> {
    let unresolved:i64=store.connection.query_row("SELECT COUNT(*) FROM email_attempts WHERE brief_id=?1 AND outcome IN ('in_progress','uncertain')",[&brief.id],|r|r.get(0))?;
    ensure!(
        unresolved == 0,
        "an earlier email submission is uncertain; reconcile its provider outcome before retrying"
    );
    let subject = brief.subject.as_deref().context("summary subject absent")?;
    let body = brief.body.as_deref().context("summary body absent")?;
    let id = uuid::Uuid::now_v7().to_string();
    store.connection.execute(
        "INSERT INTO email_attempts(id,brief_id,started_at,outcome) VALUES(?1,?2,?3,'in_progress')",
        params![id, brief.id, crate::now()],
    )?;
    let home = crate::home()?;
    let spawn = tokio::process::Command::new(home.join(".local/bin/email"))
        .arg("--idempotency-key")
        .arg(&brief.email_key)
        .arg(subject)
        .arg("-")
        .env_clear()
        .env("HOME", home)
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn();
    let mut child = match spawn {
        Ok(child) => child,
        Err(error) => {
            store.connection.execute("UPDATE email_attempts SET outcome='failed',finished_at=?2,failure='Email process could not start' WHERE id=?1",params![id,crate::now()])?;
            return Err(error).context("Email process could not start");
        }
    };
    let operation = async {
        let mut stdin = child
            .stdin
            .take()
            .context("Email standard input unavailable")?;
        stdin.write_all(body.as_bytes()).await?;
        stdin.shutdown().await?;
        drop(stdin);
        Ok::<_, anyhow::Error>(child.wait_with_output().await?)
    };
    let output = tokio::time::timeout(Duration::from_secs(180), operation).await;
    match output {
        Ok(Ok(output)) if output.status.success() => {
            let response = String::from_utf8(output.stdout)?;
            let receipt=response.trim().strip_prefix("Accepted ").filter(|s|!s.is_empty() && !s.contains(char::is_whitespace)).context("Email success did not contain an acceptance receipt; outcome remains uncertain")?;
            store.connection.execute("UPDATE email_attempts SET outcome='accepted',finished_at=?2,provider_message_id=?3 WHERE id=?1",params![id,crate::now(),receipt])?;
            Ok(
                json!({"brief_id":brief.id,"email_attempt_id":id,"outcome":"accepted","provider_message_id":receipt}),
            )
        }
        _ => {
            store.connection.execute("UPDATE email_attempts SET outcome='uncertain',finished_at=?2,failure='Email invocation did not establish acceptance; inspect provider before retry' WHERE id=?1",params![id,crate::now()])?;
            bail!("Email submission did not establish acceptance; attempt {id} is uncertain")
        }
    }
}

pub fn reconcile(
    root: &Path,
    attempt: &str,
    receipt: Option<&str>,
    not_accepted: bool,
) -> Result<Value> {
    ensure!(
        receipt.is_some() != not_accepted,
        "choose an observed receipt or confirmed not accepted"
    );
    let _guard = crate::gate(root).enter()?;
    let _lock = runner_lock(root)?;
    let store = Store::open(root)?;
    if let Some(receipt) = receipt {
        ensure!(
            !receipt.trim().is_empty() && !receipt.contains(char::is_whitespace),
            "invalid provider receipt"
        );
    }
    let changed=store.connection.execute("UPDATE email_attempts SET outcome=?2,provider_message_id=?3,finished_at=?4,failure=?5 WHERE id=?1 AND outcome IN ('in_progress','uncertain')",params![attempt,if receipt.is_some(){"accepted"}else{"failed"},receipt,crate::now(),if receipt.is_some(){None}else{Some("Operator confirmed no provider acceptance")}])?;
    ensure!(changed == 1, "attempt is absent or already resolved");
    Ok(json!({"email_attempt_id":attempt,"outcome":if receipt.is_some(){"accepted"}else{"failed"}}))
}

pub fn schedule(root: &Path, operation: &str) -> Result<Value> {
    const KEY: &str = "paperboy/daily";
    let clockwork = clockwork::api::Client::new(crate::home()?.join(".local/bin/clockwork"));
    if operation == "status" {
        return Ok(serde_json::to_value(clockwork.binding(KEY)?)?);
    }
    let _guard = crate::gate(root).enter()?;
    let _lock = runner_lock(root)?;
    if operation == "disable" {
        return Ok(serde_json::to_value(clockwork.disable(KEY, None)?)?);
    }
    ensure!(operation == "enable", "unsupported schedule operation");
    let definition = schedule_definition(root)?;
    let manifest = root.join("daily.toml");
    if manifest.exists() {
        crate::store::regular(&manifest)?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&manifest)?;
    file.write_all(toml::to_string(&definition)?.as_bytes())?;
    file.sync_all()?;
    let registered = clockwork.register(&manifest)?;
    Ok(serde_json::to_value(
        clockwork.switch(KEY, &registered.digest)?,
    )?)
}

/// Prepare the exact selected release without changing schedule activation.
pub fn schedule_definition(root: &Path) -> Result<clockwork::api::Manifest> {
    let executable = fs::canonicalize(crate::home()?.join(".local/bin/paperboy"))?;
    let release = executable
        .parent()
        .and_then(Path::parent)
        .context("installed release missing")?;
    let spec = crate::installation::specification();
    let info = cell_install::verify_release_at(&spec.layout(), release, &|path| spec.legacy(path))?;
    let selected = fs::canonicalize(
        crate::home()?.join("Library/Application Support/Paperboy/install/current"),
    )?;
    ensure!(
        selected == release,
        "schedule enable requires the selected installed Paperboy executable"
    );
    let logs = root.join("logs");
    private_directory(&logs)?;
    let home = crate::home()?;
    let codex = crate::agent::source_config()?.codex_path;
    let definition: clockwork::api::Manifest = serde_json::from_value(
        json!({"schema_version":2,"key":"paperboy/daily","release_id":info.release_id,"release_root":release,"authority":"current-user-background","overlap":"skip","failure":{"on_abend":"halt-until-approved"},"arguments":["run","--scheduled"],"cwd":root,"timeout_seconds":2100,"schedule":{"kind":"local-calendar","hour":9,"minute":0,"run_at_load":false},"launch":{"kind":"direct","program":executable,"sha256":cell_install::file_digest(&executable)?},"environment":{"HOME":home,"PATH":"/usr/bin:/bin:/usr/sbin:/sbin","CONVERSATIONS_CODEX":codex},"output":{"stdout":logs.join("daily.stdout.log"),"stderr":logs.join("daily.stderr.log")}}),
    )?;
    Ok(definition)
}

pub fn show(root: &Path, id: &str) -> Result<Value> {
    let store = Store::open(root)?;
    let brief = store.brief(id)?;
    let mut statement=store.connection.prepare("SELECT id,job_id,created_at,submitted_at,finished_at,outcome,failure FROM agent_attempts WHERE brief_id=?1 ORDER BY created_at,id")?;
    let agent=statement.query_map([id],|r|Ok(json!({"id":r.get::<_,String>(0)?,"job_id":r.get::<_,String>(1)?,"created_at":r.get::<_,i64>(2)?,"submitted_at":r.get::<_,Option<i64>>(3)?,"finished_at":r.get::<_,Option<i64>>(4)?,"outcome":r.get::<_,String>(5)?,"failure":r.get::<_,Option<String>>(6)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut statement=store.connection.prepare("SELECT id,started_at,finished_at,outcome,provider_message_id,failure FROM email_attempts WHERE brief_id=?1 ORDER BY started_at,id")?;
    let emails=statement.query_map([id],|r|Ok(json!({"id":r.get::<_,String>(0)?,"started_at":r.get::<_,i64>(1)?,"finished_at":r.get::<_,Option<i64>>(2)?,"outcome":r.get::<_,String>(3)?,"provider_message_id":r.get::<_,Option<String>>(4)?,"failure":r.get::<_,Option<String>>(5)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(json!({"brief":brief,"agent_attempts":agent,"email_attempts":emails}))
}

pub fn state() -> Result<PathBuf> {
    crate::state_root()
}
