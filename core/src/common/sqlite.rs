//! Thin SQLite helper wrapping rusqlite with our conventions.

use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};
use std::path::Path;

pub type SqliteConn = Connection;

pub fn open_readonly(path: &Path) -> Result<SqliteConn> {
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("opening SQLite database {}", path.display()))
}
