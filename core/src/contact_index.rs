use crate::case::{Case, CaseRegistryEntry};
use anyhow::{Context, Result};
use regex::Regex;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct ContactIndex {
    name_to_numbers: HashMap<String, HashSet<String>>,
    number_to_name: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct ContactRow {
    phone: String,
    #[serde(default)]
    name: String,
}

impl ContactIndex {
    pub fn load_for_case(case: &Case) -> Result<Self> {
        let mut idx = ContactIndex::default();

        if let Some(entry) = case.registry_entry()? {
            idx.ingest_registry(&entry);
        }

        // Primary contacts export
        let contacts_dir = case.root_path().join("evidence").join("contacts");
        idx.ingest_contacts_exports(&contacts_dir)?;

        // Derive from evidence filenames as a fallback (sms/mms/calls exports)
        idx.ingest_numbers_from_dir(&case.root_path().join("evidence"))?;

        // Persist what we discovered so other modules can reuse it without re-scanning.
        let all_numbers: Vec<String> = idx.number_to_name.keys().cloned().collect();
        if !all_numbers.is_empty() {
            let _ = case.remember_phone_numbers(&all_numbers);
        }
        for (name, nums) in &idx.name_to_numbers {
            let nums: Vec<String> = nums.iter().cloned().collect();
            let _ = case.remember_contact_mapping(name, &nums);
        }

        Ok(idx)
    }

    fn ingest_contacts_exports(&mut self, contacts_dir: &Path) -> Result<()> {
        self.ingest_contacts_csv(&contacts_dir.join("contacts_focus_numbers.csv"))?;
        self.ingest_contacts_csv(&contacts_dir.join("contacts.csv"))?;
        self.ingest_contacts_json(&contacts_dir.join("contacts_focus_numbers.json"))?;

        if !contacts_dir.exists() {
            return Ok(());
        }

        for entry in fs::read_dir(contacts_dir)
            .with_context(|| format!("Failed to read contacts dir {}", contacts_dir.display()))?
        {
            let path = entry?.path();
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            if !name.starts_with("contacts_") {
                continue;
            }
            match path.extension().and_then(|value| value.to_str()) {
                Some("csv") => self.ingest_contacts_csv(&path)?,
                Some("json") => self.ingest_contacts_json(&path)?,
                _ => {}
            }
        }

        Ok(())
    }

    fn ingest_registry(&mut self, entry: &CaseRegistryEntry) {
        for (name, numbers) in &entry.contacts {
            for n in numbers {
                self.register(name, n);
            }
        }
        for number in &entry.phone_numbers {
            self.register(number, number);
        }
    }

    fn ingest_contacts_csv(&mut self, path: &Path) -> Result<()> {
        if !path.exists() {
            return Ok(());
        }

        let mut rdr = csv::Reader::from_path(path)
            .with_context(|| format!("Failed to read contacts csv {}", path.display()))?;
        for row in rdr.deserialize::<ContactRow>().flatten() {
            self.register(&row.name, &row.phone);
        }
        Ok(())
    }

    fn ingest_contacts_json(&mut self, path: &Path) -> Result<()> {
        if !path.exists() {
            return Ok(());
        }
        let data = fs::read_to_string(path)
            .with_context(|| format!("Failed to read contacts json {}", path.display()))?;
        let rows: Vec<ContactRow> = serde_json::from_str(&data).unwrap_or_default();
        for row in rows {
            self.register(&row.name, &row.phone);
        }
        Ok(())
    }

    fn ingest_numbers_from_dir(&mut self, root: &Path) -> Result<()> {
        let patterns = ["sms_", "mms_", "call_history_"];
        let re = Regex::new(r"(\+?\d[\d\-]+)").unwrap();

        fn walk<F: FnMut(&Path)>(dir: &Path, depth: usize, cb: &mut F) {
            if depth == 0 || !dir.is_dir() {
                return;
            }
            if let Ok(rd) = fs::read_dir(dir) {
                for entry in rd.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        walk(&path, depth - 1, cb);
                    } else {
                        cb(&path);
                    }
                }
            }
        }

        let mut visit_file = |path: &Path| {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if patterns.iter().any(|p| name.starts_with(p)) {
                    if let Some(stem) = PathBuf::from(name).file_stem().and_then(|s| s.to_str()) {
                        for cap in re.captures_iter(stem) {
                            if let Some(m) = cap.get(1) {
                                self.register(stem, m.as_str());
                            }
                        }
                    }
                }
            }
        };

        walk(root, 4, &mut visit_file);
        Ok(())
    }

    fn register(&mut self, raw_name: &str, raw_phone: &str) {
        let phone = normalize_phone(raw_phone);
        if phone.is_empty() {
            return;
        }

        let name_key = normalize_name(raw_name);
        if !name_key.is_empty() {
            self.name_to_numbers
                .entry(name_key.clone())
                .or_default()
                .insert(phone.clone());
            self.number_to_name
                .entry(phone.clone())
                .or_insert_with(|| raw_name.trim().to_string());
        } else {
            self.number_to_name
                .entry(phone.clone())
                .or_insert_with(|| phone.clone());
        }
    }

    pub fn phones_for_name(&self, name: &str) -> Vec<String> {
        let key = normalize_name(name);
        match self.name_to_numbers.get(&key) {
            Some(set) => set.iter().cloned().collect(),
            None => Vec::new(),
        }
    }

    pub fn name_for_phone(&self, phone: &str) -> Option<String> {
        let normalized = normalize_phone(phone);
        self.number_to_name.get(&normalized).cloned()
    }

    pub fn known_numbers(&self) -> Vec<String> {
        self.number_to_name.keys().cloned().collect()
    }
}

/// Split free-form contact expressions such as ["john", "doe", "&&", "jane", "doe"]
/// into clean name tokens.
pub fn parse_contact_terms(args: &[String]) -> Vec<String> {
    if args.is_empty() {
        return Vec::new();
    }

    let joined = args.join(" ");
    joined
        .split("&&")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

pub fn normalize_phone(raw: &str) -> String {
    raw.chars().filter(|c| c.is_ascii_digit()).collect()
}

pub fn normalize_name(raw: &str) -> String {
    raw.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn ingests_wildcard_contact_exports() -> Result<()> {
        let dir = tempdir()?;
        let contacts_dir = dir.path().join("contacts");
        fs::create_dir_all(&contacts_dir)?;
        fs::write(
            contacts_dir.join("contacts_focus_numbers.csv"),
            "phone,name\n5157837352,Known Person\n",
        )?;
        fs::write(
            contacts_dir.join("contacts_manual_review.csv"),
            "phone,name\n5154994652,Manual Match\n",
        )?;

        let mut idx = ContactIndex::default();
        idx.ingest_contacts_exports(&contacts_dir)?;

        assert_eq!(
            idx.name_for_phone("5157837352").as_deref(),
            Some("Known Person")
        );
        assert_eq!(
            idx.name_for_phone("5154994652").as_deref(),
            Some("Manual Match")
        );
        Ok(())
    }
}
