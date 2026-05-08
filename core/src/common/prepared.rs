use crate::chronos::wal::{ReplayResult, WalReplayer};
use crate::common::resolver::{ResolveMethod, ResolvedPath};
use anyhow::{anyhow, Context, Result};
use rusqlite::{Connection, OpenFlags};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreparedKind {
    PlainFile,
    SqliteLike,
}

#[derive(Debug, Clone)]
pub struct PreparedArtifact {
    pub artifact_key: String,
    pub source_path: PathBuf,
    pub working_path: PathBuf,
    pub resolution_method: ResolveMethod,
    pub kind: PreparedKind,
    pub sha256: Option<String>,
}

impl PreparedArtifact {
    pub fn open_sqlite_ro(&self) -> Result<Connection> {
        if self.kind != PreparedKind::SqliteLike {
            return Err(anyhow!("artifact {} is not SQLite-like", self.artifact_key));
        }

        Connection::open_with_flags(
            &self.working_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_URI,
        )
        .with_context(|| format!("Failed to open {}", self.working_path.display()))
    }
}

#[derive(Debug, Clone)]
pub struct PrepareContext {
    pub case_root: PathBuf,
    pub clean_root: PathBuf,
    pub temp_root: PathBuf,
}

pub fn prepare_artifact(
    ctx: &PrepareContext,
    resolved: &ResolvedPath,
    sqlite_like: bool,
) -> Result<PreparedArtifact> {
    if sqlite_like {
        prepare_sqlite_like(ctx, resolved)
    } else {
        prepare_plain_file(ctx, resolved)
    }
}

pub fn prepare_sqlite_like(
    ctx: &PrepareContext,
    resolved: &ResolvedPath,
) -> Result<PreparedArtifact> {
    fs::create_dir_all(&ctx.clean_root)?;
    let working_path = ctx.clean_root.join(file_name_for(
        resolved.clean_file_name.as_deref(),
        &resolved.source_path,
        &resolved.artifact_key,
    ));

    if resolved.method == ResolveMethod::CleanCopy && resolved.source_path == working_path {
        return Ok(PreparedArtifact {
            artifact_key: resolved.artifact_key.clone(),
            source_path: resolved.source_path.clone(),
            working_path: working_path.clone(),
            resolution_method: resolved.method,
            kind: PreparedKind::SqliteLike,
            sha256: Some(compute_sha256(&working_path)?),
        });
    }

    if let Some(parent) = working_path.parent() {
        fs::create_dir_all(parent)?;
    }

    remove_existing_sqlite_artifact(&working_path)?;

    if needs_sqlite_replay(&resolved.source_path)? {
        let wal_replayer = WalReplayer::new(300);
        match wal_replayer.replay_with_recovery(&resolved.source_path, &working_path) {
            Ok(ReplayResult::Success) | Ok(ReplayResult::PartialRecovery) => {}
            Ok(ReplayResult::Failed) => {
                copy_with_sidecars(&resolved.source_path, &working_path)?;
            }
            Err(_) => {
                copy_with_sidecars(&resolved.source_path, &working_path)?;
            }
        }
    } else {
        copy_main_file(&resolved.source_path, &working_path)?;
    }

    Ok(PreparedArtifact {
        artifact_key: resolved.artifact_key.clone(),
        source_path: resolved.source_path.clone(),
        working_path: working_path.clone(),
        resolution_method: resolved.method,
        kind: PreparedKind::SqliteLike,
        sha256: Some(compute_sha256(&working_path)?),
    })
}

pub fn prepare_plain_file(
    ctx: &PrepareContext,
    resolved: &ResolvedPath,
) -> Result<PreparedArtifact> {
    fs::create_dir_all(&ctx.temp_root)?;
    let working_path = ctx.temp_root.join(file_name_for(
        resolved.clean_file_name.as_deref(),
        &resolved.source_path,
        &resolved.artifact_key,
    ));

    if resolved.method == ResolveMethod::CleanCopy && resolved.source_path == working_path {
        return Ok(PreparedArtifact {
            artifact_key: resolved.artifact_key.clone(),
            source_path: resolved.source_path.clone(),
            working_path: working_path.clone(),
            resolution_method: resolved.method,
            kind: PreparedKind::PlainFile,
            sha256: Some(compute_sha256(&working_path)?),
        });
    }

    fs::copy(&resolved.source_path, &working_path).with_context(|| {
        format!(
            "Failed to copy {} to {}",
            resolved.source_path.display(),
            working_path.display()
        )
    })?;

    Ok(PreparedArtifact {
        artifact_key: resolved.artifact_key.clone(),
        source_path: resolved.source_path.clone(),
        working_path: working_path.clone(),
        resolution_method: resolved.method,
        kind: PreparedKind::PlainFile,
        sha256: Some(compute_sha256(&working_path)?),
    })
}

pub fn compute_sha256(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)
        .with_context(|| format!("Failed to open {} for hashing", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0_u8; 8192];

    loop {
        let read = file.read(&mut buf)?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

fn copy_with_sidecars(src: &Path, dst: &Path) -> Result<()> {
    copy_main_file(src, dst)?;

    for suffix in ["-wal", "-shm", "-journal"] {
        let sidecar = PathBuf::from(format!("{}{}", src.display(), suffix));
        if sidecar.exists() {
            let sidecar_dst = PathBuf::from(format!("{}{}", dst.display(), suffix));
            fs::copy(&sidecar, &sidecar_dst).with_context(|| {
                format!(
                    "Failed to copy {} to {}",
                    sidecar.display(),
                    sidecar_dst.display()
                )
            })?;
        }
    }

    Ok(())
}

fn copy_main_file(src: &Path, dst: &Path) -> Result<()> {
    fs::copy(src, dst)
        .with_context(|| format!("Failed to copy {} to {}", src.display(), dst.display()))?;
    Ok(())
}

fn needs_sqlite_replay(path: &Path) -> Result<bool> {
    for suffix in ["-wal", "-journal"] {
        let sidecar = PathBuf::from(format!("{}{}", path.display(), suffix));
        if !sidecar.exists() {
            continue;
        }

        let size = fs::metadata(&sidecar)
            .with_context(|| format!("Failed to stat {}", sidecar.display()))?
            .len();
        if size > 0 {
            return Ok(true);
        }
    }

    Ok(false)
}

fn remove_existing_sqlite_artifact(path: &Path) -> Result<()> {
    for candidate in [
        path.to_path_buf(),
        PathBuf::from(format!("{}-wal", path.display())),
        PathBuf::from(format!("{}-shm", path.display())),
        PathBuf::from(format!("{}-journal", path.display())),
    ] {
        if candidate.exists() {
            fs::remove_file(&candidate)
                .with_context(|| format!("Failed to remove {}", candidate.display()))?;
        }
    }

    Ok(())
}

fn file_name_for(clean_file_name: Option<&str>, source_path: &Path, artifact_key: &str) -> PathBuf {
    clean_file_name
        .map(PathBuf::from)
        .or_else(|| source_path.file_name().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from(format!("{artifact_key}.bin")))
}
