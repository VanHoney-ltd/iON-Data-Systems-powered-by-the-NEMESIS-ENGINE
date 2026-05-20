//! NYX Data Models

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HistoryItem {
    pub id: i64,
    pub url: String,
    pub title: Option<String>,
    pub visit_time: DateTime<Utc>,
    pub visit_count: i32,
    pub load_successful: bool,
    pub origin: Option<String>,
    pub redirect_source: Option<String>,
    pub redirect_destination: Option<String>,
    pub visit_duration: Option<i64>,
    #[serde(default)]
    pub http_status: Option<i32>,
    #[serde(default)]
    pub attributes: HashMap<String, String>,
    pub is_deleted: bool,
    #[serde(default)]
    pub tombstone_time: Option<DateTime<Utc>>,
    pub search_terms: Vec<String>,
    pub domain: String,
    pub search_engine: Option<SearchEngine>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum SearchEngine {
    Google,
    Bing,
    DuckDuckGo,
    Yahoo,
    YouTube,
    Internal,
    Other,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Bookmark {
    pub id: i64,
    pub title: Option<String>,
    pub url: Option<String>,
    pub folder: Option<String>,
    #[serde(default)]
    pub parent_id: Option<i64>,
    #[serde(default)]
    pub position: i32,
    pub created_time: DateTime<Utc>,
    pub modified_time: DateTime<Utc>,
    pub is_favorite: bool,
    #[serde(default)]
    pub sync_metadata: Option<String>,
    #[serde(default)]
    pub attributes: HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TabEntry {
    pub id: i64,
    pub url: String,
    pub title: Option<String>,
    pub device_name: String,
    #[serde(default)]
    pub device_id: String,
    pub last_viewed_time: DateTime<Utc>,
    pub is_pinned: bool,
    #[serde(default)]
    pub is_private: bool,
    #[serde(default)]
    pub session_data: Option<String>,
    pub sync_time: DateTime<Utc>,
    #[serde(default)]
    pub is_hidden: bool,
    #[serde(default)]
    pub is_deleted: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AutofillEntry {
    pub id: i64,
    pub field_name: String,
    pub value: String,
    // nyx/main.rs writes this as a plain String; keep flexible for display
    pub value_type: String,
    pub use_count: i32,
    pub first_used: DateTime<Utc>,
    pub last_used: DateTime<Utc>,
    #[serde(default)]
    pub correction_data: Option<String>,
    #[serde(default)]
    pub is_secure: bool,
    #[serde(default)]
    pub attributes: HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum AutofillValueType {
    Name,
    Email,
    Phone,
    Address,
    CreditCard,
    Password,
    Other,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SearchTerm {
    pub id: i64,
    pub term: String,
    // nyx/main.rs writes this as a plain String; keep flexible for display
    pub search_engine: String,
    pub timestamp: DateTime<Utc>,
    #[serde(default)]
    pub url_context: Option<String>,
    #[serde(default)]
    pub frequency: i32,
    #[serde(default)]
    pub is_autocomplete: bool,
    #[serde(default)]
    pub device_context: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ReadingListItem {
    pub id: i64,
    pub url: String,
    pub title: Option<String>,
    pub preview_text: Option<String>,
    pub added_time: DateTime<Utc>,
    pub archived_time: Option<DateTime<Utc>>,
    pub is_archived: bool,
    pub offline_data_path: Option<String>,
    pub estimated_read_time: Option<i32>,
    pub attributes: HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TimelineEntry {
    pub timestamp: DateTime<Utc>,
    pub event_type: TimelineEventType,
    pub source_device: Option<String>,
    pub url: Option<String>,
    pub title: Option<String>,
    pub related_events: Vec<i64>,
    pub duration: Option<i64>,
    pub confidence: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum TimelineEventType {
    Visit,
    Bookmark,
    Search,
    TabSwitch,
    ReadingListAdd,
    Autofill,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NyxResult {
    pub history: Vec<HistoryItem>,
    pub bookmarks: Vec<Bookmark>,
    pub tabs: Vec<TabEntry>,
    pub autofill: Vec<AutofillEntry>,
    pub searches: Vec<SearchTerm>,
    pub reading_list: Vec<ReadingListItem>,
    pub timeline: Vec<TimelineEntry>,
}

#[derive(Debug, Clone)]
pub struct FilterOptions {
    pub search_terms: Vec<String>,
    pub domains: Vec<String>,
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
    pub min_visit_count: Option<i32>,
    pub include_deleted: bool,
    pub search_engines: Vec<SearchEngine>,
    pub limit: Option<usize>,
}
