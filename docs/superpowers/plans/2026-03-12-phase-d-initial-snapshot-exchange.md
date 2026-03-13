# Phase D — Initial Snapshot Exchange

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable the inviter to generate a full encrypted workspace snapshot (notes, scripts, attachments) for one or more peers after completing the invite handshake, and enable the invitee to apply that snapshot to create a new local workspace — using the same workspace UUID on all peers.

**Architecture:** Extend the existing snapshot bundle format (`swarm/snapshot.rs`) to carry attachment blobs encrypted with the same AES-256-GCM key used for the payload. Add two Tauri commands: `create_snapshot_for_peers` (inviter) and `apply_swarm_snapshot` (invitee). Mirror the `execute_import` pattern for workspace creation. Add minimal UI: a `PostAcceptDialog` (Send Now / Later), a `SendSnapshotDialog` (peer picker + file save), and an "Apply Workspace" action in the existing `SwarmInviteDialog`.

**Tech Stack:** Rust (rusqlite, aes_gcm, ed25519_dalek, zip, hkdf), Tauri v2, React 19, TypeScript, Tailwind v4

---

## File Structure

| Action | File | Purpose |
|--------|------|---------|
| Modify | `krillnotes-core/src/core/swarm/crypto.rs` | Expose symmetric key; add `encrypt_blob` / `decrypt_blob` |
| Modify | `krillnotes-core/src/core/swarm/snapshot.rs` | Accept attachment blobs in create; return them + `workspace_name` in parse |
| Modify | `krillnotes-core/src/core/workspace.rs` | Add `attachments` to `WorkspaceSnapshot`; extend `to_snapshot_json`; add `get_latest_operation_id`; add `Workspace::create_with_id` |
| Modify | `krillnotes-desktop/src-tauri/src/lib.rs` | Add `create_snapshot_for_peers` and `apply_swarm_snapshot` Tauri commands |
| Create | `krillnotes-desktop/src/components/PostAcceptDialog.tsx` | "Send Now / Later" modal shown after `accept_peer` resolves |
| Create | `krillnotes-desktop/src/components/SendSnapshotDialog.tsx` | Peer checkbox list + file-save dialog for snapshot creation |
| Modify | `krillnotes-desktop/src/components/InviteManagerDialog.tsx` | Show `PostAcceptDialog` after successful `accept_peer`; wire `SendSnapshotDialog` |
| Modify | `krillnotes-desktop/src/components/WorkspacePeersDialog.tsx` | Add "Create Snapshot" button wired to `SendSnapshotDialog` |
| Modify | `krillnotes-desktop/src/components/SwarmInviteDialog.tsx` | Add "Apply Workspace" action in Snapshot branch |
| Modify | `krillnotes-desktop/src/types.ts` | Add `SnapshotCreatedResult` interface |

---

## Chunk 1: Crypto — Symmetric Key Exposure + Blob Encryption

### Task 1: Extend `crypto.rs`

**Files:**
- Modify: `krillnotes-core/src/core/swarm/crypto.rs`

- [ ] **Step 1: Read `crypto.rs` in full**

  Open the file and note:
  - How `encrypt_for_recipients` generates its AES-256-GCM symmetric key (variable name + type)
  - How `decrypt_payload` unwraps the per-recipient key
  - Which `use` imports are already present (`aes_gcm`, `rand`, etc.)

- [ ] **Step 2: Write failing tests for `encrypt_blob` / `decrypt_blob`**

  Add to the `#[cfg(test)]` block:

  ```rust
  #[test]
  fn test_encrypt_decrypt_blob_roundtrip() {
      let key = [42u8; 32];
      let plaintext = b"hello attachment data";
      let ct = encrypt_blob(&key, plaintext).unwrap();
      assert_ne!(ct.as_slice(), plaintext.as_slice());
      let pt = decrypt_blob(&key, &ct).unwrap();
      assert_eq!(pt, plaintext);
  }

  #[test]
  fn test_decrypt_blob_wrong_key_fails() {
      let key = [42u8; 32];
      let wrong = [99u8; 32];
      let ct = encrypt_blob(&key, b"secret").unwrap();
      assert!(decrypt_blob(&wrong, &ct).is_err());
  }

  #[test]
  fn test_decrypt_blob_truncated_fails() {
      let key = [1u8; 32];
      assert!(decrypt_blob(&key, &[0u8; 5]).is_err());
  }
  ```

  Run: `cargo test -p krillnotes-core`
  Expected: FAIL — `encrypt_blob` / `decrypt_blob` not defined.

- [ ] **Step 3: Implement `encrypt_blob` and `decrypt_blob`**

  Add after the existing public functions. Use the same `aes_gcm` imports already present:

  ```rust
  /// Encrypt a blob with a raw AES-256-GCM key.
  /// Output: 12-byte random nonce prepended to ciphertext+tag.
  pub fn encrypt_blob(key: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>> {
      use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
      use aes_gcm::aead::Aead;
      use rand::RngCore;

      let cipher = Aes256Gcm::new_from_slice(key)
          .map_err(|e| KrillnotesError::Crypto(format!("aes init: {e}")))?;
      let mut nonce_bytes = [0u8; 12];
      rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
      let nonce = Nonce::from_slice(&nonce_bytes);
      let ct = cipher.encrypt(nonce, plaintext)
          .map_err(|e| KrillnotesError::Crypto(format!("encrypt blob: {e}")))?;
      let mut out = nonce_bytes.to_vec();
      out.extend_from_slice(&ct);
      Ok(out)
  }

  /// Decrypt a blob produced by `encrypt_blob`.
  pub fn decrypt_blob(key: &[u8; 32], ciphertext: &[u8]) -> Result<Vec<u8>> {
      use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
      use aes_gcm::aead::Aead;

      if ciphertext.len() < 12 {
          return Err(KrillnotesError::Crypto("blob ciphertext too short".to_string()));
      }
      let nonce = Nonce::from_slice(&ciphertext[..12]);
      let cipher = Aes256Gcm::new_from_slice(key)
          .map_err(|e| KrillnotesError::Crypto(format!("aes init: {e}")))?;
      cipher.decrypt(nonce, &ciphertext[12..])
          .map_err(|e| KrillnotesError::Crypto(format!("decrypt blob: {e}")))
  }
  ```

  Run: `cargo test -p krillnotes-core`
  Expected: the two new tests PASS; existing tests still PASS.

- [ ] **Step 4: Write failing tests for key-returning variants**

  ```rust
  #[test]
  fn test_encrypt_for_recipients_with_key_roundtrip() {
      let sender = make_key();
      let recip = make_key();
      let vk = recip.verifying_key();
      let payload = b"test payload";
      let (ct, sym_key, entries) = encrypt_for_recipients_with_key(payload, &[&vk]).unwrap();
      assert_eq!(sym_key.len(), 32);
      // The key must successfully decrypt the payload.
      let pt = decrypt_payload_with_key(&ct, &entries[0], &recip).unwrap().0;
      assert_eq!(pt, payload);
  }

  #[test]
  fn test_decrypt_payload_with_key_returns_key() {
      let sender = make_key();
      let recip = make_key();
      let vk = recip.verifying_key();
      let (ct, sym_key, entries) = encrypt_for_recipients_with_key(b"data", &[&vk]).unwrap();
      let (_, returned_key) = decrypt_payload_with_key(&ct, &entries[0], &recip).unwrap();
      assert_eq!(returned_key, sym_key);
  }
  ```

  Run: `cargo test -p krillnotes-core`
  Expected: FAIL — functions not defined.

- [ ] **Step 5: Add `encrypt_for_recipients_with_key` and `decrypt_payload_with_key`**

  Refactor the internals of `encrypt_for_recipients` and `decrypt_payload` to extract helpers, then expose the key:

  ```rust
  /// Like `encrypt_for_recipients` but also returns the raw AES-256-GCM key
  /// so the caller can encrypt associated blobs with the same key material.
  pub fn encrypt_for_recipients_with_key(
      payload: &[u8],
      keys: &[&VerifyingKey],
  ) -> Result<(Vec<u8>, [u8; 32], Vec<RecipientEntry>)> {
      // 1. Generate fresh symmetric key (extract from existing encrypt_for_recipients impl).
      // 2. AES-256-GCM encrypt payload → ciphertext.
      // 3. X25519 ECDH + HKDF-SHA256 wrap the key for each recipient.
      // Keep existing encrypt_for_recipients calling this and discarding the key.
      todo!()  // replace with actual impl extracted from encrypt_for_recipients
  }

  /// Like `decrypt_payload` but also returns the unwrapped AES-256-GCM key
  /// so the caller can decrypt associated blobs.
  pub fn decrypt_payload_with_key(
      ciphertext: &[u8],
      entry: &RecipientEntry,
      recipient_key: &SigningKey,
  ) -> Result<(Vec<u8>, [u8; 32])> {
      // Unwrap the recipient entry to get the sym key, decrypt payload, return both.
      todo!()  // replace with actual impl extracted from decrypt_payload
  }
  ```

  Update the existing `encrypt_for_recipients` to call `encrypt_for_recipients_with_key` and discard the key (no breaking change).
  Update the existing `decrypt_payload` to call `decrypt_payload_with_key` and discard the key (no breaking change).

  Run: `cargo test -p krillnotes-core`
  Expected: all PASS.

- [ ] **Step 6: Commit**

  ```bash
  git add krillnotes-core/src/core/swarm/crypto.rs
  git commit -m "feat(crypto): expose symmetric key + add encrypt/decrypt_blob for attachment support"
  ```

---

## Chunk 2: Core — WorkspaceSnapshot + Snapshot Bundle Attachments

### Task 2: Extend `workspace.rs`

**Files:**
- Modify: `krillnotes-core/src/core/workspace.rs`

- [ ] **Step 1: Write failing test — `to_snapshot_json` must include attachments**

  Find the existing `test_to_snapshot_json_roundtrip` test. Add a new test alongside it:

  ```rust
  #[test]
  fn test_to_snapshot_json_includes_attachments() {
      let dir = tempfile::tempdir().unwrap();
      let db_path = dir.path().join("notes.db");
      let key = ed25519_dalek::SigningKey::from_bytes(&[1u8; 32]);
      let mut ws = Workspace::create(&db_path, "", "test-id", key).unwrap();
      let root_id = ws.list_all_notes().unwrap()[0].id.clone();
      ws.attach_file(&root_id, "test.txt", None, b"hello bytes").unwrap();
      let json = ws.to_snapshot_json().unwrap();
      let snap: WorkspaceSnapshot = serde_json::from_slice(&json).unwrap();
      assert_eq!(snap.attachments.len(), 1);
      assert_eq!(snap.attachments[0].filename, "test.txt");
  }
  ```

  Run: `cargo test -p krillnotes-core test_to_snapshot_json_includes_attachments`
  Expected: FAIL — `WorkspaceSnapshot` has no `attachments` field.

- [ ] **Step 2: Add `attachments` to `WorkspaceSnapshot`**

  Find the struct near the top of `workspace.rs` (line ~46):

  ```rust
  use crate::core::attachment::AttachmentMeta;  // add if not already imported

  #[derive(Debug, Serialize, Deserialize)]
  pub struct WorkspaceSnapshot {
      pub version: u32,
      pub notes: Vec<Note>,
      pub user_scripts: Vec<UserScript>,
      #[serde(default)]                         // ← keeps old snapshots deserializing
      pub attachments: Vec<AttachmentMeta>,
  }
  ```

  The `#[serde(default)]` is required for forward compatibility — old snapshots without the field still deserialize.

- [ ] **Step 3: Extend `to_snapshot_json`** (~line 4408)

  ```rust
  pub fn to_snapshot_json(&self) -> Result<Vec<u8>> {
      let notes = self.list_all_notes()?;
      let user_scripts = self.list_user_scripts()?;
      let attachments = self.list_all_attachments()?;
      let snapshot = WorkspaceSnapshot { version: 1, notes, user_scripts, attachments };
      Ok(serde_json::to_vec(&snapshot)?)
  }
  ```

  Run: `cargo test -p krillnotes-core test_to_snapshot_json_includes_attachments`
  Expected: PASS.

- [ ] **Step 4: Add `get_latest_operation_id`**

  Check if a method with this name or equivalent already exists (grep for `latest_operation` or check OperationLog). If not, add after `to_snapshot_json`:

  ```rust
  /// Returns the `operation_id` of the most recent logged operation, or `None` if log is empty.
  pub fn get_latest_operation_id(&self) -> Result<Option<String>> {
      let conn = self.storage.connection();
      let mut stmt = conn.prepare(
          "SELECT operation_id FROM operations ORDER BY timestamp DESC LIMIT 1"
      )?;
      let result = stmt.query_row([], |row| row.get::<_, String>(0)).optional()?;
      Ok(result)
  }
  ```

  Write a test:

  ```rust
  #[test]
  fn test_get_latest_operation_id_empty_workspace() {
      let dir = tempfile::tempdir().unwrap();
      let db_path = dir.path().join("notes.db");
      let key = ed25519_dalek::SigningKey::from_bytes(&[2u8; 32]);
      let ws = Workspace::create(&db_path, "", "test-id", key).unwrap();
      // A freshly created workspace has no operations logged yet.
      assert!(ws.get_latest_operation_id().unwrap().is_none());
  }
  ```

  Run: `cargo test -p krillnotes-core`
  Expected: PASS.

- [ ] **Step 5: Add `Workspace::create_with_id`**

  The `apply_swarm_snapshot` command must preserve the original workspace UUID (required for CRDT convergence across peers, and also for correct attachment-key derivation). Add a constructor variant:

  ```rust
  /// Like `create` but uses a caller-supplied `workspace_id` instead of generating a fresh UUID.
  /// Use when restoring a workspace from a snapshot so peers share the same UUID.
  pub fn create_with_id(
      path: &Path,
      password: &str,
      identity_pubkey: &str,
      signing_key: SigningKey,
      workspace_id: &str,
  ) -> Result<Self> {
      // Delegate to create's internals but substitute the workspace_id.
      // Simplest approach: call create() then immediately overwrite workspace_id in DB + struct.
      // OR: extract the internals of create() into a private helper that accepts workspace_id.
      // Choose whichever is less duplication. The key constraint is that workspace_id
      // must be set BEFORE derive_attachment_key is called (it uses workspace_id as input).
      todo!()
  }
  ```

  Look at `Workspace::create` to find the attachment key derivation line (it calls `derive_attachment_key(password, &workspace_id)`). The simplest correct implementation: copy `create`'s body and replace the `uuid::Uuid::new_v4().to_string()` line with `workspace_id.to_string()`.

  Write a test:

  ```rust
  #[test]
  fn test_create_with_id_preserves_uuid() {
      let dir = tempfile::tempdir().unwrap();
      let db_path = dir.path().join("notes.db");
      let key = ed25519_dalek::SigningKey::from_bytes(&[3u8; 32]);
      let custom_id = "my-fixed-workspace-uuid";
      let ws = Workspace::create_with_id(&db_path, "", "test-id", key, custom_id).unwrap();
      assert_eq!(ws.workspace_id(), custom_id);
  }
  ```

  Run: `cargo test -p krillnotes-core`
  Expected: PASS.

- [ ] **Step 6: Commit**

  ```bash
  git add krillnotes-core/src/core/workspace.rs
  git commit -m "feat(workspace): attachments in WorkspaceSnapshot, get_latest_operation_id, create_with_id"
  ```

### Task 3: Extend snapshot bundle with attachment blobs

**Files:**
- Modify: `krillnotes-core/src/core/swarm/snapshot.rs`

- [ ] **Step 1: Write failing roundtrip test with attachments**

  ```rust
  #[test]
  fn test_snapshot_with_attachments_roundtrip() {
      let sender_key = make_key();
      let recipient_key = make_key();
      let payload = b"{}";
      let att_id = "att-uuid-abc";
      let att_blob = b"raw attachment bytes here";

      let bundle = create_snapshot_bundle(SnapshotParams {
          workspace_id: "ws-1".to_string(),
          workspace_name: "Test WS".to_string(),
          source_device_id: "dev-1".to_string(),
          as_of_operation_id: "op-1".to_string(),
          workspace_json: payload.to_vec(),
          sender_key: &sender_key,
          recipient_keys: vec![&recipient_key.verifying_key()],
          recipient_peer_ids: vec!["peer-pub-key".to_string()],
          attachment_blobs: vec![(att_id.to_string(), att_blob.to_vec())],
      }).unwrap();

      let parsed = parse_snapshot_bundle(&bundle, &recipient_key).unwrap();
      assert_eq!(parsed.workspace_json, payload.to_vec());
      assert_eq!(parsed.workspace_name, "Test WS");
      assert_eq!(parsed.attachment_blobs.len(), 1);
      assert_eq!(parsed.attachment_blobs[0].0, att_id);
      assert_eq!(parsed.attachment_blobs[0].1, att_blob.to_vec());
  }

  #[test]
  fn test_snapshot_empty_attachments_roundtrip() {
      // Existing roundtrip test must still pass with attachment_blobs: vec![].
      let sender_key = make_key();
      let recipient_key = make_key();
      let bundle = create_snapshot_bundle(SnapshotParams {
          workspace_id: "ws-1".to_string(),
          workspace_name: "Test".to_string(),
          source_device_id: "dev-1".to_string(),
          as_of_operation_id: "op-1".to_string(),
          workspace_json: b"payload".to_vec(),
          sender_key: &sender_key,
          recipient_keys: vec![&recipient_key.verifying_key()],
          recipient_peer_ids: vec!["p1".to_string()],
          attachment_blobs: vec![],
      }).unwrap();
      let parsed = parse_snapshot_bundle(&bundle, &recipient_key).unwrap();
      assert_eq!(parsed.attachment_blobs.len(), 0);
  }
  ```

  Also update the old `test_snapshot_encrypt_decrypt_roundtrip` and `test_snapshot_wrong_key_fails` tests to add `attachment_blobs: vec![]` to their `SnapshotParams`.

  Run: `cargo test -p krillnotes-core swarm::snapshot`
  Expected: FAIL — `SnapshotParams` has no `attachment_blobs` field.

- [ ] **Step 2: Extend `SnapshotParams` and `ParsedSnapshot`**

  ```rust
  pub struct SnapshotParams<'a> {
      pub workspace_id: String,
      pub workspace_name: String,
      pub source_device_id: String,
      pub as_of_operation_id: String,
      pub workspace_json: Vec<u8>,
      pub sender_key: &'a SigningKey,
      pub recipient_keys: Vec<&'a VerifyingKey>,
      pub recipient_peer_ids: Vec<String>,
      /// (attachment_id, plaintext_bytes). Encrypted into the bundle with the same key as the payload.
      pub attachment_blobs: Vec<(String, Vec<u8>)>,
  }

  pub struct ParsedSnapshot {
      pub workspace_id: String,
      pub workspace_name: String,           // ← new: from header.workspace_name
      pub as_of_operation_id: String,
      pub sender_public_key: String,
      pub workspace_json: Vec<u8>,
      pub attachment_blobs: Vec<(String, Vec<u8>)>,  // ← new: (att_id, plaintext)
  }
  ```

- [ ] **Step 3: Extend `create_snapshot_bundle` to encrypt attachment blobs**

  Replace the `encrypt_for_recipients` call with `encrypt_for_recipients_with_key` to obtain the symmetric key:

  ```rust
  use crate::core::swarm::crypto::{encrypt_for_recipients_with_key, encrypt_blob};

  let (ciphertext, sym_key, mut entries) =
      encrypt_for_recipients_with_key(&params.workspace_json, &params.recipient_keys)?;

  for (entry, peer_id) in entries.iter_mut().zip(params.recipient_peer_ids.iter()) {
      entry.peer_id = peer_id.clone();
  }

  // Encrypt each attachment blob with the same symmetric key.
  let mut att_entries: Vec<(String, Vec<u8>)> = Vec::new();
  for (att_id, plaintext) in &params.attachment_blobs {
      let ct = encrypt_blob(&sym_key, plaintext)?;
      att_entries.push((att_id.clone(), ct));
  }

  let has_attachments = !att_entries.is_empty();
  ```

  Update the header:
  ```rust
  let header = SwarmHeader {
      // ... existing fields ...
      has_attachments,   // was hardcoded false before
      // ...
  };
  ```

  In the ZIP-writing section, add attachment entries after `payload.enc`:
  ```rust
  for (att_id, att_ct) in &att_entries {
      zip.start_file(format!("attachments/{att_id}.enc"), opts)?;
      zip.write_all(att_ct)?;
  }
  ```

  Also add `attachment_blobs` to the manifest files list for signature:
  ```rust
  // The signature covers header + payload. Attachment blobs are authenticated
  // indirectly via the AES-GCM tag on each blob using the payload-derived key.
  // No change to sign_manifest needed.
  ```

- [ ] **Step 4: Extend `parse_snapshot_bundle` to decrypt attachment blobs and return `workspace_name`**

  Replace the `decrypt_payload` call with `decrypt_payload_with_key`:

  ```rust
  use crate::core::swarm::crypto::{decrypt_payload_with_key, decrypt_blob};

  let mut plaintext = None;
  let mut sym_key_found = None;
  for entry in &recipients {
      if let Ok((pt, key)) = decrypt_payload_with_key(&ciphertext, entry, recipient_key) {
          plaintext = Some(pt);
          sym_key_found = Some(key);
          break;
      }
  }
  let workspace_json = plaintext
      .ok_or_else(|| KrillnotesError::Swarm("no recipient entry matched our key".to_string()))?;
  let sym_key = sym_key_found.unwrap();

  // Decrypt attachment blobs — entries are named "attachments/<id>.enc".
  let mut attachment_blobs = Vec::new();
  for i in 0..zip.len() {
      let mut file = zip.by_index(i)
          .map_err(|e| KrillnotesError::Swarm(format!("zip index {i}: {e}")))?;
      let name = file.name().to_string();
      if let Some(att_id) = name.strip_prefix("attachments/").and_then(|n| n.strip_suffix(".enc")) {
          let mut ct = Vec::new();
          file.read_to_end(&mut ct)
              .map_err(|e| KrillnotesError::Swarm(format!("read att {att_id}: {e}")))?;
          let pt = decrypt_blob(&sym_key, &ct)?;
          attachment_blobs.push((att_id.to_string(), pt));
      }
  }

  Ok(ParsedSnapshot {
      workspace_id: header.workspace_id,
      workspace_name: header.workspace_name,   // ← new
      as_of_operation_id: header.as_of_operation_id.unwrap_or_default(),
      sender_public_key: header.source_identity,
      workspace_json,
      attachment_blobs,
  })
  ```

  Run: `cargo test -p krillnotes-core`
  Expected: all PASS.

- [ ] **Step 5: Commit**

  ```bash
  git add krillnotes-core/src/core/swarm/snapshot.rs
  git commit -m "feat(snapshot): encrypt attachment blobs in bundle; return workspace_name from parse"
  ```

---

## Chunk 3: Tauri Commands

### Task 4: `create_snapshot_for_peers` command

**Files:**
- Modify: `krillnotes-desktop/src-tauri/src/lib.rs`

- [ ] **Step 1: Find `workspace.workspace_id()` and `workspace.get_workspace_metadata()`**

  Grep `workspace.rs` for `pub fn workspace_id` and `pub fn get_workspace_metadata`. Note the exact return types. We'll use these to get the workspace ID and name.

- [ ] **Step 2: Write the command**

  Add before the `generate_handler!` macro:

  ```rust
  #[derive(Serialize)]
  #[serde(rename_all = "camelCase")]
  pub struct SnapshotCreatedResult {
      pub saved_path: String,
      pub peer_count: usize,
      pub as_of_operation_id: String,
  }

  #[tauri::command]
  async fn create_snapshot_for_peers(
      window: tauri::Window,
      state: State<'_, AppState>,
      identity_uuid: String,
      peer_public_keys: Vec<String>,   // base64-encoded Ed25519 verifying keys
      save_path: String,
  ) -> std::result::Result<SnapshotCreatedResult, String> {
      use krillnotes_core::core::swarm::snapshot::{create_snapshot_bundle, SnapshotParams};
      use krillnotes_core::core::workspace::WorkspaceSnapshot;

      let identity_uuid = Uuid::parse_str(&identity_uuid).map_err(|e| e.to_string())?;

      // 1. Sender signing key + display name.
      let (signing_key, source_display_name) = {
          let ids = state.unlocked_identities.lock().expect("Mutex poisoned");
          let id = ids.get(&identity_uuid).ok_or("Identity not unlocked")?;
          (
              Ed25519SigningKey::from_bytes(&id.signing_key.to_bytes()),
              id.display_name.clone(),
          )
      };
      let source_device_id = krillnotes_core::get_device_id();

      // 2. Decode recipient verifying keys.
      let recipient_vks: Vec<Ed25519VerifyingKey> = peer_public_keys
          .iter()
          .map(|pk_b64| {
              let bytes = BASE64.decode(pk_b64).map_err(|e| e.to_string())?;
              let arr: [u8; 32] = bytes.try_into().map_err(|_| "key wrong length".to_string())?;
              Ed25519VerifyingKey::from_bytes(&arr).map_err(|e| e.to_string())
          })
          .collect::<std::result::Result<_, _>>()?;

      // 3. Collect workspace data (hold lock only briefly).
      let (workspace_id, workspace_name, workspace_json, attachment_blobs, as_of_op_id) = {
          let workspaces = state.workspaces.lock().expect("Mutex poisoned");
          let ws = workspaces.get(window.label()).ok_or("Workspace not open")?;

          let workspace_id = ws.workspace_id().to_string();
          let workspace_name = ws.get_workspace_metadata()
              .map(|m| m.name.clone())
              .unwrap_or_else(|_| workspace_id.clone());

          let workspace_json = ws.to_snapshot_json().map_err(|e| e.to_string())?;

          // Get attachment metadata from the snapshot JSON to load blobs.
          let snapshot: WorkspaceSnapshot = serde_json::from_slice(&workspace_json)
              .map_err(|e| e.to_string())?;
          let mut attachment_blobs = Vec::new();
          for meta in &snapshot.attachments {
              let plaintext = ws.get_attachment_bytes(&meta.id).map_err(|e| e.to_string())?;
              attachment_blobs.push((meta.id.clone(), plaintext));
          }

          let as_of_op_id = ws.get_latest_operation_id()
              .map_err(|e| e.to_string())?
              .unwrap_or_default();

          (workspace_id, workspace_name, workspace_json, attachment_blobs, as_of_op_id)
      };

      // 4. Build the bundle.
      let recipient_refs: Vec<&Ed25519VerifyingKey> = recipient_vks.iter().collect();
      let bundle_bytes = create_snapshot_bundle(SnapshotParams {
          workspace_id: workspace_id.clone(),
          workspace_name,
          source_device_id,
          as_of_operation_id: as_of_op_id.clone(),
          workspace_json,
          sender_key: &signing_key,
          recipient_keys: recipient_refs,
          recipient_peer_ids: peer_public_keys.clone(),
          attachment_blobs,
      }).map_err(|e| e.to_string())?;

      // 5. Write to file.
      std::fs::write(&save_path, &bundle_bytes).map_err(|e| e.to_string())?;

      // 6. Update last_sent_op for each recipient.
      // Check peer_registry.rs for the method name — likely update_last_sent_op(peer_identity_id, op_id).
      // If the method doesn't exist, add it to PeerRegistry (UPDATE sync_peers SET last_sent_op = ?
      // WHERE peer_identity_id = ?).
      if !as_of_op_id.is_empty() {
          let mut workspaces = state.workspaces.lock().expect("Mutex poisoned");
          if let Some(ws) = workspaces.get_mut(window.label()) {
              for pk in &peer_public_keys {
                  let _ = ws.update_peer_last_sent_op(pk, &as_of_op_id);
              }
          }
      }

      Ok(SnapshotCreatedResult {
          saved_path: save_path,
          peer_count: peer_public_keys.len(),
          as_of_operation_id: as_of_op_id,
      })
  }
  ```

  > **Workspace helpers to verify / add if missing:**
  > - `ws.workspace_id() -> &str` — already confirmed to exist
  > - `ws.get_workspace_metadata() -> Result<WorkspaceMetadata>` — already confirmed to exist
  > - `ws.get_attachment_bytes(id) -> Result<Vec<u8>>` — used by export.rs, should exist
  > - `ws.update_peer_last_sent_op(peer_identity_id, op_id)` — check `peer_registry.rs`; add if missing:
  >   ```rust
  >   pub fn update_peer_last_sent_op(&mut self, peer_identity_id: &str, op_id: &str) -> Result<()> {
  >       self.storage.connection().execute(
  >           "UPDATE sync_peers SET last_sent_op = ? WHERE peer_identity_id = ?",
  >           rusqlite::params![op_id, peer_identity_id],
  >       )?;
  >       Ok(())
  >   }
  >   ```

- [ ] **Step 3: Register in `generate_handler!`**

  Add `create_snapshot_for_peers` to the list.

- [ ] **Step 4: Build check**

  ```bash
  cd krillnotes-desktop && cargo build -p krillnotes-desktop 2>&1 | head -60
  ```

  Expected: no errors.

- [ ] **Step 5: Commit**

  ```bash
  git add krillnotes-desktop/src-tauri/src/lib.rs krillnotes-core/src/core/workspace.rs
  git commit -m "feat(tauri): add create_snapshot_for_peers command"
  ```

### Task 5: `apply_swarm_snapshot` command

**Files:**
- Modify: `krillnotes-desktop/src-tauri/src/lib.rs`

- [ ] **Step 1: Study `execute_import` (~line 1656)**

  Read the `execute_import` function to understand the exact call order:
  1. `Workspace::create` at a given path with a fresh random DB password
  2. `bind_workspace` to link the workspace UUID with the identity
  3. `create_workspace_window` + `store_workspace` to open it in the app
  4. Return `WorkspaceInfo`

  Our command mirrors this pattern. The difference: we use `Workspace::create_with_id` and restore from parsed snapshot data instead of a ZIP archive.

- [ ] **Step 2: Find `workspaces_dir` / default workspace location**

  Grep for `workspaces_dir` or the path where `execute_import` creates its `folder_path`. The frontend passes `folder_path`; for `apply_swarm_snapshot`, we derive it from the app data dir + workspace UUID (so the invitee doesn't need to pick a location manually). Check `crate::settings` for relevant helpers.

- [ ] **Step 3: Write the command**

  ```rust
  #[tauri::command]
  async fn apply_swarm_snapshot(
      window: tauri::Window,
      app: tauri::AppHandle,
      state: State<'_, AppState>,
      path: String,
      identity_uuid: String,
      workspace_name_override: Option<String>,
  ) -> std::result::Result<WorkspaceInfo, String> {
      use krillnotes_core::core::swarm::snapshot::parse_snapshot_bundle;
      use krillnotes_core::core::workspace::WorkspaceSnapshot;
      use krillnotes_core::core::export::WorkspaceMetadata;

      let identity_uuid_parsed = Uuid::parse_str(&identity_uuid).map_err(|e| e.to_string())?;

      // 1. Read + decrypt bundle.
      let data = std::fs::read(&path).map_err(|e| e.to_string())?;
      let import_seed = {
          let ids = state.unlocked_identities.lock().expect("Mutex poisoned");
          let id = ids.get(&identity_uuid_parsed).ok_or("Identity not unlocked")?;
          id.signing_key.to_bytes()
      };
      let recipient_key = Ed25519SigningKey::from_bytes(&import_seed);
      let parsed = parse_snapshot_bundle(&data, &recipient_key).map_err(|e| e.to_string())?;

      let snapshot: WorkspaceSnapshot = serde_json::from_slice(&parsed.workspace_json)
          .map_err(|e| e.to_string())?;

      // 2. Determine workspace path (same pattern as execute_import + folder_path).
      //    Use workspaces_dir(app) if that helper exists; otherwise derive from app data dir.
      //    Directory name = workspace UUID so each workspace has a unique folder.
      let folder = crate::settings::default_workspace_folder(&app, &parsed.workspace_id)
          .map_err(|e| e.to_string())?;
      std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
      let db_path = folder.join("notes.db");
      if db_path.exists() {
          return Err(format!("Workspace {} already exists locally.", parsed.workspace_id));
      }

      // 3. Generate a fresh DB password (same as execute_import).
      let workspace_password: String = {
          let mut bytes = [0u8; 32];
          rand::rngs::OsRng.fill_bytes(&mut bytes);
          BASE64.encode(&bytes)
      };

      // 4. Create workspace DB with the snapshot's UUID.
      let pubkey_str = BASE64.encode(
          Ed25519SigningKey::from_bytes(&import_seed).verifying_key().as_bytes()
      );
      let mut ws = Workspace::create_with_id(
          &db_path, &workspace_password, &pubkey_str,
          Ed25519SigningKey::from_bytes(&import_seed),
          &parsed.workspace_id,
      ).map_err(|e| e.to_string())?;

      // 5. Set workspace name (from override or snapshot header).
      let ws_name = workspace_name_override
          .filter(|s| !s.trim().is_empty())
          .unwrap_or_else(|| parsed.workspace_name.clone());
      let mut meta = ws.get_workspace_metadata().unwrap_or_default();
      meta.name = ws_name;
      ws.set_workspace_metadata(&meta).map_err(|e| e.to_string())?;

      // 6. Restore notes + user scripts.
      ws.import_snapshot_json(&parsed.workspace_json).map_err(|e| e.to_string())?;

      // 7. Restore attachment blobs.
      // attach_file_with_id(id, note_id, filename, mime_type, data)
      for (att_id, plaintext) in &parsed.attachment_blobs {
          if let Some(meta) = snapshot.attachments.iter().find(|a| a.id == *att_id) {
              ws.attach_file_with_id(
                  att_id,
                  &meta.note_id,
                  &meta.filename,
                  meta.mime_type.as_deref(),
                  plaintext,
              ).map_err(|e| e.to_string())?;
          }
      }

      // 8. Register sender as sync peer.
      // Check peer_registry.rs for upsert method; add if needed.
      let _ = ws.upsert_sync_peer(
          &format!("identity:{}", parsed.sender_public_key),  // placeholder device_id
          &parsed.sender_public_key,
          None,                                                // last_sent_op (we haven't sent yet)
          Some(&parsed.as_of_operation_id),                   // last_received_op = snapshot watermark
      );

      // 9. Bind workspace to identity + open window (mirror execute_import exactly).
      let workspace_uuid = ws.workspace_id().to_string();
      {
          let ids = state.unlocked_identities.lock().expect("Mutex poisoned");
          let unlocked = ids.get(&identity_uuid_parsed).ok_or("Identity not unlocked")?;
          let seed = unlocked.signing_key.to_bytes();
          let mgr = state.identity_manager.lock().expect("Mutex poisoned");
          mgr.bind_workspace(
              &identity_uuid_parsed,
              &workspace_uuid,
              &db_path.display().to_string(),
              &workspace_password,
              &seed,
          ).map_err(|e| format!("bind_workspace: {e}"))?;
      }
      let _ = std::fs::create_dir_all(folder.join("attachments"));
      let label = generate_unique_label(&state, &folder);
      let new_window = create_workspace_window(&app, &label, &window)?;
      store_workspace(&state, label.clone(), ws, folder);
      new_window.set_title(&format!("Krillnotes - {label}")).map_err(|e| e.to_string())?;
      if window.label() == "main" {
          window.close().map_err(|e| e.to_string())?;
      }
      get_workspace_info_internal(&state, &label)
  }
  ```

  > **Helpers to verify / add:**
  > - `crate::settings::default_workspace_folder(&app, workspace_id)` — check what `execute_import` uses for path derivation. If no such helper exists, inline the path logic (app data dir + "workspaces" + workspace_id).
  > - `WorkspaceMetadata::default()` — add `#[derive(Default)]` to `WorkspaceMetadata` in `export.rs` if not present.
  > - `AttachmentMeta.mime_type` field — check `attachment.rs`; if it doesn't exist, pass `None`.
  > - `ws.upsert_sync_peer(device_id, identity_id, last_sent_op, last_received_op)` — check `peer_registry.rs`; add if missing:
  >   ```rust
  >   pub fn upsert_sync_peer(&mut self, device_id: &str, identity_id: &str,
  >       last_sent_op: Option<&str>, last_received_op: Option<&str>) -> Result<()> {
  >       self.storage.connection().execute(
  >           "INSERT INTO sync_peers (peer_device_id, peer_identity_id, last_sent_op, last_received_op)
  >            VALUES (?, ?, ?, ?)
  >            ON CONFLICT(peer_device_id) DO UPDATE SET
  >              peer_identity_id = excluded.peer_identity_id,
  >              last_sent_op = COALESCE(excluded.last_sent_op, last_sent_op),
  >              last_received_op = COALESCE(excluded.last_received_op, last_received_op)",
  >           rusqlite::params![device_id, identity_id, last_sent_op, last_received_op],
  >       )?;
  >       Ok(())
  >   }
  >   ```

- [ ] **Step 4: Register in `generate_handler!`**

  Add `apply_swarm_snapshot`.

- [ ] **Step 5: Build check**

  ```bash
  cd krillnotes-desktop && cargo build -p krillnotes-desktop 2>&1 | head -60
  ```

  Expected: no errors.

- [ ] **Step 6: Commit**

  ```bash
  git add krillnotes-desktop/src-tauri/src/lib.rs krillnotes-core/src/core/workspace.rs \
          krillnotes-core/src/core/peer_registry.rs
  git commit -m "feat(tauri): add apply_swarm_snapshot command"
  ```

---

## Chunk 4: Frontend

### Task 6: `PostAcceptDialog`

**Files:**
- Create: `krillnotes-desktop/src/components/PostAcceptDialog.tsx`
- Modify: `krillnotes-desktop/src/components/InviteManagerDialog.tsx`

- [ ] **Step 1: Read `DeleteConfirmDialog.tsx`**

  Open it to note the exact CSS classes used for modal overlays, buttons (primary/secondary), and headings. Use the same classes for visual consistency.

- [ ] **Step 2: Create `PostAcceptDialog.tsx`**

  ```tsx
  interface PostAcceptDialogProps {
    open: boolean;
    peerName: string;
    onSendNow: () => void;
    onLater: () => void;
  }

  export function PostAcceptDialog({ open, peerName, onSendNow, onLater }: PostAcceptDialogProps) {
    if (!open) return null;
    return (
      <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
        <div className="bg-white dark:bg-zinc-800 rounded-lg p-6 max-w-sm w-full shadow-xl">
          <h2 className="text-lg font-semibold mb-2">Peer accepted</h2>
          <p className="text-sm text-zinc-500 dark:text-zinc-400 mb-6">
            <strong>{peerName}</strong> has been added as a peer.
            Send them the workspace snapshot now so they can join?
          </p>
          <div className="flex justify-end gap-3">
            <button
              onClick={onLater}
              className="px-4 py-2 text-sm rounded-md border border-zinc-300 dark:border-zinc-600 hover:bg-zinc-50 dark:hover:bg-zinc-700"
            >
              Later
            </button>
            <button
              onClick={onSendNow}
              className="px-4 py-2 text-sm rounded-md bg-blue-600 text-white hover:bg-blue-700"
            >
              Send Snapshot
            </button>
          </div>
        </div>
      </div>
    );
  }
  ```

  Adjust CSS classes to match what `DeleteConfirmDialog.tsx` uses.

- [ ] **Step 3: Wire into `InviteManagerDialog`**

  1. Read `InviteManagerDialog.tsx` and find where `accept_peer` is invoked (the `invoke("accept_peer", ...)` call).

  2. Add state:
     ```tsx
     const [postAcceptPeer, setPostAcceptPeer] = useState<{
       name: string;
       publicKey: string;
     } | null>(null);
     const [showSendSnapshot, setShowSendSnapshot] = useState(false);
     const [sendSnapshotFor, setSendSnapshotFor] = useState<string[]>([]);
     ```

  3. After `accept_peer` resolves successfully:
     ```tsx
     const peerName = result.localName || result.inviteeDeclaredName || result.peerIdentityId;
     setPostAcceptPeer({ name: peerName, publicKey: result.peerIdentityId });
     ```

  4. Render `PostAcceptDialog`:
     ```tsx
     <PostAcceptDialog
       open={postAcceptPeer !== null}
       peerName={postAcceptPeer?.name ?? ''}
       onSendNow={() => {
         setSendSnapshotFor([postAcceptPeer!.publicKey]);
         setPostAcceptPeer(null);
         setShowSendSnapshot(true);
       }}
       onLater={() => setPostAcceptPeer(null)}
     />
     ```

  (`SendSnapshotDialog` is wired in Task 7.)

- [ ] **Step 4: Type-check**

  ```bash
  cd krillnotes-desktop && npx tsc --noEmit 2>&1 | head -30
  ```

  Expected: no errors.

- [ ] **Step 5: Commit**

  ```bash
  git add krillnotes-desktop/src/components/PostAcceptDialog.tsx \
          krillnotes-desktop/src/components/InviteManagerDialog.tsx
  git commit -m "feat(ui): PostAcceptDialog after peer acceptance with Send Now / Later choice"
  ```

### Task 7: `SendSnapshotDialog`

**Files:**
- Create: `krillnotes-desktop/src/components/SendSnapshotDialog.tsx`
- Modify: `krillnotes-desktop/src/types.ts`
- Modify: `krillnotes-desktop/src/components/InviteManagerDialog.tsx`
- Modify: `krillnotes-desktop/src/components/WorkspacePeersDialog.tsx`

- [ ] **Step 1: Add types**

  In `types.ts`, add:
  ```typescript
  export interface SnapshotCreatedResult {
    savedPath: string;
    peerCount: number;
    asOfOperationId: string;
  }
  ```

- [ ] **Step 2: Read `WorkspacePeersDialog.tsx`**

  Note: how `identityUuid` is passed in, how `PeerInfo[]` is loaded, and any existing button/action patterns to follow.

- [ ] **Step 3: Create `SendSnapshotDialog.tsx`**

  ```tsx
  import { invoke } from '@tauri-apps/api/core';
  import { save } from '@tauri-apps/plugin-dialog';
  import { useState, useEffect } from 'react';
  import type { PeerInfo, SnapshotCreatedResult } from '../types';

  interface SendSnapshotDialogProps {
    open: boolean;
    identityUuid: string;
    preSelectedPublicKeys: string[];  // peers to pre-check (from "Send Now" path)
    onClose: () => void;
    onSuccess: (result: SnapshotCreatedResult) => void;
  }

  export function SendSnapshotDialog({
    open, identityUuid, preSelectedPublicKeys, onClose, onSuccess,
  }: SendSnapshotDialogProps) {
    const [peers, setPeers] = useState<PeerInfo[]>([]);
    const [selected, setSelected] = useState<Set<string>>(new Set());
    const [savePath, setSavePath] = useState<string>('');
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);

    useEffect(() => {
      if (!open) return;
      setSelected(new Set(preSelectedPublicKeys));
      setSavePath('');
      setError(null);
      invoke<PeerInfo[]>('list_workspace_peers')
        .then(setPeers)
        .catch(e => setError(String(e)));
    }, [open, preSelectedPublicKeys]);

    const chooseSavePath = async () => {
      const path = await save({
        filters: [{ name: 'Swarm Bundle', extensions: ['swarm'] }],
        defaultPath: 'snapshot.swarm',
      });
      if (path) setSavePath(path);
    };

    const toggle = (pk: string) => {
      setSelected(prev => {
        const next = new Set(prev);
        next.has(pk) ? next.delete(pk) : next.add(pk);
        return next;
      });
    };

    const handleCreate = async () => {
      if (selected.size === 0) { setError('Select at least one peer.'); return; }
      if (!savePath) { setError('Choose a save location first.'); return; }
      setLoading(true);
      setError(null);
      try {
        const result = await invoke<SnapshotCreatedResult>('create_snapshot_for_peers', {
          identityUuid,
          peerPublicKeys: Array.from(selected),
          savePath,
        });
        onSuccess(result);
        onClose();
      } catch (e) {
        setError(String(e));
      } finally {
        setLoading(false);
      }
    };

    if (!open) return null;
    return (
      <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
        <div className="bg-white dark:bg-zinc-800 rounded-lg p-6 max-w-md w-full shadow-xl">
          <h2 className="text-lg font-semibold mb-4">Create Workspace Snapshot</h2>
          <p className="text-sm text-zinc-500 dark:text-zinc-400 mb-3">
            Select peers to include. Each peer receives the same encrypted snapshot.
          </p>

          <div className="space-y-1 mb-4 max-h-48 overflow-y-auto">
            {peers.map(p => (
              <label key={p.peerIdentityId} className="flex items-center gap-2 text-sm cursor-pointer">
                <input
                  type="checkbox"
                  checked={selected.has(p.peerIdentityId)}
                  onChange={() => toggle(p.peerIdentityId)}
                />
                <span>{p.displayName}</span>
                <span className="text-zinc-400 text-xs">{p.fingerprint}</span>
              </label>
            ))}
            {peers.length === 0 && <p className="text-sm text-zinc-400">No peers registered.</p>}
          </div>

          <div className="flex items-center gap-2 mb-4">
            <button
              onClick={chooseSavePath}
              className="px-3 py-1.5 text-sm rounded border border-zinc-300 dark:border-zinc-600"
            >
              Choose location…
            </button>
            {savePath && <span className="text-xs text-zinc-500 truncate">{savePath}</span>}
          </div>

          {error && <p className="text-sm text-red-500 mb-3">{error}</p>}

          <div className="flex justify-end gap-3">
            <button onClick={onClose} className="px-4 py-2 text-sm rounded border border-zinc-300 dark:border-zinc-600">
              Cancel
            </button>
            <button
              onClick={handleCreate}
              disabled={loading}
              className="px-4 py-2 text-sm rounded bg-blue-600 text-white hover:bg-blue-700 disabled:opacity-50"
            >
              {loading ? 'Creating…' : 'Create Snapshot'}
            </button>
          </div>
        </div>
      </div>
    );
  }
  ```

- [ ] **Step 4: Complete wiring in `InviteManagerDialog`**

  Import and render `SendSnapshotDialog` using the state from Task 6 Step 3:
  ```tsx
  <SendSnapshotDialog
    open={showSendSnapshot}
    identityUuid={identityUuid}
    preSelectedPublicKeys={sendSnapshotFor}
    onClose={() => setShowSendSnapshot(false)}
    onSuccess={result => {
      // Optionally show a toast: `Snapshot saved to ${result.savedPath}`
    }}
  />
  ```

- [ ] **Step 5: Wire into `WorkspacePeersDialog`**

  Add a "Create Snapshot" button (e.g. in the dialog header or footer). On click:
  ```tsx
  setSendSnapshotFor(peers.map(p => p.peerIdentityId));
  setShowSendSnapshot(true);
  ```

  Render `<SendSnapshotDialog ... />` in this dialog as well, or lift the state up to the parent if both dialogs are siblings in the component tree.

- [ ] **Step 6: Type-check**

  ```bash
  cd krillnotes-desktop && npx tsc --noEmit 2>&1 | head -30
  ```

- [ ] **Step 7: Commit**

  ```bash
  git add krillnotes-desktop/src/components/SendSnapshotDialog.tsx \
          krillnotes-desktop/src/components/InviteManagerDialog.tsx \
          krillnotes-desktop/src/components/WorkspacePeersDialog.tsx \
          krillnotes-desktop/src/types.ts
  git commit -m "feat(ui): SendSnapshotDialog for peer snapshot creation with multi-peer support"
  ```

### Task 8: Apply snapshot UI in `SwarmInviteDialog`

**Files:**
- Modify: `krillnotes-desktop/src/components/SwarmInviteDialog.tsx`

- [ ] **Step 1: Read `SwarmInviteDialog.tsx` in full**

  Find the `SwarmMode.Snapshot` branch (currently just shows metadata). Note:
  - How `swarmFilePath` is available (prop? state? derive from the file being opened)
  - How `targetIdentityUuid` is available from `SwarmFileInfo.Snapshot`
  - How `onWorkspaceCreated` / `onOpenWorkspace` callbacks should be surfaced to the parent

- [ ] **Step 2: Add the "Apply Workspace" UI**

  In the Snapshot branch, replace/extend the display to add:

  ```tsx
  const [nameOverride, setNameOverride] = useState(info.workspaceName);
  const [applying, setApplying] = useState(false);
  const [applyError, setApplyError] = useState<string | null>(null);

  const handleApply = async () => {
    if (!info.targetIdentityUuid) {
      setApplyError('No matching identity found. Make sure the correct identity is unlocked.');
      return;
    }
    setApplying(true);
    setApplyError(null);
    try {
      await invoke('apply_swarm_snapshot', {
        path: swarmFilePath,          // the path to the .swarm file being displayed
        identityUuid: info.targetIdentityUuid,
        workspaceNameOverride: nameOverride.trim() || undefined,
      });
      onClose();  // the command opens a new window; just close the dialog
    } catch (e) {
      setApplyError(String(e));
    } finally {
      setApplying(false);
    }
  };
  ```

  UI elements:
  - Workspace name field (editable, pre-filled with `info.workspaceName`)
  - "Apply Workspace" button
  - Error display
  - If `targetIdentityUuid` is null (no matching identity), show a warning instead of the button

- [ ] **Step 3: Ensure `swarmFilePath` is available in `SwarmInviteDialog`**

  If the file path isn't currently threaded through as a prop, add it. The file path is needed so the command can re-read the bundle bytes. Check where `open_swarm_file_cmd` is called — the path is known there.

- [ ] **Step 4: Type-check**

  ```bash
  cd krillnotes-desktop && npx tsc --noEmit
  ```

- [ ] **Step 5: Commit**

  ```bash
  git add krillnotes-desktop/src/components/SwarmInviteDialog.tsx
  git commit -m "feat(ui): apply snapshot action in SwarmInviteDialog for Snapshot .swarm files"
  ```

---

## Final Verification

- [ ] Run all Rust tests: `cargo test -p krillnotes-core`
  Expected: all PASS.

- [ ] TypeScript: `cd krillnotes-desktop && npx tsc --noEmit`
  Expected: no errors.

- [ ] Full build: `cd krillnotes-desktop && npm run tauri build 2>&1 | tail -20`
  Expected: no errors.

- [ ] Manual smoke test end-to-end:
  1. Inviter: create workspace, add notes with attachments + user scripts
  2. Complete invite → accept → `PostAcceptDialog` appears
  3. Click "Send Snapshot" → `SendSnapshotDialog` opens with invitee pre-selected
  4. Choose save path → "Create Snapshot" → `snapshot.swarm` file created
  5. Invitee: open `snapshot.swarm` → `SwarmInviteDialog` shows Snapshot branch
  6. Optionally rename → "Apply Workspace" → new workspace window opens
  7. Verify: all notes, scripts, and attachments present; inviter registered as peer

- [ ] Update `CHANGELOG.md` with Phase D summary
