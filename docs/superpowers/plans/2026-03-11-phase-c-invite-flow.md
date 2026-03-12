# Phase C — Invite Flow Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the Phase C invite flow — multi-use signed `.swarm` invite files, response handling, and UI for both the inviter and invitee sides.

**Architecture:** A new `InviteManager` in `krillnotes-core` manages invite records on disk and produces/verifies Ed25519-signed `.swarm` files. Seven Tauri commands expose this to the frontend. Four new React components cover the full flow: inviter creates invites and reviews responses; invitee imports invites and generates responses. The older stub invite commands (`create_invite_bundle_cmd`, `create_accept_bundle_cmd`, `create_snapshot_bundle_cmd`, `create_workspace_from_snapshot_cmd`) are removed and replaced.

**Tech Stack:** Rust + ed25519-dalek + serde_json (canonical JSON signing) + Tauri v2 + React 19 + Tailwind v4 + i18next

**Spec:** `docs/superpowers/specs/2026-03-10-peer-contact-manager-design.md` — Phase C section

---

## File Map

| File | Action | Responsibility |
|------|--------|----------------|
| `krillnotes-core/src/core/invite.rs` | **Create** | `InviteManager`, `InviteRecord`, `InviteFile`, `InviteResponseFile`, sign/verify helpers |
| `krillnotes-core/src/core/error.rs` | **Modify** | Add `InvalidSignature` and `InviteExpired` variants |
| `krillnotes-core/src/lib.rs` | **Modify** | `pub mod invite` re-export |
| `krillnotes-desktop/src-tauri/src/lib.rs` | **Modify** | `AppState` + 7 new Tauri commands; remove 4 old stub invite commands |
| `krillnotes-desktop/src/types.ts` | **Modify** | `InviteInfo`, `PendingPeer`, `InviteFileData` interfaces |
| `krillnotes-desktop/src/components/InviteManagerDialog.tsx` | **Create** | Lists open invites, revoke action, entry to sub-dialogs |
| `krillnotes-desktop/src/components/CreateInviteDialog.tsx` | **Create** | Expiry selector, preview, create + save invite `.swarm` |
| `krillnotes-desktop/src/components/AcceptPeerDialog.tsx` | **Create** | Fingerprint verification gate, trust selector, accept/reject response |
| `krillnotes-desktop/src/components/ImportInviteDialog.tsx` | **Create** | Invitee: workspace metadata display, fingerprint gate, save response `.swarm` |
| `krillnotes-desktop/src/components/WorkspacePeersDialog.tsx` | **Modify** | Replace old invite form with "Invites" button → `InviteManagerDialog`; add "Import Invite" button → `ImportInviteDialog` |

---

## Chunk 1: Rust Core — invite.rs

### Task 1: Structs and error variants

**Files:**
- Create: `krillnotes-core/src/core/invite.rs`
- Modify: `krillnotes-core/src/core/error.rs`

- [ ] **Step 1: Add error variants**

In `krillnotes-core/src/core/error.rs`, add after the `Swarm(String)` variant:

```rust
    #[error("Invalid signature")]
    InvalidSignature,
    #[error("Invite expired or revoked")]
    InviteExpiredOrRevoked,
```

- [ ] **Step 2: Create invite.rs with structs**

Create `krillnotes-core/src/core/invite.rs`:

```rust
use std::path::{Path, PathBuf};
use std::collections::BTreeMap;
use chrono::{DateTime, Utc, Duration};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{SigningKey, Signer, VerifyingKey, Verifier, Signature};
use crate::core::error::KrillnotesError;

type Result<T> = std::result::Result<T, KrillnotesError>;

// ── On-disk invite record (plaintext, managed by inviter) ─────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InviteRecord {
    pub invite_id: Uuid,
    pub workspace_id: String,
    pub workspace_name: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked: bool,
    pub use_count: u32,
}

// ── .swarm file formats ───────────────────────────────────────────────────────

/// The invite `.swarm` file sent to invitees. All workspace_* fields are optional.
/// NOTE: No `rename_all` — field names already match the spec's snake_case wire format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InviteFile {
    #[serde(rename = "type")]
    pub file_type: String,
    pub invite_id: String,
    pub workspace_id: String,
    pub workspace_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_author_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_author_org: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_homepage_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_license: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_language: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspace_tags: Vec<String>,
    pub inviter_public_key: String,
    pub inviter_declared_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    pub signature: String,
}

/// The response `.swarm` file sent back by the invitee.
/// NOTE: No `rename_all` — field names match the spec's snake_case wire format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InviteResponseFile {
    #[serde(rename = "type")]
    pub file_type: String,
    pub invite_id: String,
    pub invitee_public_key: String,
    pub invitee_declared_name: String,
    pub signature: String,
}
```

- [ ] **Step 3: Commit structs**

```bash
git add krillnotes-core/src/core/invite.rs krillnotes-core/src/core/error.rs
git commit -m "feat(invite): add InviteRecord, InviteFile, InviteResponseFile structs and error variants"
```

---

### Task 2: Signing and verification helpers

**Files:**
- Modify: `krillnotes-core/src/core/invite.rs` (append)

- [ ] **Step 1: Write failing tests first**

Append to `invite.rs` (tests only — helpers not yet written):

```rust
#[cfg(test)]
mod signing_tests {
    use super::*;

    fn test_key() -> SigningKey {
        SigningKey::from_bytes(&[42u8; 32])
    }

    #[test]
    fn sign_verify_roundtrip() {
        let key = test_key();
        let pubkey_b64 = STANDARD.encode(key.verifying_key().to_bytes());
        let payload = serde_json::json!({ "hello": "world", "number": 42 });
        let sig = sign_payload(&payload, &key);
        let mut signed = payload.clone();
        signed["signature"] = serde_json::Value::String(sig);
        assert!(verify_payload(&signed, signed["signature"].as_str().unwrap(), &pubkey_b64).is_ok());
    }

    #[test]
    fn verify_fails_on_tampered_payload() {
        let key = test_key();
        let pubkey_b64 = STANDARD.encode(key.verifying_key().to_bytes());
        let payload = serde_json::json!({ "hello": "world" });
        let sig = sign_payload(&payload, &key);
        let mut tampered = payload.clone();
        tampered["hello"] = serde_json::Value::String("evil".to_string());
        tampered["signature"] = serde_json::Value::String(sig);
        assert!(verify_payload(&tampered, tampered["signature"].as_str().unwrap(), &pubkey_b64).is_err());
    }
}
```

- [ ] **Step 2: Run — expect compile failure (`sign_payload` not defined yet)**

```bash
cargo test -p krillnotes-core signing_tests 2>&1 | head -10
```

- [ ] **Step 3: Implement signing helpers**

Append the helpers to `invite.rs` (before the test module):

```rust
// ── Signing helpers ───────────────────────────────────────────────────────────

/// Sorts all JSON object keys recursively (for canonical serialization).
fn sort_json(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let sorted: BTreeMap<String, serde_json::Value> = map
                .into_iter()
                .map(|(k, v)| (k, sort_json(v)))
                .collect();
            serde_json::Value::Object(sorted.into_iter().collect())
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(sort_json).collect())
        }
        other => other,
    }
}

/// Sign a JSON value (with signature field removed) using Ed25519.
/// Returns base64-encoded signature.
pub fn sign_payload(payload: &serde_json::Value, signing_key: &SigningKey) -> String {
    let mut v = payload.clone();
    if let Some(obj) = v.as_object_mut() {
        obj.remove("signature");
    }
    let canonical = serde_json::to_string(&sort_json(v)).expect("serialization cannot fail");
    let sig = signing_key.sign(canonical.as_bytes());
    STANDARD.encode(sig.to_bytes())
}

/// Verify a JSON payload against a base64-encoded Ed25519 signature and public key.
pub fn verify_payload(
    payload: &serde_json::Value,
    signature_b64: &str,
    public_key_b64: &str,
) -> Result<()> {
    let pubkey_bytes: [u8; 32] = STANDARD
        .decode(public_key_b64)
        .map_err(|_| KrillnotesError::InvalidSignature)?
        .try_into()
        .map_err(|_| KrillnotesError::InvalidSignature)?;
    let verifying_key =
        VerifyingKey::from_bytes(&pubkey_bytes).map_err(|_| KrillnotesError::InvalidSignature)?;
    let sig_bytes: [u8; 64] = STANDARD
        .decode(signature_b64)
        .map_err(|_| KrillnotesError::InvalidSignature)?
        .try_into()
        .map_err(|_| KrillnotesError::InvalidSignature)?;
    let signature = Signature::from_bytes(&sig_bytes);

    let mut v = payload.clone();
    if let Some(obj) = v.as_object_mut() {
        obj.remove("signature");
    }
    let canonical = serde_json::to_string(&sort_json(v)).expect("serialization cannot fail");
    verifying_key
        .verify(canonical.as_bytes(), &signature)
        .map_err(|_| KrillnotesError::InvalidSignature)
}

```

- [ ] **Step 4: Run tests — expect green now**

```bash
cargo test -p krillnotes-core signing_tests
```
Expected: 2 tests pass.

- [ ] **Step 5: Commit**

```bash
git add krillnotes-core/src/core/invite.rs
git commit -m "feat(invite): add Ed25519 sign/verify helpers with canonical JSON"
```

---

### Task 3: InviteManager — inviter side

**Files:**
- Modify: `krillnotes-core/src/core/invite.rs` (append)

- [ ] **Step 1: Write failing tests**

Append to `invite.rs` test module (or add new `#[cfg(test)] mod manager_tests`):

```rust
#[cfg(test)]
mod manager_tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> (TempDir, SigningKey) {
        let dir = tempfile::tempdir().unwrap();
        let key = SigningKey::from_bytes(&[1u8; 32]);
        (dir, key)
    }

    #[test]
    fn create_and_list_invite() {
        let (dir, key) = setup();
        let mut mgr = InviteManager::new(dir.path().to_path_buf()).unwrap();
        let (record, file) = mgr
            .create_invite("ws-id", "My Workspace", None, &key, "Alice", None, None, None, None, None, vec![])
            .unwrap();
        assert_eq!(record.workspace_id, "ws-id");
        assert!(!record.revoked);
        assert_eq!(record.use_count, 0);
        assert_eq!(file.file_type, "krillnotes-invite-v1");
        let list = mgr.list_invites().unwrap();
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn revoke_invite() {
        let (dir, key) = setup();
        let mut mgr = InviteManager::new(dir.path().to_path_buf()).unwrap();
        let (record, _) = mgr
            .create_invite("ws-id", "My Workspace", None, &key, "Alice", None, None, None, None, None, vec![])
            .unwrap();
        mgr.revoke_invite(record.invite_id).unwrap();
        let list = mgr.list_invites().unwrap();
        assert!(list[0].revoked);
    }

    #[test]
    fn expires_in_days() {
        let (dir, key) = setup();
        let mut mgr = InviteManager::new(dir.path().to_path_buf()).unwrap();
        let (record, file) = mgr
            .create_invite("ws-id", "My Workspace", Some(7), &key, "Alice", None, None, None, None, None, vec![])
            .unwrap();
        assert!(record.expires_at.is_some());
        assert!(file.expires_at.is_some());
    }
}
```

- [ ] **Step 2: Run tests — expect compile failure (InviteManager not defined yet)**

```bash
cargo test -p krillnotes-core manager_tests 2>&1 | head -20
```

- [ ] **Step 3: Implement InviteManager**

Append to `invite.rs` (before the test modules):

```rust
// ── InviteManager ─────────────────────────────────────────────────────────────

pub struct InviteManager {
    invites_dir: PathBuf,
}

impl InviteManager {
    pub fn new(invites_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&invites_dir)?;
        Ok(Self { invites_dir })
    }

    fn path_for(&self, id: Uuid) -> PathBuf {
        self.invites_dir.join(format!("{}.json", id))
    }

    fn save_record(&self, record: &InviteRecord) -> Result<()> {
        let json = serde_json::to_string_pretty(record)?;
        std::fs::write(self.path_for(record.invite_id), json)?;
        Ok(())
    }

    /// Create a new invite and return the record + signed InviteFile.
    #[allow(clippy::too_many_arguments)]
    pub fn create_invite(
        &mut self,
        workspace_id: &str,
        workspace_name: &str,
        expires_in_days: Option<u32>,
        signing_key: &SigningKey,
        inviter_declared_name: &str,
        workspace_description: Option<String>,
        workspace_author_name: Option<String>,
        workspace_author_org: Option<String>,
        workspace_homepage_url: Option<String>,
        workspace_license: Option<String>,
        workspace_tags: Vec<String>,
    ) -> Result<(InviteRecord, InviteFile)> {
        let invite_id = Uuid::new_v4();
        let now = Utc::now();
        let expires_at = expires_in_days.map(|d| now + Duration::days(d as i64));

        let record = InviteRecord {
            invite_id,
            workspace_id: workspace_id.to_string(),
            workspace_name: workspace_name.to_string(),
            created_at: now,
            expires_at,
            revoked: false,
            use_count: 0,
        };
        self.save_record(&record)?;

        let pubkey_b64 = STANDARD.encode(signing_key.verifying_key().to_bytes());
        let mut file = InviteFile {
            file_type: "krillnotes-invite-v1".to_string(),
            invite_id: invite_id.to_string(),
            workspace_id: workspace_id.to_string(),
            workspace_name: workspace_name.to_string(),
            workspace_description,
            workspace_author_name,
            workspace_author_org,
            workspace_homepage_url,
            workspace_license,
            workspace_language: None,
            workspace_tags,
            inviter_public_key: pubkey_b64,
            inviter_declared_name: inviter_declared_name.to_string(),
            expires_at: expires_at.map(|dt| dt.to_rfc3339()),
            signature: String::new(),
        };
        let payload = serde_json::to_value(&file)?;
        file.signature = sign_payload(&payload, signing_key);
        Ok((record, file))
    }

    pub fn list_invites(&self) -> Result<Vec<InviteRecord>> {
        let mut records = Vec::new();
        for entry in std::fs::read_dir(&self.invites_dir)? {
            let entry = entry?;
            if entry.path().extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let json = std::fs::read_to_string(entry.path())?;
            let record: InviteRecord = serde_json::from_str(&json)?;
            records.push(record);
        }
        records.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(records)
    }

    pub fn get_invite(&self, invite_id: Uuid) -> Result<Option<InviteRecord>> {
        let path = self.path_for(invite_id);
        if !path.exists() {
            return Ok(None);
        }
        let json = std::fs::read_to_string(path)?;
        Ok(Some(serde_json::from_str(&json)?))
    }

    pub fn revoke_invite(&mut self, invite_id: Uuid) -> Result<()> {
        let mut record = self
            .get_invite(invite_id)?
            .ok_or_else(|| KrillnotesError::Swarm(format!("Invite {} not found", invite_id)))?;
        record.revoked = true;
        self.save_record(&record)
    }

    pub fn increment_use_count(&mut self, invite_id: Uuid) -> Result<()> {
        let mut record = self
            .get_invite(invite_id)?
            .ok_or_else(|| KrillnotesError::Swarm(format!("Invite {} not found", invite_id)))?;
        record.use_count += 1;
        self.save_record(&record)
    }

    /// Parse and verify a response `.swarm` file (inviter side).
    /// Returns the PendingPeer data. Does NOT check invite validity here —
    /// the Tauri command does that after looking up the record.
    pub fn parse_and_verify_response(path: &Path) -> Result<InviteResponseFile> {
        let json = std::fs::read_to_string(path)?;
        let response: InviteResponseFile = serde_json::from_str(&json)?;
        if response.file_type != "krillnotes-invite-response-v1" {
            return Err(KrillnotesError::Swarm("Not a response file".to_string()));
        }
        let payload = serde_json::to_value(&response)?;
        verify_payload(&payload, &response.signature, &response.invitee_public_key)?;
        Ok(response)
    }

    /// Parse and verify an invite `.swarm` file (invitee side).
    pub fn parse_and_verify_invite(path: &Path) -> Result<InviteFile> {
        let json = std::fs::read_to_string(path)?;
        let invite: InviteFile = serde_json::from_str(&json)?;
        if invite.file_type != "krillnotes-invite-v1" {
            return Err(KrillnotesError::Swarm("Not an invite file".to_string()));
        }
        let payload = serde_json::to_value(&invite)?;
        verify_payload(&payload, &invite.signature, &invite.inviter_public_key)?;
        Ok(invite)
    }

    /// Build and sign a response file (invitee side). Writes to `save_path`.
    pub fn build_and_save_response(
        invite: &InviteFile,
        signing_key: &SigningKey,
        declared_name: &str,
        save_path: &Path,
    ) -> Result<()> {
        let pubkey_b64 = STANDARD.encode(signing_key.verifying_key().to_bytes());
        let mut response = InviteResponseFile {
            file_type: "krillnotes-invite-response-v1".to_string(),
            invite_id: invite.invite_id.clone(),
            invitee_public_key: pubkey_b64,
            invitee_declared_name: declared_name.to_string(),
            signature: String::new(),
        };
        let payload = serde_json::to_value(&response)?;
        response.signature = sign_payload(&payload, signing_key);
        let json = serde_json::to_string_pretty(&response)?;
        std::fs::write(save_path, json)?;
        Ok(())
    }
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p krillnotes-core manager_tests
```
Expected: 3 tests pass.

- [ ] **Step 5: Write round-trip test (invite → response → verify)**

Append to `manager_tests`:

```rust
    #[test]
    fn invite_response_roundtrip() {
        let (dir, inviter_key) = setup();
        let mut mgr = InviteManager::new(dir.path().to_path_buf()).unwrap();
        let (_, invite_file) = mgr
            .create_invite("ws-id", "My Workspace", None, &inviter_key, "Alice", None, None, None, None, None, vec![])
            .unwrap();

        // Invitee builds response
        let invitee_key = SigningKey::from_bytes(&[2u8; 32]);
        let response_path = dir.path().join("response.swarm");
        InviteManager::build_and_save_response(&invite_file, &invitee_key, "Bob", &response_path).unwrap();

        // Inviter parses and verifies response
        let response = InviteManager::parse_and_verify_response(&response_path).unwrap();
        assert_eq!(response.invitee_declared_name, "Bob");
        assert_eq!(response.invite_id, invite_file.invite_id);
    }
```

- [ ] **Step 6: Run all invite tests**

```bash
cargo test -p krillnotes-core invite
```
Expected: all pass.

- [ ] **Step 7: Re-export module**

In `krillnotes-core/src/lib.rs`, add alongside other module exports:

```rust
pub mod invite;
```

(Find the existing `pub mod contact;` line and add `pub mod invite;` after it.)

- [ ] **Step 8: Commit**

```bash
git add krillnotes-core/src/core/invite.rs krillnotes-core/src/lib.rs
git commit -m "feat(invite): InviteManager with create/list/revoke/sign/verify and round-trip tests"
```

---

## Chunk 2: Tauri Commands

### Task 4: AppState + initialization

**Files:**
- Modify: `krillnotes-desktop/src-tauri/src/lib.rs`

- [ ] **Step 1: Add `invite_managers` to AppState**

Find the `AppState` struct (around line 37). Add field:

```rust
pub invite_managers: Arc<Mutex<HashMap<Uuid, krillnotes_core::core::invite::InviteManager>>>,
```

- [ ] **Step 2: Initialize in AppState::new (or wherever other managers are initialized)**

Find where `contact_managers` is initialized (e.g. in `tauri::Builder::manage(AppState { ... })`). Add alongside it:

```rust
invite_managers: Arc::new(Mutex::new(HashMap::new())),
```

- [ ] **Step 3: Create InviteManager on identity unlock**

Find the unlock identity command (search for `contact_managers.lock()` in the unlock flow). After the ContactManager is created and inserted, add:

```rust
let invites_dir = config_dir
    .join("identities")
    .join(uuid.to_string())
    .join("invites");
let im = krillnotes_core::core::invite::InviteManager::new(invites_dir)
    .map_err(|e| e.to_string())?;
state.invite_managers.lock().expect("Mutex poisoned").insert(uuid, im);
```

- [ ] **Step 4: Drop InviteManager on identity lock**

Find where `contact_managers` is removed on lock. Add alongside it:

```rust
state.invite_managers.lock().expect("Mutex poisoned").remove(&uuid);
```

- [ ] **Step 5: Build to check for compile errors**

```bash
cd krillnotes-desktop && cargo build --manifest-path src-tauri/Cargo.toml 2>&1 | grep "^error"
```
Expected: no errors.

- [ ] **Step 6: Commit**

```bash
git add krillnotes-desktop/src-tauri/src/lib.rs
git commit -m "feat(invite): add invite_managers to AppState, create/drop on identity lock/unlock"
```

---

### Task 5: Inviter-side Tauri commands

**Files:**
- Modify: `krillnotes-desktop/src-tauri/src/lib.rs`

Add these five commands. The access pattern mirrors the contact commands: lock the mutex, look up by identity UUID, return `Err(String)` if not unlocked.

- [ ] **Step 1: `list_invites` command**

```rust
#[tauri::command]
fn list_invites(
    state: State<'_, AppState>,
    identity_uuid: String,
) -> std::result::Result<Vec<InviteInfo>, String> {
    let uuid = Uuid::parse_str(&identity_uuid).map_err(|e| e.to_string())?;
    let ims = state.invite_managers.lock().expect("Mutex poisoned");
    let im = ims.get(&uuid).ok_or("Identity not unlocked")?;
    let records = im.list_invites().map_err(|e| e.to_string())?;
    Ok(records.into_iter().map(InviteInfo::from).collect())
}
```

Where `InviteInfo` is:

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InviteInfo {
    pub invite_id: String,
    pub workspace_id: String,
    pub workspace_name: String,
    pub created_at: String,
    pub expires_at: Option<String>,
    pub revoked: bool,
    pub use_count: u32,
}

impl From<krillnotes_core::core::invite::InviteRecord> for InviteInfo {
    fn from(r: krillnotes_core::core::invite::InviteRecord) -> Self {
        Self {
            invite_id: r.invite_id.to_string(),
            workspace_id: r.workspace_id,
            workspace_name: r.workspace_name,
            created_at: r.created_at.to_rfc3339(),
            expires_at: r.expires_at.map(|dt| dt.to_rfc3339()),
            revoked: r.revoked,
            use_count: r.use_count,
        }
    }
}
```

- [ ] **Step 2: `create_invite` command**

```rust
#[tauri::command]
fn create_invite(
    window: tauri::Window,
    state: State<'_, AppState>,
    identity_uuid: String,
    workspace_name: String,   // passed from frontend (WorkspaceInfo.name)
    expires_in_days: Option<u32>,
    save_path: String,
) -> std::result::Result<InviteInfo, String> {
    let uuid = Uuid::parse_str(&identity_uuid).map_err(|e| e.to_string())?;

    // Get signing key + declared name from unlocked identity
    let (signing_key, declared_name) = {
        let ids = state.unlocked_identities.lock().expect("Mutex poisoned");
        let id = ids.get(&uuid).ok_or("Identity not unlocked")?;
        (id.signing_key.clone(), id.display_name.clone())
    };

    // Get workspace id + metadata from the current window's workspace.
    // NOTE: WorkspaceMetadata has no `name` field — pass workspace_name from frontend.
    let (ws_id, ws_desc, ws_author, ws_org, ws_url, ws_license, ws_tags) = {
        let wss = state.workspaces.lock().expect("Mutex poisoned");
        let ws = wss.get(window.label()).ok_or("No workspace open")?;
        let meta = ws.get_workspace_metadata().map_err(|e| e.to_string())?;
        (
            ws.workspace_id().to_string(),
            meta.description,
            meta.author_name,
            meta.author_org,
            meta.homepage_url,
            meta.license,
            meta.tags,
        )
    };

    let mut ims = state.invite_managers.lock().expect("Mutex poisoned");
    let im = ims.get_mut(&uuid).ok_or("Identity not unlocked")?;
    let (record, file) = im
        .create_invite(
            &ws_id, &workspace_name, expires_in_days, &signing_key, &declared_name,
            ws_desc, ws_author, ws_org, ws_url, ws_license, ws_tags,
        )
        .map_err(|e| e.to_string())?;

    let json = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
    std::fs::write(&save_path, json).map_err(|e| e.to_string())?;

    Ok(InviteInfo::from(record))
}
```

Note: `ws.name()` — check the actual method name on `Workspace` using LSP hover if unsure. It may be `workspace_name()` or a field accessor. Use `documentSymbol` on `workspace.rs` to confirm before coding.

- [ ] **Step 3: `revoke_invite` command**

```rust
#[tauri::command]
fn revoke_invite(
    state: State<'_, AppState>,
    identity_uuid: String,
    invite_id: String,
) -> std::result::Result<(), String> {
    let uuid = Uuid::parse_str(&identity_uuid).map_err(|e| e.to_string())?;
    let invite_uuid = Uuid::parse_str(&invite_id).map_err(|e| e.to_string())?;
    let mut ims = state.invite_managers.lock().expect("Mutex poisoned");
    let im = ims.get_mut(&uuid).ok_or("Identity not unlocked")?;
    im.revoke_invite(invite_uuid).map_err(|e| e.to_string())
}
```

- [ ] **Step 4: `import_invite_response` command**

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingPeer {
    pub invite_id: String,
    pub invitee_public_key: String,
    pub invitee_declared_name: String,
    pub fingerprint: String,
}

#[tauri::command]
fn import_invite_response(
    state: State<'_, AppState>,
    identity_uuid: String,
    path: String,
) -> std::result::Result<PendingPeer, String> {
    use krillnotes_core::core::invite::InviteManager;
    use krillnotes_core::core::contact::generate_fingerprint;

    let uuid = Uuid::parse_str(&identity_uuid).map_err(|e| e.to_string())?;
    let response = InviteManager::parse_and_verify_response(std::path::Path::new(&path))
        .map_err(|e| e.to_string())?;

    // Validate invite is still active
    let invite_uuid = Uuid::parse_str(&response.invite_id).map_err(|e| e.to_string())?;
    {
        let mut ims = state.invite_managers.lock().expect("Mutex poisoned");
        let im = ims.get_mut(&uuid).ok_or("Identity not unlocked")?;
        let record = im
            .get_invite(invite_uuid)
            .map_err(|e| e.to_string())?
            .ok_or("Invite not found")?;
        if record.revoked {
            return Err("Invite has been revoked".to_string());
        }
        if let Some(exp) = record.expires_at {
            if Utc::now() > exp {
                return Err("Invite has expired".to_string());
            }
        }
        im.increment_use_count(invite_uuid).map_err(|e| e.to_string())?;
    }

    let fingerprint = generate_fingerprint(&response.invitee_public_key).map_err(|e| e.to_string())?;
    Ok(PendingPeer {
        invite_id: response.invite_id,
        invitee_public_key: response.invitee_public_key,
        invitee_declared_name: response.invitee_declared_name,
        fingerprint,
    })
}
```

- [ ] **Step 5: Check how Phase A parses TrustLevel**

Before writing `accept_peer`, check how the existing `create_contact` Tauri command converts a trust level string to `TrustLevel`. Use LSP `findReferences` on `TrustLevel` in `lib.rs` or read the `create_contact` command body. Follow the same pattern — it may be a match statement or a helper function. Do NOT assume `FromStr` is implemented; verify first.

- [ ] **Step 6: `accept_peer` command**

```rust
#[tauri::command]
fn accept_peer(
    window: tauri::Window,
    state: State<'_, AppState>,
    identity_uuid: String,
    invitee_public_key: String,
    declared_name: String,
    trust_level: String,
    local_name: Option<String>,
) -> std::result::Result<ContactInfo, String> {
    use krillnotes_core::core::contact::TrustLevel;

    let uuid = Uuid::parse_str(&identity_uuid).map_err(|e| e.to_string())?;
    // Use the same trust level parsing as the existing create_contact command (check its implementation first)
    let trust: TrustLevel = parse_trust_level(&trust_level)?;

    // Create or update contact (handle duplicate public key per spec C5)
    let contact = {
        let cms = state.contact_managers.lock().expect("Mutex poisoned");
        let cm = cms.get(&uuid).ok_or("Identity not unlocked")?;
        // Use find_or_create_by_public_key to avoid duplicate contacts for same key
        let mut c = cm
            .find_or_create_by_public_key(&invitee_public_key, &declared_name, trust)
            .map_err(|e| e.to_string())?;
        if let Some(name) = local_name {
            c.local_name = Some(name);
            cm.save_contact(&c).map_err(|e| e.to_string())?;
        }
        c
    };

    // Add as pre-authorised workspace peer using the existing convenience method.
    // `add_contact_as_peer` builds the `identity:<pubkey>` placeholder device ID internally.
    {
        let wss = state.workspaces.lock().expect("Mutex poisoned");
        if let Some(ws) = wss.get(window.label()) {
            ws.add_contact_as_peer(&invitee_public_key)
                .map_err(|e| e.to_string())?;
        }
    }

    Ok(contact_to_info(&contact))
}
```

- [ ] **Step 6: Remove old stub invite commands**

Search for and remove the following command functions and their `tauri::generate_handler!` entries:
- `create_invite_bundle_cmd`
- `create_accept_bundle_cmd`
- `create_snapshot_bundle_cmd`
- `create_workspace_from_snapshot_cmd`

Also remove `WorkspacePeersDialog`'s calls to these from the frontend (covered in Chunk 3).

- [ ] **Step 7: Register new commands in `generate_handler!`**

Find the `tauri::generate_handler![...]` macro call and add:
```
list_invites,
create_invite,
revoke_invite,
import_invite_response,
accept_peer,
import_invite,
respond_to_invite,
```

(The last two are added in Task 6.)

- [ ] **Step 8: Build**

```bash
cd krillnotes-desktop && cargo build --manifest-path src-tauri/Cargo.toml 2>&1 | grep "^error"
```
Expected: no errors.

- [ ] **Step 9: Commit**

```bash
git add krillnotes-desktop/src-tauri/src/lib.rs
git commit -m "feat(invite): add list_invites, create_invite, revoke_invite, import_invite_response, accept_peer commands; remove old invite stubs"
```

---

### Task 6: Invitee-side Tauri commands

**Files:**
- Modify: `krillnotes-desktop/src-tauri/src/lib.rs`

- [ ] **Step 1: `InviteFileData` struct**

Add near other response structs:

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InviteFileData {
    pub invite_id: String,
    pub workspace_id: String,
    pub workspace_name: String,
    pub workspace_description: Option<String>,
    pub workspace_author_name: Option<String>,
    pub workspace_author_org: Option<String>,
    pub workspace_homepage_url: Option<String>,
    pub workspace_license: Option<String>,
    pub workspace_language: Option<String>,
    pub workspace_tags: Vec<String>,
    pub inviter_public_key: String,
    pub inviter_declared_name: String,
    pub inviter_fingerprint: String,
    pub expires_at: Option<String>,
}
```

- [ ] **Step 2: `import_invite` command**

```rust
#[tauri::command]
fn import_invite(path: String) -> std::result::Result<InviteFileData, String> {
    use krillnotes_core::core::invite::InviteManager;
    use krillnotes_core::core::contact::generate_fingerprint;

    let invite = InviteManager::parse_and_verify_invite(std::path::Path::new(&path))
        .map_err(|e| e.to_string())?;

    // Check expiry (informational — UI may still show it but disable Respond)
    let fingerprint = generate_fingerprint(&invite.inviter_public_key).map_err(|e| e.to_string())?;

    Ok(InviteFileData {
        invite_id: invite.invite_id,
        workspace_id: invite.workspace_id,
        workspace_name: invite.workspace_name,
        workspace_description: invite.workspace_description,
        workspace_author_name: invite.workspace_author_name,
        workspace_author_org: invite.workspace_author_org,
        workspace_homepage_url: invite.workspace_homepage_url,
        workspace_license: invite.workspace_license,
        workspace_language: invite.workspace_language,
        workspace_tags: invite.workspace_tags,
        inviter_public_key: invite.inviter_public_key,
        inviter_declared_name: invite.inviter_declared_name,
        inviter_fingerprint: fingerprint,
        expires_at: invite.expires_at,
    })
}
```

- [ ] **Step 3: `respond_to_invite` command**

```rust
#[tauri::command]
fn respond_to_invite(
    state: State<'_, AppState>,
    identity_uuid: String,
    invite_path: String,
    save_path: String,
) -> std::result::Result<(), String> {
    use krillnotes_core::core::invite::InviteManager;

    let uuid = Uuid::parse_str(&identity_uuid).map_err(|e| e.to_string())?;

    let (signing_key, declared_name) = {
        let ids = state.unlocked_identities.lock().expect("Mutex poisoned");
        let id = ids.get(&uuid).ok_or("Identity not unlocked")?;
        (id.signing_key.clone(), id.display_name.clone())
    };

    let invite = InviteManager::parse_and_verify_invite(std::path::Path::new(&invite_path))
        .map_err(|e| e.to_string())?;

    InviteManager::build_and_save_response(
        &invite,
        &signing_key,
        &declared_name,
        std::path::Path::new(&save_path),
    )
    .map_err(|e| e.to_string())
}
```

- [ ] **Step 4: Build**

```bash
cd krillnotes-desktop && cargo build --manifest-path src-tauri/Cargo.toml 2>&1 | grep "^error"
```
Expected: no errors.

- [ ] **Step 5: Commit**

```bash
git add krillnotes-desktop/src-tauri/src/lib.rs
git commit -m "feat(invite): add import_invite and respond_to_invite commands for invitee side"
```

---

## Chunk 3: Frontend

### Task 7: TypeScript types

**Files:**
- Modify: `krillnotes-desktop/src/types.ts`

- [ ] **Step 1: Add types**

Append to `types.ts`:

```typescript
export interface InviteInfo {
  inviteId: string;
  workspaceId: string;
  workspaceName: string;
  createdAt: string;
  expiresAt: string | null;
  revoked: boolean;
  useCount: number;
}

export interface PendingPeer {
  inviteId: string;
  inviteePublicKey: string;
  inviteeDeclaredName: string;
  fingerprint: string;
}

export interface InviteFileData {
  inviteId: string;
  workspaceId: string;
  workspaceName: string;
  workspaceDescription: string | null;
  workspaceAuthorName: string | null;
  workspaceAuthorOrg: string | null;
  workspaceHomepageUrl: string | null;
  workspaceLicense: string | null;
  workspaceLanguage: string | null;
  workspaceTags: string[];
  inviterPublicKey: string;
  inviterDeclaredName: string;
  inviterFingerprint: string;
  expiresAt: string | null;
}
```

- [ ] **Step 2: Type check**

```bash
cd krillnotes-desktop && npx tsc --noEmit 2>&1 | head -30
```
Expected: no new errors.

- [ ] **Step 3: Commit**

```bash
git add krillnotes-desktop/src/types.ts
git commit -m "feat(invite): add InviteInfo, PendingPeer, InviteFileData TypeScript interfaces"
```

---

### Task 8: CreateInviteDialog

**Files:**
- Create: `krillnotes-desktop/src/components/CreateInviteDialog.tsx`

- [ ] **Step 1: Create component**

```tsx
import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { save } from '@tauri-apps/plugin-dialog';
import { useTranslation } from 'react-i18next';
import type { InviteInfo } from '../types';

interface Props {
  identityUuid: string;
  workspaceName: string;
  onCreated: (invite: InviteInfo) => void;
  onClose: () => void;
}

const EXPIRY_OPTIONS = [
  { label: 'No expiry', value: null },
  { label: '7 days', value: 7 },
  { label: '30 days', value: 30 },
  { label: 'Custom', value: -1 },
];

export function CreateInviteDialog({ identityUuid, workspaceName, onCreated, onClose }: Props) {
  const { t } = useTranslation();
  const [expiryDays, setExpiryDays] = useState<number | null>(null);
  const [customDays, setCustomDays] = useState('');
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const effectiveDays =
    expiryDays === -1 ? (parseInt(customDays) || null) : expiryDays;

  const handleCreate = async () => {
    setCreating(true);
    setError(null);
    try {
      const savePath = await save({
        defaultPath: `invite_${workspaceName.replace(/\s+/g, '_')}.swarm`,
        filters: [{ name: 'Swarm Invite', extensions: ['swarm'] }],
      });
      if (!savePath) { setCreating(false); return; }

      const invite = await invoke<InviteInfo>('create_invite', {
        identityUuid,
        workspaceName,
        expiresInDays: effectiveDays ?? undefined,
        savePath,
      });
      onCreated(invite);
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setCreating(false);
    }
  };

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-white dark:bg-zinc-900 rounded-xl shadow-xl p-6 w-full max-w-md">
        <h2 className="text-lg font-semibold mb-4">{t('invite.createTitle')}</h2>

        <p className="text-sm text-zinc-500 mb-4">
          {t('invite.createDescription', { workspaceName })}
        </p>

        <label className="block text-sm font-medium mb-1">{t('invite.expiry')}</label>
        <select
          className="w-full border rounded px-3 py-2 mb-4 dark:bg-zinc-800 dark:border-zinc-700"
          value={expiryDays ?? 'null'}
          onChange={e => setExpiryDays(e.target.value === 'null' ? null : parseInt(e.target.value))}
        >
          {EXPIRY_OPTIONS.map(opt => (
            <option key={String(opt.value)} value={String(opt.value)}>{opt.label}</option>
          ))}
        </select>

        {expiryDays === -1 && (
          <div className="mb-4">
            <label className="block text-sm font-medium mb-1">{t('invite.customDays')}</label>
            <input
              type="number"
              min="1"
              className="w-full border rounded px-3 py-2 dark:bg-zinc-800 dark:border-zinc-700"
              value={customDays}
              onChange={e => setCustomDays(e.target.value)}
            />
          </div>
        )}

        {error && <p className="text-red-500 text-sm mb-3">{error}</p>}

        <div className="flex justify-end gap-2">
          <button onClick={onClose} className="px-4 py-2 text-sm rounded border dark:border-zinc-700">
            {t('common.cancel')}
          </button>
          <button
            onClick={handleCreate}
            disabled={creating || (expiryDays === -1 && !parseInt(customDays))}
            className="px-4 py-2 text-sm rounded bg-blue-600 text-white disabled:opacity-50"
          >
            {creating ? t('common.saving') : t('invite.createAndSave')}
          </button>
        </div>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Type check**

```bash
cd krillnotes-desktop && npx tsc --noEmit 2>&1 | head -30
```

- [ ] **Step 3: Commit**

```bash
git add krillnotes-desktop/src/components/CreateInviteDialog.tsx
git commit -m "feat(invite): add CreateInviteDialog component"
```

---

### Task 9: AcceptPeerDialog

**Files:**
- Create: `krillnotes-desktop/src/components/AcceptPeerDialog.tsx`

- [ ] **Step 1: Create component**

```tsx
import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { useTranslation } from 'react-i18next';
import type { PendingPeer, ContactInfo } from '../types';

interface Props {
  identityUuid: string;
  pendingPeer: PendingPeer | null;
  onAccepted: (contact: ContactInfo) => void;
  onClose: () => void;
}

const TRUST_LEVELS = ['Tofu', 'CodeVerified', 'Vouched', 'VerifiedInPerson'];

export function AcceptPeerDialog({ identityUuid, pendingPeer, onAccepted, onClose }: Props) {
  const { t } = useTranslation();
  const [peer, setPeer] = useState<PendingPeer | null>(pendingPeer);
  const [trustLevel, setTrustLevel] = useState('Tofu');
  const [localName, setLocalName] = useState('');
  const [fingerprintConfirmed, setFingerprintConfirmed] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [isDuplicate, setIsDuplicate] = useState(false);

  const handleImport = async () => {
    const path = await open({ filters: [{ name: 'Swarm Response', extensions: ['swarm'] }] });
    if (!path) return;
    try {
      const result = await invoke<PendingPeer>('import_invite_response', {
        identityUuid,
        path: typeof path === 'string' ? path : path[0],
      });
      // Check if this public key is already in contacts (spec C5)
      try {
        const existing = await invoke<ContactInfo | null>('get_contact_by_public_key', {
          identityUuid,
          publicKey: result.inviteePublicKey,
        });
        setIsDuplicate(!!existing);
      } catch { setIsDuplicate(false); }
      setPeer(result);
      setFingerprintConfirmed(false);
    } catch (e) {
      setError(String(e));
    }
  };

  const handleAccept = async () => {
    if (!peer) return;
    setLoading(true);
    setError(null);
    try {
      const contact = await invoke<ContactInfo>('accept_peer', {
        identityUuid,
        inviteePublicKey: peer.inviteePublicKey,
        declaredName: peer.inviteeDeclaredName,
        trustLevel,
        localName: localName || undefined,
      });
      onAccepted(contact);
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-white dark:bg-zinc-900 rounded-xl shadow-xl p-6 w-full max-w-md">
        <h2 className="text-lg font-semibold mb-4">{t('invite.acceptTitle')}</h2>

        {!peer ? (
          <div className="text-center py-6">
            <p className="text-sm text-zinc-500 mb-4">{t('invite.importResponsePrompt')}</p>
            <button onClick={handleImport} className="px-4 py-2 text-sm rounded bg-blue-600 text-white">
              {t('invite.importResponse')}
            </button>
          </div>
        ) : (
          <>
            <div className="mb-4 p-3 bg-zinc-100 dark:bg-zinc-800 rounded">
              <p className="text-sm font-medium">{peer.inviteeDeclaredName}</p>
              <p className="text-xs text-zinc-500 font-mono mt-1">{peer.fingerprint}</p>
            </div>

            {isDuplicate && (
              <p className="text-sm text-amber-600 dark:text-amber-400 mb-3">
                {t('invite.duplicateContact')}
              </p>
            )}

            <p className="text-sm text-amber-600 dark:text-amber-400 mb-3">
              {t('invite.fingerprintVerifyPrompt')}
            </p>

            <label className="flex items-center gap-2 mb-4 text-sm cursor-pointer">
              <input
                type="checkbox"
                checked={fingerprintConfirmed}
                onChange={e => setFingerprintConfirmed(e.target.checked)}
              />
              {t('invite.fingerprintConfirm')}
            </label>

            <label className="block text-sm font-medium mb-1">{t('contacts.trustLevel')}</label>
            <select
              className="w-full border rounded px-3 py-2 mb-4 dark:bg-zinc-800 dark:border-zinc-700"
              value={trustLevel}
              onChange={e => setTrustLevel(e.target.value)}
            >
              {TRUST_LEVELS.map(t => <option key={t} value={t}>{t}</option>)}
            </select>

            <label className="block text-sm font-medium mb-1">{t('contacts.localName')}</label>
            <input
              type="text"
              placeholder={t('contacts.localNamePlaceholder')}
              className="w-full border rounded px-3 py-2 mb-4 dark:bg-zinc-800 dark:border-zinc-700"
              value={localName}
              onChange={e => setLocalName(e.target.value)}
            />

            {error && <p className="text-red-500 text-sm mb-3">{error}</p>}

            <div className="flex justify-end gap-2">
              <button onClick={onClose} className="px-4 py-2 text-sm rounded border dark:border-zinc-700">
                {t('common.reject')}
              </button>
              <button
                onClick={handleAccept}
                disabled={loading || !fingerprintConfirmed}
                className="px-4 py-2 text-sm rounded bg-green-600 text-white disabled:opacity-50"
              >
                {loading ? t('common.saving') : t('common.accept')}
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Type check**

```bash
cd krillnotes-desktop && npx tsc --noEmit 2>&1 | head -30
```

- [ ] **Step 3: Commit**

```bash
git add krillnotes-desktop/src/components/AcceptPeerDialog.tsx
git commit -m "feat(invite): add AcceptPeerDialog with fingerprint gate"
```

---

### Task 10: ImportInviteDialog

**Files:**
- Create: `krillnotes-desktop/src/components/ImportInviteDialog.tsx`

This is the invitee's dialog — they open an invite `.swarm` file, view workspace metadata, verify the inviter's fingerprint, then save a response `.swarm` file.

- [ ] **Step 1: Create component**

```tsx
import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { save } from '@tauri-apps/plugin-dialog';
import { useTranslation } from 'react-i18next';
import type { InviteFileData } from '../types';

interface Props {
  identityUuid: string;
  invitePath: string;       // path to the invite .swarm file already selected
  inviteData: InviteFileData;
  onResponded: () => void;
  onClose: () => void;
}

export function ImportInviteDialog({ identityUuid, invitePath, inviteData, onResponded, onClose }: Props) {
  const { t } = useTranslation();
  const [fingerprintConfirmed, setFingerprintConfirmed] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const isExpired = inviteData.expiresAt
    ? new Date(inviteData.expiresAt) < new Date()
    : false;

  const handleRespond = async () => {
    setLoading(true);
    setError(null);
    try {
      const savePath = await save({
        defaultPath: `response_${inviteData.workspaceName.replace(/\s+/g, '_')}.swarm`,
        filters: [{ name: 'Swarm Response', extensions: ['swarm'] }],
      });
      if (!savePath) { setLoading(false); return; }

      await invoke('respond_to_invite', {
        identityUuid,
        invitePath,
        savePath,
      });
      onResponded();
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-white dark:bg-zinc-900 rounded-xl shadow-xl p-6 w-full max-w-lg">
        <h2 className="text-lg font-semibold mb-1">{t('invite.importTitle')}</h2>
        <p className="text-sm text-zinc-500 mb-4">{t('invite.importSubtitle')}</p>

        {/* Workspace info */}
        <div className="mb-4 p-4 border rounded dark:border-zinc-700 space-y-1">
          <p className="font-medium">{inviteData.workspaceName}</p>
          {inviteData.workspaceDescription && (
            <p className="text-sm text-zinc-500">{inviteData.workspaceDescription}</p>
          )}
          {inviteData.workspaceAuthorName && (
            <p className="text-xs text-zinc-500">
              {t('invite.by')} {inviteData.workspaceAuthorName}
              {inviteData.workspaceAuthorOrg && ` (${inviteData.workspaceAuthorOrg})`}
            </p>
          )}
          {inviteData.workspaceLicense && (
            <p className="text-xs text-zinc-400">{t('invite.license')}: {inviteData.workspaceLicense}</p>
          )}
          {inviteData.workspaceTags.length > 0 && (
            <div className="flex flex-wrap gap-1 mt-1">
              {inviteData.workspaceTags.map(tag => (
                <span key={tag} className="text-xs bg-zinc-100 dark:bg-zinc-800 px-2 py-0.5 rounded-full">
                  {tag}
                </span>
              ))}
            </div>
          )}
        </div>

        {/* Inviter fingerprint */}
        <div className="mb-4 p-3 bg-zinc-100 dark:bg-zinc-800 rounded">
          <p className="text-xs font-medium text-zinc-600 dark:text-zinc-400 mb-1">
            {t('invite.invitedBy')}
          </p>
          <p className="text-sm font-medium">{inviteData.inviterDeclaredName}</p>
          <p className="text-xs font-mono text-zinc-500 mt-1">{inviteData.inviterFingerprint}</p>
        </div>

        <p className="text-sm text-amber-600 dark:text-amber-400 mb-3">
          {t('invite.fingerprintVerifyPrompt')}
        </p>

        <label className="flex items-center gap-2 mb-4 text-sm cursor-pointer">
          <input
            type="checkbox"
            checked={fingerprintConfirmed}
            onChange={e => setFingerprintConfirmed(e.target.checked)}
          />
          {t('invite.fingerprintConfirm')}
        </label>

        {isExpired && (
          <p className="text-red-500 text-sm mb-3">{t('invite.expired')}</p>
        )}
        {error && <p className="text-red-500 text-sm mb-3">{error}</p>}

        <div className="flex justify-end gap-2">
          <button onClick={onClose} className="px-4 py-2 text-sm rounded border dark:border-zinc-700">
            {t('common.cancel')}
          </button>
          <button
            onClick={handleRespond}
            disabled={loading || !fingerprintConfirmed || isExpired}
            className="px-4 py-2 text-sm rounded bg-blue-600 text-white disabled:opacity-50"
          >
            {loading ? t('common.saving') : t('invite.respond')}
          </button>
        </div>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Type check**

```bash
cd krillnotes-desktop && npx tsc --noEmit 2>&1 | head -30
```

- [ ] **Step 3: Commit**

```bash
git add krillnotes-desktop/src/components/ImportInviteDialog.tsx
git commit -m "feat(invite): add ImportInviteDialog for invitee side"
```

---

### Task 11: InviteManagerDialog

**Files:**
- Create: `krillnotes-desktop/src/components/InviteManagerDialog.tsx`

- [ ] **Step 1: Create component**

```tsx
import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { useTranslation } from 'react-i18next';
import type { InviteInfo, PendingPeer, ContactInfo } from '../types';
import { CreateInviteDialog } from './CreateInviteDialog';
import { AcceptPeerDialog } from './AcceptPeerDialog';

interface Props {
  identityUuid: string;
  workspaceName: string;
  onClose: () => void;
}

export function InviteManagerDialog({ identityUuid, workspaceName, onClose }: Props) {
  const { t } = useTranslation();
  const [invites, setInvites] = useState<InviteInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [showCreate, setShowCreate] = useState(false);
  const [pendingPeer, setPendingPeer] = useState<PendingPeer | null>(null);
  const [showAccept, setShowAccept] = useState(false);

  const load = async () => {
    setLoading(true);
    try {
      const list = await invoke<InviteInfo[]>('list_invites', { identityUuid });
      setInvites(list);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => { load(); }, [identityUuid]);

  const handleRevoke = async (inviteId: string) => {
    try {
      await invoke('revoke_invite', { identityUuid, inviteId });
      await load();
    } catch (e) {
      setError(String(e));
    }
  };

  const handleImportResponse = async () => {
    const path = await open({ filters: [{ name: 'Swarm Response', extensions: ['swarm'] }] });
    if (!path) return;
    try {
      const peer = await invoke<PendingPeer>('import_invite_response', {
        identityUuid,
        path: typeof path === 'string' ? path : path[0],
      });
      setPendingPeer(peer);
      setShowAccept(true);
    } catch (e) {
      setError(String(e));
    }
  };

  const formatExpiry = (invite: InviteInfo) => {
    if (!invite.expiresAt) return t('invite.noExpiry');
    const date = new Date(invite.expiresAt);
    const expired = date < new Date();
    return expired
      ? t('invite.expiredOn', { date: date.toLocaleDateString() })
      : t('invite.expiresOn', { date: date.toLocaleDateString() });
  };

  return (
    <>
      <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
        <div className="bg-white dark:bg-zinc-900 rounded-xl shadow-xl p-6 w-full max-w-lg max-h-[80vh] flex flex-col">
          <div className="flex items-center justify-between mb-4">
            <h2 className="text-lg font-semibold">
              {workspaceName} — {t('invite.manageTitle')}
            </h2>
            <button onClick={onClose} className="text-zinc-400 hover:text-zinc-600 text-xl leading-none">×</button>
          </div>

          {error && <p className="text-red-500 text-sm mb-3">{error}</p>}

          <div className="flex gap-2 mb-4">
            <button
              onClick={() => setShowCreate(true)}
              className="px-3 py-1.5 text-sm rounded bg-blue-600 text-white"
            >
              {t('invite.createInvite')}
            </button>
            <button
              onClick={handleImportResponse}
              className="px-3 py-1.5 text-sm rounded border dark:border-zinc-700"
            >
              {t('invite.importResponse')}
            </button>
          </div>

          <div className="overflow-y-auto flex-1">
            {loading ? (
              <p className="text-sm text-zinc-500 text-center py-8">{t('common.loading')}</p>
            ) : invites.length === 0 ? (
              <p className="text-sm text-zinc-500 text-center py-8">{t('invite.noInvites')}</p>
            ) : (
              <ul className="space-y-2">
                {invites.map(invite => (
                  <li
                    key={invite.inviteId}
                    className="flex items-center justify-between p-3 border rounded dark:border-zinc-700"
                  >
                    <div>
                      <p className="text-sm">{formatExpiry(invite)}</p>
                      <p className="text-xs text-zinc-500">
                        {t('invite.usedCount', { count: invite.useCount })}
                        {invite.revoked && (
                          <span className="ml-2 text-red-500">{t('invite.revoked')}</span>
                        )}
                      </p>
                    </div>
                    {!invite.revoked && (
                      <button
                        onClick={() => handleRevoke(invite.inviteId)}
                        className="text-xs text-red-500 hover:underline"
                      >
                        {t('invite.revoke')}
                      </button>
                    )}
                  </li>
                ))}
              </ul>
            )}
          </div>
        </div>
      </div>

      {showCreate && (
        <CreateInviteDialog
          identityUuid={identityUuid}
          workspaceName={workspaceName}
          onCreated={() => load()}
          onClose={() => setShowCreate(false)}
        />
      )}

      {showAccept && (
        <AcceptPeerDialog
          identityUuid={identityUuid}
          pendingPeer={pendingPeer}
          onAccepted={(_contact: ContactInfo) => { load(); }}
          onClose={() => { setShowAccept(false); setPendingPeer(null); }}
        />
      )}
    </>
  );
}
```

- [ ] **Step 2: Type check**

```bash
cd krillnotes-desktop && npx tsc --noEmit 2>&1 | head -30
```

- [ ] **Step 3: Commit**

```bash
git add krillnotes-desktop/src/components/InviteManagerDialog.tsx
git commit -m "feat(invite): add InviteManagerDialog with list/revoke/import-response"
```

---

### Task 12: Wire into WorkspacePeersDialog

**Files:**
- Modify: `krillnotes-desktop/src/components/WorkspacePeersDialog.tsx`

- [ ] **Step 1: Read the current file**

Read `krillnotes-desktop/src/components/WorkspacePeersDialog.tsx` in full before editing.

- [ ] **Step 2: Remove old invite form state and commands**

Remove all state related to the old invite form:
- `showInviteForm`, `inviteContactName`, `invitePublicKey`, `inviteRole`, `inviteCreating`, `inviteError`, `inviteSuccess`

Remove calls to:
- `create_invite_bundle_cmd`
- `create_accept_bundle_cmd`

Remove the inline invite form JSX section.

- [ ] **Step 3: Add InviteManagerDialog and ImportInviteDialog imports**

```tsx
import { InviteManagerDialog } from './InviteManagerDialog';
import { ImportInviteDialog } from './ImportInviteDialog';
```

- [ ] **Step 4: Add state and handlers**

```tsx
const [showInviteManager, setShowInviteManager] = useState(false);
const [importInviteData, setImportInviteData] = useState<InviteFileData | null>(null);
const [importInvitePath, setImportInvitePath] = useState<string | null>(null);

const handleImportInvite = async () => {
  const path = await open({ filters: [{ name: 'Swarm Invite', extensions: ['swarm'] }] });
  if (!path) return;
  const p = typeof path === 'string' ? path : path[0];
  try {
    const data = await invoke<InviteFileData>('import_invite', { path: p });
    setImportInvitePath(p);
    setImportInviteData(data);
  } catch (e) {
    setError(String(e));
  }
};
```

- [ ] **Step 5: Add buttons to the peers panel toolbar**

In the toolbar area (near the existing "Add to contacts" or header buttons), add:

```tsx
<button
  onClick={() => setShowInviteManager(true)}
  className="px-3 py-1.5 text-sm rounded border dark:border-zinc-700"
>
  {t('invite.manageInvites')}
</button>
<button
  onClick={handleImportInvite}
  className="px-3 py-1.5 text-sm rounded border dark:border-zinc-700"
>
  {t('invite.importInvite')}
</button>
```

- [ ] **Step 6: Render dialogs at bottom of JSX**

```tsx
{showInviteManager && workspaceInfo && (
  <InviteManagerDialog
    identityUuid={identityUuid}
    workspaceName={workspaceInfo.name}
    onClose={() => setShowInviteManager(false)}
  />
)}
{importInviteData && importInvitePath && (
  <ImportInviteDialog
    identityUuid={identityUuid}
    invitePath={importInvitePath}
    inviteData={importInviteData}
    onResponded={() => {}}
    onClose={() => { setImportInviteData(null); setImportInvitePath(null); }}
  />
)}
```

- [ ] **Step 7: Type check**

```bash
cd krillnotes-desktop && npx tsc --noEmit 2>&1 | head -30
```

- [ ] **Step 8: Commit**

```bash
git add krillnotes-desktop/src/components/WorkspacePeersDialog.tsx
git commit -m "feat(invite): wire InviteManagerDialog and ImportInviteDialog into WorkspacePeersDialog; remove old invite stubs"
```

---

### Task 13: i18n keys

**Files:**
- Modify: `krillnotes-desktop/src/i18n/locales/en.json`

- [ ] **Step 1: Add keys to en.json**

Add an `invite` section:

```json
"invite": {
  "createTitle": "Create Invite",
  "createDescription": "Create a signed invite for {{workspaceName}}. Share the file with anyone you want to invite.",
  "createInvite": "Create Invite",
  "createAndSave": "Create & Save",
  "expiry": "Expiry",
  "noExpiry": "No expiry",
  "customDays": "Days until expiry",
  "expiresOn": "Expires {{date}}",
  "expiredOn": "Expired {{date}}",
  "expired": "This invite has expired.",
  "manageTitle": "Invites",
  "manageInvites": "Invites",
  "noInvites": "No invites yet.",
  "usedCount_one": "Used {{count}} time",
  "usedCount_other": "Used {{count}} times",
  "revoked": "Revoked",
  "revoke": "Revoke",
  "importResponse": "Import Response",
  "importResponsePrompt": "Import a response .swarm file from a peer.",
  "importInvite": "Import Invite",
  "importTitle": "Workspace Invite",
  "importSubtitle": "You have been invited to join a workspace.",
  "by": "by",
  "license": "License",
  "invitedBy": "Invited by",
  "fingerprintVerifyPrompt": "Ask the inviter to read their fingerprint aloud and confirm it matches before proceeding.",
  "fingerprintConfirm": "Yes, the fingerprint matches",
  "respond": "Respond",
  "acceptTitle": "Accept Peer",
  "noExpiry": "No expiry",
  "duplicateContact": "This peer is already in your contacts. Accepting will update their workspace peer status."
}
```

- [ ] **Step 2: Add placeholder keys for all other locales**

For each locale file (`de.json`, `es.json`, `fr.json`, `ja.json`, `zh.json`, `pt.json`): copy the `invite` section from `en.json` as a placeholder. (Translation can happen separately.)

- [ ] **Step 3: Type check**

```bash
cd krillnotes-desktop && npx tsc --noEmit 2>&1 | head -30
```

- [ ] **Step 4: Full build smoke test**

```bash
cd krillnotes-desktop && npm run tauri dev -- --no-watch 2>&1 | head -40
```
Or if that's not available, just build:
```bash
cd krillnotes-desktop && cargo build --manifest-path src-tauri/Cargo.toml 2>&1 | grep "^error"
```

- [ ] **Step 5: Commit**

```bash
git add krillnotes-desktop/src/i18n/locales/
git commit -m "feat(invite): add i18n keys for invite flow (en + placeholder for all locales)"
```

---

## Final Step: PR

- [ ] Push branch and open PR targeting `master`

```bash
git push github-https HEAD
```

Then use the `commit-commands:commit-push-pr` skill to open the PR with a summary of all changes.
