use crate::case::Case;
use crate::common::target::{CandidateKind, CandidatePath, KnownTarget};
use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ResolveMethod {
    CleanCopy,
    HashedManifest,
    DirectTree,
    AlternateCandidate,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolvedPath {
    pub artifact_key: String,
    pub source_path: PathBuf,
    pub method: ResolveMethod,
    pub domain: String,
    pub relative_path: String,
    pub clean_file_name: Option<String>,
    pub attempted: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ResolverContext {
    pub backup_root: PathBuf,
    pub case_root: PathBuf,
    pub clean_root: PathBuf,
    pub manifest_db_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolverAuditRecord {
    pub artifact_key: String,
    pub candidates: Vec<String>,
    pub chosen: Option<String>,
    pub method: Option<String>,
}

pub trait ArtifactResolver {
    fn resolve_known_target(&self, target: &KnownTarget) -> Result<Option<ResolvedPath>>;
    fn resolve_exact(
        &self,
        artifact_key: &str,
        domain: &str,
        relative_path: &str,
    ) -> Result<Option<ResolvedPath>>;
}

#[derive(Debug, Clone)]
struct ManifestIndex {
    file_ids: HashMap<(String, String), String>,
}

pub struct BackupResolver {
    pub ctx: ResolverContext,
    manifest_index: Option<ManifestIndex>,
}

impl BackupResolver {
    pub fn new(ctx: ResolverContext) -> Self {
        let manifest_index = load_manifest_index(ctx.manifest_db_path.as_deref())
            .ok()
            .flatten();
        Self {
            ctx,
            manifest_index,
        }
    }

    pub fn from_case(case: &Case) -> Result<Self> {
        let backup_root = discover_backup_root(case)?;
        let manifest_db_path = backup_root
            .join("Manifest.db")
            .exists()
            .then(|| backup_root.join("Manifest.db"));
        Ok(Self::new(ResolverContext {
            backup_root,
            case_root: case.root_path(),
            clean_root: case.root_path().join("clean"),
            manifest_db_path,
        }))
    }

    pub fn clean_path_for(&self, target: &KnownTarget) -> PathBuf {
        self.ctx.clean_root.join(target.clean_path())
    }

    pub fn direct_tree_path(&self, domain: &str, relative_path: &str) -> PathBuf {
        self.ctx.backup_root.join(domain).join(relative_path)
    }

    pub fn manifest_hashed_path(&self, file_id: &str) -> PathBuf {
        let prefix = file_id.get(0..2).unwrap_or_default();
        self.ctx.backup_root.join(prefix).join(file_id)
    }

    pub fn backup_root(&self) -> &Path {
        &self.ctx.backup_root
    }

    pub fn case_root(&self) -> &Path {
        &self.ctx.case_root
    }

    pub fn clean_root(&self) -> &Path {
        &self.ctx.clean_root
    }

    pub fn resolve_from_clean(&self, target: &KnownTarget) -> Option<ResolvedPath> {
        let clean_path = self.clean_path_for(target);
        if clean_path.exists() {
            return Some(ResolvedPath {
                artifact_key: target.artifact_key.to_string(),
                source_path: clean_path.clone(),
                method: ResolveMethod::CleanCopy,
                domain: "clean".to_string(),
                relative_path: target.clean_file_name.to_string(),
                clean_file_name: Some(target.clean_file_name.to_string()),
                attempted: vec![clean_path],
            });
        }
        None
    }

    pub fn resolve_from_manifest_hash(&self, target: &KnownTarget) -> Result<Option<ResolvedPath>> {
        let mut attempted = Vec::new();

        for candidate in primary_candidates(&target.candidates) {
            if let Some(file_id) = self.lookup_manifest_file_id(candidate) {
                let hashed = self.manifest_hashed_path(&file_id);
                attempted.push(hashed.clone());
                if hashed.exists() {
                    return Ok(Some(ResolvedPath {
                        artifact_key: target.artifact_key.to_string(),
                        source_path: hashed,
                        method: ResolveMethod::HashedManifest,
                        domain: candidate.domain.clone(),
                        relative_path: candidate.relative_path.clone(),
                        clean_file_name: Some(target.clean_file_name.to_string()),
                        attempted,
                    }));
                }
            }
        }

        Ok(None)
    }

    pub fn resolve_from_direct_tree_candidate(&self, target: &KnownTarget) -> Option<ResolvedPath> {
        let mut attempted = Vec::new();

        for candidate in primary_candidates(&target.candidates) {
            let direct = self.direct_tree_path(&candidate.domain, &candidate.relative_path);
            attempted.push(direct.clone());
            if direct.exists() {
                return Some(ResolvedPath {
                    artifact_key: target.artifact_key.to_string(),
                    source_path: direct,
                    method: ResolveMethod::DirectTree,
                    domain: candidate.domain.clone(),
                    relative_path: candidate.relative_path.clone(),
                    clean_file_name: Some(target.clean_file_name.to_string()),
                    attempted,
                });
            }
        }

        None
    }

    pub fn search_direct_tree_file_by_name(
        &self,
        domain: &str,
        relative_base: &str,
        file_name: &str,
    ) -> Result<Option<PathBuf>> {
        let base = self.direct_tree_path(domain, relative_base);
        if !base.exists() {
            return Ok(None);
        }

        for entry in walkdir::WalkDir::new(&base)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }
            if entry.file_name().to_string_lossy() == file_name {
                return Ok(Some(entry.path().to_path_buf()));
            }
        }

        Ok(None)
    }

    fn resolve_alternate_candidate(&self, target: &KnownTarget) -> Option<ResolvedPath> {
        let mut attempted = Vec::new();

        for candidate in alternate_candidates(&target.candidates) {
            if let Some(file_id) = self.lookup_manifest_file_id(candidate) {
                let hashed = self.manifest_hashed_path(&file_id);
                attempted.push(hashed.clone());
                if hashed.exists() {
                    return Some(ResolvedPath {
                        artifact_key: target.artifact_key.to_string(),
                        source_path: hashed,
                        method: ResolveMethod::AlternateCandidate,
                        domain: candidate.domain.clone(),
                        relative_path: candidate.relative_path.clone(),
                        clean_file_name: Some(target.clean_file_name.to_string()),
                        attempted,
                    });
                }
            }

            let direct = self.direct_tree_path(&candidate.domain, &candidate.relative_path);
            attempted.push(direct.clone());
            if direct.exists() {
                return Some(ResolvedPath {
                    artifact_key: target.artifact_key.to_string(),
                    source_path: direct,
                    method: ResolveMethod::AlternateCandidate,
                    domain: candidate.domain.clone(),
                    relative_path: candidate.relative_path.clone(),
                    clean_file_name: Some(target.clean_file_name.to_string()),
                    attempted,
                });
            }
        }

        None
    }

    fn lookup_manifest_file_id(&self, candidate: &CandidatePath) -> Option<String> {
        self.manifest_index.as_ref().and_then(|manifest| {
            manifest
                .file_ids
                .get(&(candidate.domain.clone(), candidate.relative_path.clone()))
                .cloned()
        })
    }
}

impl ArtifactResolver for BackupResolver {
    fn resolve_known_target(&self, target: &KnownTarget) -> Result<Option<ResolvedPath>> {
        if let Some(clean) = self.resolve_from_clean(target) {
            return Ok(Some(clean));
        }

        if let Some(hashed) = self.resolve_from_manifest_hash(target)? {
            return Ok(Some(hashed));
        }

        if let Some(direct) = self.resolve_from_direct_tree_candidate(target) {
            return Ok(Some(direct));
        }

        Ok(self.resolve_alternate_candidate(target))
    }

    fn resolve_exact(
        &self,
        artifact_key: &str,
        domain: &str,
        relative_path: &str,
    ) -> Result<Option<ResolvedPath>> {
        let target = KnownTarget {
            artifact_key: "",
            clean_file_name: "",
            candidates: vec![CandidatePath {
                domain: domain.to_string(),
                relative_path: relative_path.to_string(),
                kind: CandidateKind::Primary,
            }],
            sqlite_like: false,
        };

        let mut resolved = match self.resolve_from_manifest_hash(&target)? {
            Some(found) => Some(found),
            None => self.resolve_from_direct_tree_candidate(&target),
        };

        if let Some(path) = &mut resolved {
            path.artifact_key = artifact_key.to_string();
            path.clean_file_name = None;
        }

        Ok(resolved)
    }
}

pub fn write_resolver_audit(case_root: &Path, record: &ResolverAuditRecord) -> Result<()> {
    let log_dir = case_root.join("logs");
    fs::create_dir_all(&log_dir)?;
    let path = log_dir.join(format!("resolver_{}.json", record.artifact_key));
    fs::write(path, serde_json::to_vec_pretty(record)?)?;
    Ok(())
}

pub fn discover_backup_root(case: &Case) -> Result<PathBuf> {
    let chronos_manifest = case.root_path().join("chronos_manifest.json");
    if chronos_manifest.exists() {
        if let Ok(raw) = fs::read(&chronos_manifest) {
            if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&raw) {
                if let Some(backup_root) = value.get("backup_root").and_then(|v| v.as_str()) {
                    let candidate = PathBuf::from(backup_root);
                    if candidate.exists() && looks_like_backup_root(&candidate) {
                        return Ok(candidate);
                    }
                }
            }
        }
    }

    let search_paths = vec![
        case.backup_path(),
        PathBuf::from("./backups"),
        dirs::home_dir()
            .unwrap_or_default()
            .join(".local/share/idevicebackup"),
    ];

    for base in search_paths {
        if !base.exists() {
            continue;
        }

        if looks_like_backup_root(&base) {
            return Ok(base);
        }

        if let Ok(entries) = fs::read_dir(&base) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() && looks_like_backup_root(&path) {
                    return Ok(path);
                }
            }
        }
    }

    Ok(case.backup_path())
}

fn looks_like_backup_root(path: &Path) -> bool {
    path.join("Manifest.db").exists()
}

fn load_manifest_index(path: Option<&Path>) -> Result<Option<ManifestIndex>> {
    let Some(path) = path else {
        return Ok(None);
    };

    if !path.exists() {
        return Ok(None);
    }

    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("Failed to open Manifest.db at {}", path.display()))?;

    let mut stmt = conn.prepare(
        "SELECT fileID, domain, relativePath FROM Files WHERE domain IS NOT NULL AND relativePath IS NOT NULL",
    )?;

    let rows = stmt.query_map([], |row| {
        let file_id: String = row.get(0)?;
        let domain: String = row.get(1)?;
        let relative_path: String = row.get(2)?;
        Ok((file_id, domain, relative_path))
    })?;

    let mut file_ids = HashMap::new();
    for row in rows {
        let (file_id, domain, relative_path) = row?;
        file_ids.insert((domain, relative_path), file_id);
    }

    Ok(Some(ManifestIndex { file_ids }))
}

fn primary_candidates(candidates: &[CandidatePath]) -> impl Iterator<Item = &CandidatePath> {
    candidates
        .iter()
        .filter(|candidate| candidate.kind == CandidateKind::Primary)
}

fn alternate_candidates(candidates: &[CandidatePath]) -> impl Iterator<Item = &CandidatePath> {
    candidates
        .iter()
        .filter(|candidate| candidate.kind == CandidateKind::Alternate)
}
