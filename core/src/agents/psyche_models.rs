use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PsycheMessageAnalysis {
    pub message_id: Option<i64>,
    pub timestamp: DateTime<Utc>,
    pub sender: String,
    pub recipient: String,
    pub content: String,
    pub contact_label: Option<String>,
    pub sentiment: Sentiment,
    pub emotion: Emotion,
    pub intent: Intent,
    pub aggression_level: f64,
    pub manipulation_score: f64,
    pub deception_indicators: Vec<String>,
    pub linguistic_features: LinguisticFeatures,
    pub context_flags: Vec<String>,
    pub risk_level: RiskLevel,
    pub confidence: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PsycheCallAnalysis {
    pub call_id: Option<i64>,
    pub timestamp: DateTime<Utc>,
    pub participants: Vec<String>,
    pub summary: String,
    pub risk_level: RiskLevel,
    pub confidence: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RiskSummary {
    pub overall_risk: RiskLevel,
    pub highlights: Vec<String>,
    pub red_flags: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SpeakerProfile {
    pub speaker_id: String,
    pub name: Option<String>,
    pub risk_level: RiskLevel,
    pub traits: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Sentiment {
    pub polarity: f64,
    pub subjectivity: f64,
    pub emotional_tone: EmotionalTone,
    pub volatility: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum EmotionalTone {
    Positive,
    Neutral,
    Negative,
    Mixed,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Emotion {
    pub primary_emotion: String,
    pub intensity: f64,
    pub secondary_emotions: Vec<String>,
    pub emotional_shift: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Intent {
    Informational,
    Supportive,
    Persuasive,
    Aggressive,
    Defensive,
    Other,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LinguisticFeatures {
    pub word_count: usize,
    pub sentence_count: usize,
    pub avg_sentence_length: f64,
    pub readability_score: f64,
    pub complexity_score: f64,
    pub pronoun_usage: PronounUsage,
    pub filler_words: Vec<String>,
    pub intensifiers: Vec<String>,
    pub qualifiers: Vec<String>,
    pub hedging_language: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PronounUsage {
    pub first_person_singular: usize,
    pub first_person_plural: usize,
    pub second_person: usize,
    pub third_person: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}
