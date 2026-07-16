use anyhow::{Context, Result};
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Case {
    pub name: String,
    root: PathBuf,
}

impl Case {
    pub fn new(name: impl Into<String>) -> Result<Self> {
        let name = name.into();
        let root = locate_case_root(&name)?;

        // Persist the resolved root so future invocations from any directory reuse it.
        persist_case_root(&name, &root)?;

        Ok(Self { name, root })
    }

    pub fn from_root(name: impl Into<String>, root: impl Into<PathBuf>) -> Self {
        let name = name.into();
        let root = root.into();
        let _ = persist_case_root(&name, &root);
        Self { name, root }
    }

    pub fn open(&self, module: &str) -> Result<()> {
        self.ensure_base_dirs()?;
        fs::create_dir_all(self.path(module))?;
        self.log(&format!("{} initialized", module.to_uppercase()), None)?;
        Ok(())
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn root_path(&self) -> PathBuf {
        self.root.clone()
    }

    pub fn workspace(&self) -> CaseWorkspace {
        CaseWorkspace::new(self.root.clone())
    }

    pub fn backup_path(&self) -> PathBuf {
        self.root.join("backup")
    }

    pub fn ion_backup_path(&self) -> Result<PathBuf> {
        if let Ok(dir) = std::env::var("iON_BACKUP_DIR") {
            return Ok(PathBuf::from(dir).join("cases").join(&self.name));
        }
        let home =
            dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
        Ok(home
            .join("iON")
            .join("backups")
            .join("cases")
            .join(&self.name))
    }

    pub fn source_backup_root(&self) -> Result<PathBuf> {
        for candidate in [
            self.root.join("source").join("backup"),
            self.root.join("backup"),
        ] {
            if candidate.exists() {
                return Ok(candidate);
            }
        }

        Err(anyhow::anyhow!(
            "No source backup root found under {}",
            self.root.display()
        ))
    }

    pub fn discovered_helios_roots(&self) -> Result<Vec<PathBuf>> {
        let mut discovered = Vec::new();
        let mut seen = HashSet::new();
        let workspace = self.workspace();

        for candidate in [
            dirs::home_dir()
                .map(|home| {
                    home.join("iON")
                        .join("prepared")
                        .join("helios")
                        .join(&self.name)
                })
                .unwrap_or_else(|| PathBuf::from("./prepared/helios").join(&self.name)),
            workspace.helios_full_root(),
            workspace.helios_sms_only_root(),
            self.root.join("prepared"),
            self.root.join("source").join("imports"),
            self.root.clone(),
        ] {
            collect_helios_extract_roots(&candidate, &mut discovered, &mut seen);
        }

        Ok(discovered)
    }

    pub fn active_backup_root(&self) -> Result<PathBuf> {
        if let Some(root) = self.discovered_helios_roots()?.into_iter().next() {
            return Ok(root);
        }

        self.source_backup_root()
    }

    pub fn evidence_path(&self, subdir: &str) -> PathBuf {
        self.root.join("evidence").join(subdir)
    }

    pub fn path(&self, subdir: &str) -> PathBuf {
        self.root.join(subdir)
    }

    pub fn write_file(&self, path: &Path, data: Vec<u8>) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, data).with_context(|| format!("Failed to write {}", path.display()))
    }

    pub fn log(&self, message: &str, context: Option<&str>) -> Result<()> {
        let logs_dir = self.root.join("logs");
        fs::create_dir_all(&logs_dir)?;
        let log_path = logs_dir.join("case.log");
        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .with_context(|| format!("Failed to open log {}", log_path.display()))?;

        match context {
            Some(ctx) => writeln!(file, "[{}] {} | {}", timestamp, message, ctx)?,
            None => writeln!(file, "[{}] {}", timestamp, message)?,
        }

        Ok(())
    }

    fn ensure_base_dirs(&self) -> Result<()> {
        fs::create_dir_all(&self.root)?;
        fs::create_dir_all(self.root.join("logs"))?;
        fs::create_dir_all(self.root.join("evidence"))?;
        fs::create_dir_all(self.root.join("intake"))?;
        fs::create_dir_all(self.root.join("backup"))?;
        fs::create_dir_all(self.root.join("clean"))?;
        Ok(())
    }

    /// Persist phone numbers associated with this case so other modules can auto-resolve them.
    pub fn remember_phone_numbers<S: AsRef<str>>(&self, phones: &[S]) -> Result<()> {
        let mut entry = load_registry_entry(&self.name)?;
        entry.root = self.root.to_string_lossy().to_string();

        for phone in phones {
            let normalized = normalize_phone(phone.as_ref());
            if normalized.is_empty() {
                continue;
            }
            if !entry.phone_numbers.iter().any(|p| p == &normalized) {
                entry.phone_numbers.push(normalized);
            }
        }

        persist_registry_entry(&self.name, &entry)
    }

    /// Persist a friendly contact name -> phone numbers mapping for this case.
    pub fn remember_contact_mapping<S: AsRef<str>>(&self, name: &str, phones: &[S]) -> Result<()> {
        let mut entry = load_registry_entry(&self.name)?;
        entry.root = self.root.to_string_lossy().to_string();

        let key = normalize_name(name);
        let mut existing: HashSet<String> = entry
            .contacts
            .get(&key)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .collect();

        for phone in phones {
            let normalized = normalize_phone(phone.as_ref());
            if !normalized.is_empty() {
                existing.insert(normalized);
            }
        }

        if !existing.is_empty() {
            entry.contacts.insert(key, existing.into_iter().collect());
        }

        persist_registry_entry(&self.name, &entry)
    }

    /// Read the stored registry entry for this case, if any.
    pub fn registry_entry(&self) -> Result<Option<CaseRegistryEntry>> {
        let entry = load_registry_entry(&self.name)?;
        if entry.root.is_empty() {
            return Ok(None);
        }
        Ok(Some(entry))
    }

    /// Get the device phone number stored for this case, if any.
    pub fn device_phone_number(&self) -> Result<Option<String>> {
        let entry = load_registry_entry(&self.name)?;
        Ok(entry.device_phone_number)
    }

    /// Store the device phone number for this case.
    pub fn set_device_phone_number(&self, phone: &str) -> Result<()> {
        let mut entry = load_registry_entry(&self.name)?;
        entry.root = self.root.to_string_lossy().to_string();
        let normalized = normalize_phone(phone);
        entry.device_phone_number = if normalized.is_empty() {
            None
        } else {
            Some(normalized)
        };
        persist_registry_entry(&self.name, &entry)
    }
}

#[derive(Clone, Debug)]
pub struct CaseWorkspace {
    root: PathBuf,
}

impl CaseWorkspace {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn ensure_layout(&self) -> Result<()> {
        for dir in [
            self.logs_dir(),
            self.reports_dir(),
            self.prepared_database_root(),
            self.prepared_helios_dir(),
            self.helios_full_root(),
            self.helios_sms_only_root(),
            self.helios_metadata_root(),
            self.index_dir(),
            self.live_device_mounts_dir(),
            self.root.join("source").join("backup"),
            self.root.join("evidence"),
        ] {
            fs::create_dir_all(dir)?;
        }

        Ok(())
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    pub fn reports_dir(&self) -> PathBuf {
        self.root.join("reports")
    }

    pub fn prepared_database_root(&self) -> PathBuf {
        self.root.join("prepared").join("db")
    }

    pub fn prepared_helios_dir(&self) -> PathBuf {
        self.root.join("prepared").join("helios")
    }

    pub fn index_dir(&self) -> PathBuf {
        self.root.join("index")
    }

    pub fn live_device_mounts_dir(&self) -> PathBuf {
        self.root.join("tmp").join("device-mounts")
    }

    pub fn helios_full_root(&self) -> PathBuf {
        self.prepared_helios_dir().join("full")
    }

    pub fn helios_sms_only_root(&self) -> PathBuf {
        self.prepared_helios_dir().join("sms_only")
    }

    pub fn helios_metadata_root(&self) -> PathBuf {
        self.prepared_helios_dir().join("metadata")
    }
}

fn collect_helios_extract_roots(base: &Path, out: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>) {
    if !base.exists() {
        return;
    }

    if looks_like_helios_extract(base) {
        let candidate = base.to_path_buf();
        if seen.insert(candidate.clone()) {
            out.push(candidate);
        }
        return;
    }

    if let Ok(entries) = fs::read_dir(base) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            if looks_like_helios_extract(&path) {
                if seen.insert(path.clone()) {
                    out.push(path);
                }
                continue;
            }

            if let Ok(children) = fs::read_dir(&path) {
                for child in children.flatten() {
                    let child_path = child.path();
                    if child_path.is_dir()
                        && looks_like_helios_extract(&child_path)
                        && seen.insert(child_path.clone())
                    {
                        out.push(child_path);
                    }
                }
            }
        }
    }
}

fn looks_like_helios_extract(path: &Path) -> bool {
    path.join("_manifest")
        .join("Manifest.decrypted.db")
        .exists()
        || path.join("Manifest.decrypted.db").exists()
        || path.join("HomeDomain").exists()
        || path.join("RootDomain").exists()
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct CaseRegistryEntry {
    pub root: String,
    pub phone_numbers: Vec<String>,
    pub contacts: HashMap<String, Vec<String>>,
    /// The phone number of the device owner (the acquired device).
    /// Used for sent-message attribution in Cerberus and other modules.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_phone_number: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct CaseRegistry {
    cases: HashMap<String, CaseRegistryEntry>,
}

fn locate_case_root(name: &str) -> Result<PathBuf> {
    // Priority order:
    // 1) Previously remembered registry entry
    // 2) Environment hints (iON_CASE_ROOTS / iON_HOME)
    // 3) Common install locations (home, cwd, parents)
    // 4) Fallback to ./cases/<name>
    let remembered_root = load_registry_entry(name).ok().and_then(|e| {
        if !e.root.is_empty() {
            let p = PathBuf::from(&e.root);
            if p.exists() {
                return Some(p);
            }
        }
        None
    });

    let mut candidates: Vec<PathBuf> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();

    let mut push_candidate = |p: PathBuf| {
        if seen.insert(p.clone()) {
            candidates.push(p);
        }
    };

    // Environment provided roots (supports multiple, separated by OS path separator or comma/semicolon).
    if let Ok(var) = std::env::var("iON_CASE_ROOTS") {
        for p in split_paths_flexible(&var) {
            push_candidate(p);
        }
    }
    if let Ok(var) = std::env::var("iON_HOME") {
        push_candidate(PathBuf::from(var));
    }
    if let Ok(var) = std::env::var("iON_CASE_ROOT") {
        push_candidate(PathBuf::from(var));
    }

    if let Some(home) = dirs::home_dir() {
        push_candidate(home.clone());
        push_candidate(home.join("iON").join("backups").join("cases"));
        push_candidate(home.join("iON").join("cases"));
    }

    // Current dir and a few parents
    if let Ok(cwd) = std::env::current_dir() {
        let mut cur = Some(cwd.as_path());
        for _ in 0..4 {
            if let Some(p) = cur {
                push_candidate(p.to_path_buf());
                cur = p.parent();
            }
        }
    }

    let discovered_root = find_case_root_in_bases(name, &candidates);

    if let Some(root) = select_preferred_case_root(name, remembered_root, discovered_root) {
        return Ok(root);
    }

    // Nothing found; default to ./cases/<name> next to cwd.
    Ok(std::env::current_dir()?.join("cases").join(name))
}

fn find_case_root_in_bases(name: &str, bases: &[PathBuf]) -> Option<PathBuf> {
    for base in bases {
        for path in candidate_case_paths(base, name) {
            if path.exists() && path.is_dir() {
                return Some(path);
            }
        }
    }
    None
}

fn candidate_case_paths(base: &Path, name: &str) -> [PathBuf; 4] {
    [
        base.join("cases").join(name),
        base.join("output").join("iON").join(name),
        base.join(name).join("cases"),
        base.join(name),
    ]
}

fn select_preferred_case_root(
    name: &str,
    remembered_root: Option<PathBuf>,
    discovered_root: Option<PathBuf>,
) -> Option<PathBuf> {
    match (remembered_root, discovered_root) {
        (Some(remembered), Some(discovered))
            if is_ion_backup_case_root(&discovered, name) && remembered != discovered =>
        {
            Some(discovered)
        }
        (Some(remembered), Some(discovered))
            if is_output_case_root(&remembered, name) && remembered != discovered =>
        {
            Some(discovered)
        }
        (Some(remembered), _) => Some(remembered),
        (None, Some(discovered)) => Some(discovered),
        (None, None) => None,
    }
}

fn is_ion_backup_case_root(path: &Path, name: &str) -> bool {
    let components: Vec<String> = path
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect();

    components.len() >= 4
        && components[components.len() - 1] == name
        && components[components.len() - 2] == "cases"
        && components[components.len() - 3] == "backups"
        && components[components.len() - 4] == "iON"
}

fn is_output_case_root(path: &Path, name: &str) -> bool {
    let components: Vec<String> = path
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect();

    components.len() >= 3
        && components[components.len() - 1] == name
        && components[components.len() - 2] == "iON"
        && components[components.len() - 3] == "output"
}

fn registry_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".iON")
        .join("case_registry.json")
}

fn load_registry_entry(name: &str) -> Result<CaseRegistryEntry> {
    let reg = load_registry()?;
    Ok(reg.cases.get(name).cloned().unwrap_or_default())
}

fn persist_registry_entry(name: &str, entry: &CaseRegistryEntry) -> Result<()> {
    let mut reg = load_registry()?;
    reg.cases.insert(name.to_string(), entry.clone());
    save_registry(&reg)
}

fn load_registry() -> Result<CaseRegistry> {
    let path = registry_path();
    if !path.exists() {
        return Ok(CaseRegistry::default());
    }

    let data = fs::read(&path)?;
    let reg: CaseRegistry = serde_json::from_slice(&data).unwrap_or_default();
    Ok(reg)
}

fn save_registry(reg: &CaseRegistry) -> Result<()> {
    let path = registry_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let data = serde_json::to_vec_pretty(reg)?;
    fs::write(&path, data)?;
    Ok(())
}

fn persist_case_root(case: &str, root: &Path) -> Result<()> {
    let mut entry = load_registry_entry(case)?;
    entry.root = root.to_string_lossy().to_string();
    persist_registry_entry(case, &entry)
}

fn split_paths_flexible(raw: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for chunk in raw.split(',') {
        for p in std::env::split_paths(chunk) {
            if !p.as_os_str().is_empty() {
                out.push(p);
            }
        }
    }
    out
}

fn normalize_phone(raw: &str) -> String {
    raw.chars().filter(|c| c.is_ascii_digit()).collect()
}

fn normalize_name(raw: &str) -> String {
    raw.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_real_case_root_over_output_registry_root() {
        let remembered = Some(PathBuf::from("/tmp/output/iON/Case-001"));
        let discovered = Some(PathBuf::from("/tmp/cases/Case-001"));

        let resolved =
            select_preferred_case_root("Case-001", remembered, discovered).expect("expected root");

        assert_eq!(resolved, PathBuf::from("/tmp/cases/Case-001"));
    }

    #[test]
    fn preserves_non_output_registry_root() {
        let remembered = Some(PathBuf::from("/tmp/custom/Case-001"));
        let discovered = Some(PathBuf::from("/tmp/cases/Case-001"));

        let resolved =
            select_preferred_case_root("Case-001", remembered, discovered).expect("expected root");

        assert_eq!(resolved, PathBuf::from("/tmp/custom/Case-001"));
    }
}
