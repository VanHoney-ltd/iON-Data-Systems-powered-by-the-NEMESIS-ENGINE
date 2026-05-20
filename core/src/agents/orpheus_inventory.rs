//! Orpheus app/container inventory over HELiOS reconstructed backups.

use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize)]
pub struct AppInventoryRecord {
    pub bundle_id: String,
    pub display_name: String,
    pub container_kind: String,
    pub container_path: String,
    pub file_count: u64,
    pub total_bytes: u64,
    pub database_count: usize,
    pub plist_count: usize,
    pub categories: Vec<String>,
    pub high_value: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContainerInventoryRecord {
    pub name: String,
    pub kind: String,
    pub path: String,
    pub bundle_id: Option<String>,
    pub file_count: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DatabaseInventoryRecord {
    pub path: String,
    pub container: String,
    pub bundle_id: Option<String>,
    pub file_name: String,
    pub size_bytes: u64,
    pub category_hints: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlistInventoryRecord {
    pub path: String,
    pub container: String,
    pub bundle_id: Option<String>,
    pub file_name: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct HighValueTargetRecord {
    pub bundle_id: String,
    pub display_name: String,
    pub categories: Vec<String>,
    pub reason: String,
    pub container_path: String,
    pub database_count: usize,
    pub plist_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct OrpheusInventory {
    pub apps: Vec<AppInventoryRecord>,
    pub containers: Vec<ContainerInventoryRecord>,
    pub databases: Vec<DatabaseInventoryRecord>,
    pub plists: Vec<PlistInventoryRecord>,
    pub high_value_targets: Vec<HighValueTargetRecord>,
}

pub fn build_inventory(root: &Path) -> Result<OrpheusInventory> {
    let mut apps = Vec::new();
    let mut containers = Vec::new();
    let mut databases = Vec::new();
    let mut plists = Vec::new();

    for entry in fs::read_dir(root).with_context(|| format!("reading {}", root.display()))? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();
        let Some((kind, bundle_id)) = parse_container_name(&name) else {
            continue;
        };

        let stats = collect_container_stats(&path)?;
        let categories = bundle_categories(bundle_id.as_deref().unwrap_or(&name));
        let high_value = !categories.is_empty();

        containers.push(ContainerInventoryRecord {
            name: name.clone(),
            kind: kind.to_string(),
            path: path.display().to_string(),
            bundle_id: bundle_id.clone(),
            file_count: stats.file_count,
            total_bytes: stats.total_bytes,
        });

        for db in &stats.databases {
            databases.push(DatabaseInventoryRecord {
                path: db.display().to_string(),
                container: name.clone(),
                bundle_id: bundle_id.clone(),
                file_name: db
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
                size_bytes: fs::metadata(db).map(|m| m.len()).unwrap_or(0),
                category_hints: database_hints(db),
            });
        }

        for plist in &stats.plists {
            plists.push(PlistInventoryRecord {
                path: plist.display().to_string(),
                container: name.clone(),
                bundle_id: bundle_id.clone(),
                file_name: plist
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
                size_bytes: fs::metadata(plist).map(|m| m.len()).unwrap_or(0),
            });
        }

        if kind == "app" {
            let bundle = bundle_id.clone().unwrap_or_else(|| name.clone());
            apps.push(AppInventoryRecord {
                bundle_id: bundle.clone(),
                display_name: display_name_for_bundle(&bundle),
                container_kind: kind.to_string(),
                container_path: path.display().to_string(),
                file_count: stats.file_count,
                total_bytes: stats.total_bytes,
                database_count: stats.databases.len(),
                plist_count: stats.plists.len(),
                categories,
                high_value,
            });
        }
    }

    let high_value_targets = apps
        .iter()
        .filter(|app| app.high_value)
        .map(|app| HighValueTargetRecord {
            bundle_id: app.bundle_id.clone(),
            display_name: app.display_name.clone(),
            categories: app.categories.clone(),
            reason: format!("Matched {}", app.categories.join(", ")),
            container_path: app.container_path.clone(),
            database_count: app.database_count,
            plist_count: app.plist_count,
        })
        .collect();

    apps.sort_by(|a, b| {
        a.display_name
            .to_lowercase()
            .cmp(&b.display_name.to_lowercase())
    });
    containers.sort_by(|a, b| a.name.cmp(&b.name));
    databases.sort_by(|a, b| a.path.cmp(&b.path));
    plists.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(OrpheusInventory {
        apps,
        containers,
        databases,
        plists,
        high_value_targets,
    })
}

pub fn write_inventory_outputs(out_dir: &Path, inventory: &OrpheusInventory) -> Result<()> {
    fs::create_dir_all(out_dir)?;
    write_json(out_dir.join("apps.json"), &inventory.apps)?;
    write_json(
        out_dir.join("app_groups.json"),
        &filter_containers(&inventory.containers, "app_group"),
    )?;
    write_json(out_dir.join("containers.json"), &inventory.containers)?;
    write_json(out_dir.join("databases.json"), &inventory.databases)?;
    write_json(out_dir.join("plists.json"), &inventory.plists)?;
    write_json(
        out_dir.join("high_value_targets.json"),
        &inventory.high_value_targets,
    )?;

    write_apps_csv(out_dir.join("apps.csv"), &inventory.apps)?;
    write_containers_csv(
        out_dir.join("app_groups.csv"),
        &filter_containers(&inventory.containers, "app_group"),
    )?;
    write_containers_csv(out_dir.join("containers.csv"), &inventory.containers)?;
    write_databases_csv(out_dir.join("databases.csv"), &inventory.databases)?;
    write_plists_csv(out_dir.join("plists.csv"), &inventory.plists)?;
    write_high_value_csv(
        out_dir.join("high_value_targets.csv"),
        &inventory.high_value_targets,
    )?;

    fs::write(
        out_dir.join("installed_apps.html"),
        build_apps_html(inventory),
    )?;
    Ok(())
}

fn parse_container_name(name: &str) -> Option<(&'static str, Option<String>)> {
    if let Some(rest) = name.strip_prefix("AppDomain-") {
        Some(("app", Some(rest.to_string())))
    } else if let Some(rest) = name.strip_prefix("AppDomainGroup-") {
        Some(("app_group", Some(rest.to_string())))
    } else if let Some(rest) = name.strip_prefix("SysContainerDomain-") {
        Some(("system", Some(rest.to_string())))
    } else {
        None
    }
}

#[derive(Debug)]
struct ContainerStats {
    file_count: u64,
    total_bytes: u64,
    databases: Vec<PathBuf>,
    plists: Vec<PathBuf>,
}

fn collect_container_stats(path: &Path) -> Result<ContainerStats> {
    let mut file_count = 0;
    let mut total_bytes = 0;
    let mut databases = Vec::new();
    let mut plists = Vec::new();

    for entry in WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        file_count += 1;
        let entry_path = entry.path().to_path_buf();
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        total_bytes += size;

        let lower = entry_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase();
        if is_database_name(&lower) {
            databases.push(entry_path);
        } else if lower.ends_with(".plist") {
            plists.push(entry_path);
        }
    }

    Ok(ContainerStats {
        file_count,
        total_bytes,
        databases,
        plists,
    })
}

fn is_database_name(lower: &str) -> bool {
    lower.ends_with(".sqlite")
        || lower.ends_with(".sqlite3")
        || lower.ends_with(".db")
        || lower.ends_with(".sqlitedb")
        || lower.ends_with(".storedata")
}

fn bundle_categories(bundle_id: &str) -> Vec<String> {
    let lower = bundle_id.to_ascii_lowercase();
    let mut out = BTreeSet::new();
    for (needle, category) in [
        ("cash", "finance"),
        ("chime", "finance"),
        ("empower", "finance"),
        ("square", "finance"),
        ("venmo", "finance"),
        ("bank", "finance"),
        ("messenger", "messaging"),
        ("pinger", "messaging"),
        ("textfree", "messaging"),
        ("textme", "messaging"),
        ("secondsms", "messaging"),
        ("chrome", "browser"),
        ("brave", "browser"),
        ("google", "cloud"),
        ("photos", "cloud"),
        ("uber", "location"),
        ("lyft", "location"),
        ("maps", "location"),
        ("instagram", "social"),
        ("facebook", "social"),
        ("reddit", "social"),
        ("grindr", "social"),
        ("openai", "ai"),
        ("gemini", "ai"),
        ("deepseek", "ai"),
        ("grok", "ai"),
        ("temu", "shopping"),
        ("walmart", "shopping"),
        ("homedepot", "shopping"),
        ("lowes", "shopping"),
        ("walgreens", "shopping"),
    ] {
        if lower.contains(needle) {
            out.insert(category.to_string());
        }
    }
    out.into_iter().collect()
}

fn database_hints(path: &Path) -> Vec<String> {
    let lower = path.to_string_lossy().to_ascii_lowercase();
    let mut hints = BTreeSet::new();
    for (needle, hint) in [
        ("message", "messages"),
        ("chat", "messages"),
        ("transaction", "finance"),
        ("payment", "finance"),
        ("location", "location"),
        ("map", "location"),
        ("history", "history"),
        ("cookie", "browser"),
        ("cache", "cache"),
        ("analytics", "analytics"),
        ("account", "account"),
    ] {
        if lower.contains(needle) {
            hints.insert(hint.to_string());
        }
    }
    hints.into_iter().collect()
}

fn display_name_for_bundle(bundle_id: &str) -> String {
    let last = bundle_id.rsplit('.').next().unwrap_or(bundle_id);
    let spaced = last.replace(['-', '_'], " ");
    let mut chars = spaced.chars();
    match chars.next() {
        Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
        None => bundle_id.to_string(),
    }
}

fn filter_containers(
    containers: &[ContainerInventoryRecord],
    kind: &str,
) -> Vec<ContainerInventoryRecord> {
    containers
        .iter()
        .filter(|c| c.kind == kind)
        .cloned()
        .collect()
}

fn write_json<T: Serialize + ?Sized>(path: PathBuf, value: &T) -> Result<()> {
    fs::write(&path, serde_json::to_vec_pretty(value)?)
        .with_context(|| format!("writing {}", path.display()))
}

fn write_apps_csv(path: PathBuf, rows: &[AppInventoryRecord]) -> Result<()> {
    let mut wtr = csv::Writer::from_path(&path)?;
    for row in rows {
        wtr.write_record([
            row.bundle_id.clone(),
            row.display_name.clone(),
            row.container_kind.clone(),
            row.container_path.clone(),
            row.file_count.to_string(),
            row.total_bytes.to_string(),
            row.database_count.to_string(),
            row.plist_count.to_string(),
            row.categories.join(";"),
            row.high_value.to_string(),
        ])?;
    }
    wtr.flush()?;
    Ok(())
}

fn write_containers_csv(path: PathBuf, rows: &[ContainerInventoryRecord]) -> Result<()> {
    let mut wtr = csv::Writer::from_path(&path)?;
    for row in rows {
        wtr.write_record([
            row.name.clone(),
            row.kind.clone(),
            row.path.clone(),
            row.bundle_id.clone().unwrap_or_default(),
            row.file_count.to_string(),
            row.total_bytes.to_string(),
        ])?;
    }
    wtr.flush()?;
    Ok(())
}

fn write_databases_csv(path: PathBuf, rows: &[DatabaseInventoryRecord]) -> Result<()> {
    let mut wtr = csv::Writer::from_path(&path)?;
    for row in rows {
        wtr.write_record([
            row.path.clone(),
            row.container.clone(),
            row.bundle_id.clone().unwrap_or_default(),
            row.file_name.clone(),
            row.size_bytes.to_string(),
            row.category_hints.join(";"),
        ])?;
    }
    wtr.flush()?;
    Ok(())
}

fn write_plists_csv(path: PathBuf, rows: &[PlistInventoryRecord]) -> Result<()> {
    let mut wtr = csv::Writer::from_path(&path)?;
    for row in rows {
        wtr.write_record([
            row.path.clone(),
            row.container.clone(),
            row.bundle_id.clone().unwrap_or_default(),
            row.file_name.clone(),
            row.size_bytes.to_string(),
        ])?;
    }
    wtr.flush()?;
    Ok(())
}

fn write_high_value_csv(path: PathBuf, rows: &[HighValueTargetRecord]) -> Result<()> {
    let mut wtr = csv::Writer::from_path(&path)?;
    for row in rows {
        wtr.write_record([
            row.bundle_id.clone(),
            row.display_name.clone(),
            row.categories.join(";"),
            row.reason.clone(),
            row.container_path.clone(),
            row.database_count.to_string(),
            row.plist_count.to_string(),
        ])?;
    }
    wtr.flush()?;
    Ok(())
}

fn build_apps_html(inventory: &OrpheusInventory) -> String {
    let rows: String = inventory
        .apps
        .iter()
        .map(|app| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                escape_html(&app.display_name),
                escape_html(&app.bundle_id),
                escape_html(&app.categories.join(", ")),
                app.database_count,
                app.plist_count,
                format_bytes(app.total_bytes)
            )
        })
        .collect();

    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>Orpheus Installed Apps</title>
<style>body{{font-family:Arial,sans-serif;margin:24px;color:#1f2933}}table{{border-collapse:collapse;width:100%}}td,th{{border:1px solid #d9dee7;padding:6px;font-size:12px}}th{{background:#f3f5f8;text-align:left}}.summary{{color:#52606d}}</style>
</head><body><h1>Orpheus Installed Apps</h1><p class="summary">Apps: {} | High value: {} | Databases: {} | Plists: {}</p><table><thead><tr><th>App</th><th>Bundle ID</th><th>Categories</th><th>DBs</th><th>Plists</th><th>Size</th></tr></thead><tbody>{}</tbody></table></body></html>"#,
        inventory.apps.len(),
        inventory.high_value_targets.len(),
        inventory.databases.len(),
        inventory.plists.len(),
        rows
    )
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", bytes as f64 / 1024.0 / 1024.0 / 1024.0)
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / 1024.0 / 1024.0)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
