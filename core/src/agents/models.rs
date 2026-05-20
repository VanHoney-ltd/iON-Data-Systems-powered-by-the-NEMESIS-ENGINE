//! Shared types across all MINiOS agents.
//!
//! Only put types here that are used by 2+ agents.
//! Agent-specific structs stay in their agent files.

use serde::{Deserialize, Serialize};

/// Phone number with normalized and raw forms.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhoneNumber {
    pub raw: String,
    pub normalized: String,
    pub country_code: Option<String>,
}

/// Message direction: sent by device owner or received.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Direction {
    Sent,
    Received,
}

/// iOS timestamp wrapper (handles Core Data epoch, Unix epoch, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Timestamp {
    pub raw: i64,
    pub iso8601: String,
}

/// Geographic coordinate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoPoint {
    pub latitude: f64,
    pub longitude: f64,
    pub altitude: Option<f64>,
}

/// Generic media/file reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaRef {
    pub filename: String,
    pub relative_path: String,
    pub size_bytes: Option<u64>,
    pub mime_type: Option<String>,
}
