//! iON Chronos - iOS Backup Acquisition, Validation, and WAL-Replay Engine
//!
//! This is the foundation of the iON analytic suite, responsible for:
//! - iOS device detection and pairing
//! - Encrypted/unencrypted backup handling
//! - WAL replay for SQLite consistency
//! - Backup integrity validation
//! - Path extraction for all downstream agents

#![allow(non_snake_case)]

pub mod agents;
pub mod case;
pub mod chronos;
pub mod common;
pub mod contact_index;
pub mod desktop_server;
pub mod evidence;
pub mod evidence_review;
pub mod nemesis;
pub mod run_all;
pub mod styg;
pub mod ui;

use std::path::Path;

pub use chronos::*;

pub fn ensure_dir<P: AsRef<Path>>(path: P) -> anyhow::Result<()> {
    std::fs::create_dir_all(path)?;
    Ok(())
}
