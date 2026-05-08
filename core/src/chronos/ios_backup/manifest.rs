use aes::cipher::{BlockDecrypt, KeyInit};
use aes::Aes256;
use aes_kw::KekAes256;
use anyhow::{anyhow, Context, Result};
use plist::Value;
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use zeroize::Zeroize;

use super::file_decrypt::FilePlist;

#[derive(Debug, Clone)]
pub struct ManifestPlist {
    pub backup_keybag: Vec<u8>,
    pub manifest_key: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct DecryptedManifest {
    pub path: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct ManifestIndexEntry {
    pub file_id: String,
    pub domain: String,
    pub relative_path: String,
    pub flags: i64,
    pub size: Option<u64>,
    pub protection_class: Option<u32>,
    pub has_encryption_key: bool,
}

impl DecryptedManifest {
    pub fn open_connection(&self) -> Result<Connection> {
        Connection::open_with_flags(
            &self.path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .with_context(|| format!("Failed to open Manifest.db at {}", self.path.display()))
    }
}

pub fn load_manifest_plist(backup_dir: &Path) -> Result<ManifestPlist> {
    let plist_path = backup_dir.join("Manifest.plist");
    let data = fs::read(&plist_path)
        .with_context(|| format!("Failed to read {}", plist_path.display()))?;
    let value = read_plist_value(&data).context("Failed to parse Manifest.plist")?;

    let dict = value
        .as_dictionary()
        .ok_or_else(|| anyhow!("Manifest.plist is not a dictionary"))?;

    let backup_keybag = dict
        .get("BackupKeyBag")
        .and_then(|v| v.as_data())
        .ok_or_else(|| anyhow!("Manifest.plist missing BackupKeyBag"))?
        .to_vec();

    let manifest_key = dict
        .get("ManifestKey")
        .and_then(|v| v.as_data())
        .ok_or_else(|| anyhow!("Manifest.plist missing ManifestKey"))?
        .to_vec();

    Ok(ManifestPlist {
        backup_keybag,
        manifest_key,
    })
}

pub fn decrypt_manifest_db_to(
    backup_dir: &Path,
    class_keys: &BTreeMap<u32, Vec<u8>>,
    manifest_key: &[u8],
    output_path: &Path,
) -> Result<DecryptedManifest> {
    if manifest_key.len() < 5 {
        return Err(anyhow!("ManifestKey is too short"));
    }

    let mut class_bytes = [0u8; 4];
    class_bytes.copy_from_slice(&manifest_key[0..4]);
    let manifest_class = u32::from_le_bytes(class_bytes);
    let wrapped_manifest_key = &manifest_key[4..];

    let class_key = class_keys
        .get(&manifest_class)
        .ok_or_else(|| anyhow!("Missing class key for {}", manifest_class))?;
    let kek = KekAes256::try_from(class_key.as_slice())?;
    let mut manifest_file_key = kek.unwrap_vec(wrapped_manifest_key)?;

    let input_path = backup_dir.join("Manifest.db");
    if !input_path.is_file() {
        return Err(anyhow!("Manifest.db not found at {}", input_path.display()));
    }

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }

    decrypt_cbc_to_path(&input_path, output_path, &manifest_file_key)?;
    manifest_file_key.zeroize();

    validate_manifest_db(output_path)?;

    Ok(DecryptedManifest {
        path: output_path.to_path_buf(),
    })
}

pub fn validate_manifest_db(path: &Path) -> Result<()> {
    let mut file =
        File::open(path).with_context(|| format!("Failed to open {}", path.display()))?;
    let mut header = [0u8; 16];
    file.read_exact(&mut header)?;
    if &header != b"SQLite format 3\0" {
        return Err(anyhow!(
            "Decrypted Manifest.db does not look like SQLite (bad header)"
        ));
    }
    Ok(())
}

pub fn count_files(conn: &Connection) -> Result<u64> {
    let mut stmt = conn.prepare("SELECT COUNT(*) FROM Files WHERE flags=1")?;
    let count: i64 = stmt.query_row([], |row| row.get(0))?;
    Ok(count as u64)
}

pub fn build_index(conn: &Connection) -> Result<Vec<ManifestIndexEntry>> {
    let mut stmt = conn.prepare(
        "SELECT fileID, domain, relativePath, flags, file FROM Files WHERE flags=1 ORDER BY domain, relativePath",
    )?;
    let mut rows = stmt.query([])?;
    let mut entries = Vec::new();

    while let Some(row) = rows.next()? {
        let file_id: String = row.get(0)?;
        let domain: String = row.get(1)?;
        let relative_path: String = row.get(2)?;
        let flags: i64 = row.get(3)?;
        let file_blob: Vec<u8> = row.get(4)?;

        let plist = FilePlist::from_bplist(&file_blob).ok();
        let size = plist.as_ref().map(|p| p.filesize);
        let protection_class = plist.as_ref().map(|p| p.protection_class);
        let has_encryption_key = plist
            .as_ref()
            .map(|p| p.encryption_key.is_some())
            .unwrap_or(false);

        entries.push(ManifestIndexEntry {
            file_id,
            domain,
            relative_path,
            flags,
            size,
            protection_class,
            has_encryption_key,
        });
    }

    Ok(entries)
}

fn decrypt_cbc_to_path(input: &Path, output: &Path, key: &[u8]) -> Result<()> {
    let mut reader = BufReader::new(
        File::open(input).with_context(|| format!("Failed to open {}", input.display()))?,
    );
    let mut writer = BufWriter::new(
        File::create(output).with_context(|| format!("Failed to create {}", output.display()))?,
    );

    let cipher = Aes256::new_from_slice(key)?;
    let mut prev_block = [0u8; 16];
    let mut pending: Option<Vec<u8>> = None;
    let mut buf = vec![0u8; 1024 * 1024];

    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        if n % 16 != 0 {
            return Err(anyhow!("Encrypted data length is not a multiple of 16"));
        }
        for block in buf[..n].chunks_exact(16) {
            let mut block_buf = [0u8; 16];
            block_buf.copy_from_slice(block);
            let mut ga = block_buf.into();
            cipher.decrypt_block(&mut ga);
            for (i, b) in ga.iter_mut().enumerate() {
                *b ^= prev_block[i];
            }
            prev_block.copy_from_slice(block);

            if let Some(prev) = pending.take() {
                writer.write_all(&prev)?;
            }
            pending = Some(ga.to_vec());
        }
    }

    if let Some(last) = pending {
        if last.is_empty() {
            return Err(anyhow!("Missing final block"));
        }
        let pad = *last.last().unwrap() as usize;
        if pad == 0 || pad > 16 || pad > last.len() {
            return Err(anyhow!("Invalid PKCS7 padding"));
        }
        writer.write_all(&last[..last.len() - pad])?;
    }

    writer.flush()?;
    Ok(())
}

fn read_plist_value(data: &[u8]) -> Result<Value> {
    let cursor = Cursor::new(data);
    Ok(Value::from_reader(cursor)?)
}
