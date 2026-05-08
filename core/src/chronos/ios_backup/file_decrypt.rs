use aes::cipher::{BlockDecrypt, KeyInit};
use aes::Aes256;
use aes_kw::KekAes256;
use anyhow::{anyhow, Context, Result};
use plist::Value;
use rusqlite::Connection;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct FilePlist {
    pub filesize: u64,
    pub mtime: Option<i64>,
    pub protection_class: u32,
    pub encryption_key: Option<Vec<u8>>,
}

#[derive(Debug)]
pub struct ManifestEntry {
    pub file_id: String,
    pub domain: String,
    pub relative_path: String,
    pub file_plist: FilePlist,
}

#[derive(Debug, Default, Serialize)]
pub struct ExtractStats {
    pub extracted: usize,
    pub skipped: usize,
    pub errors: usize,
}

impl FilePlist {
    pub fn from_bplist(blob: &[u8]) -> Result<Self> {
        let value = read_plist_value(blob)?;
        let dict = value
            .as_dictionary()
            .ok_or_else(|| anyhow!("File plist is not a dictionary"))?;

        let objects = dict
            .get("$objects")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow!("File plist missing $objects"))?;

        let top = dict
            .get("$top")
            .and_then(|v| v.as_dictionary())
            .ok_or_else(|| anyhow!("File plist missing $top"))?;

        let root_uid = top
            .get("root")
            .and_then(uid_value)
            .ok_or_else(|| anyhow!("File plist missing root UID"))?;

        let data = objects
            .get(root_uid as usize)
            .and_then(|v| v.as_dictionary())
            .ok_or_else(|| anyhow!("File plist root object invalid"))?;

        let filesize = data
            .get("Size")
            .and_then(value_to_u64)
            .ok_or_else(|| anyhow!("File plist missing Size"))?;

        let mtime = data.get("LastModified").and_then(value_to_i64);

        let protection_class =
            data.get("ProtectionClass")
                .and_then(value_to_u64)
                .ok_or_else(|| anyhow!("File plist missing ProtectionClass"))? as u32;

        let encryption_key = if let Some(uid) = data.get("EncryptionKey").and_then(uid_value) {
            if let Some(obj) = objects.get(uid as usize).and_then(|v| v.as_dictionary()) {
                if let Some(data_val) = obj.get("NS.data").and_then(|v| v.as_data()) {
                    if data_val.len() > 4 {
                        Some(data_val[4..].to_vec())
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        Ok(FilePlist {
            filesize,
            mtime,
            protection_class,
            encryption_key,
        })
    }
}

pub fn find_entry(conn: &Connection, domain: &str, relative_path: &str) -> Result<ManifestEntry> {
    let mut stmt = conn.prepare(
        "SELECT fileID, domain, relativePath, file FROM Files WHERE flags=1 AND domain = ?1 AND relativePath = ?2 LIMIT 1",
    )?;
    let mut rows = stmt.query([domain, relative_path])?;
    let row = rows
        .next()?
        .ok_or_else(|| anyhow!("File not found in manifest"))?;
    let file_id: String = row.get(0)?;
    let domain: String = row.get(1)?;
    let relative_path: String = row.get(2)?;
    let file_blob: Vec<u8> = row.get(3)?;
    let file_plist = FilePlist::from_bplist(&file_blob)?;
    Ok(ManifestEntry {
        file_id,
        domain,
        relative_path,
        file_plist,
    })
}

pub fn extract_specs(
    backup_dir: &Path,
    conn: &Connection,
    class_keys: &BTreeMap<u32, Vec<u8>>,
    output_root: &Path,
    specs: &[crate::chronos::ios_backup::ExtractSpec],
    error_log_path: Option<&Path>,
    skip_log_path: Option<&Path>,
) -> Result<ExtractStats> {
    let mut stats = ExtractStats::default();
    let mut error_log = open_error_log(error_log_path);
    let mut skip_log = open_skip_log(skip_log_path);

    for spec in specs {
        let mut stmt = conn.prepare(
            "SELECT fileID, domain, relativePath, file FROM Files WHERE flags=1 AND relativePath LIKE ?1 AND domain LIKE ?2 ORDER BY domain, relativePath",
        )?;
        let mut rows =
            stmt.query([spec.relative_paths_like.as_str(), spec.domain_like.as_str()])?;
        while let Some(row) = rows.next()? {
            let file_id: String = row.get(0)?;
            let domain: String = row.get(1)?;
            let relative_path: String = row.get(2)?;
            let file_blob: Vec<u8> = row.get(3)?;
            let file_plist = match FilePlist::from_bplist(&file_blob) {
                Ok(p) => p,
                Err(err) => {
                    stats.errors += 1;
                    write_error(
                        &mut error_log,
                        &ErrorEntry {
                            file_id,
                            domain,
                            relative_path,
                            stage: "file_plist",
                            message: err.to_string(),
                        },
                    );
                    continue;
                }
            };

            let entry = ManifestEntry {
                file_id,
                domain,
                relative_path,
                file_plist,
            };

            match decrypt_entry_to_root(backup_dir, class_keys, output_root, &entry, spec) {
                Ok(ExtractOutcome::Extracted) => stats.extracted += 1,
                Ok(ExtractOutcome::Skipped(skip_entry)) => {
                    stats.skipped += 1;
                    write_skip(&mut skip_log, &skip_entry);
                }
                Err(err) => {
                    stats.errors += 1;
                    write_error(
                        &mut error_log,
                        &ErrorEntry {
                            file_id: entry.file_id.clone(),
                            domain: entry.domain.clone(),
                            relative_path: entry.relative_path.clone(),
                            stage: "decrypt",
                            message: format!("{:#}", err),
                        },
                    );
                }
            }
        }
    }

    Ok(stats)
}

pub fn decrypt_entry_to_root(
    backup_dir: &Path,
    class_keys: &BTreeMap<u32, Vec<u8>>,
    output_root: &Path,
    entry: &ManifestEntry,
    spec: &crate::chronos::ios_backup::ExtractSpec,
) -> Result<ExtractOutcome> {
    let domain = sanitize_domain(&entry.domain);
    let rel_path = sanitize_relative_path(&entry.relative_path);
    let mut dest = PathBuf::from(output_root);

    if spec.domain_subfolders {
        dest.push(domain);
    }
    if spec.preserve_folders {
        if let Some(parent) = rel_path.parent() {
            dest.push(parent);
        }
    }
    let filename = rel_path
        .file_name()
        .ok_or_else(|| anyhow!("Missing filename"))?;
    dest.push(filename);

    if spec.incremental && dest.exists() {
        let dest_mtime = file_mtime_secs(&dest);
        if should_skip_existing(dest_mtime, entry.file_plist.mtime) {
            return Ok(ExtractOutcome::Skipped(SkipEntry {
                file_id: entry.file_id.clone(),
                domain: entry.domain.clone(),
                relative_path: entry.relative_path.clone(),
                output_path: dest.display().to_string(),
                reason: "incremental_up_to_date",
                backup_mtime: entry.file_plist.mtime,
                dest_mtime,
            }));
        }
    }

    decrypt_entry_to_path(backup_dir, class_keys, entry, &dest)?;
    Ok(ExtractOutcome::Extracted)
}

pub fn decrypt_entry_to_path(
    backup_dir: &Path,
    class_keys: &BTreeMap<u32, Vec<u8>>,
    entry: &ManifestEntry,
    output_path: &Path,
) -> Result<()> {
    let source_path = file_blob_path(backup_dir, &entry.file_id)?;

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }

    if entry.file_plist.encryption_key.is_none() {
        fs::copy(&source_path, output_path).with_context(|| {
            format!(
                "Failed to copy {} -> {}",
                source_path.display(),
                output_path.display()
            )
        })?;
        return Ok(());
    }

    let class_key = class_keys
        .get(&entry.file_plist.protection_class)
        .ok_or_else(|| anyhow!("Missing class key {}", entry.file_plist.protection_class))?;
    let kek = KekAes256::try_from(class_key.as_slice())?;
    let file_key = kek.unwrap_vec(
        entry
            .file_plist
            .encryption_key
            .as_ref()
            .ok_or_else(|| anyhow!("Missing encryption key"))?,
    )?;

    decrypt_cbc_to_path(&source_path, output_path, &file_key).with_context(|| {
        format!(
            "Failed to decrypt {} -> {}",
            source_path.display(),
            output_path.display()
        )
    })?;
    Ok(())
}

pub fn decrypt_file_by_path(
    backup_dir: &Path,
    conn: &Connection,
    class_keys: &BTreeMap<u32, Vec<u8>>,
    domain: &str,
    relative_path: &str,
    output_path: &Path,
) -> Result<()> {
    let entry = find_entry(conn, domain, relative_path)?;
    decrypt_entry_to_path(backup_dir, class_keys, &entry, output_path)?;
    Ok(())
}

fn file_blob_path(backup_dir: &Path, file_id: &str) -> Result<PathBuf> {
    if file_id.len() < 2 {
        return Err(anyhow!("Invalid file ID"));
    }
    Ok(backup_dir.join(&file_id[0..2]).join(file_id))
}

fn sanitize_relative_path(rel: &str) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in Path::new(rel).components() {
        match comp {
            Component::Normal(part) => {
                let part_str = part.to_string_lossy();
                let safe = sanitize_component(&part_str);
                out.push(safe);
            }
            Component::CurDir => {}
            Component::ParentDir => {}
            Component::RootDir => {}
            Component::Prefix(_) => {}
        }
    }
    out
}

fn sanitize_domain(domain: &str) -> String {
    let cleaned: String = domain
        .chars()
        .map(|c| if c == '/' || c == '\\' { '_' } else { c })
        .collect();
    sanitize_component(&cleaned)
}

fn value_to_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Integer(i) => i
            .as_unsigned()
            .or_else(|| i.as_signed().and_then(|v| u64::try_from(v).ok())),
        Value::Real(f) => Some(*f as u64),
        _ => None,
    }
}

fn value_to_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Integer(i) => i
            .as_signed()
            .or_else(|| i.as_unsigned().and_then(|v| i64::try_from(v).ok())),
        Value::Real(f) => Some(*f as i64),
        _ => None,
    }
}

fn uid_value(value: &Value) -> Option<u64> {
    match value {
        Value::Uid(uid) => Some(uid.get()),
        _ => None,
    }
}

fn read_plist_value(blob: &[u8]) -> Result<Value> {
    let cursor = Cursor::new(blob);
    Ok(Value::from_reader(cursor)?)
}

fn decrypt_cbc_to_path(input: &Path, output: &Path, key: &[u8]) -> Result<()> {
    let mut reader = BufReader::new(File::open(input)?);
    let mut writer = BufWriter::new(File::create(output)?);

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

fn file_mtime_secs(path: &Path) -> Option<i64> {
    let Ok(meta) = fs::metadata(path) else {
        return None;
    };
    let Ok(modified) = meta.modified() else {
        return None;
    };
    system_time_to_secs(modified)
}

fn should_skip_existing(dest_mtime: Option<i64>, backup_mtime: Option<i64>) -> bool {
    let target_mtime = match backup_mtime {
        Some(v) => v,
        None => return false,
    };
    let dest_mtime = match dest_mtime {
        Some(v) => v,
        None => return false,
    };
    dest_mtime >= target_mtime
}

fn system_time_to_secs(time: SystemTime) -> Option<i64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs() as i64)
}

fn sanitize_component(value: &str) -> String {
    let mut out: String = value
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ => c,
        })
        .collect();

    while out.ends_with(' ') || out.ends_with('.') {
        out.pop();
    }

    if out.is_empty() {
        out = "_".to_string();
    }

    let stem = out.split('.').next().unwrap_or(&out);
    let upper = stem.to_ascii_uppercase();
    if is_reserved_device_name(&upper) {
        out = format!("_{}", out);
    }

    out
}

fn is_reserved_device_name(name: &str) -> bool {
    matches!(
        name,
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

#[derive(Debug, Serialize)]
struct ErrorEntry {
    file_id: String,
    domain: String,
    relative_path: String,
    stage: &'static str,
    message: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct SkipEntry {
    file_id: String,
    domain: String,
    relative_path: String,
    output_path: String,
    reason: &'static str,
    backup_mtime: Option<i64>,
    dest_mtime: Option<i64>,
}

fn open_error_log(path: Option<&Path>) -> Option<BufWriter<File>> {
    let path = path?;
    if let Some(parent) = path.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            eprintln!(
                "WARN: failed to create error log dir {}: {}",
                parent.display(),
                err
            );
            return None;
        }
    }
    match File::create(path) {
        Ok(file) => Some(BufWriter::new(file)),
        Err(err) => {
            eprintln!(
                "WARN: failed to create error log {}: {}",
                path.display(),
                err
            );
            None
        }
    }
}

fn open_skip_log(path: Option<&Path>) -> Option<BufWriter<File>> {
    let path = path?;
    if let Some(parent) = path.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            eprintln!(
                "WARN: failed to create skip log dir {}: {}",
                parent.display(),
                err
            );
            return None;
        }
    }
    match File::create(path) {
        Ok(file) => Some(BufWriter::new(file)),
        Err(err) => {
            eprintln!(
                "WARN: failed to create skip log {}: {}",
                path.display(),
                err
            );
            None
        }
    }
}

fn write_error(writer: &mut Option<BufWriter<File>>, entry: &ErrorEntry) {
    let Some(writer) = writer.as_mut() else {
        return;
    };
    if let Ok(line) = serde_json::to_string(entry) {
        let _ = writeln!(writer, "{}", line);
    }
}

fn write_skip(writer: &mut Option<BufWriter<File>>, entry: &SkipEntry) {
    let Some(writer) = writer.as_mut() else {
        return;
    };
    if let Ok(line) = serde_json::to_string(entry) {
        let _ = writeln!(writer, "{}", line);
    }
}

#[derive(Debug, Clone)]
pub enum ExtractOutcome {
    Extracted,
    Skipped(SkipEntry),
}
