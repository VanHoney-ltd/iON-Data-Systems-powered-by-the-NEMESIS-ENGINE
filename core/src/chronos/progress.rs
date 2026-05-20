//! Backup Progress Tracking

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct BackupProgress {
    total_files: Arc<AtomicU64>,
    processed_files: Arc<AtomicU64>,
    current_file: Arc<RwLock<String>>,
    start_time: Instant,
    last_update: Arc<RwLock<Instant>>,
}

#[derive(Debug, Clone)]
pub struct ProgressSnapshot {
    pub total_files: u64,
    pub processed_files: u64,
    pub progress_percentage: f64,
    pub current_file: String,
    pub elapsed_time: Duration,
    pub estimated_remaining: Option<Duration>,
}

impl BackupProgress {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            total_files: Arc::new(AtomicU64::new(0)),
            processed_files: Arc::new(AtomicU64::new(0)),
            current_file: Arc::new(RwLock::new(String::new())),
            start_time: Instant::now(),
            last_update: Arc::new(RwLock::new(Instant::now())),
        })
    }

    pub fn update_total(&self, count: u64) {
        self.total_files.store(count, Ordering::Relaxed);
        self.update_timestamp();
    }

    pub fn increment_processed(&self) {
        self.processed_files.fetch_add(1, Ordering::Relaxed);
        self.update_timestamp();
    }

    pub fn set_current_file(&self, filename: &str) {
        *self.current_file.write().unwrap() = filename.to_string();
        self.update_timestamp();
    }

    fn update_timestamp(&self) {
        *self.last_update.write().unwrap() = Instant::now();
    }

    pub fn progress_percentage(&self) -> f64 {
        let total = self.total_files.load(Ordering::Relaxed);
        let processed = self.processed_files.load(Ordering::Relaxed);

        if total == 0 {
            0.0
        } else {
            (processed as f64 / total as f64) * 100.0
        }
    }

    pub fn snapshot(&self) -> ProgressSnapshot {
        let total = self.total_files.load(Ordering::Relaxed);
        let processed = self.processed_files.load(Ordering::Relaxed);
        let current_file = self.current_file.read().unwrap().clone();
        let elapsed = self.start_time.elapsed();

        let estimated_remaining = if processed > 0 && total > 0 {
            let rate = processed as f64 / elapsed.as_secs_f64();
            let remaining = (total - processed) as f64 / rate;
            Some(Duration::from_secs_f64(remaining))
        } else {
            None
        };

        ProgressSnapshot {
            total_files: total,
            processed_files: processed,
            progress_percentage: self.progress_percentage(),
            current_file,
            elapsed_time: elapsed,
            estimated_remaining,
        }
    }

    pub fn reset(&self) {
        self.total_files.store(0, Ordering::Relaxed);
        self.processed_files.store(0, Ordering::Relaxed);
        *self.current_file.write().unwrap() = String::new();
        *self.last_update.write().unwrap() = Instant::now();
    }
}
