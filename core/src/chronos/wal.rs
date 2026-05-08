//! WAL Replay and Database Checkpointing

use anyhow::{Context, Result};
use rusqlite::{backup::Backup, Connection, OpenFlags};
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct WalReplayer {
    _timeout_seconds: u64,
}

#[derive(Debug, Clone)]
pub enum ReplayResult {
    Success,
    PartialRecovery,
    Failed,
}

impl WalReplayer {
    pub fn new(timeout_seconds: u64) -> Self {
        Self {
            _timeout_seconds: timeout_seconds,
        }
    }

    pub fn replay_with_recovery(&self, src: &Path, dst: &Path) -> Result<ReplayResult> {
        match self.attempt_replay(src, dst) {
            Ok(success) => Ok(success),
            Err(e) => {
                // Try fallback approach
                match self.fallback_replay(src, dst) {
                    Ok(fallback_result) => Ok(fallback_result),
                    Err(_) => Err(e), // Return original error if fallback fails
                }
            }
        }
    }

    fn attempt_replay(&self, src: &Path, dst: &Path) -> Result<ReplayResult> {
        // Open source database read-only
        let src_conn = Connection::open_with_flags(
            src,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .with_context(|| format!("Failed to open source database: {}", src.display()))?;

        // Create destination database
        let mut dst_conn = Connection::open(dst)
            .with_context(|| format!("Failed to create destination database: {}", dst.display()))?;

        {
            // Perform backup operation
            let backup = Backup::new(&src_conn, &mut dst_conn)
                .with_context(|| "Failed to create backup object")?;

            // Run backup with progress monitoring
            backup
                .run_to_completion(5, Duration::from_millis(25), None)
                .with_context(|| "Backup operation failed")?;
        }

        Ok(ReplayResult::Success)
    }

    fn fallback_replay(&self, src: &Path, dst: &Path) -> Result<ReplayResult> {
        // Alternative method for handling corrupted or problematic databases
        // This could use .dump/.read approach or other recovery methods

        // For now, we'll try a simpler approach with different flags
        let src_conn = Connection::open_with_flags(
            src,
            OpenFlags::SQLITE_OPEN_READ_ONLY
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_URI,
        )
        .with_context(|| {
            format!(
                "Failed to open source database (fallback): {}",
                src.display()
            )
        })?;

        let mut dst_conn = Connection::open(dst).with_context(|| {
            format!(
                "Failed to create destination database (fallback): {}",
                dst.display()
            )
        })?;

        {
            let backup = Backup::new(&src_conn, &mut dst_conn)?;
            backup.run_to_completion(10, Duration::from_millis(50), None)?;
        }

        Ok(ReplayResult::PartialRecovery)
    }

    pub fn validate_database(&self, db_path: &Path) -> Result<bool> {
        let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;

        // Check if database is accessible and has valid schema
        let result = conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table'",
            [],
            |row| {
                let count: i32 = row.get(0)?;
                Ok(count > 0)
            },
        );

        match result {
            Ok(has_tables) => Ok(has_tables),
            Err(_) => Ok(false),
        }
    }
}
