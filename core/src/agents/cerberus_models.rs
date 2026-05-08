//! CERBERUS Data Models (unified comms: messages, calls, voicemail, contacts)

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Messages / Attachments (original cerberus)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Message {
    #[serde(rename = "Date")]
    pub date: String,
    #[serde(rename = "Timestamp")]
    pub timestamp: i64,
    #[serde(rename = "Direction")]
    pub direction: String,
    #[serde(rename = "Phone")]
    pub phone_number: String,
    #[serde(rename = "Text")]
    pub text: String,
    #[serde(rename = "Service")]
    pub service: String,
    #[serde(rename = "iMessage")]
    pub is_imessage: bool,
    #[serde(rename = "Attachments")]
    pub has_attachments: bool,
    #[serde(rename = "MessageID")]
    pub message_id: Option<i64>,
    #[serde(rename = "ThreadID")]
    pub thread_id: Option<i64>,
    #[serde(rename = "SearchRelevance")]
    pub search_relevance: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Attachment {
    #[serde(default)]
    pub date: i64,
    #[serde(default)]
    pub timestamp: i64,
    #[serde(default)]
    pub direction: String,
    #[serde(default)]
    pub phone_number: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub transfer_name: Option<String>,
    #[serde(default)]
    pub message_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchTerm {
    #[serde(default)]
    pub term: String,
    #[serde(default)]
    pub term_type: SearchTermType,
    #[serde(default)]
    pub case_sensitive: bool,
    #[serde(default)]
    pub whole_word: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum SearchTermType {
    ExactMatch,
    #[default]
    Contains,
    Regex,
    Fuzzy,
}

#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub terms: Vec<SearchTerm>,
    pub phone_numbers: Vec<String>,
    pub start_date: Option<chrono::NaiveDate>,
    pub end_date: Option<chrono::NaiveDate>,
    pub ignore_country_code: bool,
    pub message_types: Vec<MessageType>,
    pub limit: Option<usize>,
    pub offset: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum MessageType {
    SMS,
    MMS,
    #[serde(rename = "iMessage")]
    IMessage,
    #[default]
    All,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SearchResult {
    #[serde(default)]
    pub message_id: Option<i64>,
    #[serde(default)]
    pub relevance_score: f64,
    #[serde(default)]
    pub matching_terms: Vec<String>,
    #[serde(default)]
    pub context_snippet: String,
    #[serde(default)]
    pub phone_number: String,
    #[serde(default)]
    pub timestamp: i64,
}

pub trait HasPhone {
    fn phone(&self) -> &str;
}

pub trait HasTimestamp {
    fn timestamp(&self) -> i64;
}

impl HasPhone for Message {
    fn phone(&self) -> &str {
        &self.phone_number
    }
}

impl HasTimestamp for Message {
    fn timestamp(&self) -> i64 {
        self.timestamp
    }
}

impl HasPhone for Attachment {
    fn phone(&self) -> &str {
        self.phone_number.as_deref().unwrap_or("")
    }
}

impl HasTimestamp for Attachment {
    fn timestamp(&self) -> i64 {
        self.timestamp
    }
}

// ---------------------------------------------------------------------------
// Contacts (from original contacts agent)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactRecord {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub phones: Vec<String>,
    #[serde(default)]
    pub emails: Vec<String>,
    #[serde(default)]
    pub urls: Vec<String>,
    pub organization: Option<String>,
    pub note: Option<String>,
    pub birthday: Option<String>,
    #[serde(default)]
    pub job_title: Option<String>,
    #[serde(default)]
    pub nickname: Option<String>,
    #[serde(default)]
    pub blocked: bool,
    #[serde(default)]
    pub department: Option<String>,
}

// ---------------------------------------------------------------------------
// Call history / Voicemail / Audio (from original hermes agent)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CallRecord {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub timestamp: i64,
    #[serde(default)]
    pub duration: i64,
    #[serde(default)]
    pub call_type: String,
    #[serde(default)]
    pub phone_number: String,
    #[serde(default)]
    pub answered: bool,
    #[serde(default)]
    pub face_time: bool,
    #[serde(default)]
    pub call_provider: Option<String>,
    #[serde(default)]
    pub country_code: Option<String>,
    #[serde(default)]
    pub network: Option<String>,
    #[serde(default)]
    pub disconnected_cause: Option<String>,
    #[serde(default)]
    pub bytes_sent: Option<i64>,
    #[serde(default)]
    pub bytes_received: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VoicemailRecord {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub date: Option<i64>,
    #[serde(default)]
    pub timestamp: i64,
    #[serde(default)]
    pub duration: Option<i64>,
    #[serde(default)]
    pub phone_number: Option<String>,
    #[serde(default)]
    pub sender: Option<String>,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub file_path: Option<String>,
    #[serde(default)]
    pub file_size: Option<u64>,
    #[serde(default)]
    pub hash: Option<String>,
    #[serde(default)]
    pub transcript_path: Option<String>,
    #[serde(default)]
    pub diarization_path: Option<String>,
    #[serde(default)]
    pub confidence: Option<f64>,
    #[serde(default)]
    pub speakers: Option<Vec<String>>,
    #[serde(default)]
    pub is_transcribed: bool,
    #[serde(default)]
    pub is_diarized: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AudioRecord {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub source_type: AudioSource,
    #[serde(default)]
    pub source_id: Option<i64>,
    #[serde(default)]
    pub file_path: String,
    #[serde(default)]
    pub original_format: String,
    #[serde(default)]
    pub converted_path: Option<String>,
    #[serde(default)]
    pub duration: f64,
    #[serde(default)]
    pub sample_rate: i32,
    #[serde(default)]
    pub channels: i32,
    #[serde(default)]
    pub file_size: u64,
    #[serde(default)]
    pub hash: String,
    #[serde(default)]
    pub extracted_date: DateTime<Local>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub enum AudioSource {
    #[default]
    Voicemail,
    CallRecording,
    MMSAttachment,
    FaceTimeAudio,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Transcript {
    #[serde(default)]
    pub audio_id: i64,
    #[serde(default)]
    pub segments: Vec<TranscriptSegment>,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub confidence: f64,
    #[serde(default)]
    pub word_count: usize,
    #[serde(default)]
    pub duration: f64,
    #[serde(default)]
    pub generated_date: DateTime<Local>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TranscriptSegment {
    #[serde(default)]
    pub id: usize,
    #[serde(default)]
    pub start_time: f64,
    #[serde(default)]
    pub end_time: f64,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub confidence: f64,
    #[serde(default)]
    pub words: Vec<Word>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Word {
    #[serde(default)]
    pub word: String,
    #[serde(default)]
    pub start_time: f64,
    #[serde(default)]
    pub end_time: f64,
    #[serde(default)]
    pub confidence: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SpeakerSegment {
    #[serde(default)]
    pub id: usize,
    #[serde(default)]
    pub start_time: f64,
    #[serde(default)]
    pub end_time: f64,
    #[serde(default)]
    pub speaker: String,
    #[serde(default)]
    pub confidence: f64,
    #[serde(default)]
    pub is_overlapping: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DiarizedTranscript {
    #[serde(default)]
    pub audio_id: i64,
    #[serde(default)]
    pub speaker_segments: Vec<SpeakerSegment>,
    #[serde(default)]
    pub transcript_segments: Vec<TranscriptSegment>,
    #[serde(default)]
    pub merged_transcript: String,
    #[serde(default)]
    pub speaker_mapping: HashMap<String, Option<String>>,
    #[serde(default)]
    pub confidence: f64,
    #[serde(default)]
    pub generated_date: DateTime<Local>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HermesResult {
    #[serde(default)]
    pub calls: Vec<CallRecord>,
    #[serde(default)]
    pub voicemails: Vec<VoicemailRecord>,
    #[serde(default)]
    pub audio_files: Vec<AudioRecord>,
    #[serde(default)]
    pub transcripts: Vec<Transcript>,
    #[serde(default)]
    pub diarized_transcripts: Vec<DiarizedTranscript>,
}

#[derive(Debug, Clone)]
pub struct TranscriptionConfig {
    pub model: String,
    pub language: Option<String>,
    pub translate: bool,
    pub word_timestamps: bool,
    pub speaker_diarization: bool,
    pub ollama_endpoint: String,
}

impl Default for TranscriptionConfig {
    fn default() -> Self {
        Self {
            model: "whisper-large-v3".to_string(),
            language: None,
            translate: false,
            word_timestamps: true,
            speaker_diarization: true,
            ollama_endpoint: "http://localhost:11434".to_string(),
        }
    }
}
