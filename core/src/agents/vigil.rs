//! Vigil — Deterministic video evidence inventory.

use anyhow::{Context, Result};
use serde_json::json;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

use crate::agents::charon_models::{AssetRecord, MediaType, CHARON_ASSET_SCHEMA_VERSION};
use crate::agents::intake::IntakeRecord;
use crate::agents::{Agent, AgentCtx};
use crate::common::prepared::compute_sha256;
use crate::evidence::EvidenceRecord;
use serde::{Deserialize, Serialize};

const VIGIL_VIDEO_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VideoEvidenceRecord {
    pub schema_version: u32,
    pub asset_id: Option<String>,
    pub asset_numeric_id: Option<i64>,
    pub filename: String,
    pub media_type: String,
    pub timestamp_utc: Option<String>,
    pub source_path: Option<PathBuf>,
    pub copied_path: Option<PathBuf>,
    pub source_resolution_method: Option<String>,
    pub duration_seconds: Option<f64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub container: Option<String>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub file_size: Option<u64>,
    pub sha256: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

pub struct VigilAgent;

impl Agent for VigilAgent {
    const NAME: &'static str = "Vigil";
    const SLUG: &'static str = "vigil";
    const SCHEMA_VERSION: u32 = VIGIL_VIDEO_SCHEMA_VERSION;

    fn extract(ctx: &AgentCtx) -> Result<Vec<EvidenceRecord>> {
        let charon_dir = ctx.case.evidence_path("charon");
        let assets_path = charon_dir.join("assets.json");
        let manifest_path = charon_dir.join("copy_manifest.json");

        let assets = load_charon_assets(&assets_path)?;
        let copy_manifest = load_copy_manifest(&manifest_path)?;
        let ffprobe_available = ffprobe_available();

        let (mut videos, mut stats) =
            collect_video_inventory(&assets, &copy_manifest, &charon_dir, ffprobe_available);
        let intake_videos = collect_intake_videos(ctx, ffprobe_available)?;
        stats.intake_video_files = intake_videos.len();
        videos.extend(intake_videos);
        export_vigil_outputs(ctx, &videos)?;

        let mut warnings = Vec::new();
        if !ffprobe_available {
            warnings
                .push("ffprobe not available; codec/container enrichment was skipped".to_string());
        }
        if stats.unresolved_videos > 0 {
            warnings.push(format!(
                "{} video assets had no deterministic source or copied path",
                stats.unresolved_videos
            ));
        }
        if stats.videos_with_hash == 0
            && stats.videos_with_copied_path == 0
            && stats.videos_with_resolved_source_path > 0
        {
            warnings.push(
                "Resolved source videos were not hashed by default; hashes are emitted when Charon provides them or copied/exported files exist".to_string(),
            );
        }

        // Build report as first record
        let mut records = Vec::new();
        records.push(EvidenceRecord {
            schema_version: Self::SCHEMA_VERSION,
            source_agent: Self::NAME.to_string(),
            record_type: "report".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            payload: json!({
                "case_id": ctx.case.name(),
                "state": "complete",
                "source_agent": "charon",
                "charon_asset_schema_version": assets.first().map(|a| a.schema_version).unwrap_or(CHARON_ASSET_SCHEMA_VERSION),
                "charon_assets_path": assets_path.display().to_string(),
                "charon_copy_manifest_path": manifest_path.exists().then(|| manifest_path.display().to_string()),
                "assets_examined": stats.assets_examined,
                "video_assets": stats.video_assets,
                "intake_video_files": stats.intake_video_files,
                "total_video_records": videos.len(),
                "videos_with_resolved_source_path": stats.videos_with_resolved_source_path,
                "videos_with_copied_path": stats.videos_with_copied_path,
                "videos_with_hash": stats.videos_with_hash,
                "ffprobe_enriched": stats.ffprobe_enriched,
                "unresolved_videos": stats.unresolved_videos,
                "warnings": warnings,
                "source_breakdown": source_breakdown(&videos),
            }),
        });

        // Build video records
        for video in videos {
            records.push(EvidenceRecord {
                schema_version: Self::SCHEMA_VERSION,
                source_agent: Self::NAME.to_string(),
                record_type: "video".to_string(),
                timestamp: video.timestamp_utc.clone().unwrap_or_default(),
                payload: json!({
                    "asset_id": video.asset_id,
                    "asset_numeric_id": video.asset_numeric_id,
                    "filename": video.filename,
                    "media_type": video.media_type,
                    "source_path": video.source_path.map(|p| p.display().to_string()),
                    "copied_path": video.copied_path.map(|p| p.display().to_string()),
                    "source_resolution_method": video.source_resolution_method,
                    "duration_seconds": video.duration_seconds,
                    "width": video.width,
                    "height": video.height,
                    "container": video.container,
                    "video_codec": video.video_codec,
                    "audio_codec": video.audio_codec,
                    "file_size": video.file_size,
                    "sha256": video.sha256,
                    "latitude": video.latitude,
                    "longitude": video.longitude,
                }),
            });
        }

        Ok(records)
    }
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

#[derive(Debug, Default)]
struct VideoStats {
    assets_examined: usize,
    video_assets: usize,
    videos_with_resolved_source_path: usize,
    videos_with_copied_path: usize,
    videos_with_hash: usize,
    ffprobe_enriched: usize,
    unresolved_videos: usize,
    intake_video_files: usize,
}

#[derive(Debug, Default)]
struct VideoProbe {
    container: Option<String>,
    video_codec: Option<String>,
    audio_codec: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    duration_seconds: Option<f64>,
    file_size: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
struct FfprobeOutput {
    #[serde(default)]
    streams: Vec<FfprobeStream>,
    format: Option<FfprobeFormat>,
}

#[derive(Debug, Deserialize, Default)]
struct FfprobeStream {
    codec_name: Option<String>,
    codec_type: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
}

#[derive(Debug, Deserialize, Default)]
struct FfprobeFormat {
    format_name: Option<String>,
    duration: Option<String>,
    size: Option<String>,
}

fn load_charon_assets(path: &Path) -> Result<Vec<AssetRecord>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read(path)
        .with_context(|| format!("Failed to read Charon assets at {}", path.display()))?;
    let assets: Vec<AssetRecord> = serde_json::from_slice(&raw)
        .with_context(|| format!("Failed to parse Charon assets JSON at {}", path.display()))?;
    Ok(assets)
}

fn collect_intake_videos(
    ctx: &AgentCtx,
    ffprobe_enabled: bool,
) -> Result<Vec<VideoEvidenceRecord>> {
    let intake_path = ctx
        .case
        .evidence_path("intake")
        .join("external_evidence.json");
    if !intake_path.exists() {
        return Ok(Vec::new());
    }
    let raw =
        fs::read(&intake_path).with_context(|| format!("reading {}", intake_path.display()))?;
    let intake_records: Vec<IntakeRecord> = serde_json::from_slice(&raw)
        .with_context(|| format!("parsing {}", intake_path.display()))?;
    let mut records = Vec::new();

    for item in intake_records {
        if item.media_type != "video" {
            continue;
        }
        let copied_path = PathBuf::from(&item.managed_path);
        let probe = if ffprobe_enabled {
            probe_video(&copied_path).ok().flatten().unwrap_or_default()
        } else {
            VideoProbe::default()
        };

        records.push(VideoEvidenceRecord {
            schema_version: VIGIL_VIDEO_SCHEMA_VERSION,
            asset_id: Some(item.record_id),
            asset_numeric_id: None,
            filename: item.file_name,
            media_type: "IntakeVideo".to_string(),
            timestamp_utc: item.modified_utc,
            source_path: Some(PathBuf::from(item.original_path)),
            copied_path: Some(copied_path),
            source_resolution_method: Some("intake_managed_copy".to_string()),
            duration_seconds: probe.duration_seconds,
            width: probe.width,
            height: probe.height,
            container: probe.container,
            video_codec: probe.video_codec,
            audio_codec: probe.audio_codec,
            file_size: probe.file_size.or(Some(item.size_bytes)),
            sha256: Some(item.sha256),
            latitude: None,
            longitude: None,
        });
    }
    if records.is_empty() {
        records.extend(collect_managed_intake_videos(ctx, ffprobe_enabled)?);
    }
    Ok(records)
}

fn collect_managed_intake_videos(
    ctx: &AgentCtx,
    ffprobe_enabled: bool,
) -> Result<Vec<VideoEvidenceRecord>> {
    let files_root = ctx.case.evidence_path("intake").join("files");
    if !files_root.exists() {
        return Ok(Vec::new());
    }
    let mut records = Vec::new();
    for entry in WalkDir::new(&files_root)
        .follow_links(false)
        .into_iter()
        .filter_map(|entry| entry.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if !is_video_path(path) {
            continue;
        }
        let metadata = entry
            .metadata()
            .with_context(|| format!("reading metadata {}", path.display()))?;
        let sha256 = compute_sha256(path).ok();
        let probe = if ffprobe_enabled {
            probe_video(path).ok().flatten().unwrap_or_default()
        } else {
            VideoProbe::default()
        };
        let filename = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("video")
            .to_string();
        records.push(VideoEvidenceRecord {
            schema_version: VIGIL_VIDEO_SCHEMA_VERSION,
            asset_id: sha256
                .as_ref()
                .map(|value| format!("intake_{}", &value[..16])),
            asset_numeric_id: None,
            filename,
            media_type: "IntakeVideo".to_string(),
            timestamp_utc: metadata
                .modified()
                .ok()
                .map(chrono::DateTime::<chrono::Utc>::from)
                .map(|value| value.to_rfc3339()),
            source_path: Some(path.to_path_buf()),
            copied_path: Some(path.to_path_buf()),
            source_resolution_method: Some("intake_managed_files_fallback".to_string()),
            duration_seconds: probe.duration_seconds,
            width: probe.width,
            height: probe.height,
            container: probe.container.or_else(|| infer_container_from_path(path)),
            video_codec: probe.video_codec,
            audio_codec: probe.audio_codec,
            file_size: probe.file_size.or(Some(metadata.len())),
            sha256,
            latitude: None,
            longitude: None,
        });
    }
    Ok(records)
}

fn is_video_path(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "mp4" | "mov" | "m4v" | "avi" | "mkv" | "webm" | "3gp"
    )
}

fn infer_container_from_path(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
}

fn export_vigil_outputs(ctx: &AgentCtx, videos: &[VideoEvidenceRecord]) -> Result<()> {
    fs::create_dir_all(&ctx.evidence_dir)?;
    fs::write(
        ctx.evidence_dir.join("video_catalog.json"),
        serde_json::to_vec_pretty(videos)?,
    )?;
    export_videos_csv(videos, &ctx.evidence_dir.join("video_catalog.csv"))?;
    export_videos_html(ctx, videos, &ctx.evidence_dir.join("index.html"))?;
    Ok(())
}

fn export_videos_csv(videos: &[VideoEvidenceRecord], path: &Path) -> Result<()> {
    let mut wtr = csv::Writer::from_path(path)?;
    wtr.write_record([
        "Asset ID",
        "Filename",
        "Media Type",
        "Timestamp UTC",
        "Source Path",
        "Copied Path",
        "Duration Seconds",
        "Width",
        "Height",
        "Container",
        "Video Codec",
        "Audio Codec",
        "File Size",
        "SHA256",
        "Latitude",
        "Longitude",
    ])?;
    for video in videos {
        wtr.write_record([
            video.asset_id.clone().unwrap_or_default(),
            video.filename.clone(),
            video.media_type.clone(),
            video.timestamp_utc.clone().unwrap_or_default(),
            video
                .source_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            video
                .copied_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            video
                .duration_seconds
                .map(|value| format!("{:.3}", value))
                .unwrap_or_default(),
            video
                .width
                .map(|value| value.to_string())
                .unwrap_or_default(),
            video
                .height
                .map(|value| value.to_string())
                .unwrap_or_default(),
            video.container.clone().unwrap_or_default(),
            video.video_codec.clone().unwrap_or_default(),
            video.audio_codec.clone().unwrap_or_default(),
            video
                .file_size
                .map(|value| value.to_string())
                .unwrap_or_default(),
            video.sha256.clone().unwrap_or_default(),
            video
                .latitude
                .map(|value| value.to_string())
                .unwrap_or_default(),
            video
                .longitude
                .map(|value| value.to_string())
                .unwrap_or_default(),
        ])?;
    }
    wtr.flush()?;
    Ok(())
}

fn export_videos_html(ctx: &AgentCtx, videos: &[VideoEvidenceRecord], path: &Path) -> Result<()> {
    let rows = videos
        .iter()
        .map(|video| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                html_escape(&video.media_type),
                html_escape(&video.filename),
                html_escape(&video.container.clone().unwrap_or_default()),
                html_escape(&video.video_codec.clone().unwrap_or_default()),
                video
                    .duration_seconds
                    .map(|value| format!("{:.1}s", value))
                    .unwrap_or_default(),
                html_escape(
                    &video
                        .copied_path
                        .as_ref()
                        .or(video.source_path.as_ref())
                        .map(|path| path.display().to_string())
                        .unwrap_or_default()
                )
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let html = format!(
        r#"<!doctype html>
<html><head><meta charset="utf-8"><title>Vigil Video Catalog - {case}</title>
<style>
body {{ font-family: Arial, sans-serif; margin: 32px; color: #1f2933; }}
h1 {{ font-size: 22px; margin-bottom: 4px; }}
.meta {{ color: #5c6873; margin-bottom: 20px; }}
table {{ border-collapse: collapse; width: 100%; font-size: 12px; }}
th, td {{ border: 1px solid #d8dee4; padding: 6px 8px; text-align: left; vertical-align: top; }}
th {{ background: #eef2f6; }}
td:last-child {{ overflow-wrap: anywhere; }}
</style></head><body>
<h1>Vigil Video Catalog</h1>
<div class="meta">Case: {case} | Videos: {count}</div>
<table><thead><tr><th>Type</th><th>File</th><th>Container</th><th>Video Codec</th><th>Duration</th><th>Path</th></tr></thead>
<tbody>{rows}</tbody></table>
</body></html>"#,
        case = html_escape(ctx.case.name()),
        count = videos.len(),
        rows = rows
    );
    fs::write(path, html)?;
    Ok(())
}

fn source_breakdown(videos: &[VideoEvidenceRecord]) -> BTreeMap<String, usize> {
    let mut breakdown = BTreeMap::new();
    for video in videos {
        *breakdown.entry(video.media_type.clone()).or_insert(0) += 1;
    }
    breakdown
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

fn collect_video_inventory(
    assets: &[AssetRecord],
    copy_manifest: &CopyManifest,
    charon_dir: &Path,
    ffprobe_enabled: bool,
) -> (Vec<VideoEvidenceRecord>, VideoStats) {
    let mut stats = VideoStats {
        assets_examined: assets.len(),
        ..VideoStats::default()
    };
    let mut records = Vec::new();

    for asset in assets {
        if !is_video_asset(asset) {
            continue;
        }
        stats.video_assets += 1;

        let manifest_entry = manifest_entry_for(asset, copy_manifest);
        let source_path = resolved_source_path(asset, manifest_entry);
        let copied_path = resolved_copied_path(charon_dir, manifest_entry);
        if source_path.is_some() {
            stats.videos_with_resolved_source_path += 1;
        }
        if copied_path.is_some() {
            stats.videos_with_copied_path += 1;
        }
        if source_path.is_none() && copied_path.is_none() {
            stats.unresolved_videos += 1;
        }

        let media_path = source_path.as_ref().or(copied_path.as_ref());
        let probe = if ffprobe_enabled {
            media_path
                .and_then(|path| probe_video(path).ok().flatten())
                .unwrap_or_default()
        } else {
            VideoProbe::default()
        };
        if probe.container.is_some()
            || probe.video_codec.is_some()
            || probe.audio_codec.is_some()
            || probe.width.is_some()
            || probe.height.is_some()
        {
            stats.ffprobe_enriched += 1;
        }

        let sha256 = copied_path
            .as_ref()
            .and_then(|path| compute_sha256(path).ok())
            .or_else(|| asset.hash.clone());
        if sha256.is_some() {
            stats.videos_with_hash += 1;
        }

        let (latitude, longitude) = valid_location(asset);
        records.push(VideoEvidenceRecord {
            schema_version: VIGIL_VIDEO_SCHEMA_VERSION,
            asset_id: Some(asset.uuid.clone()),
            asset_numeric_id: Some(asset.id),
            filename: asset.filename.clone(),
            media_type: format!("{:?}", asset.media_type),
            timestamp_utc: Some(asset.created_date.to_rfc3339()),
            source_path,
            copied_path,
            source_resolution_method: asset.source_resolution_method.clone(),
            duration_seconds: probe.duration_seconds.or(asset.duration),
            width: probe
                .width
                .or_else(|| asset.width.and_then(|value| u32::try_from(value).ok())),
            height: probe
                .height
                .or_else(|| asset.height.and_then(|value| u32::try_from(value).ok())),
            container: probe.container.or_else(|| infer_container(asset)),
            video_codec: probe.video_codec,
            audio_codec: probe.audio_codec,
            file_size: probe.file_size.or(asset.file_size),
            sha256,
            latitude,
            longitude,
        });
    }

    (records, stats)
}

fn is_video_asset(asset: &AssetRecord) -> bool {
    matches!(
        asset.media_type,
        MediaType::Video
            | MediaType::LivePhoto
            | MediaType::Timelapse
            | MediaType::SlowMo
            | MediaType::Timecode
    ) || asset.mime_type.starts_with("video/")
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

fn probe_video(path: &Path) -> Result<Option<VideoProbe>> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=format_name,duration,size:stream=codec_name,codec_type,width,height",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .with_context(|| format!("Failed to run ffprobe on {}", path.display()))?;

    if !output.status.success() {
        return Ok(None);
    }

    let parsed: FfprobeOutput = serde_json::from_slice(&output.stdout)
        .with_context(|| format!("Failed to parse ffprobe output for {}", path.display()))?;

    let mut video_codec = None;
    let mut audio_codec = None;
    let mut width = None;
    let mut height = None;

    for stream in parsed.streams {
        match stream.codec_type.as_deref() {
            Some("video") => {
                if video_codec.is_none() {
                    video_codec = stream.codec_name;
                }
                if width.is_none() {
                    width = stream.width;
                }
                if height.is_none() {
                    height = stream.height;
                }
            }
            Some("audio") => {
                if audio_codec.is_none() {
                    audio_codec = stream.codec_name;
                }
            }
            _ => {}
        }
    }

    let container = parsed
        .format
        .as_ref()
        .and_then(|format| format.format_name.clone());
    let duration_seconds = parsed
        .format
        .as_ref()
        .and_then(|format| format.duration.as_deref())
        .and_then(parse_f64);
    let file_size = parsed
        .format
        .as_ref()
        .and_then(|format| format.size.as_deref())
        .and_then(parse_u64);

    Ok(Some(VideoProbe {
        container,
        video_codec,
        audio_codec,
        width,
        height,
        duration_seconds,
        file_size,
    }))
}

fn parse_f64(raw: &str) -> Option<f64> {
    raw.trim().parse::<f64>().ok()
}

fn parse_u64(raw: &str) -> Option<u64> {
    raw.trim().parse::<u64>().ok()
}

fn ffprobe_available() -> bool {
    Command::new("ffprobe")
        .arg("-version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn infer_container(asset: &AssetRecord) -> Option<String> {
    let candidate = asset
        .original_filename
        .as_deref()
        .unwrap_or(&asset.filename);
    let ext = Path::new(candidate)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let container = match ext.as_str() {
        "mp4" | "m4v" => Some("mp4".to_string()),
        "mov" => Some("mov".to_string()),
        "3gp" => Some("3gp".to_string()),
        "avi" => Some("avi".to_string()),
        _ if asset.mime_type.starts_with("video/") => {
            Some(asset.mime_type.trim_start_matches("video/").to_string())
        }
        _ => None,
    };
    container
}

fn valid_location(asset: &AssetRecord) -> (Option<f64>, Option<f64>) {
    let Some(location) = asset.location_metadata.as_ref() else {
        return (None, None);
    };
    if location.latitude.is_finite()
        && location.longitude.is_finite()
        && (-90.0..=90.0).contains(&location.latitude)
        && (-180.0..=180.0).contains(&location.longitude)
    {
        (Some(location.latitude), Some(location.longitude))
    } else {
        (None, None)
    }
}

fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_charon_assets_returns_empty_list() {
        let tmp = tempfile::tempdir().unwrap();
        let assets = load_charon_assets(&tmp.path().join("missing.json")).unwrap();
        assert!(assets.is_empty());
    }

    #[test]
    fn exports_video_catalog_csv() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("videos.csv");
        let videos = vec![VideoEvidenceRecord {
            schema_version: VIGIL_VIDEO_SCHEMA_VERSION,
            asset_id: Some("intake_1".to_string()),
            asset_numeric_id: None,
            filename: "camera.mp4".to_string(),
            media_type: "IntakeVideo".to_string(),
            timestamp_utc: None,
            source_path: Some(PathBuf::from("/tmp/camera.mp4")),
            copied_path: Some(PathBuf::from("/tmp/managed.mp4")),
            source_resolution_method: Some("intake_managed_copy".to_string()),
            duration_seconds: Some(4.0),
            width: Some(1920),
            height: Some(1080),
            container: Some("mov,mp4,m4a,3gp,3g2,mj2".to_string()),
            video_codec: Some("h264".to_string()),
            audio_codec: Some("aac".to_string()),
            file_size: Some(10),
            sha256: Some("abc".to_string()),
            latitude: None,
            longitude: None,
        }];

        export_videos_csv(&videos, &path).unwrap();
        let csv = fs::read_to_string(path).unwrap();

        assert!(csv.contains("camera.mp4"));
        assert!(csv.contains("h264"));
    }
}
