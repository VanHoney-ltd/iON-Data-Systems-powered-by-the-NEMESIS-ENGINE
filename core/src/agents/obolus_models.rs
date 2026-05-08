//! OBOLUS Data Models

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Note {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub html_body: Option<String>,
    #[serde(default)]
    pub plain_text: Option<String>,
    #[serde(default)]
    pub folder: String,
    #[serde(default)]
    pub folder_id: Option<i64>,
    #[serde(default)]
    pub account: Option<String>,
    #[serde(default)]
    pub account_id: Option<i64>,
    #[serde(default)]
    pub created: Option<DateTime<Local>>,
    #[serde(default)]
    pub modified: Option<DateTime<Local>>,
    #[serde(default)]
    pub deleted: Option<DateTime<Local>>,
    #[serde(default)]
    pub is_deleted: bool,
    #[serde(default)]
    pub is_password_protected: bool,
    #[serde(default)]
    pub is_locked: bool,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub notebook: Option<String>,
    #[serde(default)]
    pub share_status: Option<String>,
    #[serde(default)]
    pub sync_status: Option<String>,
    #[serde(default)]
    pub note_type: NoteType,
    #[serde(default)]
    pub word_count: usize,
    #[serde(default)]
    pub character_count: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub enum NoteType {
    #[default]
    Regular,
    Checklist,
    Scanned,
    Sketch,
    Audio,
    Video,
    WebClip,
    Document,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Folder {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub parent_id: Option<i64>,
    #[serde(default)]
    pub account_id: Option<i64>,
    #[serde(default)]
    pub created: Option<DateTime<Local>>,
    #[serde(default)]
    pub modified: Option<DateTime<Local>>,
    #[serde(default)]
    pub note_count: usize,
    #[serde(default)]
    pub is_smart_folder: bool,
    #[serde(default)]
    pub smart_folder_criteria: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Account {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub account_type: String,
    #[serde(default)]
    pub is_cloud: bool,
    #[serde(default)]
    pub last_sync: Option<DateTime<Local>>,
    #[serde(default)]
    pub note_count: usize,
    #[serde(default)]
    pub folder_count: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Attachment {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub note_id: i64,
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub mime_type: String,
    #[serde(default)]
    pub file_size: Option<u64>,
    #[serde(default)]
    pub created: Option<DateTime<Local>>,
    #[serde(default)]
    pub hash: Option<String>,
    #[serde(default)]
    pub file_path: Option<String>,
    #[serde(default)]
    pub is_embedded: bool,
    #[serde(default)]
    pub content_id: Option<String>,
    #[serde(default)]
    pub download_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub terms: Vec<String>,
    pub folders: Vec<String>,
    pub accounts: Vec<String>,
    pub note_types: Vec<NoteType>,
    pub date_range: Option<DateRange>,
    pub include_deleted: bool,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct DateRange {
    pub start: DateTime<Local>,
    pub end: DateTime<Local>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SearchResult {
    #[serde(default)]
    pub note_id: i64,
    #[serde(default)]
    pub relevance_score: f64,
    #[serde(default)]
    pub matching_terms: Vec<String>,
    #[serde(default)]
    pub context_snippet: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub modified: Option<DateTime<Local>>,
}

pub struct EvidencePackage {
    pub notes: Vec<Note>,
    pub folders: Vec<Folder>,
    pub accounts: Vec<Account>,
    pub attachments: Vec<Attachment>,
    pub hash_manifest: HashMap<String, String>,
}
