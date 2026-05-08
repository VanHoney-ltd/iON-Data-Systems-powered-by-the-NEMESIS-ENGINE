//! Chronos - The iON Backup Engine
//!
//! Agent 0: Foundation for all other iON modules

mod awake;
pub mod backup;
pub mod config;
pub mod device;
pub mod error;
pub mod idevicebackup2;
pub mod ios_backup;
pub mod manifest;
pub mod mvt_builder;
pub mod progress;
pub mod validation;
pub mod wal;

pub use crate::ensure_dir;
pub use backup::{create_backup, BackupManager, BackupResult};
pub use config::{BackupEncryption, ChronosConfig};
pub use device::{
    probe_live_status, ConnectedDeviceStatus, DeviceInfo, DeviceManager, IfuseMount,
    LiveDeviceStatus, PairingState, ToolStatus,
};
pub use error::ChronosError;
pub use ios_backup as ios_backup_decrypt;
pub use manifest::{ChronosManifest, ChronosPreparedRecord};
pub use progress::BackupProgress;
pub use wal::{ReplayResult, WalReplayer};

// Re-export command builders
pub use idevicebackup2::{
    check_tool_available as check_idevicebackup2_available, enable_encryption,
    full_encrypted_backup, IDeviceBackup2Builder, IDeviceBackup2Command, IDeviceBackup2Options,
};
pub use mvt_builder::{
    check_mvt_android_available, check_mvt_available, MvtAndroidBuilder, MvtAndroidCheckApkBuilder,
    MvtAndroidCheckBackupBuilder, MvtAndroidCheckBuilder, MvtIosBuilder, MvtIosCheckBackupBuilder,
    MvtIosDecryptBackupBuilder,
};

use crate::case::Case;
use anyhow::Result;

/// Main entry point for Chronos functionality
pub fn run_chronos(case: Case, config: ChronosConfig) -> Result<BackupResult> {
    create_backup(case, config)
}
