//! Stub for MVT builder module (module under refactor)

use anyhow::Result;

pub struct MvtAndroidBuilder;
pub struct MvtAndroidCheckApkBuilder;
pub struct MvtAndroidCheckBackupBuilder;
pub struct MvtAndroidCheckBuilder;
pub struct MvtIosBuilder;
pub struct MvtIosCheckBackupBuilder;
pub struct MvtIosDecryptBackupBuilder;

pub fn check_mvt_android_available() -> Result<bool> {
    Ok(false)
}

pub fn check_mvt_available() -> Result<bool> {
    Ok(false)
}
