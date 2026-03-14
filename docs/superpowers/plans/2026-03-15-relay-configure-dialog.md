# Relay Configure Dialog Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire up relay server registration/login so that clicking "Configure" on a relay peer opens a dialog that calls the relay server, stores encrypted credentials, and enables relay syncing in subsequent `poll_sync` calls.

**Architecture:** Identity-scoped relay credentials are stored encrypted at `<config_dir>/relay/<identity_uuid>.json`. `configure_relay` runs the 3-step PoP registration flow; `relay_login` re-authenticates. `poll_sync` loads credentials from disk on every call and registers a `RelayChannel` if present. The React `ConfigureRelayDialog` presents Register/Login tabs; clicking "Configure" on a relay peer opens it.

**Tech Stack:** Rust (ed25519-dalek, HKDF, reqwest blocking, hex), Tauri v2, React 19, TypeScript, Tailwind v4

**Spec:** `docs/superpowers/specs/2026-03-15-relay-configure-dialog-design.md`

**Working directory:** `.worktrees/feat/sync-engine/` (all paths below are relative to this root)

---

## Chunk 1: Rust — relay_key, commands, poll_sync

### File Map

| File | Change |
|------|--------|
| `krillnotes-core/src/core/identity.rs` | Add `relay_key()` method to `UnlockedIdentity` |
| `krillnotes-desktop/src-tauri/Cargo.toml` | Add `hex = "0.4"` dependency |
| `krillnotes-desktop/src-tauri/src/commands/sync.rs` | Add imports; add `RelayInfo` struct; implement `configure_relay`, `relay_login`, `has_relay_credentials`, `get_relay_info`; update `poll_sync` |
| `krillnotes-desktop/src-tauri/src/lib.rs` | Register `get_relay_info` in `generate_handler!` |

---

### Task 1: Add `relay_key()` to `UnlockedIdentity`

**Files:**
- Modify: `krillnotes-core/src/core/identity.rs` (around line 136 — the `impl UnlockedIdentity` block)

Context: `UnlockedIdentity` already has a `contacts_key()` method using HKDF. `relay_key()` is identical but with a different info string.

- [ ] **Open `identity.rs` and find the `contacts_key` method**

  ```bash
  # from .worktrees/feat/sync-engine/
  grep -n "fn contacts_key\|fn relay_key" krillnotes-core/src/core/identity.rs
  ```

  Expected: `contacts_key` at ~line 138, no `relay_key` yet.

- [ ] **Add `relay_key()` immediately after `contacts_key()` in the `impl UnlockedIdentity` block**

  ```rust
  /// Derives a 32-byte encryption key for this identity's relay credentials.
  /// Uses HKDF-SHA256 with the Ed25519 seed as IKM.
  pub fn relay_key(&self) -> [u8; 32] {
      let hk = hkdf::Hkdf::<sha2::Sha256>::new(None, self.signing_key.as_bytes());
      let mut okm = [0u8; 32];
      hk.expand(b"krillnotes-relay-v1", &mut okm)
          .expect("HKDF expand failed — output length is valid");
      okm
  }
  ```

- [ ] **Add tests to `krillnotes-core/src/core/identity_tests.rs`**

  The project uses `#[path = "identity_tests.rs"] mod tests;` in `identity.rs` (line 870) — all tests go in that file, not inline. Open `identity_tests.rs` and append at the end (all imports are already in scope via `use super::*`):

  ```rust
  #[test]
  fn test_relay_key_differs_from_contacts_key() {
      // All imports (SigningKey, Uuid, UnlockedIdentity) are in scope via super::*
      let signing_key = SigningKey::generate(&mut rand::rngs::OsRng);
      let verifying_key = signing_key.verifying_key();
      let unlocked = UnlockedIdentity {
          identity_uuid: Uuid::new_v4(),
          display_name: "Test".to_string(),
          signing_key,
          verifying_key,
      };
      assert_ne!(unlocked.relay_key(), unlocked.contacts_key(),
          "relay_key and contacts_key must differ");
  }

  #[test]
  fn test_relay_key_deterministic() {
      let seed = [0x11u8; 32];
      let signing_key = SigningKey::from_bytes(&seed);
      let verifying_key = signing_key.verifying_key();
      let unlocked = UnlockedIdentity {
          identity_uuid: Uuid::new_v4(),
          display_name: "Test".to_string(),
          signing_key,
          verifying_key,
      };
      assert_eq!(unlocked.relay_key(), unlocked.relay_key(),
          "relay_key must be deterministic");
  }
  ```

- [ ] **Run the tests**

  ```bash
  cargo test -p krillnotes-core test_relay_key -- --nocapture
  ```

  Expected: 2 tests pass.

- [ ] **Commit**

  ```bash
  git add krillnotes-core/src/core/identity.rs
  git commit -m "feat(core): add relay_key() to UnlockedIdentity"
  ```

---

### Task 2: Add `hex` dependency to desktop crate

**Files:**
- Modify: `krillnotes-desktop/src-tauri/Cargo.toml`

The `hex` crate is used in `krillnotes-core` (for `decrypt_pop_challenge`) but is not yet a dependency of `krillnotes-desktop`. We need it to hex-encode `verifying_key.to_bytes()` for the relay `device_public_key`.

- [ ] **Add `hex = "0.4"` to `[dependencies]` in `krillnotes-desktop/src-tauri/Cargo.toml`**

  Add after the `base64 = "0.22"` line:

  ```toml
  hex = "0.4"
  ```

- [ ] **Verify it compiles**

  ```bash
  cd krillnotes-desktop && cargo check -p krillnotes-desktop-lib 2>&1 | head -20
  ```

  Expected: no errors (warnings are fine).

- [ ] **Commit**

  ```bash
  git add krillnotes-desktop/src-tauri/Cargo.toml
  git commit -m "chore(desktop): add hex dependency"
  ```

---

### Task 3: Add imports and `RelayInfo` struct to `sync.rs`

**Files:**
- Modify: `krillnotes-desktop/src-tauri/src/commands/sync.rs` (top of file)

- [ ] **Replace the existing import block (lines 1–16) with the expanded version**

  Current imports:
  ```rust
  use crate::AppState;
  use tauri::{Emitter, State, Window};
  use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
  use krillnotes_core::core::{
      device::get_device_id,
      sync::{FolderChannel, SyncContext, SyncEngine, SyncEvent},
  };
  ```

  Replace with:
  ```rust
  use crate::AppState;
  use chrono::Utc;
  use tauri::{Emitter, State, Window};
  use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
  use uuid::Uuid;
  use krillnotes_core::core::{
      device::get_device_id,
      sync::{FolderChannel, SyncContext, SyncEngine, SyncEvent},
      sync::relay::{
          RelayCredentials,
          load_relay_credentials,
          save_relay_credentials,
      },
  };
  #[cfg(feature = "relay")]
  use krillnotes_core::core::sync::relay::{RelayChannel, RelayClient};
  #[cfg(feature = "relay")]
  use krillnotes_core::core::sync::relay::auth::decrypt_pop_challenge;

  /// Relay account info returned by `get_relay_info`.
  /// Serialised with camelCase keys so the TypeScript interface matches.
  #[derive(serde::Serialize)]
  #[serde(rename_all = "camelCase")]
  pub struct RelayInfo {
      pub relay_url: String,
      pub email: String,
  }
  ```

  > **Note on feature gating:** `load_relay_credentials` / `save_relay_credentials` / `RelayCredentials` are NOT feature-gated (they live in `auth.rs` which has no `#[cfg(feature = "relay")]`). Only `RelayChannel`, `RelayClient`, and `decrypt_pop_challenge` are gated.

- [ ] **Check it compiles**

  ```bash
  cd krillnotes-desktop && cargo check -p krillnotes-desktop-lib 2>&1 | tail -5
  ```

  Expected: clean (or only pre-existing warnings).

- [ ] **Commit**

  ```bash
  git add krillnotes-desktop/src-tauri/src/commands/sync.rs
  git commit -m "feat(desktop/sync): add relay imports and RelayInfo struct"
  ```

---

### Task 4: Implement `has_relay_credentials` and `get_relay_info`

**Files:**
- Modify: `krillnotes-desktop/src-tauri/src/commands/sync.rs` (the two stub functions near end of file)

- [ ] **Find the `has_relay_credentials` stub**

  ```bash
  grep -n "has_relay_credentials\|get_relay_info" krillnotes-desktop/src-tauri/src/commands/sync.rs
  ```

- [ ] **Replace the `has_relay_credentials` stub with the real implementation**

  Current stub (~lines 182–187):
  ```rust
  pub async fn has_relay_credentials(
      _window: Window,
      _state: State<'_, AppState>,
  ) -> Result<bool, String> {
      Ok(false)
  }
  ```

  Replace with:
  ```rust
  pub async fn has_relay_credentials(
      window: Window,
      state: State<'_, AppState>,
  ) -> Result<bool, String> {
      let workspace_label = window.label().to_string();
      let identity_uuid: Uuid = {
          let m = state.workspace_identities.lock().map_err(|e| e.to_string())?;
          *m.get(&workspace_label).ok_or("No identity bound to this workspace")?
      };
      let relay_key = {
          let m = state.unlocked_identities.lock().map_err(|e| e.to_string())?;
          m.get(&identity_uuid)
              .ok_or("Identity not unlocked")?
              .relay_key()
      };
      let relay_dir = crate::settings::config_dir().join("relay");
      let creds = load_relay_credentials(&relay_dir, &identity_uuid.to_string(), &relay_key)
          .map_err(|e| e.to_string())?;
      Ok(creds.is_some())
  }
  ```

- [ ] **Add `get_relay_info` directly after `has_relay_credentials`**

  ```rust
  // ── get_relay_info ──────────────────────────────────────────────────────────

  /// Return relay account info (URL + email) if credentials are stored for the
  /// identity bound to this workspace window. Returns `null` if not configured.
  #[tauri::command]
  pub async fn get_relay_info(
      window: Window,
      state: State<'_, AppState>,
  ) -> Result<Option<RelayInfo>, String> {
      let workspace_label = window.label().to_string();
      let identity_uuid: Uuid = {
          let m = state.workspace_identities.lock().map_err(|e| e.to_string())?;
          *m.get(&workspace_label).ok_or("No identity bound to this workspace")?
      };
      let relay_key = {
          let m = state.unlocked_identities.lock().map_err(|e| e.to_string())?;
          m.get(&identity_uuid)
              .ok_or("Identity not unlocked")?
              .relay_key()
      };
      let relay_dir = crate::settings::config_dir().join("relay");
      match load_relay_credentials(&relay_dir, &identity_uuid.to_string(), &relay_key)
          .map_err(|e| e.to_string())?
      {
          Some(creds) => Ok(Some(RelayInfo {
              relay_url: creds.relay_url,
              email: creds.email,
          })),
          None => Ok(None),
      }
  }
  ```

- [ ] **Verify compilation**

  ```bash
  cd krillnotes-desktop && cargo check -p krillnotes-desktop-lib 2>&1 | tail -5
  ```

- [ ] **Commit**

  ```bash
  git add krillnotes-desktop/src-tauri/src/commands/sync.rs
  git commit -m "feat(desktop/sync): implement has_relay_credentials and get_relay_info"
  ```

---

### Task 5: Implement `configure_relay`

**Files:**
- Modify: `krillnotes-desktop/src-tauri/src/commands/sync.rs` (the `configure_relay` stub)

The registration flow: `register()` → decrypt PoP challenge with identity signing key → `register_verify()` → save credentials.

- [ ] **Find the `configure_relay` stub**

  ```bash
  grep -n "fn configure_relay" krillnotes-desktop/src-tauri/src/commands/sync.rs
  ```

- [ ] **Replace the stub body**

  Current stub (~lines 116–124):
  ```rust
  pub async fn configure_relay(
      _state: State<'_, AppState>,
      _identity_uuid: String,
      _relay_url: String,
      _email: String,
      _password: String,
  ) -> Result<(), String> {
      Err("configure_relay not yet implemented".to_string())
  }
  ```

  Replace with:
  ```rust
  pub async fn configure_relay(
      state: State<'_, AppState>,
      identity_uuid: String,
      relay_url: String,
      email: String,
      password: String,
  ) -> Result<(), String> {
      let uuid = Uuid::parse_str(&identity_uuid).map_err(|e| e.to_string())?;

      // Capture signing key, verifying key, and relay encryption key in one lock.
      let (signing_key, verifying_key, relay_key) = {
          let m = state.unlocked_identities.lock().map_err(|e| e.to_string())?;
          let id = m.get(&uuid)
              .ok_or("Identity is not unlocked — please unlock your identity first")?;
          // Use .clone() — consistent with how poll_sync clones the signing key (line 63).
          let sk = id.signing_key.clone();
          let vk = id.verifying_key;
          let rk = id.relay_key();
          (sk, vk, rk)
      };

      // device_public_key is hex-encoded (not Base64 — relay API uses hex throughout).
      let device_public_key = hex::encode(verifying_key.to_bytes());

      let client = RelayClient::new(&relay_url);

      // Step 1: Register → receive PoP challenge.
      let result = client
          .register(&email, &password, &identity_uuid, &device_public_key)
          .map_err(|e| e.to_string())?;

      // Step 2: Decrypt the PoP challenge using the identity's Ed25519 signing key.
      let nonce_bytes = decrypt_pop_challenge(
          &signing_key,
          &result.challenge.encrypted_nonce,
          &result.challenge.server_public_key,
      )
      .map_err(|e| e.to_string())?;
      let nonce_hex = hex::encode(&nonce_bytes);

      // Step 3: Verify registration — obtain session token.
      let session = client
          .register_verify(&device_public_key, &nonce_hex)
          .map_err(|e| e.to_string())?;

      // Build and save credentials (encrypted with relay_key via AES-256-GCM).
      let creds = RelayCredentials {
          relay_url,
          email,
          session_token: session.session_token,
          // 30 days is a local approximation; relay server governs actual expiry.
          session_expires_at: Utc::now() + chrono::Duration::days(30),
          device_public_key,
      };
      let relay_dir = crate::settings::config_dir().join("relay");
      save_relay_credentials(&relay_dir, &identity_uuid, &creds, &relay_key)
          .map_err(|e| e.to_string())?;

      Ok(())
  }
  ```

  > **Note on `ed25519_dalek`:** We use `.clone()` on the signing key (same as `poll_sync`) so no direct `ed25519_dalek` type constructors are called — the type is already resolved through `AppState`. No explicit `ed25519-dalek` dep needed in `Cargo.toml`.

- [ ] **Verify compilation**

  ```bash
  cd krillnotes-desktop && cargo check -p krillnotes-desktop-lib 2>&1 | tail -10
  ```

- [ ] **Commit**

  ```bash
  git add krillnotes-desktop/src-tauri/src/commands/sync.rs
  git commit -m "feat(desktop/sync): implement configure_relay registration flow"
  ```

---

### Task 6: Implement `relay_login`

**Files:**
- Modify: `krillnotes-desktop/src-tauri/src/commands/sync.rs` (the `relay_login` stub)

Note: the stub signature lacks `relay_url` — the stub must be updated to add it. The frontend Login tab passes `relay_url` so the user can re-configure with a different server.

- [ ] **Replace the `relay_login` stub entirely** (stub is at ~lines 133–140):

  ```rust
  pub async fn relay_login(
      state: State<'_, AppState>,
      identity_uuid: String,
      relay_url: String,
      email: String,
      password: String,
  ) -> Result<(), String> {
      let uuid = Uuid::parse_str(&identity_uuid).map_err(|e| e.to_string())?;

      let relay_key = {
          let m = state.unlocked_identities.lock().map_err(|e| e.to_string())?;
          m.get(&uuid)
              .ok_or("Identity is not unlocked — please unlock your identity first")?
              .relay_key()
      };

      let relay_dir = crate::settings::config_dir().join("relay");

      // Reuse existing device_public_key if credentials are already stored,
      // otherwise derive it fresh from the verifying key.
      let device_public_key = {
          match load_relay_credentials(&relay_dir, &identity_uuid, &relay_key)
              .map_err(|e| e.to_string())?
          {
              Some(existing) => existing.device_public_key,
              None => {
                  let m = state.unlocked_identities.lock().map_err(|e| e.to_string())?;
                  let id = m.get(&uuid)
                      .ok_or("Identity is not unlocked")?;
                  hex::encode(id.verifying_key.to_bytes())
              }
          }
      };

      let client = RelayClient::new(&relay_url);
      let session = client
          .login(&email, &password)
          .map_err(|e| e.to_string())?;

      let creds = RelayCredentials {
          relay_url,
          email,
          session_token: session.session_token,
          session_expires_at: Utc::now() + chrono::Duration::days(30),
          device_public_key,
      };
      save_relay_credentials(&relay_dir, &identity_uuid, &creds, &relay_key)
          .map_err(|e| e.to_string())?;

      Ok(())
  }
  ```

- [ ] **Verify compilation**

  ```bash
  cd krillnotes-desktop && cargo check -p krillnotes-desktop-lib 2>&1 | tail -10
  ```

- [ ] **Commit**

  ```bash
  git add krillnotes-desktop/src-tauri/src/commands/sync.rs
  git commit -m "feat(desktop/sync): implement relay_login"
  ```

---

### Task 7: Update `poll_sync` to register `RelayChannel` when credentials exist

**Files:**
- Modify: `krillnotes-desktop/src-tauri/src/commands/sync.rs` (the `poll_sync` function)

Two changes:
1. Extend the identity lock block to also capture `relay_key` and `sender_device_key_hex`
2. After `workspace` is obtained, register a `RelayChannel` if credentials exist

- [ ] **Find the identity lock block in `poll_sync`**

  ```bash
  grep -n "signing_key\|sender_display_name\|identity_pubkey" \
    krillnotes-desktop/src-tauri/src/commands/sync.rs
  ```

  It currently looks like (lines ~59–64):
  ```rust
  let (signing_key, sender_display_name, identity_pubkey) = {
      let m = state.unlocked_identities.lock().map_err(|e| e.to_string())?;
      let id = m.get(&identity_uuid).ok_or("Identity not unlocked")?;
      let pubkey = BASE64.encode(id.verifying_key.as_bytes());
      (id.signing_key.clone(), id.display_name.clone(), pubkey)
  };
  ```

- [ ] **Replace that block to also capture `relay_key` and `sender_device_key_hex`**

  ```rust
  let (signing_key, sender_display_name, identity_pubkey, relay_key, sender_device_key_hex) = {
      let m = state.unlocked_identities.lock().map_err(|e| e.to_string())?;
      let id = m.get(&identity_uuid).ok_or("Identity not unlocked")?;
      let pubkey_b64 = BASE64.encode(id.verifying_key.as_bytes()); // FolderChannel uses Base64
      let pubkey_hex = hex::encode(id.verifying_key.to_bytes());   // RelayChannel uses hex
      let rk = id.relay_key();
      (id.signing_key.clone(), id.display_name.clone(), pubkey_b64, rk, pubkey_hex)
  };
  ```

- [ ] **Find the workspace acquisition in `poll_sync`**

  ```bash
  grep -n "workspaces.get_mut\|engine.register_channel\|FolderChannel" \
    krillnotes-desktop/src-tauri/src/commands/sync.rs
  ```

  It currently looks like (lines ~76–88):
  ```rust
  let mut engine = SyncEngine::new();
  engine.register_channel(Box::new(FolderChannel::new(identity_pubkey, device_id)));

  // -- Hold contact_managers + workspaces for the poll ----------------------
  let mut contact_managers = state.contact_managers.lock().map_err(|e| e.to_string())?;
  let contact_manager = contact_managers
      .get_mut(&identity_uuid)
      .ok_or("Contact manager not found — is the identity unlocked?")?;

  let mut workspaces = state.workspaces.lock().map_err(|e| e.to_string())?;
  let workspace = workspaces
      .get_mut(&workspace_label)
      .ok_or_else(|| format!("Workspace not found: {workspace_label}"))?;
  ```

- [ ] **After the `workspace` is obtained, add the relay channel registration block**

  ```rust
  // Try to add relay channel if credentials exist for this identity.
  // load_relay_credentials is NOT feature-gated (pure disk I/O in auth.rs).
  // Only RelayClient/RelayChannel construction is gated.
  let relay_dir = crate::settings::config_dir().join("relay");
  if let Ok(Some(creds)) = load_relay_credentials(
      &relay_dir,
      &identity_uuid.to_string(),
      &relay_key,
  ) {
      #[cfg(feature = "relay")]
      {
          let relay_client = RelayClient::new(&creds.relay_url)
              .with_session_token(&creds.session_token);
          let workspace_id_str = workspace.workspace_id().to_string();
          let relay_channel = RelayChannel::new(
              relay_client,
              workspace_id_str,
              sender_device_key_hex.clone(),
          );
          engine.register_channel(Box::new(relay_channel));
      }
  }
  ```

  Place this block immediately after the `let workspace = workspaces.get_mut(...)...` line, before `let mut ctx = SyncContext { ... }`.

  > **Note:** Since the desktop crate already enables `features = ["relay"]` in Cargo.toml, the `#[cfg(feature = "relay")]` block will always compile in practice. The split keeps `load_relay_credentials` unconditional so it can be used for other purposes without the relay HTTP feature.

- [ ] **Verify compilation**

  ```bash
  cd krillnotes-desktop && cargo check -p krillnotes-desktop-lib 2>&1 | tail -10
  ```

- [ ] **Commit**

  ```bash
  git add krillnotes-desktop/src-tauri/src/commands/sync.rs
  git commit -m "feat(desktop/sync): register RelayChannel in poll_sync when credentials exist"
  ```

---

### Task 8: Register `get_relay_info` in Tauri handler list

**Files:**
- Modify: `krillnotes-desktop/src-tauri/src/lib.rs`

- [ ] **Find the `generate_handler!` block**

  ```bash
  grep -n "has_relay_credentials\|generate_handler" \
    krillnotes-desktop/src-tauri/src/lib.rs
  ```

- [ ] **Add `get_relay_info` to the handler list immediately after `has_relay_credentials`**

  ```rust
  has_relay_credentials,
  get_relay_info,
  ```

- [ ] **Verify compilation**

  ```bash
  cd krillnotes-desktop && cargo check -p krillnotes-desktop-lib 2>&1 | tail -5
  ```

  Expected: clean.

- [ ] **Commit**

  ```bash
  git add krillnotes-desktop/src-tauri/src/lib.rs
  git commit -m "feat(desktop): register get_relay_info Tauri command"
  ```

---

## Chunk 2: Frontend — ConfigureRelayDialog + WorkspacePeersDialog

### File Map

| File | Change |
|------|--------|
| `krillnotes-desktop/src/types.ts` | Add `RelayInfo` interface |
| `krillnotes-desktop/src/components/ConfigureRelayDialog.tsx` | New file — tab-based relay config dialog |
| `krillnotes-desktop/src/components/WorkspacePeersDialog.tsx` | Wire Configure button for relay to open the dialog |

---

### Task 9: Add `RelayInfo` to `types.ts`

**Files:**
- Modify: `krillnotes-desktop/src/types.ts`

- [ ] **Add the `RelayInfo` interface** near other sync-related types in `types.ts`:

  ```typescript
  export interface RelayInfo {
    relayUrl: string;
    email: string;
  }
  ```

- [ ] **TypeScript check**

  ```bash
  cd krillnotes-desktop && npx tsc --noEmit 2>&1 | head -20
  ```

  Expected: no new errors.

- [ ] **Commit**

  ```bash
  git add krillnotes-desktop/src/types.ts
  git commit -m "feat(frontend/types): add RelayInfo interface"
  ```

---

### Task 10: Create `ConfigureRelayDialog`

**Files:**
- Create: `krillnotes-desktop/src/components/ConfigureRelayDialog.tsx`

- [ ] **Create the file with the full implementation**

  ```tsx
  // This Source Code Form is subject to the terms of the Mozilla Public
  // License, v. 2.0. If a copy of the MPL was not distributed with this
  // file, You can obtain one at https://mozilla.org/MPL/2.0/.
  //
  // Copyright (c) 2024-2026 TripleACS Pty Ltd t/a 2pi Software

  import { useState, useEffect } from 'react';
  import { invoke } from '@tauri-apps/api/core';
  import type { RelayInfo } from '../types';

  interface Props {
    identityUuid: string;
    peerDeviceId: string;
    onClose: () => void;
    onConfigured: () => void;
  }

  type Tab = 'register' | 'login';

  function mapError(raw: string): string {
    const s = raw.toLowerCase();
    if (s.includes('identity') && (s.includes('lock') || s.includes('unlock')))
      return 'Please unlock your identity before configuring relay.';
    if (s.includes('http 409') || s.includes('already') || s.includes('conflict'))
      return 'Email already registered — try the Login tab.';
    if (s.includes('http 401') || s.includes('invalid') || s.includes('unauthorized'))
      return 'Invalid credentials. Please check your email and password.';
    if (s.includes('http 404') || s.includes('not found'))
      return 'Relay server not found at this URL.';
    if (s.includes('http 4') || s.includes('http 5') || s.includes('unavailable') || s.includes('connect'))
      return 'Cannot reach relay server. Check the URL and try again.';
    return raw;
  }

  export default function ConfigureRelayDialog({
    identityUuid,
    peerDeviceId,
    onClose,
    onConfigured,
  }: Props) {
    const [activeTab, setActiveTab] = useState<Tab>('register');
    const [relayUrl, setRelayUrl] = useState('');
    const [email, setEmail] = useState('');
    const [password, setPassword] = useState('');
    const [confirmPassword, setConfirmPassword] = useState('');
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [initialising, setInitialising] = useState(true);

    // On mount: check if credentials are already stored to pre-fill and pick tab.
    useEffect(() => {
      invoke<RelayInfo | null>('get_relay_info')
        .then(info => {
          if (info) {
            setRelayUrl(info.relayUrl);
            setEmail(info.email);
            setActiveTab('login');
          }
        })
        .catch(() => {/* ignore — fall back to register tab */})
        .finally(() => setInitialising(false));
    }, []);

    useEffect(() => {
      const handler = (e: KeyboardEvent) => { if (e.key === 'Escape') onClose(); };
      window.addEventListener('keydown', handler);
      return () => window.removeEventListener('keydown', handler);
    }, [onClose]);

    const handleSubmit = async (e: React.FormEvent) => {
      e.preventDefault();
      setError(null);

      if (activeTab === 'register' && password !== confirmPassword) {
        setError('Passwords do not match.');
        return;
      }
      if (!relayUrl.trim() || !email.trim() || !password) {
        setError('All fields are required.');
        return;
      }

      setLoading(true);
      try {
        if (activeTab === 'register') {
          await invoke('configure_relay', { identityUuid, relayUrl, email, password });
        } else {
          await invoke('relay_login', { identityUuid, relayUrl, email, password });
        }
        // Ensure the peer is marked as relay channel, storing the URL in
        // channelParams for display/reference (relay routing uses disk credentials,
        // not this field, but the integration tests and peer display use it).
        await invoke('update_peer_channel', {
          peerDeviceId,
          channelType: 'relay',
          channelParams: JSON.stringify({ relay_url: relayUrl }),
        });
        onConfigured();
      } catch (err) {
        setError(mapError(String(err)));
      } finally {
        setLoading(false);
      }
    };

    const inputClass =
      'w-full px-3 py-1.5 text-sm rounded border border-[var(--color-border)] ' +
      'bg-[var(--color-background)] text-[var(--color-foreground)] ' +
      'focus:outline-none focus:ring-1 focus:ring-blue-500';

    return (
      <div className="fixed inset-0 z-70 flex items-center justify-center bg-black/50">
        <div className="bg-[var(--color-background)] border border-[var(--color-border)] rounded-lg shadow-xl w-[420px] flex flex-col">

          {/* Header */}
          <div className="flex items-center justify-between p-4 border-b border-[var(--color-border)]">
            <h2 className="text-base font-semibold">Configure Relay</h2>
            <button
              onClick={onClose}
              className="text-[var(--color-muted-foreground)] hover:text-[var(--color-foreground)] px-2"
            >
              ✕
            </button>
          </div>

          {/* Tabs */}
          <div className="flex border-b border-[var(--color-border)]">
            {(['register', 'login'] as Tab[]).map(tab => (
              <button
                key={tab}
                onClick={() => { setActiveTab(tab); setError(null); }}
                className={
                  'flex-1 py-2 text-sm font-medium capitalize ' +
                  (activeTab === tab
                    ? 'border-b-2 border-blue-500 text-[var(--color-foreground)]'
                    : 'text-[var(--color-muted-foreground)] hover:text-[var(--color-foreground)]')
                }
              >
                {tab}
              </button>
            ))}
          </div>

          {/* Form */}
          {initialising ? (
            <p className="p-6 text-sm text-center text-[var(--color-muted-foreground)]">Loading…</p>
          ) : (
            <form onSubmit={handleSubmit} className="p-4 space-y-3">
              <div>
                <label className="block text-xs font-medium mb-1">Relay URL</label>
                <input
                  type="url"
                  value={relayUrl}
                  onChange={e => setRelayUrl(e.target.value)}
                  placeholder="https://relay.example.com"
                  className={inputClass}
                  required
                />
              </div>
              <div>
                <label className="block text-xs font-medium mb-1">Email</label>
                <input
                  type="email"
                  value={email}
                  onChange={e => setEmail(e.target.value)}
                  placeholder="you@example.com"
                  className={inputClass}
                  required
                />
              </div>
              <div>
                <label className="block text-xs font-medium mb-1">Password</label>
                <input
                  type="password"
                  value={password}
                  onChange={e => setPassword(e.target.value)}
                  className={inputClass}
                  required
                />
              </div>
              {activeTab === 'register' && (
                <div>
                  <label className="block text-xs font-medium mb-1">Confirm Password</label>
                  <input
                    type="password"
                    value={confirmPassword}
                    onChange={e => setConfirmPassword(e.target.value)}
                    className={inputClass}
                    required
                  />
                </div>
              )}

              {error && (
                <p className="text-xs text-red-500 bg-red-500/10 px-3 py-2 rounded">
                  {error}
                </p>
              )}

              <div className="flex justify-end gap-2 pt-1">
                <button
                  type="button"
                  onClick={onClose}
                  className="px-3 py-1.5 text-sm rounded border border-[var(--color-border)] hover:bg-[var(--color-secondary)]"
                >
                  Cancel
                </button>
                <button
                  type="submit"
                  disabled={loading}
                  className="px-3 py-1.5 text-sm font-medium bg-blue-600 text-white rounded hover:bg-blue-700 disabled:opacity-50"
                >
                  {loading
                    ? (activeTab === 'register' ? 'Registering…' : 'Logging in…')
                    : (activeTab === 'register' ? 'Register' : 'Log in')}
                </button>
              </div>
            </form>
          )}
        </div>
      </div>
    );
  }
  ```

- [ ] **TypeScript check**

  ```bash
  cd krillnotes-desktop && npx tsc --noEmit 2>&1 | head -20
  ```

  Expected: no new errors.

- [ ] **Commit**

  ```bash
  git add krillnotes-desktop/src/components/ConfigureRelayDialog.tsx
  git commit -m "feat(frontend): add ConfigureRelayDialog with register/login tabs"
  ```

---

### Task 11: Update `WorkspacePeersDialog` to open `ConfigureRelayDialog`

**Files:**
- Modify: `krillnotes-desktop/src/components/WorkspacePeersDialog.tsx`

Three changes:
1. Import `ConfigureRelayDialog`
2. Add `showConfigureRelay: PeerInfo | null` state
3. Update the Configure button's `onClick` and disable logic; render the dialog

- [ ] **Add the import** at the top of `WorkspacePeersDialog.tsx`, after the existing local imports:

  ```typescript
  import ConfigureRelayDialog from './ConfigureRelayDialog';
  ```

- [ ] **Add state** inside the component (after existing state declarations, ~line 70):

  ```typescript
  const [showConfigureRelay, setShowConfigureRelay] = useState<PeerInfo | null>(null);
  ```

- [ ] **Update the Configure button** (currently ~lines 258–265)

  Current:
  ```tsx
  <button
    onClick={() => handleUpdateChannel(peer, selectedChannelType)}
    disabled={selectedChannelType !== 'folder' && selectedChannelType === peer.channelType && !(peer.peerDeviceId in pendingChannelType)}
    className="text-xs px-2 py-0.5 rounded border border-[var(--color-border)] hover:bg-[var(--color-secondary)] disabled:opacity-40"
  >
    {t('peers.configure', 'Configure')}
  </button>
  ```

  Replace with:
  ```tsx
  <button
    onClick={() => {
      if (selectedChannelType === 'relay') {
        setShowConfigureRelay(peer);
      } else {
        handleUpdateChannel(peer, selectedChannelType);
      }
    }}
    disabled={
      selectedChannelType !== 'relay' &&
      selectedChannelType !== 'folder' &&
      selectedChannelType === peer.channelType &&
      !(peer.peerDeviceId in pendingChannelType)
    }
    className="text-xs px-2 py-0.5 rounded border border-[var(--color-border)] hover:bg-[var(--color-secondary)] disabled:opacity-40"
  >
    {t('peers.configure', 'Configure')}
  </button>
  ```

- [ ] **Add the `ConfigureRelayDialog` render** before the closing `</div>` of the outer modal wrapper, after all other sub-dialogs (near the bottom of the return, around line 410):

  ```tsx
  {showConfigureRelay && (
    <ConfigureRelayDialog
      identityUuid={identityUuid}
      peerDeviceId={showConfigureRelay.peerDeviceId}
      onClose={() => setShowConfigureRelay(null)}
      onConfigured={async () => {
        setShowConfigureRelay(null);
        await loadPeers();
      }}
    />
  )}
  ```

- [ ] **TypeScript check**

  ```bash
  cd krillnotes-desktop && npx tsc --noEmit 2>&1 | head -20
  ```

  Expected: no errors.

- [ ] **Commit**

  ```bash
  git add krillnotes-desktop/src/components/WorkspacePeersDialog.tsx
  git commit -m "feat(frontend): wire Configure button for relay peers to ConfigureRelayDialog"
  ```

---

### Task 12: Final build verification

- [ ] **Run a full dev build to verify Rust + frontend compile together**

  ```bash
  cd krillnotes-desktop && npm run tauri build -- --debug 2>&1 | tail -20
  ```

  Expected: successful build with no errors.

- [ ] **Manual smoke test checklist**

  1. Open a workspace with an identity bound
  2. Open Workspace Peers dialog
  3. On a peer with Relay type, click Configure → dialog opens on Register tab
  4. Enter a relay URL, email, password, confirm password → click Register
     - With a real relay server: session token stored, no error
     - Without a relay server: error "Cannot reach relay server" shown
  5. Close dialog, click Configure again → dialog opens on Login tab with URL/email pre-filled
  6. Click Sync Now → Relay channel should now be active in the backend

- [ ] **Final commit (CHANGELOG + PR)**

  ```bash
  # Update CHANGELOG.md with the feature (add under [Unreleased])
  # Then:
  git add CHANGELOG.md
  git commit -m "docs: update changelog for relay configure dialog"

  git push github-https feat/sync-engine
  ```

  Then open a PR targeting `master`.

---

*Spec:* `docs/superpowers/specs/2026-03-15-relay-configure-dialog-design.md`
