// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2024-2026 TripleACS Pty Ltd t/a 2pi Software

//! Cross-workspace contacts address book.
//!
//! Each contact is stored as a JSON file in `~/.config/krillnotes/contacts/`.
//! The same public key always maps to the same contact file — contacts are
//! deduplicated by public key across all workspaces.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

use crate::Result;

/// How much the local user trusts this contact's claimed identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustLevel {
    /// Keys compared in person via QR code or side-by-side display.
    VerifiedInPerson,
    /// Verification code confirmed over phone/video.
    CodeVerified,
    /// A verified peer vouched for this identity.
    Vouched,
    /// Accepted at first use without independent verification.
    Tofu,
}

/// A contact in the local address book.
///
/// Stored at `~/.config/krillnotes/contacts/<contact_id>.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Contact {
    pub contact_id: Uuid,
    /// The name the person declared when creating their identity.
    pub declared_name: String,
    /// Optional local override — never propagated to peers.
    pub local_name: Option<String>,
    /// Ed25519 public key, base64-encoded.
    pub public_key: String,
    /// BLAKE3(pubkey_bytes) → 4 BIP-39 words, hyphen-separated.
    pub fingerprint: String,
    pub trust_level: TrustLevel,
    /// UUID of the contact who vouched for this one, if trust_level == Vouched.
    pub vouched_by: Option<Uuid>,
    pub first_seen: DateTime<Utc>,
    pub notes: Option<String>,
}

impl Contact {
    /// The name to display in the UI: local override if set, else declared name.
    pub fn display_name(&self) -> &str {
        self.local_name.as_deref().unwrap_or(&self.declared_name)
    }
}

/// Manages the contacts directory.
pub struct ContactManager {
    contacts_dir: PathBuf,
}

impl ContactManager {
    /// Create a `ContactManager` rooted at `config_dir`.
    ///
    /// Creates `config_dir/contacts/` if it doesn't exist.
    pub fn new(config_dir: PathBuf) -> Result<Self> {
        let contacts_dir = config_dir.join("contacts");
        std::fs::create_dir_all(&contacts_dir)?;
        Ok(Self { contacts_dir })
    }

    fn path_for(&self, id: Uuid) -> PathBuf {
        self.contacts_dir.join(format!("{id}.json"))
    }

    /// Create a new contact. Returns an error if the public key is invalid base64.
    pub fn create_contact(
        &self,
        declared_name: &str,
        public_key: &str,
        trust_level: TrustLevel,
    ) -> Result<Contact> {
        let fingerprint = generate_fingerprint(public_key)?;
        let contact = Contact {
            contact_id: Uuid::new_v4(),
            declared_name: declared_name.to_string(),
            local_name: None,
            public_key: public_key.to_string(),
            fingerprint,
            trust_level,
            vouched_by: None,
            first_seen: Utc::now(),
            notes: None,
        };
        self.save_contact(&contact)?;
        Ok(contact)
    }

    /// Save (create or overwrite) a contact file.
    pub fn save_contact(&self, contact: &Contact) -> Result<()> {
        let path = self.path_for(contact.contact_id);
        let data = serde_json::to_string_pretty(contact)?;
        std::fs::write(&path, data)?;
        Ok(())
    }

    /// Load a contact by UUID. Returns `None` if not found.
    pub fn get_contact(&self, id: Uuid) -> Result<Option<Contact>> {
        let path = self.path_for(id);
        if !path.exists() {
            return Ok(None);
        }
        let data = std::fs::read_to_string(&path)?;
        let contact: Contact = serde_json::from_str(&data)
            .map_err(|e| crate::KrillnotesError::IdentityCorrupt(format!("contact {id}: {e}")))?;
        Ok(Some(contact))
    }

    /// List all contacts in the address book.
    pub fn list_contacts(&self) -> Result<Vec<Contact>> {
        let mut contacts = Vec::new();
        for entry in std::fs::read_dir(&self.contacts_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                let data = std::fs::read_to_string(&path)?;
                if let Ok(c) = serde_json::from_str::<Contact>(&data) {
                    contacts.push(c);
                }
            }
        }
        Ok(contacts)
    }

    /// Find a contact by public key.
    pub fn find_by_public_key(&self, public_key: &str) -> Result<Option<Contact>> {
        for contact in self.list_contacts()? {
            if contact.public_key == public_key {
                return Ok(Some(contact));
            }
        }
        Ok(None)
    }

    /// Find an existing contact by public key, or create a new one (TOFU).
    pub fn find_or_create_by_public_key(
        &self,
        declared_name: &str,
        public_key: &str,
        trust_level: TrustLevel,
    ) -> Result<Contact> {
        if let Some(existing) = self.find_by_public_key(public_key)? {
            return Ok(existing);
        }
        self.create_contact(declared_name, public_key, trust_level)
    }

    /// Delete a contact file.
    pub fn delete_contact(&self, id: Uuid) -> Result<()> {
        let path = self.path_for(id);
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        Ok(())
    }
}

/// Generate a 4-word BIP-39 fingerprint from a base64-encoded public key.
///
/// Algorithm: BLAKE3(decoded_pubkey_bytes) → take first 44 bits →
/// split into four 11-bit indices → look up in BIP-39 word list.
pub fn generate_fingerprint(public_key_b64: &str) -> Result<String> {
    let key_bytes = BASE64.decode(public_key_b64)
        .map_err(|e| crate::KrillnotesError::IdentityCorrupt(
            format!("invalid public key base64: {e}")
        ))?;
    let hash = blake3::hash(&key_bytes);
    let hash_bytes = hash.as_bytes();

    // Extract four 11-bit indices from the first 6 bytes (48 bits, use 44).
    let b = hash_bytes;
    let idx0 = (((b[0] as u16) << 3) | ((b[1] as u16) >> 5)) & 0x7FF;
    let idx1 = (((b[1] as u16) << 6) | ((b[2] as u16) >> 2)) & 0x7FF;
    let idx2 = (((b[2] as u16) << 9) | ((b[3] as u16) << 1) | ((b[4] as u16) >> 7)) & 0x7FF;
    let idx3 = (((b[4] as u16) << 4) | ((b[5] as u16) >> 4)) & 0x7FF;

    // Use bip39 crate to get the English word list.
    let wordlist = bip39::Language::English.word_list();
    let words = [
        wordlist[idx0 as usize],
        wordlist[idx1 as usize],
        wordlist[idx2 as usize],
        wordlist[idx3 as usize],
    ];
    Ok(words.join("-"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn mgr(tmp: &TempDir) -> ContactManager {
        ContactManager::new(tmp.path().to_path_buf()).unwrap()
    }

    #[test]
    fn test_create_and_read_contact() {
        let tmp = TempDir::new().unwrap();
        let mgr = mgr(&tmp);
        let pubkey = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
        let c = mgr.create_contact("Alice", pubkey, TrustLevel::Tofu).unwrap();
        assert_eq!(c.declared_name, "Alice");
        assert_eq!(c.local_name, None);
        let fetched = mgr.get_contact(c.contact_id).unwrap().unwrap();
        assert_eq!(fetched.declared_name, "Alice");
    }

    #[test]
    fn test_display_name_prefers_local_name() {
        let tmp = TempDir::new().unwrap();
        let mgr = mgr(&tmp);
        let pubkey = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
        let mut c = mgr.create_contact("Bob Chen", pubkey, TrustLevel::Tofu).unwrap();
        c.local_name = Some("Robert — Field Lead".to_string());
        mgr.save_contact(&c).unwrap();
        let fetched = mgr.get_contact(c.contact_id).unwrap().unwrap();
        assert_eq!(fetched.display_name(), "Robert — Field Lead");
    }

    #[test]
    fn test_find_by_public_key_deduplicates() {
        let tmp = TempDir::new().unwrap();
        let mgr = mgr(&tmp);
        let pubkey = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
        let c1 = mgr.create_contact("Alice", pubkey, TrustLevel::Tofu).unwrap();
        // Second create with same pubkey should return existing
        let c2 = mgr.find_or_create_by_public_key("Alice", pubkey, TrustLevel::Tofu).unwrap();
        assert_eq!(c1.contact_id, c2.contact_id);
    }

    #[test]
    fn test_fingerprint_is_four_words() {
        let pubkey = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
        let fp = generate_fingerprint(pubkey).unwrap();
        let words: Vec<&str> = fp.split('-').collect();
        assert_eq!(words.len(), 4);
        assert!(words.iter().all(|w| !w.is_empty()));
    }

    #[test]
    fn test_list_contacts() {
        let tmp = TempDir::new().unwrap();
        let mgr = mgr(&tmp);
        mgr.create_contact("Alice", "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=", TrustLevel::Tofu).unwrap();
        mgr.create_contact("Bob", "BAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=", TrustLevel::Tofu).unwrap();
        let list = mgr.list_contacts().unwrap();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_delete_contact() {
        let tmp = TempDir::new().unwrap();
        let mgr = mgr(&tmp);
        let c = mgr.create_contact("Alice", "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=", TrustLevel::Tofu).unwrap();
        mgr.delete_contact(c.contact_id).unwrap();
        assert!(mgr.get_contact(c.contact_id).unwrap().is_none());
    }
}
