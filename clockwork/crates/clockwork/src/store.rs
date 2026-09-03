use std::fs::{self, OpenOptions};
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension as _, TransactionBehavior, params};

use crate::error::{Context as _, Error, Result};
use crate::manifest;
use crate::model::{
    ActivationRecord, ActivationState, BindingRecord, DefinitionRecord, DefinitionSummary,
    Manifest, Trigger,
};
use crate::paths::{Layout, current_uid};

pub(crate) struct Store {
    connection: Connection,
}

impl Store {
    pub(crate) fn open(layout: &Layout) -> Result<Self> {
        layout.prepare()?;
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
        Ok(Self { connection })
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
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .context("database_read_failed", "decode bindings")
    }

    pub(crate) fn binding(&self, key: &str) -> Result<BindingRecord> {
        self.optional_binding(key)?
            .ok_or_else(|| Error::new("binding_not_found", format!("binding {key} does not exist")))
    }

    pub(crate) fn optional_binding(&self, key: &str) -> Result<Option<BindingRecord>> {
        self.connection
            .query_row(
                "SELECT key, definition_digest, enabled, plist_sha256, updated_at \
                 FROM bindings WHERE key = ?1",
                [key],
                binding_from_row,
            )
            .optional()
            .context("database_read_failed", "read binding")
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
        Ok(BindingRecord {
            key: key.to_owned(),
            definition_digest: Some(digest.to_owned()),
            enabled: true,
            plist_sha256: Some(plist_sha256.to_owned()),
            updated_at,
        })
    }

    pub(crate) fn disable_binding(
        &mut self,
        key: &str,
        selected_digest: Option<&str>,
    ) -> Result<BindingRecord> {
        let prior = self.optional_binding(key)?;
        let definition_digest = selected_digest.map(ToOwned::to_owned).or_else(|| {
            prior
                .as_ref()
                .and_then(|binding| binding.definition_digest.clone())
        });
        let plist_sha256 = prior
            .as_ref()
            .and_then(|binding| binding.plist_sha256.clone());
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
        Ok(BindingRecord {
            key: key.to_owned(),
            definition_digest,
            enabled: false,
            plist_sha256,
            updated_at,
        })
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
        Ok(BindingRecord {
            key: binding.key,
            definition_digest: binding.definition_digest,
            enabled: false,
            plist_sha256: None,
            updated_at,
        })
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
                  WHERE key = ?2 AND enabled = 1 AND definition_digest = ?3",
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
                format!("binding {key} was disabled or changed before activation admission"),
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
        let changed = self
            .connection
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
                let changed = self
                    .connection
                    .execute(
                        "UPDATE activations SET state = 'lost', finished_at = ?2, detail = ?3 \
                         WHERE id = ?1 AND state = 'running'",
                        params![
                            id,
                            now_unix()?,
                            "recorded broker and any recorded child are absent"
                        ],
                    )
                    .context("database_write_failed", "mark stale activation lost")?;
                recovered += changed;
            }
        }
        Ok(recovered)
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
            verify_schema_one(connection)?;
        }
        1 => verify_schema_one(connection)?,
        other => {
            return Err(Error::new(
                "database_schema_unsupported",
                format!("Clockwork database schema {other} is unsupported"),
            ));
        }
    }
    Ok(())
}

fn verify_schema_one(connection: &Connection) -> Result<()> {
    let marker: Option<(String, i64)> = connection
        .query_row(
            "SELECT product, schema_version FROM clockwork_meta WHERE singleton = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .context("database_schema_invalid", "read Clockwork schema marker")?;
    if marker != Some(("clockwork".to_owned(), 1)) {
        return Err(Error::new(
            "database_schema_unsupported",
            "database does not carry the Clockwork schema-one marker",
        ));
    }
    let expected = Connection::open_in_memory().context(
        "database_schema_invalid",
        "open schema-one reference database",
    )?;
    expected
        .execute_batch(include_str!("../schema.sql"))
        .context(
            "database_schema_invalid",
            "construct schema-one reference database",
        )?;
    if schema_objects(connection)? != schema_objects(&expected)? {
        return Err(Error::new(
            "database_schema_unsupported",
            "database objects do not exactly match Clockwork schema one",
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
}
