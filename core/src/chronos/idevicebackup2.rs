//! idevicebackup2 Command Builder
//!
//! Provides a type-safe builder for constructing idevicebackup2 commands
//! following the libimobiledevice spec:
//! Usage: idevicebackup2 [OPTIONS] CMD [CMDOPTIONS] DIRECTORY

use anyhow::{anyhow, Result};
use std::path::PathBuf;
use std::process::Command;

/// Commands supported by idevicebackup2
#[derive(Debug, Clone)]
pub enum IDeviceBackup2Command {
    /// Create backup for the device
    Backup {
        /// Force full backup from device
        full: bool,
        /// Supply the password for the encrypted backup
        password: Option<String>,
    },
    /// Restore last backup to the device
    Restore {
        /// Restore system files, too
        system: bool,
        /// Do NOT reboot the device when done (default: yes)
        no_reboot: bool,
        /// Create a copy of backup folder before restoring
        copy: bool,
        /// Restore device settings from the backup
        settings: bool,
        /// Remove items which are not being restored
        remove: bool,
        /// Do not trigger re-installation of apps after restore
        skip_apps: bool,
        /// Supply the password for the encrypted source backup
        password: Option<String>,
    },
    /// Show details about last completed backup of device
    Info,
    /// List files of last completed backup in CSV format
    List,
    /// Unpack a completed backup in DIRECTORY/_unback_/
    Unback,
    /// Enable or disable backup encryption
    Encryption {
        enabled: bool,
        password: Option<String>,
    },
    /// Change backup password on target device
    ChangePassword {
        old_password: String,
        new_password: String,
    },
    /// Enable or disable cloud use (requires iCloud account)
    Cloud { enabled: bool },
}

/// Options for idevicebackup2
#[derive(Debug, Clone, Default)]
pub struct IDeviceBackup2Options {
    /// Target specific device by UDID
    pub udid: Option<String>,
    /// Use backup data from device specified by UDID
    pub source: Option<String>,
    /// Connect to network device
    pub network: bool,
    /// Request passwords interactively
    pub interactive: bool,
    /// Enable communication debugging
    pub debug: bool,
}

/// Builder for idevicebackup2 commands
#[derive(Debug, Clone)]
pub struct IDeviceBackup2Builder {
    options: IDeviceBackup2Options,
    command: IDeviceBackup2Command,
    directory: PathBuf,
}

impl IDeviceBackup2Builder {
    /// Create a new backup command builder
    pub fn backup(directory: impl Into<PathBuf>) -> Self {
        Self {
            options: IDeviceBackup2Options::default(),
            command: IDeviceBackup2Command::Backup {
                full: false,
                password: None,
            },
            directory: directory.into(),
        }
    }

    /// Create a new full backup command builder
    pub fn full_backup(directory: impl Into<PathBuf>) -> Self {
        Self {
            options: IDeviceBackup2Options::default(),
            command: IDeviceBackup2Command::Backup {
                full: true,
                password: None,
            },
            directory: directory.into(),
        }
    }

    /// Create a new restore command builder
    pub fn restore(directory: impl Into<PathBuf>) -> Self {
        Self {
            options: IDeviceBackup2Options::default(),
            command: IDeviceBackup2Command::Restore {
                system: false,
                no_reboot: false,
                copy: false,
                settings: false,
                remove: false,
                skip_apps: false,
                password: None,
            },
            directory: directory.into(),
        }
    }

    /// Create a new info command builder
    pub fn info(directory: impl Into<PathBuf>) -> Self {
        Self {
            options: IDeviceBackup2Options::default(),
            command: IDeviceBackup2Command::Info,
            directory: directory.into(),
        }
    }

    /// Create a new list command builder
    pub fn list(directory: impl Into<PathBuf>) -> Self {
        Self {
            options: IDeviceBackup2Options::default(),
            command: IDeviceBackup2Command::List,
            directory: directory.into(),
        }
    }

    /// Create a new unback command builder
    pub fn unback(directory: impl Into<PathBuf>) -> Self {
        Self {
            options: IDeviceBackup2Options::default(),
            command: IDeviceBackup2Command::Unback,
            directory: directory.into(),
        }
    }

    /// Create a new encryption command builder
    pub fn encryption(directory: impl Into<PathBuf>, enabled: bool) -> Self {
        Self {
            options: IDeviceBackup2Options::default(),
            command: IDeviceBackup2Command::Encryption {
                enabled,
                password: None,
            },
            directory: directory.into(),
        }
    }

    /// Create a new change password command builder
    pub fn change_password(
        directory: impl Into<PathBuf>,
        old_password: impl Into<String>,
        new_password: impl Into<String>,
    ) -> Self {
        Self {
            options: IDeviceBackup2Options::default(),
            command: IDeviceBackup2Command::ChangePassword {
                old_password: old_password.into(),
                new_password: new_password.into(),
            },
            directory: directory.into(),
        }
    }

    /// Create a new cloud command builder
    pub fn cloud(directory: impl Into<PathBuf>, enabled: bool) -> Self {
        Self {
            options: IDeviceBackup2Options::default(),
            command: IDeviceBackup2Command::Cloud { enabled },
            directory: directory.into(),
        }
    }

    /// Set the target device UDID
    pub fn udid(mut self, udid: impl Into<String>) -> Self {
        self.options.udid = Some(udid.into());
        self
    }

    /// Set the source device UDID
    pub fn source(mut self, source: impl Into<String>) -> Self {
        self.options.source = Some(source.into());
        self
    }

    /// Enable network device connection
    pub fn network(mut self) -> Self {
        self.options.network = true;
        self
    }

    /// Enable interactive mode (password prompts)
    pub fn interactive(mut self) -> Self {
        self.options.interactive = true;
        self
    }

    /// Enable debug mode
    pub fn debug(mut self) -> Self {
        self.options.debug = true;
        self
    }

    /// Set the password for encrypted backups (used in non-interactive mode)
    pub fn password(mut self, password: impl Into<String>) -> Self {
        let pw = password.into();
        match &mut self.command {
            IDeviceBackup2Command::Backup { password: p, .. } => {
                *p = Some(pw);
            }
            IDeviceBackup2Command::Restore { password: p, .. } => {
                *p = Some(pw);
            }
            IDeviceBackup2Command::Encryption { password: p, .. } => {
                *p = Some(pw);
            }
            _ => {}
        }
        self
    }

    /// Configure restore options (only applicable for restore command)
    pub fn restore_options(
        mut self,
        system: bool,
        no_reboot: bool,
        copy: bool,
        settings: bool,
        remove: bool,
        skip_apps: bool,
    ) -> Self {
        if let IDeviceBackup2Command::Restore {
            system: s,
            no_reboot: nr,
            copy: c,
            settings: st,
            remove: r,
            skip_apps: sa,
            ..
        } = &mut self.command
        {
            *s = system;
            *nr = no_reboot;
            *c = copy;
            *st = settings;
            *r = remove;
            *sa = skip_apps;
        }
        self
    }

    /// Build the command arguments (without the executable name)
    pub fn build_args(&self) -> Vec<String> {
        let mut args = Vec::new();

        // Add options
        if let Some(udid) = &self.options.udid {
            args.push("-u".to_string());
            args.push(udid.clone());
        }

        if let Some(source) = &self.options.source {
            args.push("-s".to_string());
            args.push(source.clone());
        }

        if self.options.network {
            args.push("-n".to_string());
        }

        if self.options.interactive {
            args.push("-i".to_string());
        }

        if self.options.debug {
            args.push("-d".to_string());
        }

        // Add command and its options
        match &self.command {
            IDeviceBackup2Command::Backup { full, password } => {
                args.push("backup".to_string());
                if *full {
                    args.push("--full".to_string());
                }
                if let Some(pw) = password {
                    args.push("--password".to_string());
                    args.push(pw.clone());
                }
            }
            IDeviceBackup2Command::Restore {
                system,
                no_reboot,
                copy,
                settings,
                remove,
                skip_apps,
                password,
            } => {
                args.push("restore".to_string());
                if *system {
                    args.push("--system".to_string());
                }
                if *no_reboot {
                    args.push("--no-reboot".to_string());
                }
                if *copy {
                    args.push("--copy".to_string());
                }
                if *settings {
                    args.push("--settings".to_string());
                }
                if *remove {
                    args.push("--remove".to_string());
                }
                if *skip_apps {
                    args.push("--skip-apps".to_string());
                }
                if let Some(password) = password {
                    args.push("--password".to_string());
                    args.push(password.clone());
                }
            }
            IDeviceBackup2Command::Info => {
                args.push("info".to_string());
            }
            IDeviceBackup2Command::List => {
                args.push("list".to_string());
            }
            IDeviceBackup2Command::Unback => {
                args.push("unback".to_string());
            }
            IDeviceBackup2Command::Encryption { enabled, password } => {
                args.push("encryption".to_string());
                args.push(if *enabled {
                    "on".to_string()
                } else {
                    "off".to_string()
                });
                if let Some(pw) = password {
                    args.push(pw.clone());
                }
            }
            IDeviceBackup2Command::ChangePassword {
                old_password,
                new_password,
            } => {
                args.push("changepw".to_string());
                args.push(old_password.clone());
                args.push(new_password.clone());
            }
            IDeviceBackup2Command::Cloud { enabled } => {
                args.push("cloud".to_string());
                args.push(if *enabled {
                    "on".to_string()
                } else {
                    "off".to_string()
                });
            }
        }

        // Add directory
        args.push(self.directory.to_string_lossy().to_string());

        args
    }

    /// Build a std::process::Command from this builder
    pub fn build(&self) -> Command {
        let mut cmd = Command::new("idevicebackup2");
        cmd.args(self.build_args());
        cmd
    }

    /// Execute the command and return the output
    pub fn execute(&self) -> Result<std::process::Output> {
        let output = self
            .build()
            .output()
            .map_err(|e| anyhow!("Failed to execute idevicebackup2: {}", e))?;
        Ok(output)
    }

    /// Get the command as a string for logging/debugging
    pub fn to_command_string(&self) -> String {
        let args = self.build_args().join(" ");
        format!("idevicebackup2 {}", args)
    }
}

/// Helper function to enable encryption on a device
pub fn enable_encryption(
    directory: impl Into<PathBuf>,
    password: Option<&str>,
    interactive: bool,
) -> Result<std::process::Output> {
    let dir: PathBuf = directory.into();

    if interactive {
        let mut builder = IDeviceBackup2Builder::encryption(&dir, true).interactive();
        if let Some(pw) = password {
            builder = builder.password(pw);
        }
        builder.execute()
    } else {
        let mut builder = IDeviceBackup2Builder::encryption(&dir, true);
        if let Some(pw) = password {
            builder = builder.password(pw);
        }
        builder.execute()
    }
}

/// Helper function to perform a full encrypted backup
///
/// This follows the two-step process:
/// 1. Enable encryption: idevicebackup2 -i encryption on [PWD]
/// 2. Run backup: idevicebackup2 backup --full <directory>
pub fn full_encrypted_backup(
    directory: impl Into<PathBuf>,
    password: Option<&str>,
    udid: Option<&str>,
) -> Result<std::process::Output> {
    let dir: PathBuf = directory.into();

    // Step 1: Enable encryption
    let mut enc_builder = IDeviceBackup2Builder::encryption(&dir, true);

    // Use interactive mode only when no password is provided
    if password.is_none() {
        enc_builder = enc_builder.interactive();
    }

    if let Some(device_id) = udid {
        enc_builder = enc_builder.udid(device_id);
    }

    if let Some(pw) = password {
        enc_builder = enc_builder.password(pw);
    }

    let enc_output = enc_builder.execute()?;

    if !enc_output.status.success() {
        let stderr = String::from_utf8_lossy(&enc_output.stderr);
        return Err(anyhow!("Failed to enable backup encryption: {}", stderr));
    }

    // Step 2: Run full backup
    let mut backup_builder = IDeviceBackup2Builder::full_backup(&dir);

    if let Some(device_id) = udid {
        backup_builder = backup_builder.udid(device_id);
    }

    backup_builder.execute()
}

/// Helper to check if idevicebackup2 is available
pub fn check_tool_available() -> bool {
    Command::new("idevicebackup2")
        .arg("--help")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Get the version of idevicebackup2
pub fn get_version() -> Result<String> {
    let output = Command::new("idevicebackup2")
        .arg("--version")
        .output()
        .map_err(|e| anyhow!("Failed to get idevicebackup2 version: {}", e))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(anyhow!("idevicebackup2 --version failed"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backup_command() {
        let builder = IDeviceBackup2Builder::backup("/tmp/backup");
        let args = builder.build_args();
        assert!(args.contains(&"backup".to_string()));
        assert!(args.contains(&"/tmp/backup".to_string()));
    }

    #[test]
    fn test_full_backup_command() {
        let builder = IDeviceBackup2Builder::full_backup("/tmp/backup");
        let args = builder.build_args();
        assert!(args.contains(&"backup".to_string()));
        assert!(args.contains(&"--full".to_string()));
        assert!(args.contains(&"/tmp/backup".to_string()));
    }

    #[test]
    fn test_encryption_command() {
        let builder = IDeviceBackup2Builder::encryption("/tmp/backup", true);
        let args = builder.build_args();
        assert!(args.contains(&"encryption".to_string()));
        assert!(args.contains(&"on".to_string()));
    }

    #[test]
    fn test_with_udid() {
        let builder = IDeviceBackup2Builder::full_backup("/tmp/backup").udid("1234567890abcdef");
        let args = builder.build_args();
        assert!(args.contains(&"-u".to_string()));
        assert!(args.contains(&"1234567890abcdef".to_string()));
    }

    #[test]
    fn test_restore_command() {
        let builder = IDeviceBackup2Builder::restore("/tmp/backup")
            .restore_options(true, false, true, true, false, false);
        let args = builder.build_args();
        assert!(args.contains(&"restore".to_string()));
        assert!(args.contains(&"--system".to_string()));
        assert!(args.contains(&"--copy".to_string()));
        assert!(args.contains(&"--settings".to_string()));
    }
}
