//! Device Management and Pairing

use crate::case::Case;
use crate::chronos::error::ChronosError;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct DeviceManager {
    case: Case,
    config: crate::chronos::config::ChronosConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DeviceInfo {
    /// ideviceinfo: UniqueDeviceID
    pub udid: String,
    /// ideviceinfo: DeviceName
    pub device_name: String,
    /// ideviceinfo: ProductType
    pub product_type: String,
    /// ideviceinfo: ProductVersion
    pub product_version: String,
    /// ideviceinfo: SerialNumber
    pub serial_number: Option<String>,
    /// ideviceinfo: InternationalMobileEquipmentIdentity
    pub imei: Option<String>,
    /// Secondary IMEI when present on dual-SIM devices.
    pub imei1: Option<String>,
    /// Secondary IMEI when present on dual-SIM devices.
    pub imei2: Option<String>,
    /// ideviceinfo: IntegratedCircuitCardIdentity
    pub iccid: Option<String>,
    /// ideviceinfo: PhoneNumber
    pub phone_number: Option<String>,
    /// ideviceinfo: BuildVersion
    pub build_version: Option<String>,
    /// ideviceinfo: ModelNumber
    pub model_number: Option<String>,
    /// ideviceinfo: ProductName or DeviceClass depending on device.
    pub model_name: Option<String>,
    /// ideviceinfo: ChipID or ChipSerialNo.
    pub chipset: Option<String>,
    /// ideviceinfo: WiFiAddress
    pub wifi_address: Option<String>,
    /// ideviceinfo: BluetoothAddress
    pub bluetooth_address: Option<String>,
    /// Raw human-readable system fields that are useful for evidence notes.
    pub raw_fields: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PairingState {
    Paired,
    Unpaired,
    Unknown,
}

impl PairingState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Paired => "paired",
            Self::Unpaired => "unpaired",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolStatus {
    pub name: String,
    pub available: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IfuseMount {
    pub mount_point: PathBuf,
    pub udid: Option<String>,
    pub filesystem: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectedDeviceStatus {
    pub udid: String,
    pub pairing_state: PairingState,
    pub pairing_detail: Option<String>,
    pub suggested_mount_point: PathBuf,
    pub active_mounts: Vec<PathBuf>,
    pub device_info: Option<DeviceInfo>,
    pub info_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveDeviceStatus {
    pub connected_devices: Vec<ConnectedDeviceStatus>,
    pub paired_count: usize,
    pub mount_root: PathBuf,
    pub ifuse_mounts: Vec<IfuseMount>,
    pub tool_status: Vec<ToolStatus>,
}

impl LiveDeviceStatus {
    pub fn connected_count(&self) -> usize {
        self.connected_devices.len()
    }

    pub fn available_tool(&self, name: &str) -> bool {
        self.tool_status
            .iter()
            .any(|tool| tool.name == name && tool.available)
    }
}

pub fn probe_live_status(case: &Case) -> LiveDeviceStatus {
    let mount_root = case.workspace().live_device_mounts_dir();
    let _ = fs::create_dir_all(&mount_root);

    let tool_status = ["idevice_id", "idevicepair", "ideviceinfo", "ifuse"]
        .into_iter()
        .map(tool_status)
        .collect::<Vec<_>>();
    let ifuse_mounts = discover_ifuse_mounts(&mount_root);
    let connected_devices = discover_connected_devices(case, &mount_root, &ifuse_mounts);
    let paired_count = connected_devices
        .iter()
        .filter(|device| device.pairing_state == PairingState::Paired)
        .count();

    LiveDeviceStatus {
        connected_devices,
        paired_count,
        mount_root,
        ifuse_mounts,
        tool_status,
    }
}

impl DeviceInfo {
    pub fn parse(info_str: &str) -> Result<Self> {
        let mut info = DeviceInfo {
            udid: String::new(),
            device_name: String::new(),
            product_type: String::new(),
            product_version: String::new(),
            serial_number: None,
            imei: None,
            imei1: None,
            imei2: None,
            iccid: None,
            phone_number: None,
            build_version: None,
            model_number: None,
            model_name: None,
            chipset: None,
            wifi_address: None,
            bluetooth_address: None,
            raw_fields: HashMap::new(),
        };

        for line in info_str.lines() {
            if let Some((key, value)) = line.split_once(": ") {
                let key = key.trim();
                let value = value.trim().to_string();
                info.raw_fields.insert(key.to_string(), value.clone());
                match key {
                    "UniqueDeviceID" => info.udid = value,
                    "DeviceName" => info.device_name = value,
                    "ProductType" => info.product_type = value,
                    "ProductVersion" => info.product_version = value,
                    "SerialNumber" => info.serial_number = Some(value),
                    "InternationalMobileEquipmentIdentity" => info.imei = Some(value),
                    "IntegratedCircuitCardIdentity" => info.iccid = Some(value),
                    "PhoneNumber" => info.phone_number = Some(value),
                    "BuildVersion" => info.build_version = Some(value),
                    "ModelNumber" => info.model_number = Some(value),
                    "ProductName" | "DeviceClass" => info.model_name = Some(value),
                    "ChipID" | "ChipSerialNo" => info.chipset = Some(value),
                    "WiFiAddress" => info.wifi_address = Some(value),
                    "BluetoothAddress" => info.bluetooth_address = Some(value),
                    "IMEI" => info.imei = Some(value),
                    "IMEI1" => info.imei1 = Some(value),
                    "IMEI2" => info.imei2 = Some(value),
                    _ => {}
                }
            }
        }

        if info.udid.is_empty() {
            return Err(anyhow::anyhow!("Could not parse device UDID from info"));
        }

        Ok(info)
    }
}

impl DeviceManager {
    pub fn new(case: Case, config: crate::chronos::config::ChronosConfig) -> Self {
        Self { case, config }
    }

    pub fn pair_device(&self) -> Result<()> {
        self.log_operation("Device pairing", || {
            let output = Command::new("idevicepair")
                .arg("pair")
                .output()
                .context("Failed to execute idevicepair pair")?;

            if output.status.success() {
                self.case.log("CHRONOS — Device paired", None)?;
                Ok(())
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                self.case.log("CHRONOS — Pairing failed", Some(&stderr))?;
                Err(ChronosError::PairingFailed(stderr.to_string()).into())
            }
        })
    }

    pub fn validate_device(&self) -> Result<()> {
        self.log_operation("Device pairing validation", || {
            let output = Command::new("idevicepair")
                .arg("validate")
                .output()
                .context("Failed to execute idevicepair validate")?;

            if output.status.success() {
                self.case
                    .log("CHRONOS — Device paired and validated", None)?;
                Ok(())
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                self.case
                    .log("CHRONOS — Pairing validation failed", Some(&stderr))?;
                Err(ChronosError::PairingFailed(stderr.to_string()).into())
            }
        })
    }

    pub fn ensure_paired(&self) -> Result<()> {
        self.log_operation("Device pairing validation", || self.validate_pairing())
    }

    fn validate_pairing(&self) -> Result<()> {
        let output = Command::new("idevicepair")
            .arg("validate")
            .output()
            .context("Failed to execute idevicepair validate")?;

        if output.status.success() {
            self.case
                .log("CHRONOS — Device paired and validated", None)?;
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            self.case
                .log("CHRONOS — Pairing validation failed", Some(&stderr))?;
            self.attempt_pairing()
        }
    }

    fn attempt_pairing(&self) -> Result<()> {
        self.case.log("CHRONOS — Attempting device pairing", None)?;

        eprintln!("🔌 CONNECT YOUR DEVICE");
        eprintln!("📱 Make sure your iPhone/iPad is:");
        eprintln!("   1. Connected via USB cable");
        eprintln!("   2. Unlocked");
        eprintln!("   3. Showing the 'Trust This Computer?' dialog");
        eprintln!("   4. You have tapped 'Trust' on the device");
        eprintln!();

        // Try up to 3 times with delays
        for attempt in 1..=3 {
            eprintln!("🔄 Pairing attempt {} of 3...", attempt);

            let output = Command::new("idevicepair")
                .arg("pair")
                .output()
                .context("Failed to execute idevicepair pair")?;

            if output.status.success() {
                eprintln!("✅ Device paired successfully!");
                // Wait a moment and validate
                std::thread::sleep(Duration::from_secs(2));
                return self.validate_pairing();
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);

                eprintln!("⚠️  Pairing attempt {} failed:", attempt);
                if !stderr.is_empty() {
                    eprintln!("   Error: {}", stderr);
                }
                if !stdout.is_empty() {
                    eprintln!("   Output: {}", stdout);
                }

                self.case.log(
                    &format!("CHRONOS — Pairing attempt {} failed", attempt),
                    Some(&stderr),
                )?;

                if attempt < 3 {
                    eprintln!("⏳ Waiting 5 seconds before retry...");
                    eprintln!("   (Check your device for the Trust dialog)");
                    std::thread::sleep(Duration::from_secs(5));
                }
            }
        }

        eprintln!();
        eprintln!("❌ PAIRING FAILED");
        eprintln!("Common causes:");
        eprintln!("   • Device not connected via USB");
        eprintln!("   • Device is locked");
        eprintln!("   • 'Trust This Computer?' was not tapped");
        eprintln!("   • USB cable is charge-only (not data)");
        eprintln!();

        Err(
            ChronosError::PairingFailed("Failed to pair device after 3 attempts. Ensure device is connected, unlocked, and you have tapped 'Trust' on the device.".to_string())
                .into(),
        )
    }

    pub fn force_pair(&self) -> Result<()> {
        eprintln!("🔧 FORCE PAIRING MODE");

        // First, try to unpair any existing pairing
        let _ = Command::new("idevicepair").arg("unpair").output();

        // Wait a moment
        std::thread::sleep(Duration::from_secs(1));

        // Now attempt pairing again
        self.attempt_pairing()
    }

    pub fn mount_live_filesystem(&self, udid: &str) -> Result<PathBuf> {
        let mount_point = self.case.workspace().live_device_mounts_dir().join(udid);
        fs::create_dir_all(&mount_point)?;

        let output = Command::new("ifuse")
            .arg("-u")
            .arg(udid)
            .arg(&mount_point)
            .output()
            .context("Failed to execute ifuse")?;

        if output.status.success() {
            self.case.log(
                "CHRONOS — Live filesystem mounted",
                Some(&mount_point.display().to_string()),
            )?;
            Ok(mount_point)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(ChronosError::ToolExecutionFailed(format!("ifuse failed: {}", stderr)).into())
        }
    }

    pub fn unmount_live_filesystem(&self, mount: &Path) -> Result<()> {
        let fusermount = Command::new("fusermount").arg("-u").arg(mount).output();

        let status_ok = match fusermount {
            Ok(output) if output.status.success() => true,
            _ => Command::new("umount")
                .arg(mount)
                .output()
                .map(|output| output.status.success())
                .unwrap_or(false),
        };

        if status_ok {
            self.case.log(
                "CHRONOS — Live filesystem unmounted",
                Some(&mount.display().to_string()),
            )?;
            Ok(())
        } else {
            Err(
                ChronosError::ToolExecutionFailed(format!("Failed to unmount {}", mount.display()))
                    .into(),
            )
        }
    }

    pub fn get_device_info(&self) -> Result<DeviceInfo> {
        self.log_operation("Device info retrieval", || {
            let output = Command::new("ideviceinfo")
                .output()
                .context("Failed to execute ideviceinfo")?;

            if output.status.success() {
                let info_str = String::from_utf8_lossy(&output.stdout);
                DeviceInfo::parse(&info_str)
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                Err(
                    ChronosError::ToolExecutionFailed(format!("ideviceinfo failed: {}", stderr))
                        .into(),
                )
            }
        })
    }

    pub fn ensure_tools_available(&self) -> Result<()> {
        for tool in &self.config.required_tools {
            match Command::new(tool).arg("--help").output() {
                Ok(_) => continue,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Err(ChronosError::ToolExecutionFailed(format!(
                        "Required tool `{}` not found. Install libimobiledevice.",
                        tool
                    ))
                    .into());
                }
                Err(e) => {
                    return Err(ChronosError::ToolExecutionFailed(format!(
                        "Failed to launch {}: {}",
                        tool, e
                    ))
                    .into());
                }
            }
        }
        Ok(())
    }

    fn log_operation<T, F>(&self, operation: &str, f: F) -> Result<T>
    where
        F: FnOnce() -> Result<T>,
    {
        if self.config.verbose {
            self.case
                .log(&format!("CHRONOS — Starting: {}", operation), None)?;
        }

        let result = f();

        if self.config.verbose {
            match &result {
                Ok(_) => self
                    .case
                    .log(&format!("CHRONOS — Completed: {}", operation), None)?,
                Err(e) => self
                    .case
                    .log(&format!("CHRONOS — Failed: {} - {}", operation, e), None)?,
            }
        }

        result
    }
}

fn tool_status(name: &str) -> ToolStatus {
    let available = Command::new("bash")
        .arg("-lc")
        .arg(format!("command -v {} >/dev/null 2>&1", name))
        .status()
        .map(|status| status.success())
        .unwrap_or(false);

    ToolStatus {
        name: name.to_string(),
        available,
        detail: if available {
            "found on PATH".to_string()
        } else {
            "not installed or not on PATH".to_string()
        },
    }
}

fn discover_ifuse_mounts(mount_root: &Path) -> Vec<IfuseMount> {
    let mut mounts = Vec::new();
    if let Ok(entries) = fs::read_dir(mount_root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                mounts.push(IfuseMount {
                    udid: path
                        .file_name()
                        .map(|value| value.to_string_lossy().to_string()),
                    mount_point: path,
                    filesystem: "ifuse".to_string(),
                });
            }
        }
    }
    mounts
}

fn discover_connected_devices(
    _case: &Case,
    mount_root: &Path,
    ifuse_mounts: &[IfuseMount],
) -> Vec<ConnectedDeviceStatus> {
    let output = Command::new("idevice_id").arg("-l").output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|udid| {
            let suggested_mount_point = mount_root.join(udid);
            let pairing = pairing_state_for_udid(udid);
            let (device_info, info_error) = match device_info_for_udid(udid) {
                Ok(info) => (Some(info), None),
                Err(error) => (None, Some(error)),
            };
            let active_mounts = ifuse_mounts
                .iter()
                .filter(|mount| mount.mount_point == suggested_mount_point)
                .map(|mount| mount.mount_point.clone())
                .collect::<Vec<_>>();

            ConnectedDeviceStatus {
                udid: udid.to_string(),
                pairing_state: pairing.0,
                pairing_detail: pairing.1,
                suggested_mount_point,
                active_mounts,
                device_info,
                info_error,
            }
        })
        .collect()
}

fn pairing_state_for_udid(udid: &str) -> (PairingState, Option<String>) {
    let output = Command::new("idevicepair")
        .arg("-u")
        .arg(udid)
        .arg("validate")
        .output();

    match output {
        Ok(output) if output.status.success() => (PairingState::Paired, None),
        Ok(output) => {
            let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let detail = if detail.is_empty() {
                None
            } else {
                Some(detail)
            };
            (PairingState::Unpaired, detail)
        }
        Err(error) => (PairingState::Unknown, Some(error.to_string())),
    }
}

fn device_info_for_udid(udid: &str) -> std::result::Result<DeviceInfo, String> {
    let output = Command::new("ideviceinfo")
        .arg("-u")
        .arg(udid)
        .output()
        .map_err(|error| error.to_string())?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    let info_str = String::from_utf8_lossy(&output.stdout);
    DeviceInfo::parse(&info_str).map_err(|error| error.to_string())
}
