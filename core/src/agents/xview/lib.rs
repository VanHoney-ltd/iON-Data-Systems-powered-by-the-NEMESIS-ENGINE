// path: iON/crates/iON_core/src/lib.rs
//! Core primitives: entitlements, audit-chain, hashing, signature envelopes, and case paths.

use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use bitflags::bitflags;
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use zeroize::Zeroize;

pub fn sha256_bytes(data: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(data);
    h.finalize().into()
}

pub fn sha256_file(path: &Path) -> Result<[u8; 32]> {
    let b = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    Ok(sha256_bytes(&b))
}

pub fn hex32(h: [u8; 32]) -> String {
    hex::encode(h)
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountTier {
    Basic,
    Plus,
    Enterprise,
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Capabilities: u64 {
        const BASIC_DB_DUMP      = 1 << 0;
        const DB_ADVANCED        = 1 << 1; // WAL capture, immutable open, page hashing, strict ordering
        const CASE_VERIFY        = 1 << 2;
        const CASE_SIGN_VERIFY   = 1 << 3;
        const MOUNT_READONLY     = 1 << 4;
        const REPORT_GENERATE    = 1 << 5;
    }
}

impl Capabilities {
    pub fn for_tier(tier: AccountTier) -> Self {
        match tier {
            AccountTier::Basic => Capabilities::BASIC_DB_DUMP | Capabilities::CASE_VERIFY,
            AccountTier::Plus => Capabilities::BASIC_DB_DUMP | Capabilities::CASE_VERIFY | Capabilities::REPORT_GENERATE,
            AccountTier::Enterprise => Capabilities::BASIC_DB_DUMP
                | Capabilities::DB_ADVANCED
                | Capabilities::CASE_VERIFY
                | Capabilities::CASE_SIGN_VERIFY
                | Capabilities::MOUNT_READONLY
                | Capabilities::REPORT_GENERATE,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct License {
    pub schema: String,
    pub customer_id: String,
    pub tier: AccountTier,
    pub issued_utc: DateTime<Utc>,
    pub expires_utc: DateTime<Utc>,
    /// Optional extra caps beyond tier baseline (string names).
    pub extra_caps: Vec<String>,
    /// Optional signature over the canonical license JSON (without `signature_b64`).
    pub signature_b64: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Entitlements {
    pub tier: AccountTier,
    pub caps: Capabilities,
    pub customer_id: String,
}

impl Entitlements {
    pub fn require(&self, cap: Capabilities) -> Result<()> {
        if self.caps.contains(cap) {
            Ok(())
        } else {
            Err(anyhow!("feature not allowed for tier={:?}: missing {:?}", self.tier, cap))
        }
    }
}

pub fn load_license(path: &Path) -> Result<License> {
    let s = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let lic: License = serde_json::from_str(&s).context("parse license json")?;
    Ok(lic)
}

/// Verifies signature if present. If you don't want signatures yet, pass None for public key.
pub fn entitlements_from_license(lic: &License, public_key_b64: Option<&str>) -> Result<Entitlements> {
    let now = Utc::now();
    if now > lic.expires_utc {
        return Err(anyhow!("license expired at {}", lic.expires_utc));
    }

    if let (Some(sig_b64), Some(pk_b64)) = (lic.signature_b64.as_deref(), public_key_b64) {
        verify_license_signature(lic, pk_b64, sig_b64)?;
    }

    let mut caps = Capabilities::for_tier(lic.tier);
    for c in &lic.extra_caps {
        match c.as_str() {
            "db_advanced" => caps |= Capabilities::DB_ADVANCED,
            "mount_readonly" => caps |= Capabilities::MOUNT_READONLY,
            "case_sign_verify" => caps |= Capabilities::CASE_SIGN_VERIFY,
            "report_generate" => caps |= Capabilities::REPORT_GENERATE,
            _ => {}
        }
    }

    Ok(Entitlements {
        tier: lic.tier,
        caps,
        customer_id: lic.customer_id.clone(),
    })
}

fn verify_license_signature(lic: &License, public_key_b64: &str, sig_b64: &str) -> Result<()> {
    let pk_bytes = STANDARD.decode(public_key_b64).context("decode public key b64")?;
    let sig_bytes = STANDARD.decode(sig_b64).context("decode signature b64")?;

    let pk = VerifyingKey::from_bytes(
        pk_bytes
            .as_slice()
            .try_into()
            .map_err(|_| anyhow!("invalid public key length"))?,
    );
    let sig = Signature::from_slice(&sig_bytes).map_err(|_| anyhow!("invalid signature bytes"))?;

    // Canonicalize license bytes WITHOUT signature field.
    let mut lic_clone = lic.clone();
    lic_clone.signature_b64 = None;
    let bytes = serde_json::to_vec(&lic_clone).context("canonicalize license")?;
    pk.verify(&bytes, &sig).map_err(|_| anyhow!("license signature verification failed"))?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub seq: u64,
    pub ts_utc: DateTime<Utc>,
    pub actor: String,
    pub command: String,
    pub params: BTreeMap<String, String>,
    pub inputs_sha256: Vec<String>,
    pub outputs_sha256: Vec<String>,
    pub prev_entry_sha256: Option<String>,
    pub entry_sha256: String,
}

fn audit_hash_entry(mut e: AuditEntry) -> Result<AuditEntry> {
    e.entry_sha256.clear();
    let bytes = serde_json::to_vec(&e)?;
    let h = sha256_bytes(&bytes);
    e.entry_sha256 = hex32(h);
    Ok(e)
}

pub struct AuditLog {
    path: PathBuf,
    seq: u64,
    last_hash: Option<String>,
}

impl AuditLog {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        Ok(Self {
            path,
            seq: 0,
            last_hash: None,
        })
    }

    pub fn append(
        &mut self,
        actor: impl Into<String>,
        command: impl Into<String>,
        params: BTreeMap<String, String>,
        inputs_sha256: Vec<String>,
        outputs_sha256: Vec<String>,
    ) -> Result<AuditEntry> {
        self.seq += 1;
        let entry = AuditEntry {
            seq: self.seq,
            ts_utc: Utc::now(),
            actor: actor.into(),
            command: command.into(),
            params,
            inputs_sha256,
            outputs_sha256,
            prev_entry_sha256: self.last_hash.clone(),
            entry_sha256: String::new(),
        };
        let entry = audit_hash_entry(entry)?;
        self.last_hash = Some(entry.entry_sha256.clone());

        let mut f = fs::OpenOptions::new().create(true).append(true).open(&self.path)?;
        serde_json::to_writer(&mut f, &entry)?;
        f.write_all(b"\n")?;
        Ok(entry)
    }

    pub fn last_hash(&self) -> Option<&str> {
        self.last_hash.as_deref()
    }
}

pub struct CasePaths;

impl CasePaths {
    pub fn meta_dir(case_root: &Path) -> PathBuf {
        case_root.join("meta")
    }
    pub fn manifest(case_root: &Path) -> PathBuf {
        Self::meta_dir(case_root).join("evidence_manifest.json")
    }
    pub fn audit_chain(case_root: &Path) -> PathBuf {
        Self::meta_dir(case_root).join("audit_chain.jsonl")
    }
    pub fn signature(case_root: &Path) -> PathBuf {
        Self::meta_dir(case_root).join("signature.json")
    }
    pub fn derived(case_root: &Path) -> PathBuf {
        case_root.join("derived")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolIdentity {
    pub name: String,
    pub version: String,
    pub build_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactRecord {
    pub id: String,
    pub path: String, // relative
    pub kind: String,
    pub size: u64,
    pub sha256: String,
    pub created_utc: DateTime<Utc>,
    pub producer: BTreeMap<String, String>,
    pub provenance: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceManifest {
    pub version: String,
    pub case_id: String,
    pub created_utc: DateTime<Utc>,
    pub tool: ToolIdentity,
    pub artifacts: Vec<ArtifactRecord>,
    pub audit_chain_sha256: String,
    pub manifest_sha256: Option<String>,
}

impl EvidenceManifest {
    pub fn compute_self_hash(&mut self) -> Result<String> {
        self.manifest_sha256 = None;
        let bytes = serde_json::to_vec(self)?;
        let h = sha256_bytes(&bytes);
        let hex = hex32(h);
        self.manifest_sha256 = Some(hex.clone());
        Ok(hex)
    }

    pub fn write_pretty(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }
}

/// Best-effort: zeroize sensitive material (e.g., password buffers) you pass here.
pub fn zeroize_string(mut s: String) {
    s.zeroize();
}
