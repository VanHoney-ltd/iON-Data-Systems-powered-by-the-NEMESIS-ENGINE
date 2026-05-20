//! Rust-based encrypted iOS backup decryptor (owner-authorized).
//!
//! This module is a compatibility wrapper over `chronos::ios_backup`.

use anyhow::Result;
use std::path::Path;

pub use crate::chronos::ios_backup::{ExtractResult, ExtractSpec, VerifyResult};
use crate::chronos::ios_backup;

/// Decrypt specific targets from an encrypted iTunes/Finder backup.
pub fn extract_from_encrypted_itunes_backup(
    encrypted_backup_root: &Path,
    output_root: &Path,
    password: &str,
    specs: &[ExtractSpec],
) -> Result<ExtractResult> {
    ios_backup::extract_from_encrypted_backup(encrypted_backup_root, output_root, password, specs)
}

/// Verify that the supplied password can decrypt the backup (no extraction).
pub fn verify_decrypt(backup_root: &Path, password: &str) -> Result<VerifyResult> {
    ios_backup::verify_decrypt(backup_root, password)
}
