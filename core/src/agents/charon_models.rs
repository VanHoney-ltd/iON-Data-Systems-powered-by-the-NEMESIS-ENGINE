//! CHARON Data Models

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const CHARON_ASSET_SCHEMA_VERSION: u32 = 1;

fn default_charon_asset_schema_version() -> u32 {
    CHARON_ASSET_SCHEMA_VERSION
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AssetRecord {
    #[serde(default = "default_charon_asset_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub uuid: String,
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub original_filename: Option<String>,
    #[serde(default)]
    pub file_path: Option<String>,
    #[serde(default)]
    pub thumbnail_path: Option<String>,
    #[serde(default)]
    pub live_photo_video_path: Option<String>,
    #[serde(default)]
    pub media_type: MediaType,
    #[serde(default)]
    pub file_size: Option<u64>,
    #[serde(default)]
    pub width: Option<i32>,
    #[serde(default)]
    pub height: Option<i32>,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub orientation: i32,
    #[serde(default)]
    pub created_date: DateTime<Utc>,
    #[serde(default)]
    pub added_date: DateTime<Utc>,
    #[serde(default)]
    pub modified_date: Option<DateTime<Utc>>,
    #[serde(default)]
    pub trashed_date: Option<DateTime<Utc>>,
    #[serde(default)]
    pub is_trashed: bool,
    #[serde(default)]
    pub is_hidden: bool,
    #[serde(default)]
    pub is_favorite: bool,
    #[serde(default)]
    pub is_cloud_asset: bool,
    #[serde(default)]
    pub cloud_state: Option<i32>,
    #[serde(default)]
    pub burst_uuid: Option<String>,
    #[serde(default)]
    pub burst_pick_type: Option<i32>,
    #[serde(default)]
    pub has_adjustments: bool,
    #[serde(default)]
    pub adjustment_date: Option<DateTime<Utc>>,
    #[serde(default)]
    pub exif_metadata: Option<ExifMetadata>,
    #[serde(default)]
    pub location_metadata: Option<LocationMetadata>,
    #[serde(default)]
    pub face_count: i32,
    #[serde(default)]
    pub album_ids: Vec<i64>,
    #[serde(default)]
    pub moment_id: Option<i64>,
    #[serde(default)]
    pub search_relevance: f64,
    #[serde(default)]
    pub hash: Option<String>,
    #[serde(default)]
    pub mime_type: String,
    #[serde(default)]
    pub uti: Option<String>,
    #[serde(default)]
    pub resolved_source_path: Option<String>,
    #[serde(default)]
    pub source_resolution_method: Option<String>,
    #[serde(default)]
    pub copy_state: Option<String>,
    #[serde(default)]
    pub copy_reason: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub enum MediaType {
    Photo,
    Video,
    LivePhoto,
    Panorama,
    Screenshot,
    Burst,
    Timelapse,
    Portrait,
    Selfie,
    SlowMo,
    Timecode,
    #[default]
    Other,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ExifMetadata {
    #[serde(default)]
    pub camera_make: Option<String>,
    #[serde(default)]
    pub camera_model: Option<String>,
    #[serde(default)]
    pub lens_model: Option<String>,
    #[serde(default)]
    pub aperture: Option<f64>,
    #[serde(default)]
    pub focal_length: Option<f64>,
    #[serde(default)]
    pub iso: Option<i32>,
    #[serde(default)]
    pub shutter_speed: Option<f64>,
    #[serde(default)]
    pub flash_fired: Option<bool>,
    #[serde(default)]
    pub metering_mode: Option<i32>,
    #[serde(default)]
    pub white_balance: Option<i32>,
    #[serde(default)]
    pub exposure_program: Option<i32>,
    #[serde(default)]
    pub software: Option<String>,
    #[serde(default)]
    pub color_space: Option<i32>,
    #[serde(default)]
    pub bits_per_sample: Option<i32>,
    #[serde(default)]
    pub compression: Option<i32>,
    #[serde(default)]
    pub exif_version: Option<String>,
    #[serde(default)]
    pub datetime_original: Option<DateTime<Utc>>,
    #[serde(default)]
    pub datetime_digitized: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct LocationMetadata {
    #[serde(default)]
    pub latitude: f64,
    #[serde(default)]
    pub longitude: f64,
    #[serde(default)]
    pub altitude: Option<f64>,
    #[serde(default)]
    pub speed: Option<f64>,
    #[serde(default)]
    pub heading: Option<f64>,
    #[serde(default)]
    pub horizontal_accuracy: Option<f64>,
    #[serde(default)]
    pub vertical_accuracy: Option<f64>,
    #[serde(default)]
    pub place_name: Option<String>,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub city: Option<String>,
    #[serde(default)]
    pub street: Option<String>,
    #[serde(default)]
    pub postal_code: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FaceRecord {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub asset_id: i64,
    #[serde(default)]
    pub person_id: Option<i64>,
    #[serde(default)]
    pub person_name: Option<String>,
    #[serde(default)]
    pub bounding_box: BoundingBox,
    #[serde(default)]
    pub face_angle: Option<f64>,
    #[serde(default)]
    pub face_confidence: f64,
    #[serde(default)]
    pub is_hidden: bool,
    #[serde(default)]
    pub is_manual: bool,
    #[serde(default)]
    pub detected_date: DateTime<Utc>,
    #[serde(default)]
    pub modified_date: Option<DateTime<Utc>>,
    #[serde(default)]
    pub face_age_type: Option<i32>,
    #[serde(default)]
    pub face_gender_type: Option<i32>,
    #[serde(default)]
    pub face_skintone_type: Option<i32>,
    #[serde(default)]
    pub face_hair_color_type: Option<i32>,
    #[serde(default)]
    pub face_bald_type: Option<i32>,
    #[serde(default)]
    pub face_eye_makeup_type: Option<i32>,
    #[serde(default)]
    pub face_lip_makeup_type: Option<i32>,
    #[serde(default)]
    pub face_eye_wear_type: Option<i32>,
    #[serde(default)]
    pub face_facial_hair_type: Option<i32>,
    #[serde(default)]
    pub face_smile_type: Option<i32>,
    #[serde(default)]
    pub cluster_sequence_number: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct BoundingBox {
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    #[serde(default)]
    pub width: f64,
    #[serde(default)]
    pub height: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AlbumRecord {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub album_type: AlbumType,
    #[serde(default)]
    pub created_date: DateTime<Utc>,
    #[serde(default)]
    pub start_date: Option<DateTime<Utc>>,
    #[serde(default)]
    pub end_date: Option<DateTime<Utc>>,
    #[serde(default)]
    pub asset_count: i32,
    #[serde(default)]
    pub parent_id: Option<i64>,
    #[serde(default)]
    pub is_hidden: bool,
    #[serde(default)]
    pub is_trashed: bool,
    #[serde(default)]
    pub cloud_owner_first_name: Option<String>,
    #[serde(default)]
    pub cloud_owner_last_name: Option<String>,
    #[serde(default)]
    pub cloud_local_state: Option<i32>,
    #[serde(default)]
    pub cloud_is_deletable: Option<bool>,
    #[serde(default)]
    pub cloud_is_my_asset: Option<bool>,
    #[serde(default)]
    pub asset_ids: Vec<i64>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub enum AlbumType {
    UserAlbum,
    SmartAlbum,
    Moment,
    MomentList,
    Project,
    Folder,
    SharedAlbum,
    CloudSharedAlbum,
    #[default]
    Unknown,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DeletedAssetRecord {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub uuid: String,
    #[serde(default)]
    pub original_filename: Option<String>,
    #[serde(default)]
    pub media_type: MediaType,
    #[serde(default)]
    pub trashed_date: DateTime<Utc>,
    #[serde(default)]
    pub file_still_exists: bool,
    #[serde(default)]
    pub thumbnail_still_exists: bool,
    #[serde(default)]
    pub metadata_still_exists: bool,
    #[serde(default)]
    pub estimated_deletion_date: Option<DateTime<Utc>>,
    #[serde(default)]
    pub recovery_status: RecoveryStatus,
    #[serde(default)]
    pub hash: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub enum RecoveryStatus {
    Recoverable,
    PartiallyRecoverable,
    NotRecoverable,
    #[default]
    Unknown,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MomentRecord {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub start_date: DateTime<Utc>,
    #[serde(default)]
    pub end_date: DateTime<Utc>,
    #[serde(default)]
    pub approximate_latitude: Option<f64>,
    #[serde(default)]
    pub approximate_longitude: Option<f64>,
    #[serde(default)]
    pub asset_count: i32,
    #[serde(default)]
    pub representative_asset_id: Option<i64>,
    #[serde(default)]
    pub place_name: Option<String>,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub city: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TimelineEntry {
    #[serde(default)]
    pub timestamp: DateTime<Utc>,
    #[serde(default)]
    pub event_type: TimelineEventType,
    #[serde(default)]
    pub asset_id: Option<i64>,
    #[serde(default)]
    pub location: Option<LocationMetadata>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub confidence: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub enum TimelineEventType {
    #[default]
    AssetCreated,
    AssetModified,
    AssetTrashed,
    LocationVisited,
    BurstCaptured,
    LivePhotoTaken,
}

#[derive(Debug, Clone)]
pub struct FilterOptions {
    pub date_range: Option<(DateTime<Utc>, DateTime<Utc>)>,
    pub media_types: Vec<MediaType>,
    pub has_gps: bool,
    pub has_faces: bool,
    pub is_favorite: Option<bool>,
    pub is_trashed: Option<bool>,
    pub min_width: Option<i32>,
    pub min_height: Option<i32>,
    pub location_bounds: Option<LocationBounds>,
    pub search_terms: Vec<String>,
    pub album_ids: Vec<i64>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct LocationBounds {
    pub min_lat: f64,
    pub max_lat: f64,
    pub min_lon: f64,
    pub max_lon: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CharonResult {
    #[serde(default)]
    pub assets: Vec<AssetRecord>,
    #[serde(default)]
    pub faces: Vec<FaceRecord>,
    #[serde(default)]
    pub albums: Vec<AlbumRecord>,
    #[serde(default)]
    pub deleted_assets: Vec<DeletedAssetRecord>,
    #[serde(default)]
    pub moments: Vec<MomentRecord>,
    #[serde(default)]
    pub timeline: Vec<TimelineEntry>,
}
