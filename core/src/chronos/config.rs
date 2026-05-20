//! Chronos Configuration

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChronosConfig {
    /// Maximum time to wait for backup completion (seconds)
    pub backup_timeout: u64,

    /// Number of retry attempts for failed operations
    pub retry_attempts: u32,

    /// Maximum time for WAL replay (seconds)
    pub wal_replay_timeout: u64,

    /// Required tools for operation
    pub required_tools: Vec<String>,

    /// Backup encryption settings
    pub backup_encryption: BackupEncryption,

    /// Backup password (required for encrypted backups)
    pub backup_password: Option<String>,

    /// Enable verbose logging
    pub verbose: bool,

    /// Skip device pairing/tools and only use an existing backup
    pub offline: bool,

    /// Use a specific backup root instead of searching under the case backup dir
    pub backup_path_override: Option<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum BackupEncryption {
    Encrypted,
}

impl Default for ChronosConfig {
    fn default() -> Self {
        Self {
            backup_timeout: 1800, // 30 minutes
            retry_attempts: 3,
            wal_replay_timeout: 300, // 5 minutes
            required_tools: vec!["idevicepair".to_string(), "idevicebackup2".to_string()],
            backup_encryption: BackupEncryption::Encrypted,
            backup_password: None,
            verbose: false,
            offline: false,
            backup_path_override: None,
        }
    }
}
