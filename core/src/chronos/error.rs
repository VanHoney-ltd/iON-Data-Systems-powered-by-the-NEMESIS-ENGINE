//! Chronos error types.

use rusqlite::Error as SqliteError;
use std::io::Error as IoError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ChronosError {
    #[error("Device pairing failed: {0}")]
    PairingFailed(String),
    #[error("Backup creation failed: {0}")]
    BackupFailed(String),
    #[error("Required file missing: {0}")]
    MissingFile(String),
    #[error("WAL replay failed: {0}")]
    WalReplayFailed(String),
    #[error("Database error: {0}")]
    DatabaseError(#[from] SqliteError),
    #[error("IO error: {0}")]
    IoError(#[from] IoError),
    #[error("Device not found or not connected")]
    DeviceNotFound,
    #[error("Backup timeout exceeded")]
    BackupTimeout,
    #[error("Invalid backup format")]
    InvalidBackupFormat,
    #[error("Manifest parsing error: {0}")]
    ManifestError(String),
    #[error("Tool execution failed: {0}")]
    ToolExecutionFailed(String),
}
