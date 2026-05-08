//! Chronos Manifest Output Types

use serde::Serialize;

#[derive(Debug, Serialize, Clone)]
pub struct ChronosPreparedRecord {
    pub artifact_key: String,
    pub source_path: String,
    pub working_path: String,
    pub resolution_method: String,
    pub sha256: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ChronosManifest {
    pub backup_root: String,
    pub prepared: Vec<ChronosPreparedRecord>,
    pub device_info: Option<crate::chronos::device::DeviceInfo>,
}
