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
use crossterm::terminal;
use serde::Serialize;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
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

        // Enforce encrypted backups before touching the device
        if !self.config.offline && self.config.backup_password.is_none() {
            return Err(anyhow::anyhow!(
                "Encrypted backups are required. Set BACKUP_PASSWORD environment variable."
            ));
        }

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
            let password = self.config.backup_password.as_deref().unwrap_or_default();

            // 1. Run Helios
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
        let password = self.config.backup_password.as_deref();

        self.perform_backup(base_dir, udid, password)
    }

    fn perform_backup(
        &self,
        base_dir: &Path,
        udid: Option<&str>,
        password: Option<&str>,
    ) -> Result<PathBuf> {
        println!("📱 Creating full encrypted backup...");
        println!("⏳ This may take 2-6 hours depending on the size of the backup and your connection speed.");
        println!("   Leave the device connected and unlocked. Do not close this window.\n");
        self.case
            .log("CHRONOS — Creating full encrypted backup", None)?;

        // Step 1: Ensure encryption is enabled
        let enc_ok = self.ensure_encryption_enabled(base_dir, udid, password);
        if let Err(ref e) = enc_ok {
            println!("⚠️  Encryption setup issue: {}", e);
            self.case.log(
                "CHRONOS — Encryption setup warning",
                Some(&format!("{}", e)),
            )?;
        }
        // We proceed even if enc_ok is Err("already enabled") — the backup will tell us if things are really wrong.

        // Step 2: Run full backup (with password if available)
        let mut backup_builder =
            crate::chronos::idevicebackup2::IDeviceBackup2Builder::full_backup(base_dir);
        if let Some(id) = udid {
            backup_builder = backup_builder.udid(id);
        }
        if let Some(pw) = password {
            backup_builder = backup_builder.password(pw);
        }

        eprintln!("🚀 Executing: {}", backup_builder.to_command_string());

        let progress_pct = Arc::new(std::sync::atomic::AtomicU8::new(0));
        let _progress = MetroidProgress::start_with_progress(Arc::clone(&progress_pct));

        let mut child = backup_builder
            .build()
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| "Failed to spawn idevicebackup2")?;

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        // Spawn a thread to read combined output and parse percentage
        let progress_clone = Arc::clone(&progress_pct);
        let reader_handle = thread::spawn(move || {
            use std::io::{BufRead, BufReader};

            fn read_and_parse<R: std::io::Read>(
                source: Option<R>,
                pct: &std::sync::atomic::AtomicU8,
            ) {
                if let Some(stream) = source {
                    let reader = BufReader::new(stream);
                    for line in reader.lines().flatten() {
                        // idevicebackup2 often prints progress like "10%" or "Progress: 10%"
                        if let Some(cap) = regex_percent(&line) {
                            pct.store(cap, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                }
            }

            fn regex_percent(line: &str) -> Option<u8> {
                // Look for the first number immediately followed by %
                let chars: Vec<char> = line.chars().collect();
                let mut i = 0;
                while i < chars.len() {
                    if chars[i].is_ascii_digit() {
                        let start = i;
                        while i < chars.len() && chars[i].is_ascii_digit() {
                            i += 1;
                        }
                        if i < chars.len() && chars[i] == '%' {
                            let num_str: String = chars[start..i].iter().collect();
                            if let Ok(n) = num_str.parse::<u8>() {
                                return Some(n);
                            }
                        }
                    }
                    i += 1;
                }
                None
            }

            read_and_parse(stdout, &progress_clone);
            read_and_parse(stderr, &progress_clone);
        });

        let status = child.wait()?;
        let _ = reader_handle.join();
        drop(_progress);

        if !status.success() {
            // We lost stdout/stderr because it was consumed by the reader thread.
            // Re-run briefly to capture error or use interactive fallback.
            self.case.log(
                "CHRONOS FAILED",
                Some(&format!(
                    "idevicebackup2 backup exited with status {:?}",
                    status.code()
                )),
            )?;

            // Interactive fallback if password-related failure is suspected
            if status.code() == Some(255) && password.is_some() {
                println!("\n🔐 Non-interactive backup failed. Falling back to interactive mode...");
                println!("   Please enter the backup password on the prompt below.\n");
                let mut interactive_builder =
                    crate::chronos::idevicebackup2::IDeviceBackup2Builder::full_backup(base_dir)
                        .interactive();
                if let Some(id) = udid {
                    interactive_builder = interactive_builder.udid(id);
                }
                let int_status = interactive_builder
                    .build()
                    .status()
                    .with_context(|| "Failed to run interactive backup")?;
                if !int_status.success() {
                    return Err(ChronosError::BackupFailed(format!(
                        "Interactive backup failed with status {:?}",
                        int_status.code()
                    ))
                    .into());
                }
                println!("✓ Full encrypted backup completed (interactive)");
                return self
                    .find_backup_root(base_dir)?
                    .ok_or_else(|| ChronosError::InvalidBackupFormat.into());
            }

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

    fn ensure_encryption_enabled(
        &self,
        base_dir: &Path,
        udid: Option<&str>,
        password: Option<&str>,
    ) -> Result<()> {
        let mut builder =
            crate::chronos::idevicebackup2::IDeviceBackup2Builder::encryption(base_dir, true);
        if password.is_none() {
            builder = builder.interactive();
        }
        if let Some(id) = udid {
            builder = builder.udid(id);
        }
        if let Some(pw) = password {
            builder = builder.password(pw);
        }

        let output = builder.execute()?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let combined = format!("{} {}", stdout, stderr).to_lowercase();

        if output.status.success()
            || combined.contains("already enabled")
            || combined.contains("is enabled")
        {
            return Ok(());
        }

        // Try interactive as last resort
        if password.is_some() {
            println!("\n🔐 Encryption enablement failed non-interactively. Falling back to interactive...");
            let mut interactive =
                crate::chronos::idevicebackup2::IDeviceBackup2Builder::encryption(base_dir, true)
                    .interactive();
            if let Some(id) = udid {
                interactive = interactive.udid(id);
            }
            let status = interactive
                .build()
                .status()
                .with_context(|| "Failed to run interactive encryption enable")?;
            if status.success() {
                return Ok(());
            }
        }

        Err(anyhow::anyhow!(
            "Failed to enable backup encryption: stdout='{}' stderr='{}'",
            stdout.trim(),
            stderr.trim()
        ))
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

        let orpheus_bin = locate_agent_binary("orpheus").ok_or_else(|| {
            anyhow::anyhow!("Could not locate 'orpheus' binary on PATH or next to chronos")
        })?;

        let mut cmd = Command::new(&orpheus_bin);
        cmd.arg("recon")
            .arg("-c")
            .arg(self.case.name())
            .arg("-r")
            .arg(root)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        println!("🚀 Executing: {}", cmd_to_string(&cmd));

        let status = cmd.status().with_context(|| {
            format!(
                "Failed to execute orpheus binary: {}",
                orpheus_bin.display()
            )
        })?;

        if !status.success() {
            let code = status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "signal".to_string());
            self.case.log("CHRONOS — Orpheus failed", Some(&code))?;
            return Err(ChronosError::ToolExecutionFailed(format!(
                "orpheus exited with status {}",
                code
            ))
            .into());
        }

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

const ANSI_RESET: &str = "\x1b[0m";
const ANSI_BOLD: &str = "\x1b[1m";
const ANSI_DIM: &str = "\x1b[2m";
const ANSI_GREEN: &str = "\x1b[32m";
const ANSI_YELLOW: &str = "\x1b[33m";
const ANSI_CYAN: &str = "\x1b[36m";

struct MetroidProgress {
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl MetroidProgress {
    #[allow(dead_code)]
    fn start() -> Self {
        Self::start_with_progress(Arc::new(std::sync::atomic::AtomicU8::new(0)))
    }

    fn start_with_progress(progress_pct: Arc<std::sync::atomic::AtomicU8>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            let start = Instant::now();
            let width = progress_width();
            let mut step = 0usize;
            while !stop_thread.load(Ordering::Relaxed) {
                let pct = progress_pct.load(std::sync::atomic::Ordering::Relaxed);
                let line = render_metroid_line(step, width, start.elapsed(), pct);
                eprint!("\r\x1b[2K{}", line);
                let _ = io::stderr().flush();
                step = step.wrapping_add(1);
                thread::sleep(Duration::from_millis(120));
            }
        });
        Self {
            stop,
            handle: Some(handle),
        }
    }
}

impl Drop for MetroidProgress {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        eprint!("\r\x1b[2K");
        let _ = io::stderr().flush();
        eprintln!();
    }
}

fn progress_width() -> usize {
    let fallback = 48usize;
    let reserved = 24usize;
    match terminal::size() {
        Ok((cols, _)) => {
            let width = (cols as usize).saturating_sub(reserved);
            if width < 24 {
                fallback
            } else {
                width.min(64)
            }
        }
        Err(_) => fallback,
    }
}

fn render_metroid_line(step: usize, width: usize, elapsed: Duration, pct: u8) -> String {
    let scene = metroid_scene(step, width);
    let elapsed = format_elapsed(elapsed);
    let pct_str = if pct > 0 {
        format!("{:>3}%", pct)
    } else {
        "   ".to_string()
    };
    format!(
        "{bold}METROID RUN{reset} {scene} {dim}{elapsed} {cyan}{pct}{reset}",
        bold = ANSI_BOLD,
        dim = ANSI_DIM,
        cyan = ANSI_CYAN,
        reset = ANSI_RESET,
        pct = pct_str,
    )
}

fn metroid_scene(step: usize, width: usize) -> String {
    const TILE: &[u8] = b"[]-=";
    let width = width.max(4);
    let samus_pos = step % width;
    let metroid_pos = (step * 3 + 7) % width;

    let mut out = String::new();
    out.push_str(ANSI_GREEN);
    for idx in 0..width {
        if idx == samus_pos {
            out.push_str(ANSI_YELLOW);
            out.push('S');
            out.push_str(ANSI_GREEN);
        } else if idx == metroid_pos {
            out.push_str(ANSI_CYAN);
            out.push('o');
            out.push_str(ANSI_GREEN);
        } else {
            out.push(TILE[idx % TILE.len()] as char);
        }
    }
    out.push_str(ANSI_RESET);
    out
}

fn format_elapsed(elapsed: Duration) -> String {
    let total = elapsed.as_secs();
    let minutes = total / 60;
    let seconds = total % 60;
    format!("T+{:02}:{:02}", minutes, seconds)
}

pub fn create_backup(case: Case, config: ChronosConfig) -> Result<BackupResult> {
    let backup_manager = BackupManager::new(case, config);
    backup_manager.create_backup()
}
