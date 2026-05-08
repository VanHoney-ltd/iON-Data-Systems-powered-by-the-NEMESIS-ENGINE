//! iOS encrypted backup support for Chronos.
//!
//! Responsibilities:
//! - Parse Manifest.plist
//! - Derive backup keys (owner password only)
//! - Decrypt Manifest.db
//! - Decrypt individual backup blobs on demand

pub mod file_decrypt;
pub mod keybag;
pub mod keys;
pub mod manifest;

use anyhow::{anyhow, Result};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

#[derive(Debug, Clone, Serialize)]
pub struct ExtractSpec {
    pub relative_paths_like: String,
    pub domain_like: String,
    pub preserve_folders: bool,
    pub domain_subfolders: bool,
    pub incremental: bool,
}

#[derive(Debug, Serialize)]
pub struct ExtractResult {
    pub input: PathBuf,
    pub output: PathBuf,
    pub extracted: usize,
    pub skipped: usize,
    pub errors: usize,
}

#[derive(Debug, Serialize)]
pub struct VerifyResult {
    pub file_count: u64,
}

/// High-level verification entrypoint.
/// This should complete in seconds if the password is correct.
pub fn verify_decrypt(backup_dir: &Path, password: &str) -> Result<VerifyResult> {
    if password.is_empty() {
        return Err(anyhow!("Decrypt password is empty"));
    }
    let plist = manifest::load_manifest_plist(backup_dir)?;
    let keybag = keybag::parse_keybag(&plist.backup_keybag)?;
    let class_keys = keys::derive_class_keys(password, &keybag)?;

    let temp = NamedTempFile::new()?;
    let decrypted = manifest::decrypt_manifest_db_to(
        backup_dir,
        &class_keys,
        &plist.manifest_key,
        temp.path(),
    )?;
    let conn = decrypted.open_connection()?;
    let file_count = manifest::count_files(&conn)?;

    Ok(VerifyResult { file_count })
}

/// Decrypt specific files from an encrypted iTunes/Finder backup.
pub fn extract_from_encrypted_backup(
    encrypted_backup_root: &Path,
    output_root: &Path,
    password: &str,
    specs: &[ExtractSpec],
) -> Result<ExtractResult> {
    if password.is_empty() {
        return Err(anyhow!("Decrypt password is empty"));
    }
    if !encrypted_backup_root.is_dir() {
        return Err(anyhow!(
            "Encrypted backup path is not a directory: {}",
            encrypted_backup_root.display()
        ));
    }
    if specs.is_empty() {
        return Err(anyhow!("No extraction specs provided"));
    }

    std::fs::create_dir_all(output_root)?;

    let plist = manifest::load_manifest_plist(encrypted_backup_root)?;
    let keybag = keybag::parse_keybag(&plist.backup_keybag)?;
    let class_keys = keys::derive_class_keys(password, &keybag)?;

    let manifest_out = output_root.join("_manifest").join("Manifest.decrypted.db");
    let decrypted = manifest::decrypt_manifest_db_to(
        encrypted_backup_root,
        &class_keys,
        &plist.manifest_key,
        &manifest_out,
    )?;

    let conn = decrypted.open_connection()?;
    let error_log = output_root.join("_manifest").join("extract_errors.jsonl");
    let skip_log = output_root.join("_manifest").join("extract_skipped.jsonl");
    let stats = file_decrypt::extract_specs(
        encrypted_backup_root,
        &conn,
        &class_keys,
        output_root,
        specs,
        Some(&error_log),
        Some(&skip_log),
    )?;

    Ok(ExtractResult {
        input: encrypted_backup_root.to_path_buf(),
        output: output_root.to_path_buf(),
        extracted: stats.extracted,
        skipped: stats.skipped,
        errors: stats.errors,
    })
}

/// Decrypt the full backup into `output_root` (domain/relativePath preserved).
pub fn extract_full_backup(
    encrypted_backup_root: &Path,
    output_root: &Path,
    password: &str,
    incremental: bool,
) -> Result<ExtractResult> {
    let spec = ExtractSpec {
        relative_paths_like: "%".to_string(),
        domain_like: "%".to_string(),
        preserve_folders: true,
        domain_subfolders: true,
        incremental,
    };
    extract_from_encrypted_backup(encrypted_backup_root, output_root, password, &[spec])
}

pub fn decrypt_file_by_path(
    backup_dir: &Path,
    manifest_db: &Path,
    class_keys: &BTreeMap<u32, Vec<u8>>,
    domain: &str,
    relative_path: &str,
    output_path: &Path,
) -> Result<()> {
    let conn = manifest::DecryptedManifest {
        path: manifest_db.to_path_buf(),
    }
    .open_connection()?;
    file_decrypt::decrypt_file_by_path(
        backup_dir,
        &conn,
        class_keys,
        domain,
        relative_path,
        output_path,
    )
}
