use std::fs;
use std::io::Write as _;
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use serde_json::json;
use sha2::{Digest as _, Sha256};
use uuid::Uuid;

use crate::error::{AppError, AppResult, Context as _};
use crate::model::DecisionAccount;

pub(crate) const ACCOUNT_SCHEMA_VERSION: i64 = 1;
pub(crate) const CAPTURE_RULE_VERSION: &str = "krisis/decision-account-classification/1";
const MAX_ACCOUNT_BYTES: usize = 1_048_576;

#[derive(Debug, Clone)]
pub(crate) struct PendingAccount {
    pub(crate) account_id: String,
    pub(crate) markdown: String,
    pub(crate) source_sha256: String,
    pub(crate) target_library_id: String,
    pub(crate) target_config_path: String,
}

#[derive(Debug, Clone)]
pub(crate) struct AnnalsConfig {
    pub(crate) binary: PathBuf,
    pub(crate) config: PathBuf,
    pub(crate) expected_library_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AnnalsReceipt {
    pub(crate) contract_version: i64,
    pub(crate) library_id: String,
    pub(crate) producer: String,
    #[serde(rename = "key")]
    pub(crate) producer_key: String,
    pub(crate) source_sha256: String,
    pub(crate) job_id: String,
    pub(crate) accepted_at: String,
    pub(crate) acceptance: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnnalsEnvelope<T> {
    ok: bool,
    data: T,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnnalsWatermark {
    contract_version: i64,
    library_id: String,
    watermark: String,
}

pub(crate) fn render(account: &DecisionAccount) -> AppResult<String> {
    let source = json!({
        "schema_version": ACCOUNT_SCHEMA_VERSION,
        "decision_id": account.id,
        "occurred_at": account.occurred_at,
        "occurred_at_precision": account.precision.as_str(),
        "capture_rule_version": CAPTURE_RULE_VERSION,
        "authority": {
            "host_id": account.authority.host_id,
            "thread_id": account.authority.thread_id,
            "turn_id": account.authority.turn_id,
            "item_id": account.authority.item_id,
            "span": {
                "start": account.authority_start,
                "end": account.authority_end
            }
        }
    });
    let source = serde_json::to_string_pretty(&source)
        .context("account_render_failed", "unable to render decision source")?;
    let markdown = format!(
        "# Decision\n\n{}\n\n## Authority\n\n> {}\n\n## Context\n\n{}\n\n## Action\n\n{}\n\n## Result\n\n{}\n\n## Source\n\n```json\n{}\n```\n",
        account.statement,
        account.authority_quote,
        account.context.as_deref().unwrap_or("Unknown."),
        account.action.as_deref().unwrap_or("Unknown."),
        account.result.as_deref().unwrap_or("Unknown."),
        source
    );
    if markdown.len() > MAX_ACCOUNT_BYTES {
        return Err(AppError::new(
            "account_too_large",
            "rendered decision account exceeds one MiB",
        ));
    }
    Ok(markdown)
}

pub(crate) fn sha256(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

pub(crate) fn accept(
    delivery: &PendingAccount,
    configuration: &AnnalsConfig,
    state_directory: &Path,
) -> AppResult<AnnalsReceipt> {
    validate_configuration(configuration)?;
    let config_path = config_path(configuration)?;
    if delivery.target_library_id != configuration.expected_library_id
        || delivery.target_config_path != config_path
    {
        return Err(AppError::new(
            "annals_target_conflict",
            "pending decision account is bound to a different Annals target",
        ));
    }
    if sha256(&delivery.markdown) != delivery.source_sha256 {
        return Err(AppError::new(
            "account_outbox_conflict",
            "pending decision-account bytes do not match their durable digest",
        ));
    }
    let temporary_path = prepare_handoff(delivery, state_directory)?;
    (|| {
        let output = Command::new(&configuration.binary)
            .arg("--config")
            .arg(&configuration.config)
            .arg("--json")
            .arg("inbox")
            .arg("accept")
            .arg("--producer")
            .arg("krisis")
            .arg("--key")
            .arg(&delivery.account_id)
            .arg(&temporary_path)
            .output()
            .context(
                "annals_delivery_failed",
                "unable to invoke Annals account acceptance",
            )?;
        if !output.status.success() {
            return Err(AppError::new(
                "annals_acceptance_rejected",
                "Annals did not accept the pending decision account",
            ));
        }
        let envelope: AnnalsEnvelope<AnnalsReceipt> = serde_json::from_slice(&output.stdout)
            .context(
                "annals_receipt_invalid",
                "Annals returned an incompatible acceptance receipt",
            )?;
        if !envelope.ok {
            return Err(AppError::new(
                "annals_receipt_invalid",
                "Annals returned a non-success acceptance envelope",
            ));
        }
        let receipt = envelope.data;
        validate_receipt(&receipt, delivery, configuration)?;
        cleanup_handoffs(delivery, state_directory)?;
        Ok(receipt)
    })()
}

fn prepare_handoff(delivery: &PendingAccount, state_directory: &Path) -> AppResult<PathBuf> {
    if let Some(path) = matching_handoffs(delivery, state_directory)?
        .into_iter()
        .next()
    {
        return Ok(path);
    }
    let prefix = handoff_prefix(delivery)?;
    let path = state_directory.join(format!("{prefix}{}.md", Uuid::now_v7()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .context(
            "annals_delivery_failed",
            "unable to create a private Annals handoff file",
        )?;
    file.write_all(delivery.markdown.as_bytes())
        .and_then(|()| file.sync_all())
        .context(
            "annals_delivery_failed",
            "unable to publish a complete Annals handoff file",
        )?;
    Ok(path)
}

fn cleanup_handoffs(delivery: &PendingAccount, state_directory: &Path) -> AppResult<()> {
    for path in matching_handoffs(delivery, state_directory)? {
        fs::remove_file(&path).context(
            "annals_handoff_cleanup_failed",
            "unable to remove an accepted Annals handoff file",
        )?;
    }
    fs::File::open(state_directory)
        .and_then(|directory| directory.sync_all())
        .context(
            "annals_handoff_cleanup_failed",
            "unable to durably clean accepted Annals handoff files",
        )
}

fn matching_handoffs(delivery: &PendingAccount, state_directory: &Path) -> AppResult<Vec<PathBuf>> {
    let prefix = handoff_prefix(delivery)?;
    let directory_metadata = fs::symlink_metadata(state_directory).context(
        "annals_handoff_unsafe",
        "unable to inspect the Krisis state directory",
    )?;
    if directory_metadata.file_type().is_symlink() || !directory_metadata.is_dir() {
        return Err(AppError::new(
            "annals_handoff_unsafe",
            "Krisis state directory is not a regular directory",
        ));
    }
    let mut paths = Vec::new();
    for entry in fs::read_dir(state_directory).context(
        "annals_handoff_unsafe",
        "unable to inspect stale Annals handoff files",
    )? {
        let entry = entry.context(
            "annals_handoff_unsafe",
            "unable to inspect a stale Annals handoff entry",
        )?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some(uuid) = name
            .strip_prefix(&prefix)
            .and_then(|suffix| suffix.strip_suffix(".md"))
        else {
            continue;
        };
        if Uuid::parse_str(uuid).is_err() {
            continue;
        }
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).context(
            "annals_handoff_unsafe",
            "unable to inspect an owned Annals handoff file",
        )?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.uid() != directory_metadata.uid()
            || metadata.nlink() != 1
            || metadata.permissions().mode() & 0o777 != 0o600
            || metadata.len() > MAX_ACCOUNT_BYTES as u64
            || fs::read(&path).map_or(true, |bytes| bytes != delivery.markdown.as_bytes())
        {
            return Err(AppError::new(
                "annals_handoff_unsafe",
                "matching Annals handoff path is not an exact private Krisis file",
            ));
        }
        paths.push(path);
    }
    paths.sort();
    Ok(paths)
}

fn handoff_prefix(delivery: &PendingAccount) -> AppResult<String> {
    if delivery.account_id.is_empty()
        || !delivery
            .account_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        || delivery.source_sha256.len() != 64
        || !delivery
            .source_sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(AppError::new(
            "account_outbox_conflict",
            "pending decision account has an unsafe handoff identity",
        ));
    }
    Ok(format!(
        ".krisis-annals-handoff-v1-{}-{}-",
        delivery.account_id, delivery.source_sha256
    ))
}

pub(crate) fn doctor(configuration: &AnnalsConfig) -> AppResult<()> {
    validate_configuration(configuration)?;
    let output = Command::new(&configuration.binary)
        .arg("--config")
        .arg(&configuration.config)
        .arg("--json")
        .arg("decision-feed")
        .arg("watermark")
        .output()
        .context(
            "annals_doctor_failed",
            "unable to invoke Annals doctor for the decisions library",
        )?;
    if !output.status.success() {
        return Err(AppError::new(
            "annals_not_ready",
            "Annals doctor did not accept the configured decisions library",
        ));
    }
    let envelope: AnnalsEnvelope<AnnalsWatermark> = serde_json::from_slice(&output.stdout)
        .context(
            "annals_receipt_invalid",
            "Annals returned an incompatible decisions-library watermark",
        )?;
    if !envelope.ok
        || envelope.data.contract_version != 1
        || envelope.data.library_id != configuration.expected_library_id
        || envelope.data.watermark.trim().is_empty()
    {
        return Err(AppError::new(
            "annals_not_ready",
            "Annals watermark does not match the configured decisions library",
        ));
    }
    Ok(())
}

pub(crate) fn config_path(configuration: &AnnalsConfig) -> AppResult<&str> {
    configuration.config.to_str().ok_or_else(|| {
        AppError::new(
            "annals_configuration_invalid",
            "Annals config path must be valid UTF-8 for durable target binding",
        )
    })
}

fn validate_configuration(configuration: &AnnalsConfig) -> AppResult<()> {
    if !configuration.binary.is_absolute()
        || !configuration.config.is_absolute()
        || configuration.expected_library_id.len() != 32
        || !configuration
            .expected_library_id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(AppError::new(
            "annals_configuration_invalid",
            "Annals binary, config, and 32-hex expected library ID must be explicit",
        ));
    }
    Ok(())
}

fn validate_receipt(
    receipt: &AnnalsReceipt,
    delivery: &PendingAccount,
    configuration: &AnnalsConfig,
) -> AppResult<()> {
    if receipt.contract_version != 1
        || receipt.library_id != configuration.expected_library_id
        || receipt.producer != "krisis"
        || receipt.producer_key != delivery.account_id
        || receipt.source_sha256 != delivery.source_sha256
        || receipt.job_id.trim().is_empty()
        || receipt.accepted_at.trim().is_empty()
        || !matches!(receipt.acceptance.as_str(), "created" | "replayed")
    {
        return Err(AppError::new(
            "annals_receipt_invalid",
            "Annals acceptance receipt does not match the pending decision account",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::PathBuf;

    use super::{
        AnnalsConfig, AnnalsEnvelope, AnnalsReceipt, CAPTURE_RULE_VERSION, PendingAccount, accept,
        cleanup_handoffs, render, sha256, validate_receipt,
    };
    use crate::error::{AppResult, Context as _};
    use crate::model::{AccountSource, DecisionAccount, MessageRole, Precision};

    fn account() -> DecisionAccount {
        DecisionAccount {
            id: "d_0123456789abcdef0123".to_owned(),
            occurred_at: 1_700_000_000,
            precision: Precision::Item,
            statement: "Use the scoped library.".to_owned(),
            authority_quote: "use the scoped library".to_owned(),
            context: Some("Decision retention needed a separate boundary.".to_owned()),
            action: None,
            result: None,
            authority_start: 4,
            authority_end: 26,
            authority: AccountSource {
                host_id: "host".to_owned(),
                thread_id: "thread".to_owned(),
                turn_id: "turn".to_owned(),
                item_id: "item".to_owned(),
                role: MessageRole::User,
                occurred_at: 1_700_000_000,
                precision: Precision::Item,
            },
            context_sources: Vec::new(),
            action_sources: Vec::new(),
            result_sources: Vec::new(),
        }
    }

    #[test]
    fn account_markdown_is_deterministic_and_complete() {
        let account = account();
        let first = render(&account).unwrap_or_else(|error| panic!("{error}"));
        let second = render(&account).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(first, second);
        assert!(first.starts_with("# Decision\n"));
        assert!(first.contains("## Authority\n\n> use the scoped library"));
        assert!(first.contains("## Action\n\nUnknown."));
        assert!(first.contains(CAPTURE_RULE_VERSION));
        assert_eq!(sha256(&first).len(), 64);
    }

    #[test]
    fn annals_acceptance_fixture_is_strict_and_matches_delivery() {
        let raw = r#"{"ok":true,"data":{"contract_version":1,"library_id":"0123456789abcdef0123456789abcdef","producer":"krisis","key":"d_0123456789abcdef0123","source_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","job_id":"job-1","accepted_at":"2026-09-03T12:00:00Z","acceptance":"replayed"}}"#;
        let envelope: AnnalsEnvelope<AnnalsReceipt> =
            serde_json::from_str(raw).unwrap_or_else(|error| panic!("{error}"));
        assert!(envelope.ok);
        let pending = PendingAccount {
            account_id: "d_0123456789abcdef0123".to_owned(),
            markdown: "account".to_owned(),
            source_sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                .to_owned(),
            target_library_id: "0123456789abcdef0123456789abcdef".to_owned(),
            target_config_path: "/tmp/annals-decisions.toml".to_owned(),
        };
        let configuration = AnnalsConfig {
            binary: PathBuf::from("/usr/local/bin/annals"),
            config: PathBuf::from("/tmp/annals-decisions.toml"),
            expected_library_id: "0123456789abcdef0123456789abcdef".to_owned(),
        };
        validate_receipt(&envelope.data, &pending, &configuration)
            .unwrap_or_else(|error| panic!("{error}"));
        let with_extra = raw.replace("\"acceptance\"", "\"unexpected\":1,\"acceptance\"");
        assert!(serde_json::from_str::<AnnalsEnvelope<AnnalsReceipt>>(&with_extra).is_err());
    }

    #[test]
    fn pending_delivery_rejects_a_changed_annals_target_before_invocation() {
        let pending = PendingAccount {
            account_id: "d_0123456789abcdef0123".to_owned(),
            markdown: "account".to_owned(),
            source_sha256: sha256("account"),
            target_library_id: "0123456789abcdef0123456789abcdef".to_owned(),
            target_config_path: "/tmp/annals-decisions.toml".to_owned(),
        };
        let configuration = AnnalsConfig {
            binary: PathBuf::from("/does/not/run/annals"),
            config: PathBuf::from("/tmp/other-annals.toml"),
            expected_library_id: "fedcba9876543210fedcba9876543210".to_owned(),
        };
        let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            accept(&pending, &configuration, directory.path())
                .err()
                .map(|error| error.code),
            Some("annals_target_conflict")
        );
    }

    #[test]
    fn accepted_handoff_cleanup_failure_is_visible() -> AppResult<()> {
        let directory = tempfile::tempdir().context("test_failed", "unable to make temp dir")?;
        let markdown = "# Decision\n";
        let digest = sha256(markdown);
        let pending = PendingAccount {
            account_id: "d_0123456789abcdef0123".to_owned(),
            markdown: markdown.to_owned(),
            source_sha256: digest.clone(),
            target_library_id: "0123456789abcdef0123456789abcdef".to_owned(),
            target_config_path: "/tmp/annals-decisions.toml".to_owned(),
        };
        let handoff = directory.path().join(format!(
            ".krisis-annals-handoff-v1-{}-{}-00000000-0000-7000-8000-000000000001.md",
            pending.account_id, digest
        ));
        fs::write(&handoff, markdown).context("test_failed", "unable to write handoff")?;
        fs::set_permissions(&handoff, fs::Permissions::from_mode(0o600))
            .context("test_failed", "unable to make handoff private")?;
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o500))
            .context("test_failed", "unable to make directory read-only")?;
        let result = cleanup_handoffs(&pending, directory.path());
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))
            .context("test_failed", "unable to restore directory permissions")?;
        assert_eq!(
            result.err().map(|error| error.code),
            Some("annals_handoff_cleanup_failed")
        );
        assert!(handoff.exists());
        Ok(())
    }

    #[test]
    fn annals_cli_interoperability_uses_the_sealed_acceptance_surface() -> AppResult<()> {
        let directory = tempfile::tempdir().context("test_failed", "unable to make temp dir")?;
        let state = directory.path().join("state");
        fs::create_dir(&state).context("test_failed", "unable to make state dir")?;
        let binary = directory.path().join("annals");
        let capture = directory.path().join("arguments");
        let captured_account = directory.path().join("account.md");
        let config = directory.path().join("decisions.toml");
        let markdown = render(&account())?;
        let digest = sha256(&markdown);
        let script = format!(
            "#!/bin/sh\nset -eu\nprintf '%s\\n' \"$@\" >'{}'\ncp \"${{10}}\" '{}'\nprintf '%s\\n' '{{\"ok\":true,\"data\":{{\"contract_version\":1,\"library_id\":\"0123456789abcdef0123456789abcdef\",\"producer\":\"krisis\",\"key\":\"d_0123456789abcdef0123\",\"source_sha256\":\"{}\",\"job_id\":\"annals-job-1\",\"accepted_at\":\"2026-09-03T12:00:00Z\",\"acceptance\":\"created\"}}}}'\n",
            capture.display(),
            captured_account.display(),
            digest
        );
        fs::write(&binary, script).context("test_failed", "unable to write fake Annals")?;
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o700))
            .context("test_failed", "unable to make fake Annals executable")?;
        fs::write(&config, "[decision_feed]\n")
            .context("test_failed", "unable to write fake Annals config")?;

        let pending = PendingAccount {
            account_id: "d_0123456789abcdef0123".to_owned(),
            markdown: markdown.clone(),
            source_sha256: digest,
            target_library_id: "0123456789abcdef0123456789abcdef".to_owned(),
            target_config_path: config.to_string_lossy().into_owned(),
        };
        let stale_handoffs = [
            state.join(format!(
                ".krisis-annals-handoff-v1-{}-{}-00000000-0000-7000-8000-000000000001.md",
                pending.account_id, pending.source_sha256
            )),
            state.join(format!(
                ".krisis-annals-handoff-v1-{}-{}-00000000-0000-7000-8000-000000000002.md",
                pending.account_id, pending.source_sha256
            )),
        ];
        for path in &stale_handoffs {
            fs::write(path, &markdown).context("test_failed", "unable to write stale handoff")?;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))
                .context("test_failed", "unable to make stale handoff private")?;
        }
        let configuration = AnnalsConfig {
            binary,
            config: config.clone(),
            expected_library_id: "0123456789abcdef0123456789abcdef".to_owned(),
        };
        let receipt = accept(&pending, &configuration, &state)?;
        assert_eq!(receipt.acceptance, "created");
        assert!(stale_handoffs.iter().all(|path| !path.exists()));
        assert_eq!(
            fs::read_to_string(&captured_account)
                .context("test_failed", "unable to read captured account")?,
            markdown
        );
        let arguments = fs::read_to_string(capture)
            .context("test_failed", "unable to read captured arguments")?;
        let lines = arguments.lines().collect::<Vec<_>>();
        assert_eq!(
            &lines[..9],
            &[
                "--config",
                config.to_string_lossy().as_ref(),
                "--json",
                "inbox",
                "accept",
                "--producer",
                "krisis",
                "--key",
                "d_0123456789abcdef0123",
            ]
        );
        assert_eq!(lines.len(), 10);
        assert!(
            PathBuf::from(lines[9])
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
        );
        assert!(
            fs::read_dir(state)
                .context("test_failed", "unable to inspect handoff cleanup")?
                .next()
                .is_none()
        );
        Ok(())
    }
}
