use rusqlite::{Connection, config::DbConfig};

/// Keep WAL coordination files available to readers after a writer closes.
pub(crate) fn persist_wal(connection: &Connection) -> rusqlite::Result<()> {
    // Closing must not remove the files a read-only connection needs. SQLite's
    // ordinary automatic checkpoints during writes remain enabled.
    connection.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true)?;
    Ok(())
}
