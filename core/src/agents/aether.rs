//! Aether — Deterministic location intelligence.

use anyhow::{Context, Result};
use chrono::Utc;
use exif::{In, Reader, Tag, Value};
use serde::{Deserialize, Serialize};
use serde_json;
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};


use crate::agents::charon_models::{AssetRecord, MediaType, CHARON_ASSET_SCHEMA_VERSION};
use crate::agents::{Agent, AgentCtx};
use crate::evidence::EvidenceRecord;

pub const AETHER_LOCATION_SCHEMA_VERSION: u32 = 1;

const SOURCE_AGENT: &str = "charon";
const EXIF_GPS_CONFIDENCE: f32 = 1.0;
const DB_GPS_CONFIDENCE: f32 = 0.65;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LocationRecord {
    pub schema_version: u32,
    pub asset_id: Option<String>,
    pub asset_numeric_id: Option<i64>,
    pub source_path: Option<std::path::PathBuf>,
    pub copied_path: Option<std::path::PathBuf>,
    pub timestamp_utc: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub altitude: Option<f64>,
    pub method: String,
    pub source_agent: String,
    pub source_resolution_method: Option<String>,
    pub confidence: f32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AetherReport {
    pub schema_version: u32,
    pub case_id: String,
    pub generated_at: String,
    pub state: String,
    pub source_agent: String,
    pub charon_asset_schema_version: u32,
    pub charon_assets_path: String,
    pub charon_copy_manifest_path: Option<String>,
    pub assets_examined: usize,
    pub assets_with_resolved_source_path: usize,
    pub assets_with_copied_path: usize,
    pub assets_with_gps: usize,
    pub exif_gps_records: usize,
    pub db_location_records: usize,
    pub assets_skipped: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
struct CopyManifest {
    #[serde(default)]
    entries: HashMap<String, CopyManifestEntry>,
}

#[derive(Debug, Deserialize, Clone)]
struct CopyManifestEntry {
    asset_id: String,
    source_path: String,
    destination_path: String,
}

#[derive(Debug, Clone)]
struct LocationExtractionStats {
    assets_examined: usize,
    assets_with_resolved_source_path: usize,
    assets_with_copied_path: usize,
    exif_gps_records: usize,
    db_location_records: usize,
    source_less_db_location_assets: usize,
    invalid_db_location_assets: usize,
}

#[derive(Debug)]
struct ExifGps {
    latitude: f64,
    longitude: f64,
    altitude: Option<f64>,
}

pub struct AetherAgent;

impl Agent for AetherAgent {
    const NAME: &'static str = "Aether";
    const SLUG: &'static str = "aether";
    const SCHEMA_VERSION: u32 = AETHER_LOCATION_SCHEMA_VERSION;

    fn extract(ctx: &AgentCtx) -> Result<Vec<EvidenceRecord>> {
        let charon_dir = ctx.case.evidence_path("charon");
        let assets_path = charon_dir.join("assets.json");
        let manifest_path = charon_dir.join("copy_manifest.json");

        let assets = load_charon_assets(&assets_path)?;
        let copy_manifest = load_copy_manifest(&manifest_path)?;

        let (locations, stats) = collect_locations(&assets, &copy_manifest, &charon_dir);

        let mut warnings = Vec::new();
        if stats.source_less_db_location_assets > 0 {
            warnings.push(format!(
                "{} assets had valid DB location metadata but no deterministic source or copied path",
                stats.source_less_db_location_assets
            ));
        }
        if stats.invalid_db_location_assets > 0 {
            warnings.push(format!(
                "{} assets had invalid or sentinel DB location metadata and were skipped",
                stats.invalid_db_location_assets
            ));
        }
        if !manifest_path.exists() {
            warnings.push(
                "No Charon copy_manifest.json present; copied-path linkage unavailable".to_string(),
            );
        }

        let report = AetherReport {
            schema_version: AETHER_LOCATION_SCHEMA_VERSION,
            case_id: ctx.case.name().to_string(),
            generated_at: Utc::now().to_rfc3339(),
            state: "complete".to_string(),
            source_agent: SOURCE_AGENT.to_string(),
            charon_asset_schema_version: assets
                .first()
                .map(|asset| asset.schema_version)
                .unwrap_or(CHARON_ASSET_SCHEMA_VERSION),
            charon_assets_path: assets_path.display().to_string(),
            charon_copy_manifest_path: manifest_path
                .exists()
                .then(|| manifest_path.display().to_string()),
            assets_examined: stats.assets_examined,
            assets_with_resolved_source_path: stats.assets_with_resolved_source_path,
            assets_with_copied_path: stats.assets_with_copied_path,
            assets_with_gps: locations.len(),
            exif_gps_records: stats.exif_gps_records,
            db_location_records: stats.db_location_records,
            assets_skipped: stats.assets_examined.saturating_sub(locations.len()),
            warnings,
        };

        let mut records = Vec::new();

        // Report record
        records.push(EvidenceRecord {
            schema_version: Self::SCHEMA_VERSION,
            source_agent: Self::NAME.to_string(),
            record_type: "report".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            payload: serde_json::to_value(&report)?,
        });

        // Location records
        for location in locations {
            records.push(EvidenceRecord {
                schema_version: Self::SCHEMA_VERSION,
                source_agent: Self::NAME.to_string(),
                record_type: "location".to_string(),
                timestamp: location.timestamp_utc.clone().unwrap_or_default(),
                payload: serde_json::to_value(&location)?,
            });
        }

        Ok(records)
    }
}

fn load_charon_assets(path: &Path) -> Result<Vec<AssetRecord>> {
    let raw = fs::read(path)
        .with_context(|| format!("Failed to read Charon assets at {}", path.display()))?;
    let assets: Vec<AssetRecord> = serde_json::from_slice(&raw)
        .with_context(|| format!("Failed to parse Charon assets JSON at {}", path.display()))?;
    Ok(assets)
}

fn load_copy_manifest(path: &Path) -> Result<CopyManifest> {
    if !path.exists() {
        return Ok(CopyManifest::default());
    }
    let raw = fs::read(path)
        .with_context(|| format!("Failed to read Charon copy manifest at {}", path.display()))?;
    let manifest: CopyManifest = serde_json::from_slice(&raw)
        .with_context(|| format!("Failed to parse Charon copy manifest at {}", path.display()))?;
    Ok(manifest)
}

fn collect_locations(
    assets: &[AssetRecord],
    copy_manifest: &CopyManifest,
    charon_dir: &Path,
) -> (Vec<LocationRecord>, LocationExtractionStats) {
    let mut locations = Vec::new();
    let mut stats = LocationExtractionStats {
        assets_examined: assets.len(),
        assets_with_resolved_source_path: 0,
        assets_with_copied_path: 0,
        exif_gps_records: 0,
        db_location_records: 0,
        source_less_db_location_assets: 0,
        invalid_db_location_assets: 0,
    };

    for asset in assets {
        let manifest_entry = manifest_entry_for(asset, copy_manifest);
        let source_path = resolved_source_path(asset, manifest_entry);
        let copied_path = resolved_copied_path(charon_dir, manifest_entry);

        if source_path.is_some() {
            stats.assets_with_resolved_source_path += 1;
        }
        if copied_path.is_some() {
            stats.assets_with_copied_path += 1;
        }

        let exif_path = source_path.as_ref().or(copied_path.as_ref());
        if let Some(path) = exif_path {
            if supports_exif_gps(path) {
                if let Ok(Some(gps)) = extract_exif_gps(path) {
                    locations.push(LocationRecord {
                        schema_version: AETHER_LOCATION_SCHEMA_VERSION,
                        asset_id: Some(asset.uuid.clone()),
                        asset_numeric_id: Some(asset.id),
                        source_path: source_path.clone(),
                        copied_path: copied_path.clone(),
                        timestamp_utc: Some(asset.created_date.to_rfc3339()),
                        latitude: Some(gps.latitude),
                        longitude: Some(gps.longitude),
                        altitude: gps.altitude,
                        method: "exif_gps".to_string(),
                        source_agent: SOURCE_AGENT.to_string(),
                        source_resolution_method: asset.source_resolution_method.clone(),
                        confidence: EXIF_GPS_CONFIDENCE,
                    });
                    stats.exif_gps_records += 1;
                    continue;
                }
            }
        }

        match valid_db_location(asset) {
            Some((latitude, longitude, altitude)) => {
                if source_path.is_none() && copied_path.is_none() {
                    stats.source_less_db_location_assets += 1;
                    // Still record DB location even without physical file
                }

                locations.push(LocationRecord {
                    schema_version: AETHER_LOCATION_SCHEMA_VERSION,
                    asset_id: Some(asset.uuid.clone()),
                    asset_numeric_id: Some(asset.id),
                    source_path: source_path.clone(),
                    copied_path: copied_path.clone(),
                    timestamp_utc: Some(asset.created_date.to_rfc3339()),
                    latitude: Some(latitude),
                    longitude: Some(longitude),
                    altitude,
                    method: "charon_location_metadata".to_string(),
                    source_agent: SOURCE_AGENT.to_string(),
                    source_resolution_method: asset.source_resolution_method.clone(),
                    confidence: DB_GPS_CONFIDENCE,
                });
                stats.db_location_records += 1;
            }
            None => {
                if asset.location_metadata.is_some() {
                    stats.invalid_db_location_assets += 1;
                }
            }
        }
    }

    (locations, stats)
}

fn manifest_entry_for<'a>(
    asset: &AssetRecord,
    manifest: &'a CopyManifest,
) -> Option<&'a CopyManifestEntry> {
    manifest
        .entries
        .get(&asset.uuid)
        .or_else(|| manifest.entries.get(&asset.id.to_string()))
        .or_else(|| {
            manifest.entries.values().find(|entry| {
                entry.asset_id == asset.uuid || entry.asset_id == asset.id.to_string()
            })
        })
}

fn resolved_source_path(
    asset: &AssetRecord,
    manifest_entry: Option<&CopyManifestEntry>,
) -> Option<PathBuf> {
    normalize_existing_path(asset.resolved_source_path.as_deref())
        .or_else(|| normalize_existing_path(manifest_entry.map(|entry| entry.source_path.as_str())))
}

fn resolved_copied_path(
    charon_dir: &Path,
    manifest_entry: Option<&CopyManifestEntry>,
) -> Option<PathBuf> {
    let raw = manifest_entry?.destination_path.trim();
    if raw.is_empty() {
        return None;
    }
    let candidate = PathBuf::from(raw);
    let resolved = if candidate.is_absolute() {
        candidate
    } else {
        charon_dir.join(candidate)
    };
    resolved.exists().then_some(resolved)
}

fn normalize_existing_path(raw: Option<&str>) -> Option<PathBuf> {
    let value = raw?.trim();
    if value.is_empty() {
        return None;
    }
    let candidate = PathBuf::from(value);
    candidate.exists().then_some(candidate)
}

fn valid_db_location(asset: &AssetRecord) -> Option<(f64, f64, Option<f64>)> {
    let meta = asset.location_metadata.as_ref()?;
    if !is_valid_latitude(meta.latitude) || !is_valid_longitude(meta.longitude) {
        return None;
    }
    Some((meta.latitude, meta.longitude, meta.altitude))
}

fn is_valid_latitude(value: f64) -> bool {
    value.is_finite() && (-90.0..=90.0).contains(&value)
}

fn is_valid_longitude(value: f64) -> bool {
    value.is_finite() && (-180.0..=180.0).contains(&value)
}

fn supports_exif_gps(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(ext.as_str(), "jpg" | "jpeg" | "tif" | "tiff")
}

fn extract_exif_gps(path: &Path) -> Result<Option<ExifGps>> {
    let file = File::open(path)
        .with_context(|| format!("Failed to open media file for EXIF at {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let exif = match Reader::new().read_from_container(&mut reader) {
        Ok(exif) => exif,
        Err(_) => return Ok(None),
    };

    let latitude_ref = exif
        .get_field(Tag::GPSLatitudeRef, In::PRIMARY)
        .and_then(|field| ascii_field_value(&field.value));
    let longitude_ref = exif
        .get_field(Tag::GPSLongitudeRef, In::PRIMARY)
        .and_then(|field| ascii_field_value(&field.value));
    let latitude = exif
        .get_field(Tag::GPSLatitude, In::PRIMARY)
        .and_then(|field| parse_gps_coordinate(&field.value, latitude_ref.as_deref()));
    let longitude = exif
        .get_field(Tag::GPSLongitude, In::PRIMARY)
        .and_then(|field| parse_gps_coordinate(&field.value, longitude_ref.as_deref()));

    let (latitude, longitude) = match (latitude, longitude) {
        (Some(latitude), Some(longitude))
            if is_valid_latitude(latitude) && is_valid_longitude(longitude) =>
        {
            (latitude, longitude)
        }
        _ => return Ok(None),
    };

    let altitude = exif
        .get_field(Tag::GPSAltitude, In::PRIMARY)
        .and_then(|field| parse_altitude(&field.value))
        .map(|altitude| {
            let is_below_sea_level = exif
                .get_field(Tag::GPSAltitudeRef, In::PRIMARY)
                .and_then(|field| parse_altitude_ref(&field.value))
                .unwrap_or(false);
            if is_below_sea_level {
                -altitude
            } else {
                altitude
            }
        });

    Ok(Some(ExifGps {
        latitude,
        longitude,
        altitude,
    }))
}

fn ascii_field_value(value: &Value) -> Option<String> {
    match value {
        Value::Ascii(values) => values.first().and_then(|bytes| {
            std::str::from_utf8(bytes)
                .ok()
                .map(|text| text.trim_matches(char::from(0)).trim().to_string())
                .filter(|text| !text.is_empty())
        }),
        _ => None,
    }
}

fn parse_gps_coordinate(value: &Value, direction: Option<&str>) -> Option<f64> {
    let components = match value {
        Value::Rational(values) if values.len() >= 3 => values,
        _ => return None,
    };

    let mut decimal =
        components[0].to_f64() + components[1].to_f64() / 60.0 + components[2].to_f64() / 3600.0;
    match direction.unwrap_or("").trim().to_ascii_uppercase().as_str() {
        "S" | "W" => decimal *= -1.0,
        _ => {}
    }
    Some(decimal)
}

fn parse_altitude(value: &Value) -> Option<f64> {
    match value {
        Value::Rational(values) => values.first().map(|value| value.to_f64()),
        _ => None,
    }
}

fn parse_altitude_ref(value: &Value) -> Option<bool> {
    match value {
        Value::Byte(values) => values.first().map(|value| *value == 1),
        _ => None,
    }
}

#[allow(dead_code)]
fn _media_type_is_visual(media_type: &MediaType) -> bool {
    matches!(
        media_type,
        MediaType::Photo
            | MediaType::Video
            | MediaType::LivePhoto
            | MediaType::Panorama
            | MediaType::Screenshot
            | MediaType::Burst
            | MediaType::Timelapse
            | MediaType::Portrait
            | MediaType::Selfie
            | MediaType::SlowMo
            | MediaType::Timecode
    )
}
