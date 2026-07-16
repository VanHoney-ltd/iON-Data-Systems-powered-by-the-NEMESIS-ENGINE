//! Backup Creation and Management

use crate::agents::cerberus::{export_contacts, extract_contacts};
use crate::case::Case;
use crate::chronos::awake::KeepAwakeGuard;
use crate::chronos::config::ChronosConfig;
use crate::chronos::device::DeviceManager;
use crate::chronos::error::ChronosError;
use crate::chronos::manifest::{ChronosManifest, ChronosPreparedRecord};
use crate::chronos::validation::validate_core_files;
use crate::common::prepared::{prepare_artifact, PrepareContext};
use crate::common::resolver::{
    write_resolver_audit, ArtifactResolver, BackupResolver, ResolverAuditRecord, ResolverContext,
};
use crate::common::target::chronos_targets;
use anyhow::{Context, Result};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const REQUIRED_FILES: &[&str] = &[
    "Manifest.db",
    "Manifest.plist",
    "Status.plist",
    "Info.plist",
];

#[derive(Debug, Serialize)]
pub struct BackupResult {
    pub backup_root: PathBuf,
    pub manifest: ChronosManifest,
    pub device_info: Option<crate::chronos::device::DeviceInfo>,
    pub duration: Duration,
    pub helios_root: Option<PathBuf>,
    pub orpheus_ok: bool,
}

pub struct BackupManager {
    case: Case,
    config: ChronosConfig,
    progress: std::sync::Arc<crate::chronos::progress::BackupProgress>,
}

impl BackupManager {
    pub fn new(case: Case, config: ChronosConfig) -> Self {
        Self {
            case,
            config,
            progress: crate::chronos::progress::BackupProgress::new(),
        }
    }

    pub fn create_backup(&self) -> Result<BackupResult> {
        let start_time = Instant::now();

        self.case.log("CHRONOS — Full Backup Starting", None)?;

        println!(
            r#"
    ╔═══════════════════════════════════════════════════════════════════╗
    ║  ██████╗██╗  ██╗██████╗  ██████╗ ███╗   ██╗ ██████╗ ███████╗     ║
    ║ ██╔════╝██║  ██║██╔══██╗██╔═══██╗████╗  ██║██╔═══██╗██╔════╝     ║
    ║ ██║     ███████║██████╔╝██║   ██║██╔██╗ ██║██║   ██║███████╗     ║
    ║ ██║     ██╔══██║██╔══██╗██║   ██║██║╚██╗██║██║   ██║╚════██║     ║
    ║ ╚██████╗██║  ██║██║  ██║╚██████╔╝██║ ╚████║╚██████╔╝███████║     ║
    ║  ╚═════╝╚═╝  ╚═╝╚═╝  ╚═╝ ╚═════╝ ╚═╝  ╚═══╝ ╚═════╝ ╚══════╝     ║
    ║                TITAN OF TIME - BACKUP CREATOR                     ║
    ╚═══════════════════════════════════════════════════════════════════╝
        "#
        );

        let base_dir = self.case.ion_backup_path()?;
        fs::create_dir_all(&base_dir).with_context(|| {
            format!("Failed to create backup directory: {}", base_dir.display())
        })?;

        let override_root = self.config.backup_path_override.clone();
        let device_manager = if self.config.offline || override_root.is_some() {
            None
        } else {
            Some(DeviceManager::new(self.case.clone(), self.config.clone()))
        };
        let prepared_device_info = if let Some(manager) = &device_manager {
            if self.config.offline || override_root.is_some() {
                None
            } else {
                self.prepare_device(manager)?;
                Some(manager.get_device_info()?)
            }
        } else {
            None
        };

        let backup_root = if let Some(override_path) = override_root {
            self.validate_backup_root(&override_path)?;
            println!("📂 Using provided backup path {}", override_path.display());
            self.case.log(
                "CHRONOS — Using provided backup path",
                Some(&format!("{}", override_path.display())),
            )?;
            override_path
        } else if let Some(existing) = self.find_backup_root(&base_dir)? {
            println!("📂 Found existing backup at {}", existing.display());
            self.case.log(
                "CHRONOS — Reusing existing backup",
                Some(&format!("{}", existing.display())),
            )?;
            existing
        } else if self.config.offline {
            return Err(anyhow::anyhow!(
                "--offline/--use-existing requires an existing backup in {}",
                base_dir.display()
            ));
        } else {
            device_manager.as_ref().ok_or_else(|| {
                anyhow::anyhow!("Device manager unavailable; cannot create new backup")
            })?;
            self.create_new_backup(&base_dir, prepared_device_info.as_ref())?
        };

        // Validate core files exist
        validate_core_files(&backup_root)?;
        self.ensure_case_backup_reference(&base_dir, &backup_root)?;

        let resolver = BackupResolver::new(ResolverContext {
            backup_root: backup_root.clone(),
            case_root: self.case.root_path(),
            clean_root: self.case.root_path().join("clean"),
            manifest_db_path: backup_root
                .join("Manifest.db")
                .exists()
                .then(|| backup_root.join("Manifest.db")),
        });

        // Process known files and create clean copies with WAL replay (or direct-tree staging)
        let mut manifest = self.summarize_known_files(&resolver, &prepared_device_info)?;

        // Save manifest (phase 1)
        let summary_path = self.case.root_path().join("chronos_manifest.json");
        self.case
            .write_file(&summary_path, serde_json::to_vec_pretty(&manifest)?)?;
        println!("✓ Chronos manifest saved: {}", summary_path.display());
        self.case.log(
            "CHRONOS — Manifest saved (phase 1)",
            Some(&format!("{}", summary_path.display())),
        )?;

        // --- Pipeline: Helios -> Chronos Phase 2 -> Orpheus ---
        let mut helios_root: Option<PathBuf> = None;
        let mut orpheus_ok = false;

        if !self.config.offline {
            // 1. Run Helios
            let Some(password) = self.config.backup_password.as_deref() else {
                println!(
                    "\n🔐 Backup acquisition completed. Skipping Helios because no decrypt password was provided to Chronos."
                );
                println!(
                    "   Use the Decrypt Backup tab when you are ready to decrypt this backup."
                );
                self.case.log(
                    "CHRONOS — Skipping Helios",
                    Some("No decrypt password was provided during acquisition"),
                )?;
                let duration = start_time.elapsed();
                println!("\n✓ CHRONOS ACQUISITION COMPLETE");
                println!("  Location: {}", backup_root.display());
                println!("  Duration: {:?}", duration);
                self.case.log(
                    "CHRONOS ACQUISITION COMPLETE",
                    Some(&format!(
                        "backup: {} helios: skipped orpheus: skipped duration: {:?}",
                        backup_root.display(),
                        duration
                    )),
                )?;
                return Ok(BackupResult {
                    backup_root,
                    manifest,
                    device_info: prepared_device_info,
                    duration,
                    helios_root: None,
                    orpheus_ok: false,
                });
            };

            let helios_requested = self.case.workspace().helios_full_root();
            self.run_helios(&backup_root, &helios_requested, password)?;

            // Helios may append .styg to the output directory
            let helios_output = resolve_helios_actual_root(&helios_requested);
            helios_root = Some(helios_output.clone());

            // 2. Chronos second part: re-extract from helios decrypted tree
            if helios_output.exists() {
                println!("\n📱 Chronos Phase 2: Re-extracting from Helios output...");
                self.case.log(
                    "CHRONOS — Phase 2 starting (helios re-extract)",
                    Some(&format!("{}", helios_output.display())),
                )?;

                let helios_resolver = BackupResolver::new(ResolverContext {
                    backup_root: helios_output.clone(),
                    case_root: self.case.root_path(),
                    clean_root: self.case.root_path().join("clean"),
                    manifest_db_path: None,
                });

                let phase2_manifest =
                    self.summarize_known_files(&helios_resolver, &prepared_device_info)?;

                // Merge phase 2 into phase 1 (overwrite duplicates)
                merge_manifest(&mut manifest, phase2_manifest);

                // Point manifest to helios root so agents use decrypted tree
                manifest.backup_root = helios_output.display().to_string();

                // Save merged manifest
                self.case
                    .write_file(&summary_path, serde_json::to_vec_pretty(&manifest)?)?;
                println!(
                    "✓ Chronos manifest merged and saved: {}",
                    summary_path.display()
                );
                self.case.log(
                    "CHRONOS — Manifest merged (phase 2)",
                    Some(&format!("{}", summary_path.display())),
                )?;
            }

            // 3. Run Orpheus
            self.run_orpheus(&helios_output)?;
            orpheus_ok = true;

            // 3.5 Extract contacts
            self.extract_contacts()?;

            // 4. Run all remaining evidence agents
            self.run_all_evidence_agents()?;

            // 5. Launch GUI
            self.launch_gui()?;
        }

        let duration = start_time.elapsed();

        println!("\n✓ CHRONOS SUCCESS");
        println!("  Location: {}", backup_root.display());
        if let Some(ref hr) = helios_root {
            println!("  Helios:   {}", hr.display());
        }
        println!("  Orpheus:  {}", if orpheus_ok { "OK" } else { "SKIPPED" });
        println!("  Duration: {:?}", duration);
        self.case.log(
            "CHRONOS COMPLETE",
            Some(&format!(
                "backup: {} helios: {} orpheus: {} duration: {:?}",
                backup_root.display(),
                helios_root
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
                if orpheus_ok { "ok" } else { "skipped" },
                duration
            )),
        )?;

        Ok(BackupResult {
            backup_root,
            manifest,
            device_info: prepared_device_info,
            duration,
            helios_root,
            orpheus_ok,
        })
    }

    fn prepare_device(&self, device_manager: &DeviceManager) -> Result<()> {
        device_manager.ensure_tools_available()?;

        println!("📱 Step 1/3: Pairing device...");
        self.case.log("CHRONOS — Running idevicepair pair", None)?;
        device_manager.pair_device()?;

        println!("📱 Step 2/3: Validating pairing...");
        self.case
            .log("CHRONOS — Running idevicepair validate", None)?;
        device_manager.validate_device()?;

        println!("📱 Step 3/3: Collecting device info...");
        self.case.log("CHRONOS — Running ideviceinfo", None)?;
        let info = device_manager.get_device_info()?;
        self.case.log(
            "CHRONOS — Device info collected",
            Some(&format!(
                "{} {} {}",
                info.device_name, info.product_type, info.product_version
            )),
        )?;

        let device_info_path = self.case.root_path().join("chronos_device_info.json");
        self.case
            .write_file(&device_info_path, serde_json::to_vec_pretty(&info)?)?;
        Ok(())
    }

    fn create_new_backup(
        &self,
        base_dir: &Path,
        device_info: Option<&crate::chronos::device::DeviceInfo>,
    ) -> Result<PathBuf> {
        println!("?? Starting iPhone backup...");
        println!("? This may take 5-30 minutes\n");

        let _keep_awake = KeepAwakeGuard::new();

        let udid = device_info.map(|d| d.udid.as_str());
        self.perform_backup(base_dir, udid)
    }

    fn perform_backup(&self, base_dir: &Path, udid: Option<&str>) -> Result<PathBuf> {
        println!("📱 Creating full encrypted backup...");
        println!("⏳ This may take 2-6 hours depending on the size of the backup and your connection speed.");
        println!("   Leave the device connected and unlocked. Do not close this window.\n");
        self.case
            .log("CHRONOS — Creating full encrypted backup", None)?;

        // Run full backup interactively so libimobiledevice owns the password prompt.
        // Do not call `idevicebackup2 encryption on` first. When encryption is
        // already enabled, libimobiledevice exits 255 and can leave the backup
        // service in a bad state for the immediately following backup command.
        let mut backup_builder =
            crate::chronos::idevicebackup2::IDeviceBackup2Builder::full_backup(base_dir)
                .interactive();
        if let Some(id) = udid {
            backup_builder = backup_builder.udid(id);
        }

        eprintln!("🚀 Executing: {}", backup_builder.to_command_string());

        let mut child = backup_builder
            .build()
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| "Failed to spawn idevicebackup2")?;

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        // Forward idevicebackup2 output so the app can parse progress and show
        // a structured progress bar instead of noisy terminal animation.
        let reader_handle = thread::spawn(move || {
            use std::io::{BufRead, BufReader};

            fn forward_lines<R: std::io::Read>(source: Option<R>) {
                if let Some(stream) = source {
                    let reader = BufReader::new(stream);
                    for line in reader.lines().flatten() {
                        println!("[idevicebackup2] {}", line);
                    }
                }
            }

            forward_lines(stdout);
            forward_lines(stderr);
        });

        let status = child.wait()?;
        let _ = reader_handle.join();
        if !status.success() {
            // We lost stdout/stderr because it was consumed by the reader thread.
            // The original interactive idevicebackup2 status is the useful failure signal.
            self.case.log(
                "CHRONOS FAILED",
                Some(&format!(
                    "idevicebackup2 backup exited with status {:?}",
                    status.code()
                )),
            )?;

            return Err(ChronosError::BackupFailed(format!(
                "idevicebackup2 exited with status {:?}",
                status.code()
            ))
            .into());
        }

        println!("✓ Full encrypted backup completed");
        self.find_backup_root(base_dir)?
            .ok_or_else(|| ChronosError::InvalidBackupFormat.into())
    }

    fn run_helios(&self, backup_root: &Path, output_dir: &Path, password: &str) -> Result<()> {
        println!("\n🔆 Launching Helios full decrypt...");
        self.case.log(
            "CHRONOS — Launching Helios",
            Some(&format!("output={}", output_dir.display())),
        )?;

        let helios_bin = locate_agent_binary("helios").ok_or_else(|| {
            anyhow::anyhow!("Could not locate 'helios' binary on PATH or next to chronos")
        })?;

        fs::create_dir_all(output_dir)?;

        let mut cmd = Command::new(&helios_bin);
        cmd.arg(self.case.name())
            .arg("-i")
            .arg(backup_root)
            .arg("-o")
            .arg(output_dir)
            .arg("-p")
            .arg(password)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        println!("🚀 Executing: {}", cmd_to_string(&cmd));

        let status = cmd.status().with_context(|| {
            format!("Failed to execute helios binary: {}", helios_bin.display())
        })?;

        if !status.success() {
            let code = status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "signal".to_string());
            self.case.log("CHRONOS — Helios failed", Some(&code))?;
            return Err(ChronosError::ToolExecutionFailed(format!(
                "helios exited with status {}",
                code
            ))
            .into());
        }

        println!("✓ Helios completed successfully");
        self.case.log("CHRONOS — Helios completed", None)?;
        Ok(())
    }

    fn run_orpheus(&self, root: &Path) -> Result<()> {
        println!("\n🎵 Launching Orpheus recon...");
        self.case.log(
            "CHRONOS — Launching Orpheus",
            Some(&format!("root={}", root.display())),
        )?;

        crate::agents::dispatch("orpheus", self.case.name()).with_context(|| {
            self.case
                .log("CHRONOS — Orpheus failed", Some("in-process dispatch"))
                .ok();
            "Orpheus in-process dispatch failed"
        })?;

        println!("✓ Orpheus completed successfully");
        self.case.log("CHRONOS — Orpheus completed", None)?;
        Ok(())
    }

    fn run_all_evidence_agents(&self) -> Result<()> {
        let agents = [
            "atlas", "plutus", "cerberus", "hermes", "charon", "nyx", "obolus", "psyche",
        ];

        for agent in agents {
            println!("\n🤖 Launching {}...", agent.to_uppercase());
            self.case.log(
                &format!("CHRONOS — Launching {}", agent.to_uppercase()),
                None,
            )?;

            let bin = locate_agent_binary(agent)
                .ok_or_else(|| anyhow::anyhow!("Could not locate '{}' binary", agent))?;

            let mut cmd = Command::new(&bin);
            cmd.arg(self.case.name())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit());

            println!("🚀 Executing: {}", cmd_to_string(&cmd));

            let status = cmd.status().with_context(|| {
                format!("Failed to execute {} binary: {}", agent, bin.display())
            })?;

            if !status.success() {
                let code = status
                    .code()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "signal".to_string());
                self.case.log(
                    &format!("CHRONOS — {} failed", agent.to_uppercase()),
                    Some(&code),
                )?;
                // Log warning but continue — one agent failing shouldn't kill the whole pipeline
                println!(
                    "⚠️  {} failed with status {}. Continuing pipeline...",
                    agent, code
                );
            } else {
                println!("✓ {} completed successfully", agent.to_uppercase());
                self.case.log(
                    &format!("CHRONOS — {} completed", agent.to_uppercase()),
                    None,
                )?;
            }
        }

        Ok(())
    }

    fn extract_contacts(&self) -> Result<()> {
        println!("\n👤 Extracting contacts...");
        self.case.log("CHRONOS — Extracting contacts", None)?;

        match extract_contacts(&self.case) {
            Ok(contacts) => {
                let path = export_contacts(&self.case, &contacts)?;
                println!(
                    "✓ Extracted {} contacts -> {}",
                    contacts.len(),
                    path.display()
                );
                self.case.log(
                    "CHRONOS — Contacts extracted",
                    Some(&format!("{} contacts", contacts.len())),
                )?;
                Ok(())
            }
            Err(e) => {
                println!("⚠️  Contacts extraction failed: {}", e);
                self.case
                    .log("CHRONOS — Contacts extraction failed", Some(&e.to_string()))?;
                // Non-fatal: don't block the pipeline
                Ok(())
            }
        }
    }

    fn launch_gui(&self) -> Result<()> {
        println!("\n🖥️  Launching iON GUI...");
        self.case
            .log("CHRONOS — Launching GUI", Some(self.case.name()))?;

        let gui_bin = locate_agent_binary("ion_ui").ok_or_else(|| {
            anyhow::anyhow!("Could not locate 'ion_ui' binary on PATH or next to chronos")
        })?;

        let mut cmd = Command::new(&gui_bin);
        cmd.arg(self.case.root_path())
            .arg("--case-name")
            .arg(self.case.name())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        println!("🚀 Executing: {}", cmd_to_string(&cmd));

        let status = cmd
            .status()
            .with_context(|| format!("Failed to execute ion_ui binary: {}", gui_bin.display()))?;

        if !status.success() {
            let code = status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "signal".to_string());
            self.case.log("CHRONOS — GUI failed", Some(&code))?;
            return Err(ChronosError::ToolExecutionFailed(format!(
                "ion_ui exited with status {}",
                code
            ))
            .into());
        }

        println!("✓ GUI closed");
        self.case.log("CHRONOS — GUI closed", None)?;
        Ok(())
    }

    fn summarize_known_files(
        &self,
        resolver: &BackupResolver,
        device_info: &Option<crate::chronos::device::DeviceInfo>,
    ) -> Result<ChronosManifest> {
        let prepare_ctx = PrepareContext {
            case_root: self.case.root_path(),
            clean_root: resolver.clean_root().to_path_buf(),
            temp_root: self.case.root_path().join("tmp"),
        };
        let targets = chronos_targets();
        let mut prepared = Vec::new();

        // Update progress
        self.progress.update_total(targets.len() as u64);

        for (index, target) in targets.iter().enumerate() {
            self.progress.set_current_file(target.artifact_key);

            let resolved = resolver.resolve_known_target(target)?;
            let audit = ResolverAuditRecord {
                artifact_key: target.artifact_key.to_string(),
                candidates: target
                    .candidates
                    .iter()
                    .map(|candidate| format!("{}/{}", candidate.domain, candidate.relative_path))
                    .collect(),
                chosen: resolved
                    .as_ref()
                    .map(|artifact| artifact.source_path.display().to_string()),
                method: resolved
                    .as_ref()
                    .map(|artifact| format!("{:?}", artifact.method)),
            };
            write_resolver_audit(resolver.case_root(), &audit)?;

            if let Some(resolved) = resolved {
                match prepare_artifact(&prepare_ctx, &resolved, target.sqlite_like) {
                    Ok(artifact) => {
                        prepared.push(ChronosPreparedRecord {
                            artifact_key: target.artifact_key.to_string(),
                            source_path: artifact.source_path.display().to_string(),
                            working_path: artifact.working_path.display().to_string(),
                            resolution_method: format!("{:?}", artifact.resolution_method),
                            sha256: artifact.sha256.clone(),
                        });
                    }
                    Err(e) => {
                        println!("WARN: Failed to prepare {}: {}", target.artifact_key, e);
                    }
                }
            } else {
                println!("WARN: Failed to resolve {}", target.artifact_key);
            }

            self.progress.increment_processed();

            // Show progress every 10% or for first/last items
            let progress = self.progress.progress_percentage();
            if index == 0 || index == targets.len() - 1 || (progress as usize).is_multiple_of(10) {
                println!(
                    "Progress: {:.1}% - Processing {}",
                    progress, target.artifact_key
                );
            }
        }

        Ok(ChronosManifest {
            backup_root: resolver.backup_root().display().to_string(),
            prepared,
            device_info: device_info.clone(),
        })
    }

    fn find_backup_root(&self, base_dir: &Path) -> Result<Option<PathBuf>> {
        if !base_dir.exists() {
            return Ok(None);
        }

        let mut candidates: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();

        if self.has_required_files(base_dir) {
            candidates.push((self.modified_time(base_dir), base_dir.to_path_buf()));
        }

        if base_dir.is_dir() {
            for entry in fs::read_dir(base_dir)? {
                let path = match entry {
                    Ok(e) => e.path(),
                    Err(_) => continue,
                };

                if path.is_dir() && self.has_required_files(&path) {
                    candidates.push((self.modified_time(&path), path));
                }
            }
        }

        candidates.sort_by_key(|(mtime, _)| *mtime);
        Ok(candidates.pop().map(|(_, path)| path))
    }

    fn validate_backup_root(&self, path: &Path) -> Result<()> {
        if !path.exists() {
            return Err(anyhow::anyhow!("Backup path not found: {}", path.display()));
        }
        if !path.is_dir() {
            return Err(anyhow::anyhow!(
                "Backup path is not a directory: {}",
                path.display()
            ));
        }
        if !self.has_required_files(path) {
            return Err(anyhow::anyhow!(
                "Backup path missing required files: {}",
                path.display()
            ));
        }
        Ok(())
    }

    fn has_required_files(&self, dir: &Path) -> bool {
        REQUIRED_FILES.iter().all(|name| dir.join(name).exists())
    }

    fn modified_time(&self, path: &Path) -> std::time::SystemTime {
        fs::metadata(path)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
    }

    fn ensure_case_backup_reference(&self, base_dir: &Path, backup_root: &Path) -> Result<()> {
        if backup_root.starts_with(base_dir) {
            return Ok(());
        }

        fs::create_dir_all(base_dir)?;
        let link_name = backup_root
            .file_name()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("external_backup"));
        let link_path = base_dir.join(link_name);

        if link_path.exists() {
            return Ok(());
        }

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(backup_root, &link_path).with_context(|| {
                format!(
                    "Failed to create backup symlink {} -> {}",
                    link_path.display(),
                    backup_root.display()
                )
            })?;
        }

        Ok(())
    }
}

fn merge_manifest(base: &mut ChronosManifest, from: ChronosManifest) {
    for record in from.prepared {
        if let Some(existing) = base
            .prepared
            .iter_mut()
            .find(|r| r.artifact_key == record.artifact_key)
        {
            *existing = record;
        } else {
            base.prepared.push(record);
        }
    }
}

fn resolve_helios_actual_root(requested: &Path) -> PathBuf {
    let styg_variant = if let Some(name) = requested.file_name().and_then(|n| n.to_str()) {
        requested.with_file_name(format!("{}.styg", name))
    } else {
        requested.to_path_buf()
    };
    if styg_variant.exists() {
        styg_variant
    } else {
        requested.to_path_buf()
    }
}

fn locate_agent_binary(name: &str) -> Option<PathBuf> {
    let exe_name = if cfg!(windows) {
        format!("{}.exe", name)
    } else {
        name.to_string()
    };

    if let Ok(current) = std::env::current_exe() {
        if let Some(dir) = current.parent() {
            let candidate = dir.join(&exe_name);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    let candidates = [
        Path::new("target").join("debug").join(&exe_name),
        Path::new("target").join("release").join(&exe_name),
        Path::new("iON")
            .join("target")
            .join("debug")
            .join(&exe_name),
        Path::new("iON")
            .join("target")
            .join("release")
            .join(&exe_name),
        Path::new("iON3")
            .join("target")
            .join("debug")
            .join(&exe_name),
        Path::new("iON3")
            .join("target")
            .join("release")
            .join(&exe_name),
    ];
    for candidate in candidates {
        if candidate.exists() {
            return Some(candidate);
        }
    }

    // Fallback: search PATH manually
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join(&exe_name);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    None
}

fn cmd_to_string(cmd: &Command) -> String {
    // Best-effort string representation for logging
    let prog = cmd.get_program().to_string_lossy();
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect();
    format!("{} {}", prog, args.join(" "))
}

pub fn create_backup(case: Case, config: ChronosConfig) -> Result<BackupResult> {
    let backup_manager = BackupManager::new(case, config);
    backup_manager.create_backup()
}
