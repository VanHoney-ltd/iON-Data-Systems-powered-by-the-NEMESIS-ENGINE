//! Orpheus Cryptographic Operations
//!
//! AES-256-GCM encryption with PBKDF2-HMAC-SHA256 key derivation.
//! Compatible with the original Python Tartarus encryption format.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use anyhow::{Context, Result};
use pbkdf2::pbkdf2_hmac;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

/// Encrypted data blob structure (JSON-serializable)
#[derive(Debug, Serialize, Deserialize)]
pub struct EncryptedBlob {
    pub salt: String,
    pub nonce: String,
    pub ciphertext: String,
    #[serde(default = "default_algorithm")]
    pub algorithm: String,
    #[serde(default = "default_kdf")]
    pub kdf: String,
    #[serde(default = "default_iterations")]
    pub iterations: u32,
}

fn default_algorithm() -> String {
    "AES-256-GCM".to_string()
}

fn default_kdf() -> String {
    "PBKDF2-HMAC-SHA256".to_string()
}

fn default_iterations() -> u32 {
    200000
}

/// Encrypt plaintext using AES-256-GCM with PBKDF2 key derivation
pub fn encrypt_text(text: &str, passphrase: &str) -> Result<String> {
    use base64::{engine::general_purpose, Engine as _};
    use rand::Rng;

    // Generate random salt (16 bytes) and nonce (12 bytes)
    let mut salt = [0u8; 16];
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill(&mut salt);
    rand::thread_rng().fill(&mut nonce_bytes);

    // Derive key using PBKDF2-HMAC-SHA256 (200,000 iterations)
    let mut key = [0u8; 32];
    pbkdf2_hmac::<Sha256>(passphrase.as_bytes(), &salt, 200_000, &mut key);

    // Encrypt with AES-256-GCM
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| anyhow::anyhow!("Invalid key length: {:?}", e))?;
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, text.as_bytes())
        .map_err(|e| anyhow::anyhow!("Encryption failed: {:?}", e))?;

    // Build JSON blob
    let blob = EncryptedBlob {
        salt: general_purpose::STANDARD.encode(&salt),
        nonce: general_purpose::STANDARD.encode(&nonce_bytes),
        ciphertext: general_purpose::STANDARD.encode(&ciphertext),
        algorithm: "AES-256-GCM".to_string(),
        kdf: "PBKDF2-HMAC-SHA256".to_string(),
        iterations: 200000,
    };

    serde_json::to_string_pretty(&blob).context("Failed to serialize encrypted blob")
}

/// Decrypt an encrypted blob using AES-256-GCM
pub fn decrypt_blob(blob: &EncryptedBlob, passphrase: &str) -> Result<String> {
    use base64::{engine::general_purpose, Engine as _};

    // Decode base64 fields
    let salt = general_purpose::STANDARD
        .decode(&blob.salt)
        .context("Invalid salt encoding")?;
    let nonce_bytes = general_purpose::STANDARD
        .decode(&blob.nonce)
        .context("Invalid nonce encoding")?;
    let ciphertext = general_purpose::STANDARD
        .decode(&blob.ciphertext)
        .context("Invalid ciphertext encoding")?;

    // Derive key using PBKDF2
    let mut key = [0u8; 32];
    let iterations = blob.iterations;
    pbkdf2_hmac::<Sha256>(passphrase.as_bytes(), &salt, iterations, &mut key);

    // Decrypt
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| anyhow::anyhow!("Invalid key length: {:?}", e))?;
    let nonce = Nonce::from_slice(&nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|e| anyhow::anyhow!("Decryption failed: {:?}", e))?;

    String::from_utf8(plaintext).context("Invalid UTF-8 in decrypted content")
}
