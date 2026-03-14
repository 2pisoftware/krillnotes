// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2024-2026 TripleACS Pty Ltd t/a 2pi Software

//! Encrypted relay credential storage.
//!
//! Credentials are AES-256-GCM encrypted and stored as a JSON envelope
//! (base64 nonce + ciphertext) at `<relay_dir>/<identity_uuid>.json`.

use aes_gcm::{aead::Aead, Aes256Gcm, Key, KeyInit, Nonce};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use chrono::{DateTime, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::core::error::KrillnotesError;

/// Relay session credentials for a given identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayCredentials {
    pub relay_url: String,
    pub email: String,
    pub session_token: String,
    pub session_expires_at: DateTime<Utc>,
    pub device_public_key: String,
}

/// On-disk format: AES-256-GCM encrypted JSON envelope.
#[derive(Serialize, Deserialize)]
struct EncryptedRelayFile {
    /// base64-encoded 12-byte nonce.
    nonce: String,
    /// base64-encoded AES-256-GCM ciphertext (includes 16-byte auth tag).
    ciphertext: String,
}

/// Save relay credentials to `<relay_dir>/<identity_uuid>.json`, encrypted
/// with `encryption_key` using AES-256-GCM.
pub fn save_relay_credentials(
    relay_dir: &Path,
    identity_uuid: &str,
    creds: &RelayCredentials,
    encryption_key: &[u8; 32],
) -> Result<(), KrillnotesError> {
    std::fs::create_dir_all(relay_dir)?;

    let plaintext = serde_json::to_vec(creds)?;

    let key = Key::<Aes256Gcm>::from_slice(encryption_key);
    let cipher = Aes256Gcm::new(key);

    let mut nonce_bytes = [0u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_ref())
        .map_err(|e| KrillnotesError::ContactEncryption(format!("relay credential encryption failed: {e}")))?;

    let envelope = EncryptedRelayFile {
        nonce: BASE64.encode(nonce_bytes),
        ciphertext: BASE64.encode(&ciphertext),
    };

    let path = relay_dir.join(format!("{identity_uuid}.json"));
    let json = serde_json::to_string(&envelope)?;
    std::fs::write(&path, json)?;

    Ok(())
}

/// Load relay credentials from `<relay_dir>/<identity_uuid>.json`.
///
/// Returns `None` if the file does not exist.
pub fn load_relay_credentials(
    relay_dir: &Path,
    identity_uuid: &str,
    encryption_key: &[u8; 32],
) -> Result<Option<RelayCredentials>, KrillnotesError> {
    let path = relay_dir.join(format!("{identity_uuid}.json"));

    if !path.exists() {
        return Ok(None);
    }

    let json = std::fs::read_to_string(&path)?;

    let envelope: EncryptedRelayFile = serde_json::from_str(&json)?;

    let nonce_bytes = BASE64.decode(&envelope.nonce).map_err(|e| {
        KrillnotesError::ContactEncryption(format!("invalid relay nonce base64: {e}"))
    })?;
    if nonce_bytes.len() != 12 {
        return Err(KrillnotesError::ContactEncryption(format!(
            "invalid relay nonce length: {} bytes",
            nonce_bytes.len()
        )));
    }
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = BASE64.decode(&envelope.ciphertext).map_err(|e| {
        KrillnotesError::ContactEncryption(format!("invalid relay ciphertext base64: {e}"))
    })?;

    let key = Key::<Aes256Gcm>::from_slice(encryption_key);
    let cipher = Aes256Gcm::new(key);

    let plaintext = cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|e| KrillnotesError::ContactEncryption(format!("relay credential decryption failed: {e}")))?;

    let creds: RelayCredentials = serde_json::from_slice(&plaintext)?;

    Ok(Some(creds))
}

/// Delete relay credentials for the given identity.
///
/// Returns `Ok(())` if the file is absent (idempotent).
pub fn delete_relay_credentials(
    relay_dir: &Path,
    identity_uuid: &str,
) -> Result<(), KrillnotesError> {
    let path = relay_dir.join(format!("{identity_uuid}.json"));
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_relay_credentials_save_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let relay_dir = dir.path().join("relay");

        let identity_uuid = "test-identity-uuid";
        let encryption_key = [0x42u8; 32];

        let creds = RelayCredentials {
            relay_url: "https://relay.example.com".to_string(),
            email: "test@example.com".to_string(),
            session_token: "tok_abc123".to_string(),
            session_expires_at: chrono::Utc::now() + chrono::Duration::days(30),
            device_public_key: "deadbeef".to_string(),
        };

        save_relay_credentials(&relay_dir, identity_uuid, &creds, &encryption_key).unwrap();
        let loaded = load_relay_credentials(&relay_dir, identity_uuid, &encryption_key).unwrap();

        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.relay_url, creds.relay_url);
        assert_eq!(loaded.email, creds.email);
        assert_eq!(loaded.session_token, creds.session_token);
        assert_eq!(loaded.device_public_key, creds.device_public_key);
    }

    #[test]
    fn test_relay_credentials_missing_file_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let relay_dir = dir.path().join("relay");
        let encryption_key = [0x42u8; 32];

        let loaded = load_relay_credentials(&relay_dir, "nonexistent", &encryption_key).unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn test_relay_credentials_delete() {
        let dir = tempfile::tempdir().unwrap();
        let relay_dir = dir.path().join("relay");
        let identity_uuid = "delete-test";
        let encryption_key = [0x11u8; 32];

        let creds = RelayCredentials {
            relay_url: "https://relay.example.com".to_string(),
            email: "del@example.com".to_string(),
            session_token: "tok_del".to_string(),
            session_expires_at: chrono::Utc::now(),
            device_public_key: "aabbcc".to_string(),
        };

        save_relay_credentials(&relay_dir, identity_uuid, &creds, &encryption_key).unwrap();
        delete_relay_credentials(&relay_dir, identity_uuid).unwrap();
        let loaded = load_relay_credentials(&relay_dir, identity_uuid, &encryption_key).unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn test_relay_credentials_delete_nonexistent_is_ok() {
        let dir = tempfile::tempdir().unwrap();
        let relay_dir = dir.path().join("relay");
        // Should not error if file doesn't exist
        delete_relay_credentials(&relay_dir, "never-existed").unwrap();
    }
}
