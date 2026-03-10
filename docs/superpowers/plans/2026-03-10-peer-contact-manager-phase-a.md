# Peer Contact Manager — Phase A Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an encrypted, per-identity contact book — CRUD UI accessible from the identity manager, with fingerprint-gated trust levels.

**Architecture:** Restructure `ContactManager` to be per-identity with AES-256-GCM encryption (key derived from identity seed via HKDF). Contacts live in `~/.config/krillnotes/identities/<uuid>/contacts/` and are decrypted into a memory cache when the identity unlocks. Six new Tauri commands expose CRUD to the frontend. Three new React components (`ContactBookDialog`, `AddContactDialog`, `EditContactDialog`) hang off the existing `IdentityManagerDialog`.

**Tech Stack:** Rust (`aes-gcm`, `hkdf`, `sha2` — already in deps), Tauri v2, React 19, TypeScript, Tailwind v4.

**Spec:** `docs/superpowers/specs/2026-03-10-peer-contact-manager-design.md`

---

## Setup: Create Worktree

- [ ] **Create feature worktree**

```bash
git -C /Users/careck/Source/Krillnotes worktree add .worktrees/feat/contact-manager-phase-a -b feat/contact-manager-phase-a
```

All subsequent work happens in `.worktrees/feat/contact-manager-phase-a/`. All `git add`/`git commit` commands in this plan use `-C /Users/careck/Source/Krillnotes/.worktrees/feat/contact-manager-phase-a` (or run from that directory directly).

---

## Chunk 1: Core — Encrypted ContactManager + Identity Key Derivation

### Files

- **Modify:** `krillnotes-core/src/core/contact.rs`
- **Modify:** `krillnotes-core/src/core/identity.rs`
- **Modify:** `krillnotes-core/src/lib.rs` (re-exports)
- **Test:** `krillnotes-core/src/core/contact.rs` (inline `#[cfg(test)]`)

---

### Task 1: Add `contacts_key()` to `UnlockedIdentity`

**Files:**
- Modify: `krillnotes-core/src/core/identity.rs`

The HKDF pattern already exists in `derive_db_password_key()` (around line 632). Follow the exact same pattern.

- [ ] **Step 1: Write the failing test**

Add to the existing `#[cfg(test)]` block in `identity.rs`:

```rust
#[test]
fn contacts_key_is_deterministic() {
    let dir = tempfile::tempdir().unwrap();
    let mgr = IdentityManager::new(dir.path().to_path_buf()).unwrap();
    let identity = mgr
        .create_identity("Test User", "passphrase123")
        .unwrap();
    let unlocked = mgr
        .unlock_identity(&identity.identity_uuid, "passphrase123")
        .unwrap();
    let key1 = unlocked.contacts_key();
    let key2 = unlocked.contacts_key();
    assert_eq!(key1, key2, "contacts_key must be deterministic");
    assert_eq!(key1.len(), 32);
    // Must differ from a different identity
    let identity2 = mgr
        .create_identity("Other User", "passphrase123")
        .unwrap();
    let unlocked2 = mgr
        .unlock_identity(&identity2.identity_uuid, "passphrase123")
        .unwrap();
    assert_ne!(unlocked.contacts_key(), unlocked2.contacts_key());
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p krillnotes-core contacts_key_is_deterministic 2>&1 | tail -5
```
Expected: compile error — `no method named contacts_key`

- [ ] **Step 3: Implement `contacts_key()` on `UnlockedIdentity`**

In `identity.rs`, add this method to the `impl UnlockedIdentity` block (the struct is defined around line 116):

```rust
/// Derives a 32-byte encryption key for this identity's contact book.
/// Uses HKDF-SHA256 with the Ed25519 seed as IKM.
pub fn contacts_key(&self) -> [u8; 32] {
    let hk = hkdf::Hkdf::<sha2::Sha256>::new(None, self.signing_key.as_bytes());
    let mut okm = [0u8; 32];
    hk.expand(b"krillnotes-contacts-v1", &mut okm)
        .expect("HKDF expand failed — output length is valid");
    okm
}
```

Note: `hkdf` and `sha2` are already dependencies (used by `derive_db_password_key`). No `Cargo.toml` changes needed.

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test -p krillnotes-core contacts_key_is_deterministic 2>&1 | tail -5
```
Expected: `test contacts_key_is_deterministic ... ok`

- [ ] **Step 5: Commit**

```bash
git -C /Users/careck/Source/Krillnotes add krillnotes-core/src/core/identity.rs
git -C /Users/careck/Source/Krillnotes commit -m "feat: add contacts_key() HKDF derivation to UnlockedIdentity"
```

---

### Task 2: Add `ContactEncryption` error variant

**Files:**
- Modify: `krillnotes-core/src/core/error.rs`

The plan uses `KrillnotesError::ContactEncryption(String)` in `contact.rs`. This variant does not exist yet and must be added before restructuring `ContactManager`.

- [ ] **Step 1: Add the variant**

Open `krillnotes-core/src/core/error.rs`. Find the `KrillnotesError` enum and add:

```rust
#[error("Contact encryption error: {0}")]
ContactEncryption(String),
```

Also add a `user_message()` match arm (if the file has one):
```rust
KrillnotesError::ContactEncryption(msg) => msg.clone(),
```

- [ ] **Step 2: Verify it compiles**

```bash
cargo build -p krillnotes-core 2>&1 | grep -E "^error" | head -10
```

- [ ] **Step 3: Commit**

```bash
git add krillnotes-core/src/core/error.rs
git commit -m "feat: add ContactEncryption error variant to KrillnotesError"
```

---

### Task 3: Add encrypted on-disk format and restructure `ContactManager`

**Files:**
- Modify: `krillnotes-core/src/core/contact.rs`

The current `ContactManager` stores plain JSON. We need to:
1. Add `EncryptedContactFile` as the on-disk format
2. Change the constructor to `for_identity(contacts_dir, key)` — creates the dir, decrypts all existing contacts into a memory cache
3. Add `encryption_key` and `cache` fields
4. Rewrite read/write methods to use the cache and encrypt on write

**Keep** the existing `ContactManager::new()` method — do not remove it. After this change, `new()` initialises an empty cache and never populates it, so `list_contacts()` on a `new()`-created manager returns an empty list. This is acceptable — `new()` is now vestigial. Verify that nothing in the production code path calls `list_contacts()` via the old `AppState.contact_manager` field (which will be replaced in Task 4).

- [ ] **Step 1: Write failing tests for encrypted ContactManager**

Add to the `#[cfg(test)]` block in `contact.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn test_key() -> [u8; 32] {
        [42u8; 32]
    }

    #[test]
    fn encrypted_contact_roundtrip() {
        let dir = tempdir().unwrap();
        let contacts_dir = dir.path().join("contacts");

        // Create a contact
        let mgr = ContactManager::for_identity(contacts_dir.clone(), test_key()).unwrap();
        let contact = mgr
            .create_contact("Alice", "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=", TrustLevel::Tofu)
            .unwrap();

        // On-disk file must NOT be readable as plain JSON Contact
        let raw = std::fs::read_to_string(mgr.path_for(contact.contact_id)).unwrap();
        assert!(serde_json::from_str::<Contact>(&raw).is_err(), "File must not be plain JSON");

        // Load fresh manager from same dir — contact must survive
        let mgr2 = ContactManager::for_identity(contacts_dir, test_key()).unwrap();
        let loaded = mgr2.get_contact(contact.contact_id).unwrap().unwrap();
        assert_eq!(loaded.declared_name, "Alice");
    }

    #[test]
    fn wrong_key_cannot_decrypt() {
        let dir = tempdir().unwrap();
        let contacts_dir = dir.path().join("contacts");

        let mgr = ContactManager::for_identity(contacts_dir.clone(), test_key()).unwrap();
        mgr.create_contact("Bob", "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=", TrustLevel::Tofu)
            .unwrap();

        let wrong_key = [99u8; 32];
        let result = ContactManager::for_identity(contacts_dir, wrong_key);
        assert!(result.is_err(), "Wrong key must fail to load contacts");
    }

    #[test]
    fn list_and_delete_contact() {
        let dir = tempdir().unwrap();
        let mgr = ContactManager::for_identity(dir.path().join("c"), test_key()).unwrap();
        mgr.create_contact("Alice", "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=", TrustLevel::Tofu).unwrap();
        mgr.create_contact("Bob", "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB=", TrustLevel::CodeVerified).unwrap();
        let list = mgr.list_contacts().unwrap();
        assert_eq!(list.len(), 2);

        let alice = list.iter().find(|c| c.declared_name == "Alice").unwrap();
        mgr.delete_contact(alice.contact_id).unwrap();
        assert_eq!(mgr.list_contacts().unwrap().len(), 1);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p krillnotes-core encrypted_contact 2>&1 | tail -10
```
Expected: compile error — `for_identity` not found

- [ ] **Step 3: Rewrite `contact.rs` with encryption support**

Replace the `ContactManager` struct and all its methods. Keep `generate_fingerprint` and `TrustLevel` and `Contact` unchanged. Keep `ContactManager::new()`.

The new `ContactManager` at the top of the file's impl block:

```rust
use aes_gcm::{Aes256Gcm, Key, Nonce, KeyInit, aead::Aead};
use hkdf::Hkdf;
use sha2::Sha256;

/// On-disk format for an encrypted contact file.
#[derive(Serialize, Deserialize)]
struct EncryptedContactFile {
    ciphertext: String, // base64-encoded AES-256-GCM ciphertext+tag
    nonce: String,      // base64-encoded 12-byte nonce
}

pub struct ContactManager {
    contacts_dir: PathBuf,
    encryption_key: Option<[u8; 32]>, // None = legacy unencrypted mode (ContactManager::new)
    cache: std::sync::RwLock<HashMap<Uuid, Contact>>,
}
```

Add these methods:

```rust
impl ContactManager {
    /// Legacy constructor — unencrypted, for backward compat. Do not use for new code.
    pub fn new(config_dir: PathBuf) -> Result<Self> {
        let contacts_dir = config_dir.join("contacts");
        std::fs::create_dir_all(&contacts_dir)?;
        Ok(Self {
            contacts_dir,
            encryption_key: None,
            cache: std::sync::RwLock::new(HashMap::new()),
        })
    }

    /// Per-identity constructor — contacts are AES-256-GCM encrypted with `key`.
    /// Decrypts and caches all existing contacts from `contacts_dir` on construction.
    pub fn for_identity(contacts_dir: PathBuf, key: [u8; 32]) -> Result<Self> {
        std::fs::create_dir_all(&contacts_dir)?;
        let mgr = Self {
            contacts_dir,
            encryption_key: Some(key),
            cache: std::sync::RwLock::new(HashMap::new()),
        };
        mgr.load_all_into_cache()?;
        Ok(mgr)
    }

    fn load_all_into_cache(&self) -> Result<()> {
        let mut cache = self.cache.write().unwrap();
        for entry in std::fs::read_dir(&self.contacts_dir)? {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let contact = self.decrypt_file(&path)?;
            cache.insert(contact.contact_id, contact);
        }
        Ok(())
    }

    fn encrypt_contact(&self, contact: &Contact) -> Result<EncryptedContactFile> {
        let key_bytes = self.encryption_key
            .ok_or_else(|| KrillnotesError::ContactEncryption("No encryption key".into()))?;
        let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
        let cipher = Aes256Gcm::new(key);
        let mut nonce_bytes = [0u8; 12];
        getrandom::getrandom(&mut nonce_bytes)
            .map_err(|e| KrillnotesError::ContactEncryption(e.to_string()))?;
        let nonce = Nonce::from_slice(&nonce_bytes);
        let plaintext = serde_json::to_vec(contact)?;
        let ciphertext = cipher.encrypt(nonce, plaintext.as_ref())
            .map_err(|e| KrillnotesError::ContactEncryption(e.to_string()))?;
        Ok(EncryptedContactFile {
            ciphertext: base64::engine::general_purpose::STANDARD.encode(&ciphertext),
            nonce: base64::engine::general_purpose::STANDARD.encode(nonce_bytes),
        })
    }

    fn decrypt_file(&self, path: &std::path::Path) -> Result<Contact> {
        let key_bytes = self.encryption_key
            .ok_or_else(|| KrillnotesError::ContactEncryption("No encryption key".into()))?;
        let raw = std::fs::read_to_string(path)?;
        let enc: EncryptedContactFile = serde_json::from_str(&raw)?;
        let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
        let cipher = Aes256Gcm::new(key);
        let nonce_bytes = base64::engine::general_purpose::STANDARD
            .decode(&enc.nonce)
            .map_err(|e| KrillnotesError::ContactEncryption(e.to_string()))?;
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = base64::engine::general_purpose::STANDARD
            .decode(&enc.ciphertext)
            .map_err(|e| KrillnotesError::ContactEncryption(e.to_string()))?;
        let plaintext = cipher.decrypt(nonce, ciphertext.as_ref())
            .map_err(|_| KrillnotesError::ContactEncryption("Decryption failed — wrong key?".into()))?;
        let contact: Contact = serde_json::from_slice(&plaintext)?;
        Ok(contact)
    }

    pub fn path_for(&self, id: Uuid) -> PathBuf {
        self.contacts_dir.join(format!("{}.json", id))
    }

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
            first_seen: chrono::Utc::now(),
            notes: None,
        };
        self.save_contact(&contact)?;
        Ok(contact)
    }

    pub fn save_contact(&self, contact: &Contact) -> Result<()> {
        if self.encryption_key.is_some() {
            let enc = self.encrypt_contact(contact)?;
            let json = serde_json::to_string_pretty(&enc)?;
            std::fs::write(self.path_for(contact.contact_id), json)?;
        } else {
            // Legacy unencrypted path
            let json = serde_json::to_string_pretty(contact)?;
            std::fs::write(self.path_for(contact.contact_id), json)?;
        }
        self.cache.write().unwrap().insert(contact.contact_id, contact.clone());
        Ok(())
    }

    pub fn get_contact(&self, id: Uuid) -> Result<Option<Contact>> {
        Ok(self.cache.read().unwrap().get(&id).cloned())
    }

    pub fn list_contacts(&self) -> Result<Vec<Contact>> {
        let cache = self.cache.read().unwrap();
        let mut list: Vec<Contact> = cache.values().cloned().collect();
        list.sort_by(|a, b| a.display_name().cmp(b.display_name()));
        Ok(list)
    }

    pub fn find_by_public_key(&self, public_key: &str) -> Result<Option<Contact>> {
        Ok(self.cache.read().unwrap().values()
            .find(|c| c.public_key == public_key)
            .cloned())
    }

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

    pub fn delete_contact(&self, id: Uuid) -> Result<()> {
        let path = self.path_for(id);
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        self.cache.write().unwrap().remove(&id);
        Ok(())
    }
}
```

Note: `getrandom` is already a transitive dependency. `aes-gcm` and `base64` are already in `krillnotes-core/Cargo.toml` (used by attachments). Verify with:
```bash
grep -E "aes-gcm|base64|getrandom" krillnotes-core/Cargo.toml
```
If missing, add them. `hkdf` and `sha2` are already present (used in `identity.rs`).

- [ ] **Step 4: Run all core tests**

```bash
cargo test -p krillnotes-core 2>&1 | tail -20
```
Expected: all tests pass including the new ones

- [ ] **Step 5: Commit**

```bash
git -C /Users/careck/Source/Krillnotes add krillnotes-core/src/core/contact.rs
git -C /Users/careck/Source/Krillnotes commit -m "feat: encrypted per-identity ContactManager with in-memory cache"
```

---

## Chunk 2: Tauri Backend — AppState + 6 New Commands

### Files

- **Modify:** `krillnotes-desktop/src-tauri/src/lib.rs`

---

### Task 3: Migrate AppState to per-identity `contact_managers`

**Files:**
- Modify: `krillnotes-desktop/src-tauri/src/lib.rs`

- [ ] **Step 1: Change `AppState` field**

Find the `AppState` struct (around line 37). Change:
```rust
// Before:
pub contact_manager: Arc<Mutex<krillnotes_core::core::contact::ContactManager>>,

// After:
pub contact_managers: Arc<Mutex<HashMap<Uuid, krillnotes_core::core::contact::ContactManager>>>,
```

- [ ] **Step 2: Update AppState initialization**

Find where `AppState` is constructed (in the `tauri::Builder` setup, likely in the `run()` function). Change:
```rust
// Before:
contact_manager: Arc::new(Mutex::new(
    krillnotes_core::core::contact::ContactManager::new(config_dir.clone())
        .expect("Failed to initialize contact manager"),
)),

// After:
contact_managers: Arc::new(Mutex::new(HashMap::new())),
```

- [ ] **Step 3: Update `unlock_identity` to create ContactManager**

In the `unlock_identity` Tauri command (around line 1771), after storing `unlocked` in `unlocked_identities`, add:

```rust
// Create per-identity ContactManager (decrypts contacts into memory)
let contacts_dir = settings::config_dir()
    .join("identities")
    .join(uuid.to_string())
    .join("contacts");
let contacts_key = unlocked.contacts_key();
match krillnotes_core::core::contact::ContactManager::for_identity(contacts_dir, contacts_key) {
    Ok(cm) => {
        state.contact_managers.lock().unwrap().insert(uuid, cm);
    }
    Err(e) => {
        // Non-fatal: log but don't fail unlock
        eprintln!("Warning: failed to initialize contact manager for {uuid}: {e}");
    }
}
```

Note: `settings::config_dir()` is the function in `lib.rs` (from the `settings` module) that returns `~/.config/krillnotes/`.

- [ ] **Step 4: Wire `create_identity` to also create ContactManager**

`create_identity` (around line 1743) auto-unlocks the identity internally and inserts it into `unlocked_identities` without going through the `unlock_identity` Tauri command. Find this command and add the same ContactManager creation block immediately after the `unlocked_identities.insert(...)` call:

```rust
let contacts_dir = settings::config_dir()
    .join("identities")
    .join(new_identity_uuid.to_string())
    .join("contacts");
let contacts_key = unlocked.contacts_key();
if let Ok(cm) = krillnotes_core::core::contact::ContactManager::for_identity(contacts_dir, contacts_key) {
    state.contact_managers.lock().unwrap().insert(new_identity_uuid, cm);
}
```

(Use the actual UUID variable name from the `create_identity` command — check the source.)

- [ ] **Step 5: Update `lock_identity` to drop ContactManager**

In the `lock_identity` command (around line 1791), after removing from `unlocked_identities`, add:

```rust
state.contact_managers.lock().unwrap().remove(&uuid);
```

- [ ] **Step 8: Fix `resolve_identity_name`**

Find `resolve_identity_name` in `lib.rs`. It currently calls `state.contact_manager`. Update it to search all unlocked contact managers:

```rust
// Before (approximate):
if let Ok(Some(contact)) = state.contact_manager.lock().unwrap().find_by_public_key(public_key) {
    return contact.display_name().to_string();
}

// After:
let cms = state.contact_managers.lock().unwrap();
for cm in cms.values() {
    if let Ok(Some(contact)) = cm.find_by_public_key(public_key) {
        return contact.display_name().to_string();
    }
}
```

- [ ] **Step 9: Verify it compiles**

```bash
cargo build -p krillnotes-desktop 2>&1 | grep -E "^error" | head -20
```
Expected: no errors (warnings about dead code are OK)

- [ ] **Step 10: Commit**

```bash
git add krillnotes-desktop/src-tauri/src/lib.rs
git commit -m "feat: per-identity ContactManager in AppState, wired to unlock/lock/create"
```

---

### Task 4: Add `ContactInfo` DTO and 6 Tauri commands

**Files:**
- Modify: `krillnotes-desktop/src-tauri/src/lib.rs`

Add `ContactInfo` near the other DTO structs at the top of `lib.rs`:

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactInfo {
    pub contact_id: String,
    pub declared_name: String,
    pub local_name: Option<String>,
    pub public_key: String,
    pub fingerprint: String,
    pub trust_level: String,
    pub first_seen: String,
    pub notes: Option<String>,
}

impl ContactInfo {
    fn from_contact(c: krillnotes_core::core::contact::Contact) -> Self {
        Self {
            contact_id: c.contact_id.to_string(),
            declared_name: c.declared_name,
            local_name: c.local_name,
            public_key: c.public_key,
            fingerprint: c.fingerprint,
            trust_level: trust_level_to_str(&c.trust_level).to_string(),
            first_seen: c.first_seen.to_rfc3339(),
            notes: c.notes,
        }
    }
}
```

- [ ] **Step 1: Write the 6 commands**

Add these commands in the Tauri commands section of `lib.rs`. Each one resolves the identity UUID, finds the ContactManager, and delegates:

```rust
/// Helper: get ContactManager for a locked or unlocked identity.
/// Returns Err if identity is not unlocked.
fn get_contact_manager<'a>(
    state: &'a AppState,
    identity_uuid_str: &str,
) -> std::result::Result<
    std::sync::MutexGuard<'a, HashMap<Uuid, krillnotes_core::core::contact::ContactManager>>,
    String,
> {
    let uuid = Uuid::parse_str(identity_uuid_str).map_err(|e| e.to_string())?;
    let cms = state.contact_managers.lock().unwrap();
    if !cms.contains_key(&uuid) {
        return Err("Identity not unlocked".to_string());
    }
    Ok(cms)
}
```

Note: the above helper has lifetime issues with MutexGuard. Instead, inline the lookup in each command as shown below:

```rust
#[tauri::command]
fn list_contacts(
    state: State<'_, AppState>,
    identity_uuid: String,
) -> std::result::Result<Vec<ContactInfo>, String> {
    let uuid = Uuid::parse_str(&identity_uuid).map_err(|e| e.to_string())?;
    let cms = state.contact_managers.lock().unwrap();
    let cm = cms.get(&uuid).ok_or("Identity not unlocked")?;
    let contacts = cm.list_contacts().map_err(|e| e.to_string())?;
    Ok(contacts.into_iter().map(ContactInfo::from_contact).collect())
}

#[tauri::command]
fn get_contact(
    state: State<'_, AppState>,
    identity_uuid: String,
    contact_id: String,
) -> std::result::Result<Option<ContactInfo>, String> {
    let uuid = Uuid::parse_str(&identity_uuid).map_err(|e| e.to_string())?;
    let cid = Uuid::parse_str(&contact_id).map_err(|e| e.to_string())?;
    let cms = state.contact_managers.lock().unwrap();
    let cm = cms.get(&uuid).ok_or("Identity not unlocked")?;
    let contact = cm.get_contact(cid).map_err(|e| e.to_string())?;
    Ok(contact.map(ContactInfo::from_contact))
}

#[tauri::command]
fn create_contact(
    state: State<'_, AppState>,
    identity_uuid: String,
    declared_name: String,
    public_key: String,
    trust_level: String,
) -> std::result::Result<ContactInfo, String> {
    let uuid = Uuid::parse_str(&identity_uuid).map_err(|e| e.to_string())?;
    let tl = parse_trust_level(&trust_level)?;
    let cms = state.contact_managers.lock().unwrap();
    let cm = cms.get(&uuid).ok_or("Identity not unlocked")?;
    let contact = cm.create_contact(&declared_name, &public_key, tl)
        .map_err(|e| e.to_string())?;
    Ok(ContactInfo::from_contact(contact))
}

#[tauri::command]
fn update_contact(
    state: State<'_, AppState>,
    identity_uuid: String,
    contact_id: String,
    local_name: Option<String>,
    notes: Option<String>,
    trust_level: String,
) -> std::result::Result<ContactInfo, String> {
    let uuid = Uuid::parse_str(&identity_uuid).map_err(|e| e.to_string())?;
    let cid = Uuid::parse_str(&contact_id).map_err(|e| e.to_string())?;
    let tl = parse_trust_level(&trust_level)?;
    let cms = state.contact_managers.lock().unwrap();
    let cm = cms.get(&uuid).ok_or("Identity not unlocked")?;
    let mut contact = cm.get_contact(cid)
        .map_err(|e| e.to_string())?
        .ok_or("Contact not found")?;
    contact.local_name = local_name;
    contact.notes = notes;
    contact.trust_level = tl;
    cm.save_contact(&contact).map_err(|e| e.to_string())?;
    Ok(ContactInfo::from_contact(contact))
}

#[tauri::command]
fn delete_contact(
    state: State<'_, AppState>,
    identity_uuid: String,
    contact_id: String,
) -> std::result::Result<(), String> {
    let uuid = Uuid::parse_str(&identity_uuid).map_err(|e| e.to_string())?;
    let cid = Uuid::parse_str(&contact_id).map_err(|e| e.to_string())?;
    let cms = state.contact_managers.lock().unwrap();
    let cm = cms.get(&uuid).ok_or("Identity not unlocked")?;
    cm.delete_contact(cid).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_fingerprint(public_key: String) -> std::result::Result<String, String> {
    krillnotes_core::core::contact::generate_fingerprint(&public_key)
        .map_err(|e| e.to_string())
}
```

Add a helper for parsing trust level strings:

```rust
fn parse_trust_level(s: &str) -> std::result::Result<krillnotes_core::core::contact::TrustLevel, String> {
    use krillnotes_core::core::contact::TrustLevel;
    match s {
        "Tofu" => Ok(TrustLevel::Tofu),
        "CodeVerified" => Ok(TrustLevel::CodeVerified),
        "Vouched" => Ok(TrustLevel::Vouched),
        "VerifiedInPerson" => Ok(TrustLevel::VerifiedInPerson),
        other => Err(format!("Unknown trust level: {other}")),
    }
}

/// Explicit string mapping — do NOT use `format!("{:?}", ...)` which is fragile.
fn trust_level_to_str(tl: &krillnotes_core::core::contact::TrustLevel) -> &'static str {
    use krillnotes_core::core::contact::TrustLevel;
    match tl {
        TrustLevel::Tofu => "Tofu",
        TrustLevel::CodeVerified => "CodeVerified",
        TrustLevel::Vouched => "Vouched",
        TrustLevel::VerifiedInPerson => "VerifiedInPerson",
    }
}
```

- [ ] **Step 2: Register all 6 commands in `tauri::generate_handler!`**

Find the `generate_handler!` macro call and add:
```rust
list_contacts,
get_contact,
create_contact,
update_contact,
delete_contact,
get_fingerprint,
```

- [ ] **Step 3: Verify it compiles**

```bash
cargo build -p krillnotes-desktop 2>&1 | grep -E "^error" | head -20
```
Expected: no errors

- [ ] **Step 4: Commit**

```bash
git -C /Users/careck/Source/Krillnotes add krillnotes-desktop/src-tauri/src/lib.rs
git -C /Users/careck/Source/Krillnotes commit -m "feat: add ContactInfo DTO and 6 contact CRUD Tauri commands"
```

---

## Chunk 3: Frontend — Contact Book UI

### Files

- **Create:** `krillnotes-desktop/src/components/ContactBookDialog.tsx`
- **Create:** `krillnotes-desktop/src/components/AddContactDialog.tsx`
- **Create:** `krillnotes-desktop/src/components/EditContactDialog.tsx`
- **Modify:** `krillnotes-desktop/src/types.ts`
- **Modify:** `krillnotes-desktop/src/components/IdentityManagerDialog.tsx`

---

### Task 5: Add TypeScript types

**Files:**
- Modify: `krillnotes-desktop/src/types.ts`

- [ ] **Step 1: Add `ContactInfo` and `TrustLevel` to `types.ts`**

```typescript
export type TrustLevel = 'Tofu' | 'CodeVerified' | 'Vouched' | 'VerifiedInPerson';

export interface ContactInfo {
  contactId: string;
  declaredName: string;
  localName: string | null;
  publicKey: string;
  fingerprint: string;
  trustLevel: TrustLevel;
  firstSeen: string; // ISO 8601
  notes: string | null;
}
```

- [ ] **Step 2: TypeScript type check**

```bash
cd /Users/careck/Source/Krillnotes/krillnotes-desktop && npx tsc --noEmit 2>&1 | head -20
```
Expected: no new errors

- [ ] **Step 3: Commit**

```bash
git -C /Users/careck/Source/Krillnotes add krillnotes-desktop/src/types.ts
git -C /Users/careck/Source/Krillnotes commit -m "feat: add ContactInfo and TrustLevel TypeScript types"
```

---

### Task 6: Create `AddContactDialog`

**Files:**
- Create: `krillnotes-desktop/src/components/AddContactDialog.tsx`

This dialog handles the "add contact" flow: name + public key + trust level, with a fingerprint verification gate for `VerifiedInPerson`.

- [ ] **Step 1: Create the component**

```typescript
import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useTranslation } from 'react-i18next';
import { TrustLevel, ContactInfo } from '../types';

interface AddContactDialogProps {
  identityUuid: string;
  onSaved: (contact: ContactInfo) => void;
  onClose: () => void;
}

const TRUST_LEVELS: { value: TrustLevel; label: string }[] = [
  { value: 'Tofu', label: 'Trust on first use' },
  { value: 'CodeVerified', label: 'Code verified (phone/video)' },
  { value: 'Vouched', label: 'Vouched for by another contact' },
  { value: 'VerifiedInPerson', label: 'Verified in person (highest)' },
];

export default function AddContactDialog({ identityUuid, onSaved, onClose }: AddContactDialogProps) {
  const { t } = useTranslation();
  const [name, setName] = useState('');
  const [publicKey, setPublicKey] = useState('');
  const [trustLevel, setTrustLevel] = useState<TrustLevel>('Tofu');
  const [fingerprint, setFingerprint] = useState<string | null>(null);
  const [fingerprintConfirmed, setFingerprintConfirmed] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Live fingerprint preview as public key is entered
  useEffect(() => {
    if (publicKey.trim().length < 10) {
      setFingerprint(null);
      return;
    }
    invoke<string>('get_fingerprint', { publicKey: publicKey.trim() })
      .then(fp => setFingerprint(fp))
      .catch(() => setFingerprint(null));
  }, [publicKey]);

  // Reset fingerprint confirmation when trust level changes away from VerifiedInPerson
  useEffect(() => {
    if (trustLevel !== 'VerifiedInPerson') setFingerprintConfirmed(false);
  }, [trustLevel]);

  const canSave =
    name.trim().length > 0 &&
    publicKey.trim().length > 0 &&
    fingerprint !== null &&
    (trustLevel !== 'VerifiedInPerson' || fingerprintConfirmed);

  async function handleSave() {
    setSaving(true);
    setError(null);
    try {
      const contact = await invoke<ContactInfo>('create_contact', {
        identityUuid,
        declaredName: name.trim(),
        publicKey: publicKey.trim(),
        trustLevel,
      });
      onSaved(contact);
    } catch (e) {
      setError(String(e));
      setSaving(false);
    }
  }

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-[var(--color-surface)] border border-[var(--color-border)] rounded-lg p-6 w-full max-w-md shadow-xl">
        <h2 className="text-lg font-semibold mb-4">Add Contact</h2>

        <div className="space-y-4">
          <div>
            <label className="block text-sm font-medium mb-1">Name</label>
            <input
              type="text"
              value={name}
              onChange={e => setName(e.target.value)}
              placeholder="Display name"
              className="w-full px-3 py-2 rounded border border-[var(--color-border)] bg-[var(--color-input)] text-sm"
            />
          </div>

          <div>
            <label className="block text-sm font-medium mb-1">Public Key</label>
            <textarea
              value={publicKey}
              onChange={e => setPublicKey(e.target.value)}
              placeholder="Paste base64-encoded Ed25519 public key"
              rows={3}
              className="w-full px-3 py-2 rounded border border-[var(--color-border)] bg-[var(--color-input)] text-sm font-mono"
            />
            {fingerprint && (
              <p className="mt-1 text-xs text-[var(--color-text-muted)] font-mono">
                Fingerprint: <span className="font-semibold">{fingerprint}</span>
              </p>
            )}
          </div>

          <div>
            <label className="block text-sm font-medium mb-1">Trust Level</label>
            <select
              value={trustLevel}
              onChange={e => setTrustLevel(e.target.value as TrustLevel)}
              className="w-full px-3 py-2 rounded border border-[var(--color-border)] bg-[var(--color-input)] text-sm"
            >
              {TRUST_LEVELS.map(tl => (
                <option key={tl.value} value={tl.value}>{tl.label}</option>
              ))}
            </select>
          </div>

          {trustLevel === 'VerifiedInPerson' && fingerprint && (
            <div className="rounded-lg border border-amber-400/50 bg-amber-50/10 p-4 space-y-3">
              <p className="text-sm font-medium">Fingerprint Verification Required</p>
              <p className="text-xs text-[var(--color-text-muted)]">
                Ask your contact to read their fingerprint aloud. Does it match what you see below?
              </p>
              <p className="text-lg font-mono font-bold tracking-wider text-center py-2">
                {fingerprint}
              </p>
              <label className="flex items-center gap-2 text-sm cursor-pointer">
                <input
                  type="checkbox"
                  checked={fingerprintConfirmed}
                  onChange={e => setFingerprintConfirmed(e.target.checked)}
                  className="rounded"
                />
                Yes, the fingerprint matches
              </label>
            </div>
          )}

          {error && (
            <p className="text-sm text-red-500">{error}</p>
          )}
        </div>

        <div className="flex justify-end gap-2 mt-6">
          <button
            onClick={onClose}
            className="px-4 py-2 text-sm rounded border border-[var(--color-border)] hover:bg-[var(--color-hover)]"
          >
            Cancel
          </button>
          <button
            onClick={handleSave}
            disabled={!canSave || saving}
            className="px-4 py-2 text-sm rounded bg-blue-600 text-white hover:bg-blue-700 disabled:opacity-50"
          >
            {saving ? 'Saving…' : 'Save Contact'}
          </button>
        </div>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: TypeScript check**

```bash
cd /Users/careck/Source/Krillnotes/krillnotes-desktop && npx tsc --noEmit 2>&1 | head -20
```
Expected: no errors

- [ ] **Step 3: Commit**

```bash
git -C /Users/careck/Source/Krillnotes add krillnotes-desktop/src/components/AddContactDialog.tsx
git -C /Users/careck/Source/Krillnotes commit -m "feat: add AddContactDialog with fingerprint verification gate"
```

---

### Task 7: Create `EditContactDialog`

**Files:**
- Create: `krillnotes-desktop/src/components/EditContactDialog.tsx`

- [ ] **Step 1: Create the component**

```typescript
import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { ContactInfo, TrustLevel } from '../types';

interface EditContactDialogProps {
  identityUuid: string;
  contact: ContactInfo;
  onSaved: (contact: ContactInfo) => void;
  onDeleted: (contactId: string) => void;
  onClose: () => void;
}

const TRUST_LEVELS: { value: TrustLevel; label: string }[] = [
  { value: 'Tofu', label: 'Trust on first use' },
  { value: 'CodeVerified', label: 'Code verified (phone/video)' },
  { value: 'Vouched', label: 'Vouched for by another contact' },
  { value: 'VerifiedInPerson', label: 'Verified in person (highest)' },
];

export default function EditContactDialog({
  identityUuid,
  contact,
  onSaved,
  onDeleted,
  onClose,
}: EditContactDialogProps) {
  const [localName, setLocalName] = useState(contact.localName ?? '');
  const [notes, setNotes] = useState(contact.notes ?? '');
  const [trustLevel, setTrustLevel] = useState<TrustLevel>(contact.trustLevel);
  const [fingerprintConfirmed, setFingerprintConfirmed] = useState(false);
  const [saving, setSaving] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (trustLevel !== 'VerifiedInPerson') setFingerprintConfirmed(false);
  }, [trustLevel]);

  const needsFingerprintConfirm =
    trustLevel === 'VerifiedInPerson' && contact.trustLevel !== 'VerifiedInPerson';

  const canSave = !needsFingerprintConfirm || fingerprintConfirmed;

  async function handleSave() {
    setSaving(true);
    setError(null);
    try {
      const updated = await invoke<ContactInfo>('update_contact', {
        identityUuid,
        contactId: contact.contactId,
        localName: localName.trim() || null,
        notes: notes.trim() || null,
        trustLevel,
      });
      onSaved(updated);
    } catch (e) {
      setError(String(e));
      setSaving(false);
    }
  }

  async function handleDelete() {
    if (!confirmDelete) {
      setConfirmDelete(true);
      return;
    }
    setDeleting(true);
    try {
      await invoke('delete_contact', {
        identityUuid,
        contactId: contact.contactId,
      });
      onDeleted(contact.contactId);
    } catch (e) {
      setError(String(e));
      setDeleting(false);
    }
  }

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-[var(--color-surface)] border border-[var(--color-border)] rounded-lg p-6 w-full max-w-md shadow-xl">
        <h2 className="text-lg font-semibold mb-1">Edit Contact</h2>
        <p className="text-sm text-[var(--color-text-muted)] mb-4">
          Declared name: <span className="font-medium">{contact.declaredName}</span>
        </p>

        <div className="space-y-4">
          {/* Read-only: fingerprint + public key */}
          <div className="rounded border border-[var(--color-border)] p-3 space-y-1">
            <p className="text-xs font-medium uppercase tracking-wider text-[var(--color-text-muted)]">Fingerprint</p>
            <p className="font-mono font-semibold">{contact.fingerprint}</p>
            <p className="text-xs font-mono text-[var(--color-text-muted)] break-all">{contact.publicKey}</p>
          </div>

          <div>
            <label className="block text-sm font-medium mb-1">Local Name Override</label>
            <input
              type="text"
              value={localName}
              onChange={e => setLocalName(e.target.value)}
              placeholder={contact.declaredName}
              className="w-full px-3 py-2 rounded border border-[var(--color-border)] bg-[var(--color-input)] text-sm"
            />
            <p className="text-xs text-[var(--color-text-muted)] mt-1">
              Shown only to you — never shared with peers
            </p>
          </div>

          <div>
            <label className="block text-sm font-medium mb-1">Trust Level</label>
            <select
              value={trustLevel}
              onChange={e => setTrustLevel(e.target.value as TrustLevel)}
              className="w-full px-3 py-2 rounded border border-[var(--color-border)] bg-[var(--color-input)] text-sm"
            >
              {TRUST_LEVELS.map(tl => (
                <option key={tl.value} value={tl.value}>{tl.label}</option>
              ))}
            </select>
          </div>

          {needsFingerprintConfirm && (
            <div className="rounded-lg border border-amber-400/50 bg-amber-50/10 p-4 space-y-3">
              <p className="text-sm font-medium">Fingerprint Verification Required</p>
              <p className="text-xs text-[var(--color-text-muted)]">
                Ask your contact to read their fingerprint aloud. Does it match?
              </p>
              <p className="text-lg font-mono font-bold tracking-wider text-center py-2">
                {contact.fingerprint}
              </p>
              <label className="flex items-center gap-2 text-sm cursor-pointer">
                <input
                  type="checkbox"
                  checked={fingerprintConfirmed}
                  onChange={e => setFingerprintConfirmed(e.target.checked)}
                  className="rounded"
                />
                Yes, the fingerprint matches
              </label>
            </div>
          )}

          <div>
            <label className="block text-sm font-medium mb-1">Notes</label>
            <textarea
              value={notes}
              onChange={e => setNotes(e.target.value)}
              placeholder="Private notes about this contact…"
              rows={3}
              className="w-full px-3 py-2 rounded border border-[var(--color-border)] bg-[var(--color-input)] text-sm"
            />
          </div>

          {error && <p className="text-sm text-red-500">{error}</p>}
        </div>

        <div className="flex justify-between mt-6">
          <button
            onClick={handleDelete}
            disabled={deleting}
            className={`px-4 py-2 text-sm rounded ${
              confirmDelete
                ? 'bg-red-600 text-white hover:bg-red-700'
                : 'border border-red-400 text-red-500 hover:bg-red-50/10'
            } disabled:opacity-50`}
          >
            {deleting ? 'Deleting…' : confirmDelete ? 'Confirm Delete' : 'Delete'}
          </button>
          <div className="flex gap-2">
            <button
              onClick={onClose}
              className="px-4 py-2 text-sm rounded border border-[var(--color-border)] hover:bg-[var(--color-hover)]"
            >
              Cancel
            </button>
            <button
              onClick={handleSave}
              disabled={!canSave || saving}
              className="px-4 py-2 text-sm rounded bg-blue-600 text-white hover:bg-blue-700 disabled:opacity-50"
            >
              {saving ? 'Saving…' : 'Save'}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: TypeScript check**

```bash
cd /Users/careck/Source/Krillnotes/krillnotes-desktop && npx tsc --noEmit 2>&1 | head -20
```
Expected: no errors

- [ ] **Step 3: Commit**

```bash
git add krillnotes-desktop/src/components/EditContactDialog.tsx
git commit -m "feat: add EditContactDialog with delete confirmation and trust gate"
```

---

### Task 8: Create `ContactBookDialog`

**Files:**
- Create: `krillnotes-desktop/src/components/ContactBookDialog.tsx`

- [ ] **Step 1: Create the component**

```typescript
import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { ContactInfo } from '../types';
import AddContactDialog from './AddContactDialog';
import EditContactDialog from './EditContactDialog';

interface ContactBookDialogProps {
  identityUuid: string;
  identityName: string;
  onClose: () => void;
}

const TRUST_BADGE: Record<string, { label: string; class: string }> = {
  Tofu:             { label: 'TOFU',     class: 'bg-gray-500/20 text-gray-400' },
  CodeVerified:     { label: 'Code',     class: 'bg-blue-500/20 text-blue-400' },
  Vouched:          { label: 'Vouched',  class: 'bg-purple-500/20 text-purple-400' },
  VerifiedInPerson: { label: 'Verified', class: 'bg-green-500/20 text-green-400' },
};

export default function ContactBookDialog({ identityUuid, identityName, onClose }: ContactBookDialogProps) {
  const [contacts, setContacts] = useState<ContactInfo[]>([]);
  const [search, setSearch] = useState('');
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [showAdd, setShowAdd] = useState(false);
  const [editing, setEditing] = useState<ContactInfo | null>(null);

  const load = useCallback(async () => {
    try {
      const list = await invoke<ContactInfo[]>('list_contacts', { identityUuid });
      setContacts(list);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [identityUuid]);

  useEffect(() => { load(); }, [load]);

  const filtered = contacts.filter(c => {
    const q = search.toLowerCase();
    return (
      (c.localName ?? c.declaredName).toLowerCase().includes(q) ||
      c.publicKey.toLowerCase().startsWith(q)
    );
  });

  function handleSaved(contact: ContactInfo) {
    setContacts(prev => {
      const idx = prev.findIndex(c => c.contactId === contact.contactId);
      if (idx >= 0) {
        const next = [...prev];
        next[idx] = contact;
        return next.sort((a, b) =>
          (a.localName ?? a.declaredName).localeCompare(b.localName ?? b.declaredName)
        );
      }
      return [...prev, contact].sort((a, b) =>
        (a.localName ?? a.declaredName).localeCompare(b.localName ?? b.declaredName)
      );
    });
    setShowAdd(false);
    setEditing(null);
  }

  function handleDeleted(contactId: string) {
    setContacts(prev => prev.filter(c => c.contactId !== contactId));
    setEditing(null);
  }

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-40">
      <div className="bg-[var(--color-surface)] border border-[var(--color-border)] rounded-lg w-full max-w-lg shadow-xl flex flex-col" style={{ maxHeight: '80vh' }}>
        {/* Header */}
        <div className="flex items-center justify-between p-4 border-b border-[var(--color-border)]">
          <div>
            <h2 className="text-lg font-semibold">Contacts</h2>
            <p className="text-xs text-[var(--color-text-muted)]">{identityName}</p>
          </div>
          <button
            onClick={onClose}
            className="text-[var(--color-text-muted)] hover:text-[var(--color-text)] px-2"
          >
            ✕
          </button>
        </div>

        {/* Search + Add */}
        <div className="flex gap-2 p-3 border-b border-[var(--color-border)]">
          <input
            type="text"
            value={search}
            onChange={e => setSearch(e.target.value)}
            placeholder="Search by name or public key…"
            className="flex-1 px-3 py-1.5 rounded border border-[var(--color-border)] bg-[var(--color-input)] text-sm"
          />
          <button
            onClick={() => setShowAdd(true)}
            className="px-3 py-1.5 text-sm rounded bg-blue-600 text-white hover:bg-blue-700 whitespace-nowrap"
          >
            + Add
          </button>
        </div>

        {/* Contact list */}
        <div className="overflow-y-auto flex-1">
          {loading && (
            <p className="text-sm text-center text-[var(--color-text-muted)] py-8">Loading…</p>
          )}
          {!loading && error && (
            <p className="text-sm text-center text-red-500 py-8">{error}</p>
          )}
          {!loading && !error && filtered.length === 0 && (
            <p className="text-sm text-center text-[var(--color-text-muted)] py-8">
              {search ? 'No contacts match your search.' : 'No contacts yet. Add one to get started.'}
            </p>
          )}
          {filtered.map(contact => {
            const badge = TRUST_BADGE[contact.trustLevel] ?? TRUST_BADGE.Tofu;
            const displayName = contact.localName ?? contact.declaredName;
            return (
              <button
                key={contact.contactId}
                onClick={() => setEditing(contact)}
                className="w-full flex items-center gap-3 px-4 py-3 text-left hover:bg-[var(--color-hover)] border-b border-[var(--color-border)] last:border-0"
              >
                <div className="flex-1 min-w-0">
                  <p className="text-sm font-medium truncate">{displayName}</p>
                  <p className="text-xs font-mono text-[var(--color-text-muted)] truncate">{contact.fingerprint}</p>
                </div>
                <span className={`text-xs px-2 py-0.5 rounded-full font-medium ${badge.class}`}>
                  {badge.label}
                </span>
              </button>
            );
          })}
        </div>

        {/* Footer count */}
        <div className="px-4 py-2 border-t border-[var(--color-border)] text-xs text-[var(--color-text-muted)]">
          {contacts.length} contact{contacts.length !== 1 ? 's' : ''}
        </div>
      </div>

      {/* Sub-dialogs */}
      {showAdd && (
        <AddContactDialog
          identityUuid={identityUuid}
          onSaved={handleSaved}
          onClose={() => setShowAdd(false)}
        />
      )}
      {editing && (
        <EditContactDialog
          identityUuid={identityUuid}
          contact={editing}
          onSaved={handleSaved}
          onDeleted={handleDeleted}
          onClose={() => setEditing(null)}
        />
      )}
    </div>
  );
}
```

- [ ] **Step 2: TypeScript check**

```bash
cd /Users/careck/Source/Krillnotes/krillnotes-desktop && npx tsc --noEmit 2>&1 | head -20
```

- [ ] **Step 3: Commit**

```bash
git -C /Users/careck/Source/Krillnotes add krillnotes-desktop/src/components/ContactBookDialog.tsx
git -C /Users/careck/Source/Krillnotes commit -m "feat: add ContactBookDialog with search and list"
```

---

### Task 9: Wire `ContactBookDialog` into `IdentityManagerDialog`

**Files:**
- Modify: `krillnotes-desktop/src/components/IdentityManagerDialog.tsx`

The `IdentityManagerDialog` has a selection model: the selected identity UUID is `selectedUuid`. We need to:
1. Add `showContacts` state
2. Add a "Contacts (n)" button in the toolbar (only when the selected identity is unlocked)
3. Load the contact count for the selected identity when it's unlocked
4. Render `ContactBookDialog` when open

- [ ] **Step 1: Add state and count loading**

Add near the top of the component (with the other state variables):

```typescript
const [showContacts, setShowContacts] = useState(false);
const [contactCount, setContactCount] = useState<number | null>(null);
```

Add a `useEffect` that loads the contact count when a selected, unlocked identity changes:

```typescript
useEffect(() => {
  if (!selectedUuid || !unlockedIds.has(selectedUuid)) {
    setContactCount(null);
    return;
  }
  invoke<ContactInfo[]>('list_contacts', { identityUuid: selectedUuid })
    .then(list => setContactCount(list.length))
    .catch(() => setContactCount(null));
}, [selectedUuid, unlockedIds]);
```

Add the import at the top:
```typescript
import { ContactInfo } from '../types';
import ContactBookDialog from './ContactBookDialog';
```

- [ ] **Step 2: Add "Contacts" button to the toolbar**

In the per-identity toolbar section (around line 515, where "rename", "change passphrase" etc. buttons are), add:

```typescript
{selectedUuid && unlockedIds.has(selectedUuid) && (
  <button
    onClick={() => setShowContacts(true)}
    className="px-3 py-1.5 text-sm rounded border border-[var(--color-border)] hover:bg-[var(--color-hover)]"
  >
    Contacts{contactCount !== null ? ` (${contactCount})` : ''}
  </button>
)}
```

- [ ] **Step 3: Render `ContactBookDialog`**

At the bottom of the return, alongside the other sub-dialogs (`CreateIdentityDialog`, `UnlockIdentityDialog`), add:

```typescript
{showContacts && selectedUuid && (
  <ContactBookDialog
    identityUuid={selectedUuid}
    identityName={identities.find(i => i.uuid === selectedUuid)?.displayName ?? ''}
    onClose={() => {
      setShowContacts(false);
      // Refresh count after closing
      if (selectedUuid && unlockedIds.has(selectedUuid)) {
        invoke<ContactInfo[]>('list_contacts', { identityUuid: selectedUuid })
          .then(list => setContactCount(list.length))
          .catch(() => {});
      }
    }}
  />
)}
```

- [ ] **Step 4: TypeScript check**

```bash
cd /Users/careck/Source/Krillnotes/krillnotes-desktop && npx tsc --noEmit 2>&1 | head -20
```
Expected: no errors

- [ ] **Step 5: Full build check**

```bash
cd /Users/careck/Source/Krillnotes/krillnotes-desktop && cargo build --manifest-path src-tauri/Cargo.toml 2>&1 | grep -E "^error" | head -10
```

- [ ] **Step 6: Commit**

```bash
git -C /Users/careck/Source/Krillnotes add krillnotes-desktop/src/components/IdentityManagerDialog.tsx
git -C /Users/careck/Source/Krillnotes commit -m "feat: wire ContactBookDialog into IdentityManagerDialog"
```

---

## Final Steps

- [ ] **Update CHANGELOG.md**

Add an entry under the next version heading:

```markdown
### Added
- Per-identity encrypted contact book (Phase A of Peer Contact Manager, issue #90)
  - Contacts stored encrypted at rest, decrypted in memory when identity is unlocked
  - CRUD UI accessible from the Identity Manager dialog
  - Fingerprint verification required to set highest trust level (Verified In Person)
```

- [ ] **Create PR**

```bash
git -C /Users/careck/Source/Krillnotes push github-https HEAD
# Then open PR targeting master
```
