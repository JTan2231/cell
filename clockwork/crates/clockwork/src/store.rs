use std::fs::{self, OpenOptions};
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension as _, TransactionBehavior, params};

use crate::error::{Context as _, Error, Result};
use crate::lock::KeyLock;
use crate::manifest;
use crate::model::{
    ActivationRecord, ActivationState, BindingRecord, DefinitionRecord, DefinitionSummary,
    Manifest, Trigger,
};
use crate::paths::{Layout, current_uid};
use clockwork::api::{AbendPolicy, IncidentRecord};

pub(crate) struct Store {
    pub(crate) connection: Connection,
    default_email_cli: String,
    _schema_gate: KeyLock,
}

impl Store {
    pub(crate) fn open(layout: &Layout) -> Result<Self> {
        layout.prepare()?;
        let schema_gate = KeyLock::acquire_activation_gate(layout, "clockwork/schema")?;
        let database = layout.database();
        prepare_database_file(&database)?;
        let connection = Connection::open(&database).context(
            "database_unavailable",
            format!("open {}", database.display()),
        )?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .context("database_unavailable", "configure SQLite busy timeout")?;
        connection
            .execute_batch(
                "PRAGMA foreign_keys = ON;\n\
                 PRAGMA synchronous = FULL;\n\
                 PRAGMA temp_store = MEMORY;",
            )
            .context("database_unavailable", "configure SQLite")?;
        initialize_or_verify_schema(&connection)?;
        connection
            .execute_batch("PRAGMA journal_mode = WAL;")
            .context(
                "database_unavailable",
                "enable SQLite WAL after schema verification",
            )?;
        fs::set_permissions(&database, fs::Permissions::from_mode(0o600)).context(
            "database_unavailable",
            format!("set private permissions on {}", database.display()),
        )?;
        Ok(Self {
            connection,
            default_email_cli: layout.default_email_cli().to_string_lossy().into_owned(),
            _schema_gate: schema_gate,
        })
    }

    pub(crate) fn register_definition(
        &mut self,
        digest: &str,
        manifest: &Manifest,
    ) -> Result<DefinitionRecord> {
        let computed_digest = manifest::definition_digest(manifest)?;
        if computed_digest != digest {
            return Err(Error::new(
                "definition_digest_invalid",
                "provided definition digest does not match its canonical manifest",
            ));
        }
        let manifest_json =
            serde_json::to_string(manifest).context("manifest_invalid", "serialize definition")?;
        let registered_at = now_unix()?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .context("database_write_failed", "begin definition registration")?;
        transaction
            .execute(
                "INSERT OR IGNORE INTO definitions(digest, key, manifest_json, registered_at) \
                 VALUES (?1, ?2, ?3, ?4)",
                params![digest, manifest.key, manifest_json, registered_at],
            )
            .context("database_write_failed", "register immutable definition")?;
        let stored: (String, i64) = transaction
            .query_row(
                "SELECT manifest_json, registered_at FROM definitions WHERE digest = ?1",
                [digest],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .context("database_read_failed", "read registered definition")?;
        if stored.0 != manifest_json {
            return Err(Error::new(
                "definition_digest_collision",
                "an existing definition has the same digest and different content",
            ));
        }
        let record = DefinitionRecord {
            digest: digest.to_owned(),
            key: manifest.key.clone(),
            registered_at: stored.1,
            manifest: manifest.clone(),
        };
        transaction
            .commit()
            .context("database_write_failed", "commit definition registration")?;
        Ok(record)
    }

    pub(crate) fn definitions(&self) -> Result<Vec<DefinitionSummary>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT digest, key, registered_at FROM definitions \
                 ORDER BY registered_at DESC, digest",
            )
            .context("database_read_failed", "prepare definition listing")?;
        let rows = statement
            .query_map([], |row| {
                Ok(DefinitionSummary {
                    digest: row.get(0)?,
                    key: row.get(1)?,
                    registered_at: row.get(2)?,
                })
            })
            .context("database_read_failed", "list definitions")?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .context("database_read_failed", "decode definitions")
    }

    pub(crate) fn definition(&self, digest: &str) -> Result<DefinitionRecord> {
        let stored = self
            .connection
            .query_row(
                "SELECT digest, key, registered_at, manifest_json \
                 FROM definitions WHERE digest = ?1",
                [digest],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()
            .context("database_read_failed", "read definition")?
            .ok_or_else(|| {
                Error::new(
                    "definition_not_found",
                    format!("definition {digest} is not registered"),
                )
            })?;
        let manifest = serde_json::from_str(&stored.3)
            .context("database_corrupt", "decode stored definition")?;
        let computed_digest = manifest::definition_digest(&manifest).map_err(|error| {
            Error::new(
                "database_corrupt",
                format!("recompute stored definition identity: {error}"),
            )
        })?;
        if manifest.key != stored.1 || computed_digest != stored.0 {
            return Err(Error::new(
                "database_corrupt",
                "stored definition key or digest does not match its canonical manifest",
            ));
        }
        Ok(DefinitionRecord {
            digest: stored.0,
            key: stored.1,
            registered_at: stored.2,
            manifest,
        })
    }

    pub(crate) fn bindings(&self) -> Result<Vec<BindingRecord>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT key, definition_digest, enabled, plist_sha256, updated_at \
                 FROM bindings ORDER BY key",
            )
            .context("database_read_failed", "prepare binding listing")?;
        let rows = statement
            .query_map([], binding_from_row)
            .context("database_read_failed", "list bindings")?;
        let bindings = rows
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("database_read_failed", "decode bindings")?;
        bindings
            .into_iter()
            .map(|binding| self.decorate_binding(binding))
            .collect()
    }

    pub(crate) fn binding(&self, key: &str) -> Result<BindingRecord> {
        self.optional_binding(key)?
            .ok_or_else(|| Error::new("binding_not_found", format!("binding {key} does not exist")))
    }

    pub(crate) fn optional_binding(&self, key: &str) -> Result<Option<BindingRecord>> {
        let binding = self
            .connection
            .query_row(
                "SELECT key, definition_digest, enabled, plist_sha256, updated_at \
                 FROM bindings WHERE key = ?1",
                [key],
                binding_from_row,
            )
            .optional()
            .context("database_read_failed", "read binding")?;
        binding
            .map(|binding| self.decorate_binding(binding))
            .transpose()
    }

    fn decorate_binding(&self, mut binding: BindingRecord) -> Result<BindingRecord> {
        binding.halted_incident = self
            .active_incident(&binding.key)?
            .map(|incident| incident.id);
        binding.failure_policy_active = match binding.definition_digest.as_deref() {
            Some(digest) => self.definition(digest)?.manifest.schema_version >= 2,
            None => false,
        };
        Ok(binding)
    }

    pub(crate) fn switch_binding(
        &mut self,
        key: &str,
        digest: &str,
        plist_sha256: &str,
    ) -> Result<BindingRecord> {
        let definition = self.definition(digest)?;
        if definition.key != key {
            return Err(Error::new(
                "binding_key_mismatch",
                format!(
                    "definition {digest} belongs to {}, not {key}",
                    definition.key
                ),
            ));
        }
        let updated_at = now_unix()?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .context("database_write_failed", "begin binding switch")?;
        transaction
            .execute(
                "INSERT INTO bindings( \
                    key, definition_digest, enabled, plist_sha256, updated_at \
                 ) VALUES (?1, ?2, 1, ?3, ?4) \
                 ON CONFLICT(key) DO UPDATE SET \
                    definition_digest = excluded.definition_digest, \
                    enabled = 1, \
                    plist_sha256 = excluded.plist_sha256, \
                    updated_at = excluded.updated_at",
                params![key, digest, plist_sha256, updated_at],
            )
            .context("database_write_failed", "select definition for binding")?;
        transaction
            .commit()
            .context("database_write_failed", "commit binding switch")?;
        self.binding(key)
    }

    pub(crate) fn disable_binding(
        &mut self,
        key: &str,
        selected_digest: Option<&str>,
    ) -> Result<BindingRecord> {
        let updated_at = now_unix()?;
        self.connection
            .execute(
                "INSERT INTO bindings( \
                    key, definition_digest, enabled, plist_sha256, updated_at \
                 ) VALUES (?1, ?2, 0, NULL, ?3) \
                 ON CONFLICT(key) DO UPDATE SET \
                    definition_digest = CASE \
                        WHEN ?2 IS NULL THEN bindings.definition_digest \
                        ELSE ?2 \
                    END, \
                    enabled = 0, \
                    updated_at = excluded.updated_at",
                params![key, selected_digest, updated_at],
            )
            .context("database_write_failed", "disable binding")?;
        self.binding(key)
    }

    pub(crate) fn clear_plist_identity(&mut self, key: &str) -> Result<BindingRecord> {
        let binding = self.binding(key)?;
        if binding.enabled {
            return Err(Error::new(
                "binding_state_conflict",
                format!("binding {key} is no longer disabled"),
            ));
        }
        let updated_at = now_unix()?;
        let changed = self
            .connection
            .execute(
                "UPDATE bindings SET plist_sha256 = NULL, updated_at = ?2 \
                 WHERE key = ?1 AND enabled = 0",
                params![key, updated_at],
            )
            .context("database_write_failed", "finish binding disable")?;
        if changed != 1 {
            return Err(Error::new(
                "binding_state_conflict",
                format!("binding {key} is no longer disabled"),
            ));
        }
        self.binding(key)
    }

    pub(crate) fn restore_binding(
        &mut self,
        key: &str,
        prior: Option<&BindingRecord>,
    ) -> Result<()> {
        match prior {
            Some(binding) => {
                self.connection
                    .execute(
                        "INSERT INTO bindings( \
                            key, definition_digest, enabled, plist_sha256, updated_at \
                         ) VALUES (?1, ?2, ?3, ?4, ?5) \
                         ON CONFLICT(key) DO UPDATE SET \
                            definition_digest = excluded.definition_digest, \
                            enabled = excluded.enabled, \
                            plist_sha256 = excluded.plist_sha256, \
                            updated_at = excluded.updated_at",
                        params![
                            binding.key,
                            binding.definition_digest,
                            i64::from(binding.enabled),
                            binding.plist_sha256,
                            binding.updated_at
                        ],
                    )
                    .context("database_write_failed", "restore prior binding")?;
            }
            None => {
                self.connection
                    .execute("DELETE FROM bindings WHERE key = ?1", [key])
                    .context("database_write_failed", "remove candidate binding")?;
            }
        }
        Ok(())
    }

    pub(crate) fn selected_definition(&self, key: &str) -> Result<DefinitionRecord> {
        let binding = self.binding(key)?;
        if !binding.enabled {
            return Err(Error::new(
                "binding_disabled",
                format!("binding {key} is disabled"),
            ));
        }
        let digest = binding.definition_digest.ok_or_else(|| {
            Error::new(
                "binding_invalid",
                format!("enabled binding {key} has no definition"),
            )
        })?;
        let definition = self.definition(&digest)?;
        if definition.key != key {
            return Err(Error::new(
                "binding_invalid",
                format!(
                    "enabled binding {key} selects a definition for {}",
                    definition.key
                ),
            ));
        }
        Ok(definition)
    }

    pub(crate) fn has_running_activation(&self, key: &str) -> Result<bool> {
        self.connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM activations WHERE key = ?1 AND state = 'running')",
                [key],
                |row| row.get(0),
            )
            .context("database_read_failed", "check active activation")
    }

    pub(crate) fn begin_activation(
        &mut self,
        key: &str,
        digest: &str,
        trigger: Trigger,
    ) -> Result<ActivationRecord> {
        let id = uuid::Uuid::now_v7().to_string();
        let admitted_at = now_unix()?;
        let changed = self
            .connection
            .execute(
                "INSERT INTO activations( \
                    id, key, definition_digest, trigger, state, admitted_at, broker_pid \
                 ) SELECT ?1, ?2, ?3, ?4, 'running', ?5, ?6 \
                   FROM bindings \
                  WHERE key = ?2 AND enabled = 1 AND definition_digest = ?3 \
                    AND NOT EXISTS (SELECT 1 FROM incidents WHERE key = ?2 AND resumed_at IS NULL)",
                params![
                    id,
                    key,
                    digest,
                    trigger.as_str(),
                    admitted_at,
                    i64::from(std::process::id())
                ],
            )
            .context("database_write_failed", "record activation admission")?;
        if changed != 1 {
            return Err(Error::new(
                "binding_changed",
                format!(
                    "binding {key} was disabled, halted, or changed before activation admission"
                ),
            ));
        }
        Ok(ActivationRecord {
            id,
            key: key.to_owned(),
            definition_digest: digest.to_owned(),
            trigger,
            state: ActivationState::Running,
            admitted_at,
            started_at: None,
            finished_at: None,
            broker_pid: Some(std::process::id()),
            child_pid: None,
            exit_code: None,
            signal: None,
            detail: None,
        })
    }

    pub(crate) fn record_skipped_overlap(
        &mut self,
        key: &str,
        digest: &str,
        trigger: Trigger,
    ) -> Result<ActivationRecord> {
        let id = uuid::Uuid::now_v7().to_string();
        let now = now_unix()?;
        self.connection
            .execute(
                "INSERT INTO activations( \
                    id, key, definition_digest, trigger, state, admitted_at, finished_at, detail \
                 ) VALUES (?1, ?2, ?3, ?4, 'skipped_overlap', ?5, ?5, ?6)",
                params![
                    id,
                    key,
                    digest,
                    trigger.as_str(),
                    now,
                    "another activation owns the key"
                ],
            )
            .context("database_write_failed", "record skipped overlap")?;
        Ok(ActivationRecord {
            id,
            key: key.to_owned(),
            definition_digest: digest.to_owned(),
            trigger,
            state: ActivationState::SkippedOverlap,
            admitted_at: now,
            started_at: None,
            finished_at: Some(now),
            broker_pid: None,
            child_pid: None,
            exit_code: None,
            signal: None,
            detail: Some("another activation owns the key".to_owned()),
        })
    }

    pub(crate) fn mark_started(&mut self, id: &str, child_pid: u32) -> Result<()> {
        let started_at = now_unix()?;
        let changed = self
            .connection
            .execute(
                "UPDATE activations SET started_at = ?2, child_pid = ?3 \
                 WHERE id = ?1 AND state = 'running'",
                params![id, started_at, i64::from(child_pid)],
            )
            .context("database_write_failed", "record child start")?;
        if changed != 1 {
            return Err(Error::new(
                "activation_state_conflict",
                format!("activation {id} is no longer running"),
            ));
        }
        Ok(())
    }

    pub(crate) fn finish_activation(
        &mut self,
        id: &str,
        state: ActivationState,
        exit_code: Option<i32>,
        signal: Option<i32>,
        detail: Option<&str>,
    ) -> Result<ActivationRecord> {
        if state == ActivationState::Running || state == ActivationState::SkippedOverlap {
            return Err(Error::new(
                "activation_state_invalid",
                "finish requires a terminal non-overlap state",
            ));
        }
        let mut activation = self.activation(id)?;
        let finished_at = now_unix()?;
        let definition = self.definition(&activation.definition_digest)?;
        let email_cli = definition
            .manifest
            .failure
            .email_cli
            .clone()
            .unwrap_or_else(|| self.default_email_cli.clone());
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .context("database_write_failed", "begin activation completion")?;
        let changed = transaction
            .execute(
                "UPDATE activations SET \
                    state = ?2, finished_at = ?3, exit_code = ?4, signal = ?5, detail = ?6 \
                 WHERE id = ?1 AND state = 'running'",
                params![id, state.as_str(), finished_at, exit_code, signal, detail],
            )
            .context("database_write_failed", "finish activation")?;
        if changed != 1 {
            return Err(Error::new(
                "activation_state_conflict",
                format!("activation {id} is no longer running"),
            ));
        }
        let abnormal = state != ActivationState::Exited || exit_code != Some(0) || detail.is_some();
        if abnormal && definition.manifest.schema_version >= 2 {
            let code = match (state, exit_code, signal) {
                (ActivationState::Exited, Some(code), _) if code != 0 => format!("exit_{code}"),
                (ActivationState::Signaled, _, Some(number)) => format!("signal_{number}"),
                (ActivationState::Exited, _, _) => "supervision_failed".to_owned(),
                _ => state.as_str().to_owned(),
            };
            record_abend_in(
                &transaction,
                &AbendInput {
                    key: &activation.key,
                    occurrence: &format!("activation/{id}"),
                    code: &code,
                    activation_id: Some(id),
                    definition_digest: Some(&activation.definition_digest),
                    halt: definition.manifest.failure.on_abend == AbendPolicy::HaltUntilApproved,
                    email_cli: &email_cli,
                },
            )?;
        }
        transaction
            .commit()
            .context("database_write_failed", "commit outcome and failure policy")?;
        activation.state = state;
        activation.finished_at = Some(finished_at);
        activation.exit_code = exit_code;
        activation.signal = signal;
        activation.detail = detail.map(ToOwned::to_owned);
        Ok(activation)
    }

    pub(crate) fn activation(&self, id: &str) -> Result<ActivationRecord> {
        self.connection
            .query_row(
                "SELECT id, key, definition_digest, trigger, state, admitted_at, started_at, \
                        finished_at, broker_pid, child_pid, exit_code, signal, detail \
                 FROM activations WHERE id = ?1",
                [id],
                activation_from_row,
            )
            .optional()
            .context("database_read_failed", "read activation")?
            .ok_or_else(|| {
                Error::new(
                    "activation_not_found",
                    format!("activation {id} does not exist"),
                )
            })
    }

    pub(crate) fn history(&self, key: Option<&str>, limit: usize) -> Result<Vec<ActivationRecord>> {
        let limit = i64::try_from(limit)
            .map_err(|_| Error::new("history_limit_invalid", "history limit is too large"))?;
        if let Some(key) = key {
            let mut statement = self
                .connection
                .prepare(
                    "SELECT id, key, definition_digest, trigger, state, admitted_at, started_at, \
                            finished_at, broker_pid, child_pid, exit_code, signal, detail \
                     FROM activations WHERE key = ?1 \
                     ORDER BY admitted_at DESC, id DESC LIMIT ?2",
                )
                .context("database_read_failed", "prepare activation history")?;
            let rows = statement
                .query_map(params![key, limit], activation_from_row)
                .context("database_read_failed", "read activation history")?;
            return rows
                .collect::<std::result::Result<Vec<_>, _>>()
                .context("database_read_failed", "decode activation history");
        }
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, key, definition_digest, trigger, state, admitted_at, started_at, \
                        finished_at, broker_pid, child_pid, exit_code, signal, detail \
                 FROM activations ORDER BY admitted_at DESC, id DESC LIMIT ?1",
            )
            .context("database_read_failed", "prepare activation history")?;
        let rows = statement
            .query_map([limit], activation_from_row)
            .context("database_read_failed", "read activation history")?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .context("database_read_failed", "decode activation history")
    }

    pub(crate) fn recover_stale(&mut self, key: Option<&str>) -> Result<usize> {
        let rows = if let Some(key) = key {
            let mut statement = self
                .connection
                .prepare(
                    "SELECT id, broker_pid, child_pid FROM activations \
                     WHERE state = 'running' AND key = ?1",
                )
                .context("database_read_failed", "prepare stale activation recovery")?;
            let mapped = statement
                .query_map([key], running_from_row)
                .context("database_read_failed", "read active activations")?;
            mapped
                .collect::<std::result::Result<Vec<_>, _>>()
                .context("database_read_failed", "decode active activations")?
        } else {
            let mut statement = self
                .connection
                .prepare(
                    "SELECT id, broker_pid, child_pid FROM activations WHERE state = 'running'",
                )
                .context("database_read_failed", "prepare stale activation recovery")?;
            let mapped = statement
                .query_map([], running_from_row)
                .context("database_read_failed", "read active activations")?;
            mapped
                .collect::<std::result::Result<Vec<_>, _>>()
                .context("database_read_failed", "decode active activations")?
        };

        let mut recovered = 0;
        for (id, broker_pid, child_pid) in rows {
            let broker_absent = broker_pid.is_some_and(process_demonstrably_absent);
            let child_absent = child_pid.is_none_or(process_demonstrably_absent);
            if broker_absent && child_absent {
                self.finish_activation(
                    &id,
                    ActivationState::Lost,
                    None,
                    None,
                    Some("recorded broker and any recorded child are absent"),
                )?;
                recovered += 1;
            }
        }
        Ok(recovered)
    }

    pub(crate) fn require_unhalted(&self, key: &str) -> Result<()> {
        if let Some(incident) = self.active_incident(key)? {
            return Err(Error::new(
                "binding_halted",
                format!(
                    "binding {key} is halted by incident {}; explicit approval is required",
                    incident.id
                ),
            ));
        }
        Ok(())
    }

    pub(crate) fn active_incident(&self, key: &str) -> Result<Option<IncidentRecord>> {
        self.connection
            .query_row(
                &format!("{INCIDENT_SELECT} WHERE key = ?1 AND resumed_at IS NULL"),
                [key],
                incident_from_row,
            )
            .optional()
            .context("database_read_failed", "read active scheduling halt")
    }

    pub(crate) fn incident(&self, id: &str) -> Result<IncidentRecord> {
        self.connection
            .query_row(
                &format!("{INCIDENT_SELECT} WHERE id = ?1"),
                [id],
                incident_from_row,
            )
            .optional()
            .context("database_read_failed", "read failure incident")?
            .ok_or_else(|| Error::new("incident_not_found", "failure incident does not exist"))
    }

    pub(crate) fn incidents(&self, key: Option<&str>, limit: usize) -> Result<Vec<IncidentRecord>> {
        let limit = i64::try_from(limit)
            .map_err(|_| Error::new("limit_invalid", "incident limit is too large"))?;
        let mut query = self.connection.prepare(&format!("{INCIDENT_SELECT} WHERE (?1 IS NULL OR key = ?1) ORDER BY created_at DESC, id DESC LIMIT ?2"))
            .context("database_read_failed", "prepare incident listing")?;
        let rows = query
            .query_map(params![key, limit], incident_from_row)
            .context("database_read_failed", "list failure incidents")?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .context("database_read_failed", "decode failure incidents")
    }

    pub(crate) fn incident_feed(
        &self,
        after: u64,
        limit: usize,
    ) -> Result<clockwork::api::IncidentFeed> {
        if limit == 0 || limit > 1000 {
            return Err(Error::new(
                "limit_invalid",
                "feed limit must be 1 through 1000",
            ));
        }
        let cursor = i64::try_from(after)
            .map_err(|_| Error::new("cursor_invalid", "invalid incident cursor"))?;
        let select = INCIDENT_SELECT.replace(" FROM incidents", ", rowid FROM incidents");
        let mut query = self
            .connection
            .prepare(&format!(
                "{select} WHERE rowid > ?1 ORDER BY rowid LIMIT ?2"
            ))
            .context("database_read_failed", "prepare incident feed")?;
        let rows = query
            .query_map(
                params![cursor, i64::try_from(limit + 1).unwrap_or(1001)],
                |row| Ok((incident_from_row(row)?, row.get::<_, i64>(12)?)),
            )
            .context("database_read_failed", "read incident feed")?;
        let mut records = rows
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("database_read_failed", "decode incident feed")?;
        let has_more = records.len() > limit;
        records.truncate(limit);
        Ok(clockwork::api::IncidentFeed {
            next_cursor: records.last().map_or(after, |row| row.1.cast_unsigned()),
            items: records.into_iter().map(|row| row.0).collect(),
            has_more,
        })
    }

    pub(crate) fn report_abend(
        &mut self,
        activation_id: &str,
        code: &str,
        occurrence: &str,
    ) -> Result<Option<IncidentRecord>> {
        validate_failure_metadata(code, occurrence)?;
        let activation = self.activation(activation_id)?;
        if activation.state != ActivationState::Running {
            return Err(Error::new(
                "activation_state_conflict",
                "product reports require the current running activation",
            ));
        }
        let definition = self.definition(&activation.definition_digest)?;
        if definition.manifest.schema_version < 2 {
            return Err(Error::new(
                "failure_policy_unavailable",
                "product reports require a schema-two definition",
            ));
        }
        let email_cli = definition
            .manifest
            .failure
            .email_cli
            .clone()
            .unwrap_or_else(|| self.default_email_cli.clone());
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .context("database_write_failed", "begin product abend report")?;
        let still_running: bool = transaction
            .query_row(
                "SELECT state = 'running' FROM activations WHERE id = ?1",
                [activation_id],
                |row| row.get(0),
            )
            .context("database_read_failed", "check reporting activation")?;
        if !still_running {
            return Err(Error::new(
                "activation_state_conflict",
                "reporting activation is no longer running",
            ));
        }
        let incident_id = record_abend_in(
            &transaction,
            &AbendInput {
                key: &activation.key,
                occurrence,
                code,
                activation_id: Some(activation_id),
                definition_digest: Some(&activation.definition_digest),
                halt: definition.manifest.failure.on_abend == AbendPolicy::HaltUntilApproved,
                email_cli: &email_cli,
            },
        )?;
        transaction.commit().context(
            "database_write_failed",
            "commit product abend and scheduling policy",
        )?;
        incident_id.map(|id| self.incident(&id)).transpose()
    }

    pub(crate) fn import_halt(
        &mut self,
        key: &str,
        code: &str,
        occurrence: &str,
    ) -> Result<IncidentRecord> {
        manifest::validate_key(key)?;
        validate_failure_metadata(code, occurrence)?;
        if self.optional_binding(key)?.is_none() {
            self.disable_binding(key, None)?;
        }
        let binding = self.binding(key)?;
        let definition = binding
            .definition_digest
            .as_deref()
            .map(|digest| self.definition(digest))
            .transpose()?;
        let email_cli = definition
            .as_ref()
            .and_then(|d| d.manifest.failure.email_cli.clone())
            .unwrap_or_else(|| self.default_email_cli.clone());
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .context("database_write_failed", "begin existing halt import")?;
        let incident_id = record_abend_in(
            &transaction,
            &AbendInput {
                key,
                occurrence,
                code,
                activation_id: None,
                definition_digest: binding.definition_digest.as_deref(),
                halt: true,
                email_cli: &email_cli,
            },
        )?;
        transaction
            .commit()
            .context("database_write_failed", "commit existing halt import")?;
        let id = incident_id.ok_or_else(|| {
            Error::new(
                "failure_occurrence_conflict",
                "this occurrence was previously recorded without a scheduling halt",
            )
        })?;
        self.incident(&id)
    }

    pub(crate) fn resume(&mut self, key: &str, incident_id: &str) -> Result<BindingRecord> {
        let incident = self.incident(incident_id)?;
        if incident.key != key || incident.resumed_at.is_some() {
            return Err(Error::new(
                "incident_changed",
                "approval must identify this binding's current open incident",
            ));
        }
        if self.has_running_activation(key)? {
            return Err(Error::new(
                "activation_busy",
                "finish or recover the current activation before approving continuation",
            ));
        }
        let changed = self.connection.execute("UPDATE incidents SET resumed_at = ?3 WHERE key = ?1 AND id = ?2 AND resumed_at IS NULL",
            params![key, incident_id, now_unix()?]).context("database_write_failed", "record explicit continuation approval")?;
        if changed != 1 {
            return Err(Error::new(
                "incident_changed",
                "incident changed before approval",
            ));
        }
        self.binding(key)
    }

    pub(crate) fn quick_check(&self) -> Result<String> {
        let result: String = self
            .connection
            .query_row("PRAGMA quick_check", [], |row| row.get(0))
            .context("database_check_failed", "run SQLite quick_check")?;
        if result != "ok" {
            return Err(Error::new(
                "database_check_failed",
                format!("SQLite quick_check returned {result}"),
            ));
        }
        Ok(result)
    }
}

const INCIDENT_SELECT: &str = "SELECT id, key, activation_id, definition_digest, code, occurrence, created_at, resumed_at, notification_status, first_attempt_at, last_attempt_at, notification_attempts FROM incidents";

fn incident_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<IncidentRecord> {
    Ok(IncidentRecord {
        id: row.get(0)?,
        key: row.get(1)?,
        activation_id: row.get(2)?,
        definition_digest: row.get(3)?,
        code: row.get(4)?,
        occurrence: row.get(5)?,
        created_at: row.get(6)?,
        resumed_at: row.get(7)?,
        notification_status: row.get(8)?,
        first_attempt_at: row.get(9)?,
        last_attempt_at: row.get(10)?,
        notification_attempts: row.get(11)?,
    })
}

fn validate_failure_metadata(code: &str, occurrence: &str) -> Result<()> {
    for (name, value, maximum) in [("code", code, 64), ("occurrence", occurrence, 256)] {
        if value.is_empty()
            || value.len() > maximum
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_/.:".contains(&byte))
        {
            return Err(Error::new(
                "failure_metadata_invalid",
                format!(
                    "{name} must be a bounded machine identifier containing letters, digits, dash, underscore, slash, dot, or colon"
                ),
            ));
        }
    }
    Ok(())
}

struct AbendInput<'a> {
    key: &'a str,
    occurrence: &'a str,
    code: &'a str,
    activation_id: Option<&'a str>,
    definition_digest: Option<&'a str>,
    halt: bool,
    email_cli: &'a str,
}

fn record_abend_in(connection: &Connection, input: &AbendInput<'_>) -> Result<Option<String>> {
    let prior: Option<(Option<String>, String)> = connection
        .query_row(
            "SELECT incident_id, code FROM abends WHERE key = ?1 AND occurrence = ?2",
            params![input.key, input.occurrence],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .context("database_read_failed", "check reported failure occurrence")?;
    if let Some((incident, code)) = prior {
        if code != input.code {
            return Err(Error::new(
                "failure_occurrence_conflict",
                "failure occurrence already identifies a different code",
            ));
        }
        return Ok(incident);
    }
    let now = now_unix()?;
    let incident_id = if input.halt {
        let existing: Option<String> = connection
            .query_row(
                "SELECT id FROM incidents WHERE key = ?1 AND resumed_at IS NULL",
                [input.key],
                |row| row.get(0),
            )
            .optional()
            .context("database_read_failed", "check scheduling halt")?;
        if let Some(id) = existing {
            Some(id)
        } else {
            let id = uuid::Uuid::now_v7().to_string();
            connection.execute("INSERT INTO incidents(id,key,activation_id,definition_digest,code,occurrence,created_at,email_cli) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                params![id,input.key,input.activation_id,input.definition_digest,input.code,input.occurrence,now,input.email_cli])
                .context("database_write_failed", "persist scheduling halt and pending alert")?;
            Some(id)
        }
    } else {
        None
    };
    connection.execute("INSERT INTO abends(key,occurrence,code,activation_id,incident_id,recorded_at) VALUES (?1,?2,?3,?4,?5,?6)",
        params![input.key,input.occurrence,input.code,input.activation_id,incident_id,now])
        .context("database_write_failed", "retain product or runtime abend")?;
    Ok(incident_id)
}

/// Migration is separate from opening state and from program deployment.
#[allow(clippy::too_many_lines)]
pub(crate) fn migrate(layout: &Layout, backup: &Path) -> Result<()> {
    layout.prepare()?;
    let _schema_gate =
        KeyLock::try_acquire_transition(layout, "clockwork/schema")?.ok_or_else(|| {
            Error::new(
                "migration_busy",
                "quiesce all Clockwork commands before migration",
            )
        })?;
    if !crate::launchd::pending_transitions(layout)?.is_empty() {
        return Err(Error::new(
            "migration_busy",
            "resolve pending binding transitions with the old binary before migration",
        ));
    }
    prepare_database_file(&layout.database())?;
    let connection = Connection::open(layout.database())
        .context("database_unavailable", "open migration database")?;
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .context("database_schema_invalid", "read migration schema")?;
    if version != 1 {
        return Err(Error::new(
            "database_schema_unsupported",
            "migration requires the exact Clockwork schema-one database",
        ));
    }
    verify_schema(&connection, 1)?;
    let running: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM activations WHERE state = 'running')",
            [],
            |row| row.get(0),
        )
        .context("database_read_failed", "check migration quiescence")?;
    if running {
        return Err(Error::new(
            "migration_busy",
            "use the old binary to finish or recover running activations before migration",
        ));
    }
    if !backup.is_absolute() || backup.exists() {
        return Err(Error::new(
            "backup_path_invalid",
            "backup must be a new absolute directory",
        ));
    }
    let parent = backup
        .parent()
        .ok_or_else(|| Error::new("backup_path_invalid", "backup requires a parent directory"))?;
    if parent
        .canonicalize()
        .context("backup_path_invalid", "resolve backup parent")?
        != parent
    {
        return Err(Error::new(
            "backup_path_invalid",
            "backup parent must be canonical",
        ));
    }
    fs::create_dir(backup).context("backup_failed", "create migration backup directory")?;
    fs::set_permissions(backup, fs::Permissions::from_mode(0o700))
        .context("backup_failed", "make backup directory private")?;
    let checkpoint: (i64, i64, i64) = connection
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .context("backup_failed", "checkpoint quiescent database")?;
    if checkpoint.0 != 0 {
        return Err(Error::new("migration_busy", "database checkpoint is busy"));
    }
    for suffix in ["", "-wal", "-shm"] {
        let source = layout
            .database()
            .with_file_name(format!("clockwork.db{suffix}"));
        if source.exists() {
            let mut input =
                std::fs::File::open(&source).context("backup_failed", "open backup source")?;
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(backup.join(format!("clockwork.db{suffix}")))
                .context("backup_failed", "create backup file")?;
            std::io::copy(&mut input, &mut output).context("backup_failed", "copy backup bytes")?;
            output
                .sync_all()
                .context("backup_failed", "sync backup file")?;
        }
    }
    std::fs::File::open(backup)
        .and_then(|file| file.sync_all())
        .context("backup_failed", "sync backup directory")?;
    connection
        .execute_batch("PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL;")
        .context("database_write_failed", "configure migration durability")?;
    connection
        .execute_batch(include_str!("../migrate-v1-v2.sql"))
        .context(
            "database_write_failed",
            "migrate Clockwork schema one to two",
        )?;
    verify_schema(&connection, 2)
}

fn prepare_database_file(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink()
                || !metadata.is_file()
                || metadata.nlink() != 1
                || metadata.permissions().mode() & 0o077 != 0
            {
                return Err(Error::new(
                    "database_path_unsafe",
                    format!(
                        "{} must be a private regular, non-symlink, non-hard-linked file",
                        path.display()
                    ),
                ));
            }
            if metadata.uid() != current_uid()? {
                return Err(Error::new(
                    "database_path_unsafe",
                    format!("{} must be owned by the current user", path.display()),
                ));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(path)
                .context("database_unavailable", format!("create {}", path.display()))?;
        }
        Err(error) => {
            return Err(Error::new(
                "database_unavailable",
                format!("inspect {}: {error}", path.display()),
            ));
        }
    }
    Ok(())
}

fn binding_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<BindingRecord> {
    Ok(BindingRecord {
        key: row.get(0)?,
        definition_digest: row.get(1)?,
        enabled: row.get::<_, i64>(2)? != 0,
        halted_incident: None,
        failure_policy_active: false,
        plist_sha256: row.get(3)?,
        updated_at: row.get(4)?,
    })
}

fn initialize_or_verify_schema(connection: &Connection) -> Result<()> {
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .context("database_schema_invalid", "read Clockwork schema version")?;
    match version {
        0 => {
            let existing: i64 = connection
                .query_row(
                    "SELECT count(*) FROM sqlite_schema \
                     WHERE name NOT LIKE 'sqlite_%' AND sql IS NOT NULL",
                    [],
                    |row| row.get(0),
                )
                .context("database_schema_invalid", "inspect unversioned database")?;
            if existing != 0 {
                return Err(Error::new(
                    "database_schema_unsupported",
                    "refuse to initialize a nonempty unversioned database",
                ));
            }
            connection
                .execute_batch(include_str!("../schema.sql"))
                .context("database_schema_invalid", "initialize Clockwork schema")?;
            verify_schema(connection, 2)?;
        }
        1 => {
            return Err(Error::new(
                "database_migration_required",
                "Clockwork schema one requires explicit migrate --backup DIR before this binary can open it",
            ));
        }
        2 => verify_schema(connection, 2)?,
        other => {
            return Err(Error::new(
                "database_schema_unsupported",
                format!("Clockwork database schema {other} is unsupported"),
            ));
        }
    }
    Ok(())
}

fn verify_schema(connection: &Connection, version: i64) -> Result<()> {
    let marker: Option<(String, i64)> = connection
        .query_row(
            "SELECT product, schema_version FROM clockwork_meta WHERE singleton = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .context("database_schema_invalid", "read Clockwork schema marker")?;
    if marker != Some(("clockwork".to_owned(), version)) {
        return Err(Error::new(
            "database_schema_unsupported",
            "database does not carry the Clockwork selected schema marker",
        ));
    }
    let expected = Connection::open_in_memory()
        .context("database_schema_invalid", "open schema reference database")?;
    expected
        .execute_batch(if version == 1 {
            include_str!("../schema-v1.sql")
        } else {
            include_str!("../schema.sql")
        })
        .context(
            "database_schema_invalid",
            "construct schema reference database",
        )?;
    if schema_objects(connection)? != schema_objects(&expected)? {
        return Err(Error::new(
            "database_schema_unsupported",
            "database objects do not exactly match the declared Clockwork schema",
        ));
    }
    Ok(())
}

fn schema_objects(connection: &Connection) -> Result<Vec<(String, String, String, String)>> {
    let mut statement = connection
        .prepare(
            "SELECT type, name, tbl_name, sql FROM sqlite_schema \
             WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%' \
             ORDER BY type, name",
        )
        .context(
            "database_schema_invalid",
            "inspect Clockwork schema objects",
        )?;
    let rows = statement
        .query_map([], |row| {
            let sql: String = row.get(3)?;
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                sql.split_whitespace().collect::<Vec<_>>().join(" "),
            ))
        })
        .context("database_schema_invalid", "read Clockwork schema objects")?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .context("database_schema_invalid", "decode Clockwork schema objects")
}

fn activation_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ActivationRecord> {
    let trigger = row.get::<_, String>(3)?;
    let state = row.get::<_, String>(4)?;
    Ok(ActivationRecord {
        id: row.get(0)?,
        key: row.get(1)?,
        definition_digest: row.get(2)?,
        trigger: match trigger.as_str() {
            "manual" => Trigger::Manual,
            "launchd" => Trigger::Launchd,
            _ => return Err(rusqlite::Error::InvalidQuery),
        },
        state: ActivationState::parse(&state).ok_or(rusqlite::Error::InvalidQuery)?,
        admitted_at: row.get(5)?,
        started_at: row.get(6)?,
        finished_at: row.get(7)?,
        broker_pid: integer_pid(row.get(8)?)?,
        child_pid: integer_pid(row.get(9)?)?,
        exit_code: row.get(10)?,
        signal: row.get(11)?,
        detail: row.get(12)?,
    })
}

fn running_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<(String, Option<i64>, Option<i64>)> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
}

fn integer_pid(value: Option<i64>) -> rusqlite::Result<Option<u32>> {
    value
        .map(|pid| {
            u32::try_from(pid).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Integer,
                    Box::new(error),
                )
            })
        })
        .transpose()
}

fn process_demonstrably_absent(pid: i64) -> bool {
    if pid <= 1 || pid > i64::from(i32::MAX) {
        return false;
    }
    Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| !status.success())
}

pub(crate) fn now_unix() -> Result<i64> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("clock_invalid", "system clock is before the Unix epoch")?;
    i64::try_from(elapsed.as_secs())
        .map_err(|_| Error::new("clock_invalid", "Unix time does not fit in an i64"))
}

#[cfg(test)]
mod tests {
    use rusqlite::params;
    use tempfile::tempdir;

    use super::Store;
    use crate::model::{ActivationState, Trigger};
    use crate::paths::Layout;

    #[test]
    fn disabling_an_absent_binding_creates_an_idempotent_tombstone() {
        let temporary = tempdir().expect("temporary directory");
        let layout = Layout::isolated(temporary.path());
        let mut store = Store::open(&layout).expect("store");

        let first = store
            .disable_binding("annals/inbox", None)
            .expect("disable");
        let second = store
            .disable_binding("annals/inbox", None)
            .expect("disable again");

        assert!(!first.enabled);
        assert!(first.definition_digest.is_none());
        assert!(first.plist_sha256.is_none());
        assert!(!second.enabled);
        assert!(second.definition_digest.is_none());
        assert!(second.plist_sha256.is_none());
    }

    #[test]
    fn manual_overlap_is_retained_in_activation_history() {
        let temporary = tempdir().expect("temporary directory");
        let layout = Layout::isolated(temporary.path());
        let mut store = Store::open(&layout).expect("store");
        let digest = "0".repeat(64);
        store
            .connection
            .execute(
                "INSERT INTO definitions(digest, key, manifest_json, registered_at) \
                 VALUES (?1, 'semantics/worker', '{}', 1)",
                params![digest],
            )
            .expect("definition fixture");

        let activation = store
            .record_skipped_overlap("semantics/worker", &digest, Trigger::Manual)
            .expect("record overlap");

        assert_eq!(activation.trigger, Trigger::Manual);
        assert_eq!(activation.state, ActivationState::SkippedOverlap);
        assert_eq!(
            store
                .history(Some("semantics/worker"), 1)
                .expect("history")
                .first()
                .map(|row| (&row.trigger, &row.state)),
            Some((&Trigger::Manual, &ActivationState::SkippedOverlap))
        );
    }
    fn policy_fixture(
        schema: u32,
        policy: clockwork::api::AbendPolicy,
    ) -> (tempfile::TempDir, Layout, Store, String) {
        let temporary = tempdir().expect("temporary directory");
        let layout = Layout::isolated(temporary.path());
        let mut store = Store::open(&layout).expect("store");
        let manifest = clockwork::api::Manifest {
            schema_version: schema,
            key: "example/worker".into(),
            release_id: "0".repeat(64),
            release_root: "/fixture/release".into(),
            authority: clockwork::api::Authority::CurrentUserBackground,
            overlap: clockwork::api::OverlapPolicy::Skip,
            failure: clockwork::api::FailurePolicy {
                on_abend: policy,
                email_cli: None,
            },
            timeout_seconds: None,
            arguments: vec![],
            cwd: "/fixture".into(),
            schedule: clockwork::api::Schedule::Interval {
                seconds: 60,
                run_at_load: false,
            },
            launch: clockwork::api::LaunchImage::Direct {
                program: "/fixture/release/worker".into(),
                sha256: "0".repeat(64),
            },
            environment: std::collections::BTreeMap::new(),
            output: clockwork::api::Output {
                stdout: "/fixture/out".into(),
                stderr: "/fixture/err".into(),
            },
        };
        let digest = manifest.digest().expect("digest");
        store
            .register_definition(&digest, &manifest)
            .expect("register fixture");
        store
            .switch_binding("example/worker", &digest, &"1".repeat(64))
            .expect("select fixture");
        (temporary, layout, store, digest)
    }

    #[test]
    fn every_abnormal_runtime_outcome_atomically_closes_admission() {
        use clockwork::api::AbendPolicy;
        for (state, exit, signal) in [
            (ActivationState::StartFailed, None, None),
            (ActivationState::Exited, Some(7), None),
            (ActivationState::Signaled, None, Some(15)),
            (ActivationState::TimedOut, None, Some(9)),
            (ActivationState::Lost, None, None),
        ] {
            let (_temporary, layout, mut store, digest) =
                policy_fixture(2, AbendPolicy::HaltUntilApproved);
            let activation = store
                .begin_activation("example/worker", &digest, Trigger::Launchd)
                .expect("admit");
            store
                .finish_activation(&activation.id, state, exit, signal, None)
                .expect("finish");
            assert_eq!(
                store.activation(&activation.id).expect("activation").state,
                state
            );
            let incident = store
                .active_incident("example/worker")
                .expect("incident")
                .expect("halted");
            assert_eq!(incident.notification_status, "pending");
            assert!(
                store
                    .begin_activation("example/worker", &digest, Trigger::Launchd)
                    .is_err()
            );
            drop(store);
            let store = Store::open(&layout).expect("reopen");
            assert_eq!(
                store
                    .binding("example/worker")
                    .expect("binding")
                    .halted_incident,
                Some(incident.id)
            );
        }
    }

    #[test]
    fn explicit_continue_and_legacy_definitions_do_not_halt() {
        use clockwork::api::AbendPolicy;
        for (schema, policy) in [
            (2, AbendPolicy::ContinueNextActivation),
            (1, AbendPolicy::HaltUntilApproved),
        ] {
            let (_temporary, _layout, mut store, digest) = policy_fixture(schema, policy);
            let activation = store
                .begin_activation("example/worker", &digest, Trigger::Manual)
                .expect("admit");
            store
                .finish_activation(&activation.id, ActivationState::Exited, Some(3), None, None)
                .expect("finish");
            assert!(
                store
                    .active_incident("example/worker")
                    .expect("incident")
                    .is_none()
            );
            assert_eq!(
                store
                    .binding("example/worker")
                    .expect("binding")
                    .failure_policy_active,
                schema == 2
            );
            assert!(
                store
                    .begin_activation("example/worker", &digest, Trigger::Launchd)
                    .is_ok()
            );
        }
    }

    #[test]
    fn product_report_halts_even_when_the_child_exits_zero_and_is_deduplicated_after_approval() {
        let (_temporary, _layout, mut store, digest) =
            policy_fixture(2, clockwork::api::AbendPolicy::HaltUntilApproved);
        let activation = store
            .begin_activation("example/worker", &digest, Trigger::Manual)
            .expect("admit");
        let first = store
            .report_abend(&activation.id, "model_failed", "job/one")
            .expect("report")
            .expect("incident");
        let again = store
            .report_abend(&activation.id, "model_failed", "job/one")
            .expect("report twice")
            .expect("incident");
        assert_eq!(first.id, again.id);
        assert!(store.resume("example/worker", &first.id).is_err());
        store
            .finish_activation(&activation.id, ActivationState::Exited, Some(0), None, None)
            .expect("finish");
        assert!(store.resume("example/worker", "wrong-incident").is_err());
        store
            .resume("example/worker", &first.id)
            .expect("explicit approval");
        let next = store
            .begin_activation("example/worker", &digest, Trigger::Manual)
            .expect("next admission");
        store
            .report_abend(&next.id, "model_failed", "job/one")
            .expect("repeat historical evidence");
        assert!(
            store
                .active_incident("example/worker")
                .expect("incident")
                .is_none()
        );
        assert_eq!(store.incidents(None, 20).expect("history").len(), 1);
    }

    #[test]
    fn selection_disable_and_compensation_preserve_the_failure_halt() {
        let (_temporary, _layout, mut store, digest) =
            policy_fixture(2, clockwork::api::AbendPolicy::HaltUntilApproved);
        let prior = store.binding("example/worker").expect("prior");
        let incident = store
            .import_halt("example/worker", "legacy_failure", "legacy/one")
            .expect("import");
        store
            .disable_binding("example/worker", Some(&digest))
            .expect("disable");
        store
            .clear_plist_identity("example/worker")
            .expect("clear plist");
        store
            .switch_binding("example/worker", &digest, &"2".repeat(64))
            .expect("switch");
        store
            .restore_binding("example/worker", Some(&prior))
            .expect("compensate");
        let binding = store.binding("example/worker").expect("binding");
        assert!(binding.enabled);
        assert_eq!(binding.halted_incident, Some(incident.id.clone()));
        store
            .disable_binding("example/worker", None)
            .expect("disable again");
        let resumed = store
            .resume("example/worker", &incident.id)
            .expect("explicit continuation");
        assert!(!resumed.enabled);
        assert!(resumed.halted_incident.is_none());
    }

    #[test]
    fn migration_requires_explicit_backup_and_preserves_legacy_definition_identity() {
        use std::os::unix::fs::PermissionsExt as _;
        let (_basis_dir, _basis_layout, basis, digest) =
            policy_fixture(1, clockwork::api::AbendPolicy::HaltUntilApproved);
        let definition = basis.definition(&digest).expect("legacy definition");
        let temporary = tempdir().expect("temporary");
        let layout = Layout::isolated(temporary.path());
        layout.prepare().expect("layout");
        let connection = rusqlite::Connection::open(layout.database()).expect("legacy database");
        connection
            .execute_batch(include_str!("../schema-v1.sql"))
            .expect("legacy schema");
        connection.execute("INSERT INTO definitions(digest,key,manifest_json,registered_at) VALUES (?1,'example/worker',?2,1)",
            params![digest,serde_json::to_string(&definition.manifest).expect("canonical manifest")]).expect("legacy definition row");
        connection.execute("INSERT INTO bindings(key,definition_digest,enabled,updated_at) VALUES ('example/worker',?1,0,1)", [&digest]).expect("legacy disabled binding");
        connection.execute("INSERT INTO activations(id,key,definition_digest,trigger,state,admitted_at,finished_at,exit_code) VALUES ('old-failure','example/worker',?1,'launchd','exited',1,2,3)", [&digest]).expect("legacy terminal failure");
        drop(connection);
        std::fs::set_permissions(layout.database(), std::fs::Permissions::from_mode(0o600))
            .expect("private database");
        assert!(Store::open(&layout).is_err());
        let backup = temporary
            .path()
            .canonicalize()
            .expect("canonical")
            .join("backup");
        super::migrate(&layout, &backup).expect("migration");
        let store = Store::open(&layout).expect("new schema");
        assert_eq!(
            store
                .definition(&digest)
                .expect("preserved definition")
                .digest,
            digest
        );
        let binding = store.binding("example/worker").expect("preserved binding");
        assert!(!binding.enabled);
        assert!(!binding.failure_policy_active);
        assert!(binding.halted_incident.is_none());
        assert_eq!(
            store
                .activation("old-failure")
                .expect("preserved history")
                .exit_code,
            Some(3)
        );
        assert_eq!(store.quick_check().expect("check"), "ok");
        let backup_connection =
            rusqlite::Connection::open(backup.join("clockwork.db")).expect("backup");
        super::verify_schema(&backup_connection, 1).expect("schema one backup");
        super::verify_schema(&store.connection, 2).expect("schema two live fixture");
    }
}
