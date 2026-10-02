use std::fs;
use std::io::Write as _;
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};
use uuid::Uuid;

use crate::error::{AppError, AppResult, Context as _};
use crate::model::DecisionAccount;

pub(crate) const ACCOUNT_SCHEMA_VERSION: i64 = krisis_api::account::ACCOUNT_SCHEMA_VERSION as i64;
pub(crate) use annals_api::AcceptanceReceipt as AnnalsReceipt;
#[cfg(test)]
use annals_api::SuccessEnvelope as AnnalsEnvelope;
pub(crate) use krisis_api::account::CAPTURE_RULE_VERSION;
use krisis_api::account::{
    Account, AuthorityAnchor, AuthoritySpan, MAX_ACCOUNT_BYTES, SourceMetadata,
};

#[derive(Debug, Clone)]
pub(crate) struct PendingAccount {
    pub(crate) account_id: String,
    pub(crate) markdown: String,
    pub(crate) source_sha256: String,
    pub(crate) target_library_id: String,
    pub(crate) target_config_path: String,
}

pub(crate) use decisions::api::AnnalsTarget as AnnalsConfig;

pub(crate) fn render(account: &DecisionAccount) -> AppResult<String> {
    let exported = Account {
        statement: account.statement.clone(),
        authority_quote: format!("> {}", account.authority_quote),
        context: account
            .context
            .clone()
            .unwrap_or_else(|| "Unknown.".to_owned()),
        action: account
            .action
            .clone()
            .unwrap_or_else(|| "Unknown.".to_owned()),
        result: account
            .result
            .clone()
            .unwrap_or_else(|| "Unknown.".to_owned()),
        source: SourceMetadata {
            schema_version: krisis_api::account::ACCOUNT_SCHEMA_VERSION,
            decision_id: account.id.clone(),
            occurred_at: account.occurred_at,
            occurred_at_precision: account.precision.as_str().to_owned(),
            capture_rule_version: CAPTURE_RULE_VERSION.to_owned(),
            authority: AuthorityAnchor {
                host_id: account.authority.host_id.clone(),
                thread_id: account.authority.thread_id.clone(),
                turn_id: account.authority.turn_id.clone(),
                item_id: account.authority.item_id.clone(),
                span: AuthoritySpan {
                    start: account.authority_start as u64,
                    end: account.authority_end as u64,
                },
            },
        },
    };
    krisis_api::account::render(&exported).map_err(|error| match error {
        krisis_api::account::Error::TooLarge => {
            AppError::new("account_too_large", error.to_string())
        }
        _ => AppError::new("account_render_failed", "unable to render decision source"),
    })
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
        let receipt = annals_api::Client::new(&configuration.binary, &configuration.config)
            .accept(&delivery.account_id, &temporary_path)
            .map_err(|error| {
                let (code, message) = match error.code {
                    "annals_command_unavailable" => (
                        "annals_delivery_failed",
                        "unable to invoke Annals account acceptance",
                    ),
                    "annals_command_failed" => (
                        "annals_acceptance_rejected",
                        "Annals did not accept the pending decision account",
                    ),
                    _ => (
                        "annals_receipt_invalid",
                        "Annals returned an incompatible acceptance receipt",
                    ),
                };
                AppError::new(code, message)
            })?;
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
    let watermark = annals_api::Client::new(&configuration.binary, &configuration.config)
        .watermark()
        .map_err(|error| {
            let (code, message) = match error.code {
                "annals_command_unavailable" => (
                    "annals_doctor_failed",
                    "unable to invoke Annals doctor for the decisions library",
                ),
                "annals_command_failed" => (
                    "annals_not_ready",
                    "Annals doctor did not accept the configured decisions library",
                ),
                _ => (
                    "annals_receipt_invalid",
                    "Annals returned an incompatible decisions-library watermark",
                ),
            };
            AppError::new(code, message)
        })?;
    if watermark.library_id != configuration.expected_library_id {
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
    if receipt.validate().is_err()
        || receipt.library_id != configuration.expected_library_id
        || receipt.producer_key != delivery.account_id
        || receipt.source_sha256 != delivery.source_sha256
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
    use super::{
        AnnalsConfig, AnnalsEnvelope, AnnalsReceipt, CAPTURE_RULE_VERSION, PendingAccount, render,
        sha256, validate_receipt,
    };
    use crate::model::{AccountSource, DecisionAccount, MessageRole, Precision};
    use std::path::PathBuf;

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
        assert_eq!(
            first,
            include_str!("../../krisis-api/tests/fixtures/account-v1.md")
        );
        assert!(first.starts_with("# Decision\n"));
        assert!(first.contains("## Authority\n\n> use the scoped library"));
        assert!(first.contains("## Action\n\nUnknown."));
        assert!(first.contains(CAPTURE_RULE_VERSION));
        assert_eq!(sha256(&first).len(), 64);
    }

    #[test]
    fn annals_acceptance_fixture_is_strict_and_matches_delivery() {
        let raw = r#"{"ok":true,"data":{"contract_version":2,"library_id":"0123456789abcdef0123456789abcdef","producer":"krisis","key":"d_0123456789abcdef0123","source_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","job_id":"job-1","accepted_at":"2026-09-03T12:00:00Z","acceptance":"replayed"}}"#;
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
}
