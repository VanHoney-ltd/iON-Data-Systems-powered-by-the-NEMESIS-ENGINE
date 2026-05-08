//! Runtime dependency checker for iON agents.
//!
//! Provides human-readable error messages when external tools are missing,
//! so users get "install X" instructions instead of cryptic "file not found" errors.

use anyhow::{bail, Result};
use std::path::PathBuf;
use std::process::Command;

/// A required external tool and how to install it.
pub struct ToolInfo {
    pub name: &'static str,
    pub required_by: &'static str,
    pub ubuntu: &'static str,
    pub macos: &'static str,
    pub windows: &'static str,
}

const TOOLS: &[ToolInfo] = &[
    ToolInfo {
        name: "idevicebackup2",
        required_by: "chronos (iOS backup acquisition)",
        ubuntu: "sudo apt install libimobiledevice6 libimobiledevice-utils",
        macos: "brew install libimobiledevice",
        windows: "Download libimobiledevice from https://github.com/libimobiledevice/libimobiledevice/releases",
    },
    ToolInfo {
        name: "idevicepair",
        required_by: "chronos (iOS device pairing)",
        ubuntu: "sudo apt install libimobiledevice6 libimobiledevice-utils",
        macos: "brew install libimobiledevice",
        windows: "Download libimobiledevice from https://github.com/libimobiledevice/libimobiledevice/releases",
    },
    ToolInfo {
        name: "mvt-ios",
        required_by: "chronos (iOS malware triage)",
        ubuntu: "pip install mvt",
        macos: "pip install mvt",
        windows: "pip install mvt",
    },
    ToolInfo {
        name: "ffmpeg",
        required_by: "cerberus / vox (audio normalization & transcription)",
        ubuntu: "sudo apt install ffmpeg",
        macos: "brew install ffmpeg",
        windows: "choco install ffmpeg or download from https://ffmpeg.org/download.html",
    },
    ToolInfo {
        name: "whisperx",
        required_by: "vox (AI transcription)",
        ubuntu: "pip install whisperx",
        macos: "pip install whisperx",
        windows: "pip install whisperx",
    },
    ToolInfo {
        name: "ollama",
        required_by: "psyche / cerberus (AI analysis)",
        ubuntu: "curl -fsSL https://ollama.com/install.sh | sh",
        macos: "brew install ollama",
        windows: "Download from https://ollama.com/download",
    },
];

/// Common directories where tools may be installed outside of PATH.
fn common_tool_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    // iON MVT virtualenv
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join("iON").join("mvtEnv").join("bin"));
        dirs.push(home.join("iON").join("mvtEnv").join("Scripts")); // Windows
        dirs.push(home.join(".local").join("bin"));
    }

    dirs
}

/// Resolve a tool name to its full path, searching PATH and common iON directories.
pub fn resolve_tool(name: &str) -> Option<PathBuf> {
    // Try PATH first
    if Command::new(name).arg("--help").output().is_ok()
        || Command::new(name).arg("-v").output().is_ok()
        || Command::new(name).arg("--version").output().is_ok()
    {
        return Some(PathBuf::from(name));
    }

    // Search common directories
    for dir in common_tool_dirs() {
        let candidate = dir.join(name);
        if candidate.exists() {
            return Some(candidate);
        }
        // Also try with .exe on Windows
        #[cfg(windows)]
        {
            let candidate_exe = dir.join(format!("{}.exe", name));
            if candidate_exe.exists() {
                return Some(candidate_exe);
            }
        }
    }

    None
}

/// Check whether a tool is available on PATH or in common iON directories.
pub fn is_available(name: &str) -> bool {
    resolve_tool(name).is_some()
}

/// Check a list of required tools and return a clear error if any are missing.
///
/// # Example
/// ```ignore
/// use minios::common::deps::check_required;
/// check_required(&["idevicebackup2", "idevicepair"])?;
/// ```
pub fn check_required(names: &[&str]) -> Result<()> {
    let mut missing = Vec::new();

    for name in names {
        if !is_available(name) {
            missing.push(*name);
        }
    }

    if missing.is_empty() {
        return Ok(());
    }

    let platform = std::env::consts::OS;
    let mut msg = format!(
        "Missing required external tool(s) for iON Data Systems:\n\n"
    );

    for name in &missing {
        if let Some(tool) = TOOLS.iter().find(|t| t.name == *name) {
            let install = match platform {
                "linux" => tool.ubuntu,
                "macos" => tool.macos,
                _ => tool.windows,
            };
            msg.push_str(&format!(
                "  ❌ {} (required by: {})\n     Install: {}\n\n",
                tool.name, tool.required_by, install
            ));
        } else {
            msg.push_str(&format!("  ❌ {} (unknown tool — please install it)\n\n", name));
        }
    }

    bail!("{}", msg.trim_end())
}

/// Print a warning for optional tools rather than failing.
pub fn warn_missing(names: &[&str]) {
    for name in names {
        if !is_available(name) {
            if let Some(tool) = TOOLS.iter().find(|t| t.name == *name) {
                eprintln!(
                    "⚠️  Optional tool '{}' not found (used by: {}). Some features will be unavailable.",
                    tool.name, tool.required_by
                );
            }
        }
    }
}
