//! Charon — Photo/media extraction agent.

use anyhow::Result;
use chrono::{DateTime, TimeZone, Utc};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::agents::charon_models::{AssetRecord, LocationMetadata, MediaType};
use crate::agents::{Agent, AgentCtx};
use crate::common::prepared::{prepare_artifact, PrepareContext};
use crate::common::resolver::{ArtifactResolver, BackupResolver};
use crate::common::target::photos_target;
use crate::evidence::EvidenceRecord;

pub struct CharonAgent;

/// Apple Cocoa epoch: 2001-01-01 00:00:00 UTC
const APPLE_EPOCH: i64 = 978307200;

fn apple_timestamp_to_utc(ts: f64) -> DateTime<Utc> {
    let unix_ts = ts + APPLE_EPOCH as f64;
    Utc.timestamp_opt(unix_ts as i64, ((unix_ts.fract()) * 1_000_000_000.0) as u32)
        .single()
        .unwrap_or_else(|| Utc::now())
}

fn map_media_type(kind: Option<i64>, kind_subtype: Option<i64>) -> MediaType {
    match (kind, kind_subtype) {
        (Some(1), Some(100)) => MediaType::SlowMo,
        (Some(1), Some(101)) => MediaType::Timelapse,
        (Some(1), Some(103)) => MediaType::LivePhoto,
        (Some(1), _) => MediaType::Video,
        (Some(0), Some(1)) => MediaType::Panorama,
        (Some(0), Some(2)) => MediaType::Screenshot,
        (Some(0), Some(4)) => MediaType::Portrait,
        (Some(0), Some(5)) => MediaType::Selfie,
        (Some(0), Some(6)) => MediaType::Timelapse,
        (Some(0), Some(8)) => MediaType::Burst,
        (Some(0), _) => MediaType::Photo,
        _ => MediaType::Other,
    }
}

impl Agent for CharonAgent {
    const NAME: &'static str = "Charon";
    const SLUG: &'static str = "charon";
    const SCHEMA_VERSION: u32 = 1;

    fn extract(ctx: &AgentCtx) -> Result<Vec<EvidenceRecord>> {
        let resolver = BackupResolver::from_case(&ctx.case)?;
        let target = photos_target();
        let resolved = match resolver.resolve_known_target(&target)? {
            Some(r) => r,
            None => {
                ctx.log("Charon: Photos.sqlite not found in backup");
                return Ok(vec![EvidenceRecord {
                    schema_version: Self::SCHEMA_VERSION,
                    source_agent: Self::NAME.to_string(),
                    record_type: "report".to_string(),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    payload: json!({
                        "case_id": ctx.case.name(),
                        "state": "skipped",
                        "reason": "Photos.sqlite not found",
                    }),
                }]);
            }
        };

        let prepared = prepare_artifact(
            &PrepareContext {
                case_root: ctx.case.root_path(),
                clean_root: ctx.case.root_path().join("clean"),
                temp_root: ctx.case.root_path().join("tmp"),
            },
            &resolved,
            target.sqlite_like,
        )?;

        let conn = prepared.open_sqlite_ro()?;

        let query = r#"
            SELECT
                Z_PK,
                ZUUID,
                ZFILENAME,
                ZDIRECTORY,
                ZDATECREATED,
                ZADDEDDATE,
                ZMODIFICATIONDATE,
                ZTRASHEDDATE,
                ZKIND,
                ZKINDSUBTYPE,
                ZWIDTH,
                ZHEIGHT,
                ZDURATION,
                ZORIENTATION,
                ZTRASHEDSTATE,
                ZHIDDEN,
                ZFAVORITE,
                ZLATITUDE,
                ZLONGITUDE,
                ZUNIFORMTYPEIDENTIFIER
            FROM ZASSET
            WHERE ZTRASHEDSTATE = 0
            ORDER BY ZDATECREATED DESC
        "#;

        let mut stmt = conn.prepare(query)?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, Option<i64>>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<f64>>(4)?,
                row.get::<_, Option<f64>>(5)?,
                row.get::<_, Option<f64>>(6)?,
                row.get::<_, Option<f64>>(7)?,
                row.get::<_, Option<i64>>(8)?,
                row.get::<_, Option<i64>>(9)?,
                row.get::<_, Option<i32>>(10)?,
                row.get::<_, Option<i32>>(11)?,
                row.get::<_, Option<f64>>(12)?,
                row.get::<_, Option<i32>>(13)?,
                row.get::<_, Option<i64>>(14)?,
                row.get::<_, Option<i64>>(15)?,
                row.get::<_, Option<i64>>(16)?,
                row.get::<_, Option<f64>>(17)?,
                row.get::<_, Option<f64>>(18)?,
                row.get::<_, Option<String>>(19)?,
            ))
        })?;

        let mut assets: Vec<AssetRecord> = Vec::new();
        let mut records: Vec<EvidenceRecord> = Vec::new();

        for row in rows.filter_map(|r| r.ok()) {
            let (
                id,
                uuid,
                filename,
                directory,
                date_created,
                added_date,
                modification_date,
                trashed_date,
                kind,
                kind_subtype,
                width,
                height,
                duration,
                orientation,
                trashed_state,
                hidden,
                favorite,
                latitude,
                longitude,
                uti,
            ) = row;

            let media_type = map_media_type(kind, kind_subtype);
            let created_date = date_created
                .map(apple_timestamp_to_utc)
                .unwrap_or_else(|| Utc::now());
            let added_date_dt = added_date
                .map(apple_timestamp_to_utc)
                .unwrap_or(created_date);
            let modified_date = modification_date.map(apple_timestamp_to_utc);
            let trashed_date_dt = trashed_date.map(apple_timestamp_to_utc);

            let location = if let (Some(lat), Some(lon)) = (latitude, longitude) {
                if lat != 0.0 || lon != 0.0 {
                    Some(LocationMetadata {
                        latitude: lat,
                        longitude: lon,
                        ..Default::default()
                    })
                } else {
                    None
                }
            } else {
                None
            };

            let file_path = if let (Some(dir), Some(name)) = (directory.as_ref(), filename.as_ref())
            {
                if dir.len() == 1 {
                    Some(format!("Media/PhotoData/{}/{}", dir, name))
                } else {
                    Some(format!("{}/{}", dir, name))
                }
            } else {
                None
            };

            let mime_type = uti
                .as_ref()
                .map(|u| uti_to_mime(u))
                .unwrap_or_else(|| "image/jpeg".to_string());

            let asset = AssetRecord {
                schema_version: crate::agents::charon_models::CHARON_ASSET_SCHEMA_VERSION,
                id: id.unwrap_or(0),
                uuid: uuid.unwrap_or_default(),
                filename: filename.unwrap_or_default(),
                original_filename: None,
                file_path: file_path.clone(),
                thumbnail_path: None,
                live_photo_video_path: None,
                media_type,
                file_size: None,
                width,
                height,
                duration: if duration.map(|d| d > 0.0).unwrap_or(false) {
                    duration
                } else {
                    None
                },
                orientation: orientation.unwrap_or(1),
                created_date,
                added_date: added_date_dt,
                modified_date: modified_date,
                trashed_date: trashed_date_dt,
                is_trashed: trashed_state == Some(1),
                is_hidden: hidden == Some(1),
                is_favorite: favorite == Some(1),
                is_cloud_asset: false,
                cloud_state: None,
                burst_uuid: None,
                burst_pick_type: None,
                has_adjustments: false,
                adjustment_date: None,
                exif_metadata: None,
                location_metadata: location,
                face_count: 0,
                album_ids: Vec::new(),
                moment_id: None,
                search_relevance: 0.0,
                hash: None,
                mime_type,
                uti,
                resolved_source_path: file_path,
                source_resolution_method: Some("photos_sqlite".to_string()),
                copy_state: None,
                copy_reason: None,
            };

            assets.push(asset.clone());
            records.push(EvidenceRecord {
                schema_version: Self::SCHEMA_VERSION,
                source_agent: Self::NAME.to_string(),
                record_type: "asset".to_string(),
                timestamp: chrono::Utc::now().to_rfc3339(),
                payload: serde_json::to_value(&asset)?,
            });
        }

        // Write assets.json for Aether agent
        if !assets.is_empty() {
            let charon_dir = ctx.case.root_path().join("evidence").join("charon");
            fs::create_dir_all(&charon_dir)?;
            let assets_path = charon_dir.join("assets.json");
            let json = serde_json::to_string_pretty(&assets)?;
            fs::write(&assets_path, json)?;
            ctx.log(&format!(
                "Charon: wrote {} assets to {}",
                assets.len(),
                assets_path.display()
            ));

            // Generate media catalog PDF
            if let Err(e) = generate_media_catalog(ctx, &assets) {
                ctx.log(&format!("Charon media catalog generation failed: {}", e));
            }
        }

        // Report record
        records.push(EvidenceRecord {
            schema_version: Self::SCHEMA_VERSION,
            source_agent: Self::NAME.to_string(),
            record_type: "report".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            payload: json!({
                "case_id": ctx.case.name(),
                "state": "complete",
                "records_emitted": assets.len(),
                "warnings": Vec::<String>::new(),
            }),
        });

        Ok(records)
    }
}

// ---------------------------------------------------------------------------
// Media Catalog PDF Generation
// ---------------------------------------------------------------------------

const CATALOG_CHUNK_SIZE: usize = 500;
const THUMB_WIDTH: u32 = 200;
const THUMB_HEIGHT: u32 = 200;

/// Build a lookup of lowercase filename -> actual backup path for fast resolution.
fn build_backup_file_index(case_root: &Path) -> HashMap<String, PathBuf> {
    let mut index = HashMap::new();
    let backup_dir = case_root.join("prepared").join("helios").join("full.styg");

    if !backup_dir.exists() {
        return index;
    }

    for entry in walkdir::WalkDir::new(&backup_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            index.insert(name.to_lowercase(), path.to_path_buf());
        }
    }

    index
}

/// Pre-generate thumbnails for all assets that have matching backup files.
/// Returns a map of asset.id -> thumbnail PathBuf.
fn generate_thumbnails(
    assets: &[AssetRecord],
    backup_index: &HashMap<String, PathBuf>,
    thumbs_dir: &Path,
) -> HashMap<i64, PathBuf> {
    let mut thumb_map = HashMap::new();

    for asset in assets {
        let src_path = if let Some(path) = backup_index.get(&asset.filename.to_lowercase()) {
            path.clone()
        } else if let Some(path) = resolve_asset_path(asset) {
            path
        } else {
            continue;
        };

        let thumb_path = thumbs_dir.join(format!("{}_thumb.jpg", asset.id));
        if !thumb_path.exists() {
            let is_video = matches!(
                asset.media_type,
                MediaType::Video | MediaType::SlowMo | MediaType::Timelapse | MediaType::LivePhoto
            );

            let result = if is_video {
                Command::new("ffmpeg")
                    .args([
                        "-i",
                        src_path.to_str().unwrap_or(""),
                        "-ss",
                        "00:00:01",
                        "-vframes",
                        "1",
                        "-vf",
                        &format!("scale={}:{}", THUMB_WIDTH, THUMB_HEIGHT),
                        "-y",
                        thumb_path.to_str().unwrap_or(""),
                    ])
                    .output()
            } else {
                Command::new("convert")
                    .args([
                        src_path.to_str().unwrap_or(""),
                        "-resize",
                        &format!("{}x{}", THUMB_WIDTH, THUMB_HEIGHT),
                        "-quality",
                        "75",
                        thumb_path.to_str().unwrap_or(""),
                    ])
                    .output()
            };

            if let Ok(out) = result {
                if !out.status.success() || !thumb_path.exists() {
                    // ffmpeg fallback for HEIC/other formats ImageMagick can't handle
                    if !is_video {
                        let _ = Command::new("ffmpeg")
                            .args([
                                "-i",
                                src_path.to_str().unwrap_or(""),
                                "-vf",
                                &format!("scale={}:{}", THUMB_WIDTH, THUMB_HEIGHT),
                                "-frames:v",
                                "1",
                                "-y",
                                thumb_path.to_str().unwrap_or(""),
                            ])
                            .output();
                    }
                }
            }
        }

        if thumb_path.exists() {
            thumb_map.insert(asset.id, thumb_path);
        }
    }

    thumb_map
}

/// Fallback path resolution when backup index misses.
fn resolve_asset_path(asset: &AssetRecord) -> Option<PathBuf> {
    let case_root = PathBuf::from("/home/ghost/iON/cases/pcr");
    let backup_root = case_root.join("prepared").join("helios").join("full.styg");

    let fp = asset.file_path.as_ref()?;

    let candidates = if fp.starts_with("DCIM/") {
        vec![backup_root.join("CameraRollDomain").join("Media").join(fp)]
    } else if fp.starts_with("Media/PhotoData/") {
        let parts: Vec<&str> = fp.split('/').collect();
        if parts.len() >= 4 {
            let hash_dir = parts[2];
            let filename = parts[3];
            vec![
                backup_root.join("CameraRollDomain").join(fp),
                backup_root
                    .join("CameraRollDomain")
                    .join("Media")
                    .join("PhotoData")
                    .join("UBF")
                    .join("scopes")
                    .join("syndication")
                    .join("originals")
                    .join(hash_dir)
                    .join(filename),
            ]
        } else {
            vec![backup_root.join("CameraRollDomain").join(fp)]
        }
    } else {
        vec![backup_root.join("CameraRollDomain").join("Media").join(fp)]
    };

    candidates.into_iter().find(|p| p.exists())
}

fn generate_media_catalog(ctx: &AgentCtx, assets: &[AssetRecord]) -> Result<()> {
    let evidence_dir = ctx.case.evidence_path("charon");
    fs::create_dir_all(&evidence_dir)?;

    let backup_index = build_backup_file_index(&ctx.case.root_path());
    ctx.log(&format!(
        "Charon: indexed {} backup files for catalog",
        backup_index.len()
    ));

    let thumbs_dir = evidence_dir.join("catalog_thumbs");
    fs::create_dir_all(&thumbs_dir)?;

    ctx.log("Charon: generating thumbnails...");
    let thumb_map = generate_thumbnails(assets, &backup_index, &thumbs_dir);
    ctx.log(&format!("Charon: generated {} thumbnails", thumb_map.len()));

    let chunks: Vec<&[AssetRecord]> = assets.chunks(CATALOG_CHUNK_SIZE).collect();
    let mut chunk_pdfs: Vec<PathBuf> = Vec::new();

    for (idx, chunk) in chunks.iter().enumerate() {
        let html_path = evidence_dir.join(format!("catalog_chunk_{}.html", idx));
        let pdf_path = evidence_dir.join(format!("catalog_chunk_{}.pdf", idx));

        let html = build_catalog_html(ctx.case.name(), chunk, idx, chunks.len(), &thumb_map);
        fs::write(&html_path, html)?;

        let lo_out = Command::new("libreoffice")
            .args([
                "--headless",
                "--convert-to",
                "pdf",
                "--outdir",
                evidence_dir.to_str().unwrap(),
                html_path.to_str().unwrap(),
            ])
            .output()?;

        let lo_pdf = html_path.with_extension("pdf");
        if lo_out.status.success() && lo_pdf.exists() {
            if lo_pdf != pdf_path {
                fs::rename(&lo_pdf, &pdf_path)?;
            }
        } else {
            let wp_out = Command::new("weasyprint")
                .args([html_path.to_str().unwrap(), pdf_path.to_str().unwrap()])
                .output()?;
            if !wp_out.status.success() {
                anyhow::bail!("PDF conversion failed for chunk {}", idx);
            }
        }

        let _ = fs::remove_file(&html_path);
        chunk_pdfs.push(pdf_path);
    }

    let final_pdf = evidence_dir.join("media_catalog.pdf");
    if chunk_pdfs.len() == 1 {
        fs::rename(&chunk_pdfs[0], &final_pdf)?;
    } else {
        let mut args = vec!["--empty".to_string(), "--pages".to_string()];
        for pdf in &chunk_pdfs {
            args.push(pdf.to_str().unwrap().to_string());
        }
        args.push("--".to_string());
        args.push(final_pdf.to_str().unwrap().to_string());

        let out = Command::new("qpdf").args(&args).output()?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            anyhow::bail!("qpdf merge failed: {}", stderr);
        }

        for pdf in &chunk_pdfs {
            let _ = fs::remove_file(pdf);
        }
    }

    ctx.log(&format!(
        "Charon: media catalog PDF generated: {} ({} assets, {} thumbnails)",
        final_pdf.display(),
        assets.len(),
        thumb_map.len()
    ));

    Ok(())
}

fn build_catalog_html(
    case_name: &str,
    assets: &[AssetRecord],
    chunk_idx: usize,
    total_chunks: usize,
    thumb_map: &HashMap<i64, PathBuf>,
) -> String {
    let now = Utc::now().to_rfc3339();

    // Build table rows with 3 items per row for LibreOffice compatibility
    let mut rows = Vec::new();
    for chunk in assets.chunks(3) {
        let cells: String = chunk
            .iter()
            .map(|asset| build_catalog_cell(asset, thumb_map))
            .collect();
        // Pad with empty cells if row has fewer than 3 items
        let pad = 3 - chunk.len();
        let padding = (0..pad)
            .map(|_| r#"<td class="item"><div class="thumb"><div class="placeholder">—</div></div></td>"#)
            .collect::<String>();
        rows.push(format!("<tr>{}{}</tr>", cells, padding));
    }

    let table_body = rows.join("\n");

    format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>Charon Media Catalog — {case}</title>
<style>
  @page {{ size: A4 portrait; margin: 12mm; @bottom-center {{ content: "Page " counter(page); font-size: 7pt; color: #888; }} }}
  body {{ font-family: "Segoe UI", Roboto, Helvetica, Arial, sans-serif; font-size: 8pt; color: #222; margin: 0; padding: 10px; }}
  h1 {{ font-size: 14pt; color: #1a1a2e; margin-bottom: 3px; }}
  .subtitle {{ color: #666; font-size: 8pt; margin-bottom: 12px; }}
  table {{ width: 100%; border-collapse: separate; border-spacing: 8px; table-layout: fixed; }}
  td {{ width: 33.33%; vertical-align: top; border: 1px solid #ddd; border-radius: 4px; padding: 5px; page-break-inside: avoid; }}
  .thumb {{ position: relative; width: 100%; height: 130px; background: #f0f0f0; text-align: center; border-radius: 3px; overflow: hidden; }}
  .thumb img {{ max-width: 100%; max-height: 130px; }}
  .thumb .placeholder {{ color: #999; font-size: 9pt; text-align: center; padding: 8px; }}
  .play-overlay {{ position: absolute; top: 50%; left: 50%; transform: translate(-50%, -50%); font-size: 24px; color: white; text-shadow: 0 0 6px rgba(0,0,0,0.7); pointer-events: none; }}
  .info {{ margin-top: 4px; }}
  .meta-line {{ font-size: 6.5pt; color: #444; line-height: 1.3; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }}
  .meta-line.loc {{ color: #0f3460; }}
</style>
</head>
<body>
  <h1>Charon Media Catalog</h1>
  <div class="subtitle">Case: {case} &nbsp;|&nbsp; Chunk {chunk} of {total} &nbsp;|&nbsp; Generated: {now}</div>
  <table>
    {table_body}
  </table>
</body>
</html>"#,
        case = escape_html(case_name),
        chunk = chunk_idx + 1,
        total = total_chunks,
        now = escape_html(&now),
        table_body = table_body,
    )
}

fn build_catalog_cell(asset: &AssetRecord, thumb_map: &HashMap<i64, PathBuf>) -> String {
    let is_video = matches!(
        asset.media_type,
        MediaType::Video | MediaType::SlowMo | MediaType::Timelapse | MediaType::LivePhoto
    );

    let (img_tag, has_file) = resolve_asset_image_tag(asset, thumb_map);

    let meta = format!(
        "<div class=\"meta-line\"><strong>{}</strong></div>\
         <div class=\"meta-line\">{} &nbsp;|&nbsp; {}x{}</div>\
         <div class=\"meta-line\">{}</div>",
        escape_html(&asset.filename),
        media_type_label(&asset.media_type),
        asset.width.unwrap_or(0),
        asset.height.unwrap_or(0),
        asset.created_date.format("%Y-%m-%d %H:%M")
    );

    let loc = asset
        .location_metadata
        .as_ref()
        .map(|l| {
            format!(
                "<div class=\"meta-line loc\">{:.4}, {:.4}</div>",
                l.latitude, l.longitude
            )
        })
        .unwrap_or_default();

    let video_overlay = if is_video && has_file {
        r#"<div class="play-overlay">▶</div>"#.to_string()
    } else {
        String::new()
    };

    format!(
        r#"<td class="item">
  <div class="thumb">{}{}</div>
  <div class="info">{}{}</div>
</td>"#,
        img_tag, video_overlay, meta, loc
    )
}

fn resolve_asset_image_tag(
    asset: &AssetRecord,
    thumb_map: &HashMap<i64, PathBuf>,
) -> (String, bool) {
    if let Some(thumb_path) = thumb_map.get(&asset.id) {
        return (
            format!(
                r#"<img src="{}" alt="{}" />"#,
                escape_html(thumb_path.to_str().unwrap_or("")),
                escape_html(&asset.filename)
            ),
            true,
        );
    }

    // No file found — show placeholder
    let color = match asset.media_type {
        MediaType::Photo => "#4a90d9",
        MediaType::Video | MediaType::SlowMo | MediaType::Timelapse | MediaType::LivePhoto => {
            "#d94a4a"
        }
        MediaType::Screenshot => "#4ad9a6",
        MediaType::Portrait => "#d94ad0",
        MediaType::Selfie => "#d9a64a",
        MediaType::Panorama => "#6a4ad9",
        MediaType::Burst => "#4ad9d9",
        _ => "#888888",
    };

    (
        format!(
            r#"<div class="placeholder" style="color:{};"><div style="font-size:18pt;margin-bottom:4px;">{}</div>{}</div>"#,
            color,
            media_type_emoji(&asset.media_type),
            escape_html(&asset.filename)
        ),
        false,
    )
}

fn media_type_label(mt: &MediaType) -> &'static str {
    match mt {
        MediaType::Photo => "Photo",
        MediaType::Video => "Video",
        MediaType::LivePhoto => "Live Photo",
        MediaType::Panorama => "Panorama",
        MediaType::Screenshot => "Screenshot",
        MediaType::Burst => "Burst",
        MediaType::Timelapse => "Timelapse",
        MediaType::Portrait => "Portrait",
        MediaType::Selfie => "Selfie",
        MediaType::SlowMo => "Slow-Mo",
        MediaType::Timecode => "Timecode",
        MediaType::Other => "Other",
    }
}

fn media_type_emoji(mt: &MediaType) -> &'static str {
    match mt {
        MediaType::Photo => "📷",
        MediaType::Video => "🎬",
        MediaType::LivePhoto => "📸",
        MediaType::Panorama => "🌄",
        MediaType::Screenshot => "📱",
        MediaType::Burst => "📷",
        MediaType::Timelapse => "⏱",
        MediaType::Portrait => "👤",
        MediaType::Selfie => "🤳",
        MediaType::SlowMo => "🐌",
        MediaType::Timecode => "⏰",
        MediaType::Other => "📄",
    }
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn uti_to_mime(uti: &str) -> String {
    match uti {
        "public.jpeg" => "image/jpeg",
        "public.heic" => "image/heic",
        "public.png" => "image/png",
        "public.tiff" => "image/tiff",
        "com.apple.quicktime-movie" => "video/quicktime",
        "public.mpeg-4" => "video/mp4",
        "com.compuserve.gif" => "image/gif",
        "dyn.ah62d4rv4ge81g6pq" => "image/jpeg",
        _ => "application/octet-stream",
    }
    .to_string()
}
