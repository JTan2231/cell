//! Current configuration storage. Forecasts use a read-only connection.

use std::fs::{self, OpenOptions};
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use serde::Serialize;
use time::Date;

use crate::{Config, ConfigItem, ExpenseRule, RepeatUnit, parse_date, validate_name};

const APPLICATION_ID: i32 = 0x4d41_4e54;
const SCHEMA_VERSION: i32 = 1;

#[derive(Debug, Serialize)]
pub struct ConfigSummary {
    pub id: i64,
    pub name: String,
}

pub type ItemInput = ExpenseRule;

#[derive(Default, Debug)]
pub struct ItemChanges {
    pub name: Option<String>,
    pub amount_cents: Option<i64>,
    pub first_due: Option<Date>,
    pub repeat_unit: Option<RepeatUnit>,
    pub repeat_every: Option<u32>,
    pub end_date: Option<Date>,
    pub clear_end: bool,
    pub once: bool,
}

pub struct Store {
    connection: Connection,
}

impl Store {
    /// Initialize empty state, or open an existing supported database.
    ///
    /// # Errors
    /// Returns an error for foreign, unsupported, inaccessible, or nonregular state.
    pub fn initialize(path: &Path) -> Result<Self> {
        ensure!(path.is_absolute(), "database path must be absolute");
        match fs::symlink_metadata(path) {
            Ok(metadata) => ensure!(metadata.is_file(), "database must be a regular file"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let parent = path.parent().context("database path has no parent")?;
                let mut directories = fs::DirBuilder::new();
                directories.recursive(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::DirBuilderExt;
                    directories.mode(0o700);
                }
                directories.create(parent)?;
                let mut file = OpenOptions::new();
                file.read(true).write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    file.mode(0o600);
                }
                match file.open(path) {
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                        ensure!(
                            fs::symlink_metadata(path)?.is_file(),
                            "database must be a regular file"
                        );
                    }
                    Err(error) => return Err(error.into()),
                }
            }
            Err(error) => return Err(error.into()),
        }
        Self::initialize_connection(Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE,
        )?)
    }

    /// Open existing state without initialization or migration.
    ///
    /// # Errors
    /// Returns an error for missing, foreign, unsupported, or inaccessible state.
    pub fn open(path: &Path, read_only: bool) -> Result<Self> {
        ensure!(path.is_absolute(), "database path must be absolute");
        ensure!(
            fs::symlink_metadata(path)
                .context("database missing; run mantic init")?
                .is_file(),
            "database must be a regular file"
        );
        let flags = if read_only {
            OpenFlags::SQLITE_OPEN_READ_ONLY
        } else {
            OpenFlags::SQLITE_OPEN_READ_WRITE
        };
        let connection = Connection::open_with_flags(path, flags)?;
        configure(&connection)?;
        check_schema(&connection)?;
        Ok(Self { connection })
    }

    fn initialize_connection(mut connection: Connection) -> Result<Self> {
        configure(&connection)?;
        initialize_schema(&mut connection)?;
        Ok(Self { connection })
    }

    /// Create a named configuration.
    ///
    /// # Errors
    /// Returns an error for an invalid or duplicate name, or a failed transaction.
    pub fn create_config(&mut self, name: &str) -> Result<ConfigSummary> {
        let name = clean_name(name)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("INSERT INTO config(name) VALUES (?)", [&name])
            .context("cannot create config; name must be unique")?;
        let result = ConfigSummary {
            id: tx.last_insert_rowid(),
            name,
        };
        tx.commit()?;
        Ok(result)
    }

    /// List current configurations in ID order.
    ///
    /// # Errors
    /// Returns a database read error.
    pub fn list_configs(&self) -> Result<Vec<ConfigSummary>> {
        let mut statement = self
            .connection
            .prepare("SELECT id, name FROM config ORDER BY id")?;
        let rows = statement.query_map([], |row| {
            Ok(ConfigSummary {
                id: row.get(0)?,
                name: row.get(1)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Read one configuration and its items in a consistent snapshot.
    ///
    /// # Errors
    /// Returns an error for missing configs, invalid stored rules, or database reads.
    pub fn get_config(&mut self, name: &str) -> Result<Config> {
        let name = clean_name(name)?;
        let tx = self.connection.transaction()?;
        let (id, name) = config_identity(&tx, &name)?;
        let mut statement = tx.prepare("SELECT id, config_id, name, amount_cents, first_due, repeat_unit, repeat_every, end_date FROM config_item WHERE config_id=? ORDER BY id")?;
        let rows = statement.query_map([id], stored_item)?;
        let items = rows
            .map(|row| decode_item(row?))
            .collect::<Result<Vec<_>>>()?;
        drop(statement);
        tx.commit()?;
        Ok(Config { id, name, items })
    }

    /// Rename a configuration without changing its identity or items.
    ///
    /// # Errors
    /// Returns an error for missing configs, invalid or duplicate names, or writes.
    pub fn rename_config(&mut self, name: &str, replacement: &str) -> Result<ConfigSummary> {
        let name = clean_name(name)?;
        let replacement = clean_name(replacement)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (id, _) = config_identity(&tx, &name)?;
        tx.execute(
            "UPDATE config SET name=? WHERE id=?",
            params![replacement, id],
        )
        .context("config name must be unique")?;
        tx.commit()?;
        Ok(ConfigSummary {
            id,
            name: replacement,
        })
    }

    /// Delete a configuration and its owned items atomically.
    ///
    /// # Errors
    /// Returns an error for a missing config or a failed write.
    pub fn delete_config(&mut self, name: &str) -> Result<()> {
        let name = clean_name(name)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure!(
            tx.execute("DELETE FROM config WHERE name=?", [&name])? == 1,
            "config not found: {name}"
        );
        tx.commit()?;
        Ok(())
    }

    /// Add an owned expense rule.
    ///
    /// # Errors
    /// Returns an error for a missing config, invalid rule, or failed write.
    pub fn add_item(&mut self, config: &str, input: &ItemInput) -> Result<ConfigItem> {
        let config = clean_name(config)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (config_id, _) = config_identity(&tx, &config)?;
        let mut item = ConfigItem {
            id: 1,
            config_id,
            name: clean_name(&input.name)?,
            amount_cents: input.amount_cents,
            first_due: input.first_due,
            repeat_unit: input.repeat_unit,
            repeat_every: input.repeat_every,
            end_date: input.end_date,
        };
        item.validate()?;
        tx.execute("INSERT INTO config_item(config_id,name,amount_cents,first_due,repeat_unit,repeat_every,end_date) VALUES (?,?,?,?,?,?,?)",
            params![item.config_id, item.name, item.amount_cents, item.first_due.to_string(), item.repeat_unit.map(RepeatUnit::as_str), item.repeat_every, item.end_date.map(|d| d.to_string())])?;
        item.id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(item)
    }

    /// Apply a partial rule update within one write transaction.
    ///
    /// # Errors
    /// Returns an error for missing items, invalid changes, or failed writes.
    pub fn update_item(&mut self, id: i64, changes: ItemChanges) -> Result<ConfigItem> {
        ensure!(id > 0, "item ID must be positive");
        ensure!(
            !(changes.once && (changes.repeat_unit.is_some() || changes.repeat_every.is_some())),
            "--once conflicts with recurrence changes"
        );
        ensure!(
            !(changes.clear_end && changes.end_date.is_some()),
            "cannot set and clear an end date together"
        );
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let stored = tx.query_row("SELECT id,config_id,name,amount_cents,first_due,repeat_unit,repeat_every,end_date FROM config_item WHERE id=?", [id], stored_item).optional()?.context("item not found")?;
        let mut item = decode_item(stored)?;
        if let Some(name) = changes.name {
            item.name = clean_name(&name)?;
        }
        if let Some(amount) = changes.amount_cents {
            item.amount_cents = amount;
        }
        if let Some(first) = changes.first_due {
            item.first_due = first;
        }
        if changes.once {
            item.repeat_unit = None;
            item.repeat_every = None;
        } else {
            if let Some(unit) = changes.repeat_unit {
                item.repeat_unit = Some(unit);
                item.repeat_every = Some(item.repeat_every.unwrap_or(1));
            }
            if let Some(every) = changes.repeat_every {
                item.repeat_every = Some(every);
            }
        }
        if changes.clear_end {
            item.end_date = None;
        } else if let Some(end) = changes.end_date {
            item.end_date = Some(end);
        }
        item.validate()?;
        tx.execute("UPDATE config_item SET name=?,amount_cents=?,first_due=?,repeat_unit=?,repeat_every=?,end_date=? WHERE id=?",
            params![item.name, item.amount_cents, item.first_due.to_string(), item.repeat_unit.map(RepeatUnit::as_str), item.repeat_every, item.end_date.map(|d| d.to_string()), item.id])?;
        tx.commit()?;
        Ok(item)
    }

    /// Delete one owned item.
    ///
    /// # Errors
    /// Returns an error for missing items or failed writes.
    pub fn delete_item(&mut self, id: i64) -> Result<()> {
        ensure!(id > 0, "item ID must be positive");
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure!(
            tx.execute("DELETE FROM config_item WHERE id=?", [id])? == 1,
            "item not found: {id}"
        );
        tx.commit()?;
        Ok(())
    }
}

fn initialize_schema(connection: &mut Connection) -> Result<()> {
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let application_id: i32 = tx.pragma_query_value(None, "application_id", |row| row.get(0))?;
    let version: i32 = tx.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if application_id == 0 && version == 0 {
        let objects: i64 = tx.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'",
            [],
            |row| row.get(0),
        )?;
        ensure!(objects == 0, "refusing to initialize a foreign database");
        tx.execute_batch(include_str!("schema.sql"))?;
        tx.pragma_update(None, "application_id", APPLICATION_ID)?;
        tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    } else {
        check_schema(&tx)?;
    }
    tx.commit()?;
    Ok(())
}

fn configure(connection: &Connection) -> Result<()> {
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.pragma_update(None, "foreign_keys", true)?;
    Ok(())
}

fn check_schema(connection: &Connection) -> Result<()> {
    let app: i32 = connection.pragma_query_value(None, "application_id", |row| row.get(0))?;
    let version: i32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    ensure!(
        app == APPLICATION_ID && version == SCHEMA_VERSION,
        "unsupported or foreign database; expected Mantic schema 1"
    );
    connection.prepare("SELECT id,name FROM config LIMIT 0")?;
    connection.prepare("SELECT id,config_id,name,amount_cents,first_due,repeat_unit,repeat_every,end_date FROM config_item LIMIT 0")?;
    Ok(())
}

fn clean_name(name: &str) -> Result<String> {
    validate_name(name)?;
    Ok(name.trim().to_owned())
}

fn config_identity(connection: &Connection, name: &str) -> Result<(i64, String)> {
    connection
        .query_row("SELECT id,name FROM config WHERE name=?", [name], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .optional()?
        .with_context(|| format!("config not found: {name}"))
}

struct StoredItem {
    id: i64,
    config_id: i64,
    name: String,
    amount_cents: i64,
    first_due: String,
    repeat_unit: Option<String>,
    repeat_every: Option<u32>,
    end_date: Option<String>,
}

fn stored_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredItem> {
    Ok(StoredItem {
        id: row.get(0)?,
        config_id: row.get(1)?,
        name: row.get(2)?,
        amount_cents: row.get(3)?,
        first_due: row.get(4)?,
        repeat_unit: row.get(5)?,
        repeat_every: row.get(6)?,
        end_date: row.get(7)?,
    })
}

fn decode_item(row: StoredItem) -> Result<ConfigItem> {
    let repeat_unit = match row.repeat_unit.as_deref() {
        None => None,
        Some("day") => Some(RepeatUnit::Day),
        Some("week") => Some(RepeatUnit::Week),
        Some("month") => Some(RepeatUnit::Month),
        Some("year") => Some(RepeatUnit::Year),
        Some(_) => bail!("invalid stored recurrence unit"),
    };
    let item = ConfigItem {
        id: row.id,
        config_id: row.config_id,
        name: row.name,
        amount_cents: row.amount_cents,
        first_due: parse_date(&row.first_due)?,
        repeat_unit,
        repeat_every: row.repeat_every,
        end_date: row.end_date.as_deref().map(parse_date).transpose()?,
    };
    item.validate()?;
    Ok(item)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ForecastInput, forecast};

    fn memory() -> Result<Store> {
        Store::initialize_connection(Connection::open_in_memory()?)
    }

    fn rent() -> Result<ItemInput> {
        Ok(ItemInput {
            name: "rent".into(),
            amount_cents: 150_000,
            first_due: parse_date("2026-10-01")?,
            repeat_unit: Some(RepeatUnit::Month),
            repeat_every: Some(1),
            end_date: None,
        })
    }

    #[test]
    fn names_and_ids_survive_rename_and_items_have_one_owner() -> Result<()> {
        let mut store = memory()?;
        let household = store.create_config(" household ")?;
        store.create_config("travel")?;
        let item = store.add_item("household", &rent()?)?;
        assert_eq!(item.config_id, household.id);
        assert!(store.get_config("travel")?.items.is_empty());
        let renamed = store.rename_config("household", "home")?;
        assert_eq!(household.id, renamed.id);
        assert_eq!(store.get_config("home")?.items, vec![item]);
        assert!(store.create_config("home").is_err());
        assert!(store.get_config("household").is_err());
        Ok(())
    }

    #[test]
    fn invalid_partial_updates_roll_back_and_valid_recurrence_edits_are_atomic() -> Result<()> {
        let mut store = memory()?;
        store.create_config("home")?;
        let item = store.add_item("home", &rent()?)?;
        assert!(
            store
                .update_item(
                    item.id,
                    ItemChanges {
                        name: Some("changed".into()),
                        amount_cents: Some(-1),
                        ..ItemChanges::default()
                    }
                )
                .is_err()
        );
        assert_eq!(store.get_config("home")?.items, vec![item.clone()]);
        let once = store.update_item(
            item.id,
            ItemChanges {
                once: true,
                ..ItemChanges::default()
            },
        )?;
        assert_eq!((once.repeat_unit, once.repeat_every), (None, None));
        assert!(
            store
                .update_item(
                    item.id,
                    ItemChanges {
                        repeat_every: Some(2),
                        ..ItemChanges::default()
                    }
                )
                .is_err()
        );
        let repeated = store.update_item(
            item.id,
            ItemChanges {
                repeat_unit: Some(RepeatUnit::Week),
                repeat_every: Some(2),
                ..ItemChanges::default()
            },
        )?;
        assert_eq!(
            (repeated.repeat_unit, repeated.repeat_every),
            (Some(RepeatUnit::Week), Some(2))
        );
        Ok(())
    }

    #[test]
    fn config_deletion_cascades_only_its_items_and_ids_are_not_reused() -> Result<()> {
        let mut store = memory()?;
        store.create_config("home")?;
        store.create_config("travel")?;
        let home = store.add_item("home", &rent()?)?;
        let travel = store.add_item("travel", &rent()?)?;
        store.delete_config("home")?;
        assert!(store.update_item(home.id, ItemChanges::default()).is_err());
        assert_eq!(store.get_config("travel")?.items, vec![travel.clone()]);
        store.delete_item(travel.id)?;
        let new = store.add_item("travel", &rent()?)?;
        assert!(new.id > travel.id);
        assert!(store.delete_item(travel.id).is_err());
        Ok(())
    }

    #[test]
    fn forecasts_do_not_change_any_database_rows() -> Result<()> {
        let mut store = memory()?;
        store.create_config("home")?;
        store.add_item("home", &rent()?)?;
        let before = store.get_config("home")?;
        let changes = store.connection.total_changes();
        let input = ForecastInput {
            starting_amount_cents: 500_000,
            from: parse_date("2026-10-02")?,
            until: parse_date("2026-11-02")?,
        };
        assert_eq!(
            forecast(&before, input)?,
            forecast(&store.get_config("home")?, input)?
        );
        assert_eq!(store.connection.total_changes(), changes);
        assert_eq!(before, store.get_config("home")?);
        let count: i64 = store.connection.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(count, 2);
        Ok(())
    }

    #[test]
    fn foreign_or_newer_databases_are_refused_without_changes() -> Result<()> {
        let mut connection = Connection::open_in_memory()?;
        connection.execute_batch(
            "CREATE TABLE foreign_data(value TEXT); INSERT INTO foreign_data VALUES ('kept');",
        )?;
        assert!(initialize_schema(&mut connection).is_err());
        let retained: String =
            connection.query_row("SELECT value FROM foreign_data", [], |row| row.get(0))?;
        assert_eq!(retained, "kept");
        let mut store = memory()?;
        store.connection.pragma_update(None, "user_version", 2)?;
        assert!(check_schema(&store.connection).is_err());
        assert!(initialize_schema(&mut store.connection).is_err());
        let version: i32 = store
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))?;
        assert_eq!(version, 2);
        Ok(())
    }
}
