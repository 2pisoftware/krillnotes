# Identity Model Foundation — Design Document

**Date:** 2026-03-04
**Status:** Approved
**Spec reference:** `docs/swarm/section_11_identity_model.md`

## Summary

Implement the cryptographic identity foundation for Krillnotes as described in Swarm Protocol Section 11. This PR delivers the `IdentityManager` module in `krillnotes-core` — keypair generation, Argon2id passphrase protection, identity file I/O, and a settings registry that binds workspaces to identities with encrypted DB passwords.

**Scope:** Foundation only. No Tauri commands, no UI changes, no workspace integration. The existing "enter password per workspace" flow is unchanged. A follow-up PR will wire identity unlock into the workspace open flow.

**Out of scope:** `.krillid` export/import (deferred), identity switching UI, OS keychain integration (commercial/future).

## Design Decisions

### Symmetric DB password encryption (deviation from spec)

The spec says DB passwords are "encrypted to the identity's public key" (implying X25519 asymmetric encryption). We use **symmetric encryption** instead: HKDF from the Ed25519 seed → AES-256-GCM.

**Rationale:** The DB password never leaves the device. Sync happens via `.swarm` bundles, not DB replication. Each device generates its own random DB password. There is no scenario where one device encrypts a DB password for another device. Symmetric is simpler, fewer dependencies, equally secure for this use case.

### Single `identity.rs` module

All identity code lives in `krillnotes-core/src/core/identity.rs`. This matches the codebase convention where each concern is a single file (`workspace.rs`, `attachment.rs`, `storage.rs`). Can be split into a subdirectory later if sync adds substantial code.

### Separate settings file

`IdentityManager` owns `~/.config/krillnotes/identity_settings.json`, separate from Tauri's `settings.json`. This keeps `krillnotes-core` independent of the Tauri layer and avoids two writers competing for the same file.

### Ed25519 crate: `ed25519-dalek`

Pure Rust, audited, widely used, pairs with the existing RustCrypto stack (`chacha20poly1305`, `hkdf`, `sha2`).

## Data Types & File Formats

### Identity file (`~/.config/krillnotes/identities/<uuid>.json`)

```rust
struct IdentityFile {
    identity_uuid: Uuid,
    display_name: String,
    public_key: String,          // base64 Ed25519 verifying key (32 bytes)
    private_key_enc: EncryptedKey,
}

struct EncryptedKey {
    ciphertext: String,          // base64 AES-256-GCM ciphertext (seed + 16-byte tag)
    nonce: String,               // base64 12-byte nonce
    kdf: String,                 // "argon2id"
    kdf_params: KdfParams,
}

struct KdfParams {
    salt: String,                // base64 16-byte salt
    m_cost: u32,                 // 65536 (64 MiB)
    t_cost: u32,                 // 3 iterations
    p_cost: u32,                 // 1 thread
}
```

### Identity settings (`~/.config/krillnotes/identity_settings.json`)

```rust
struct IdentitySettings {
    identities: Vec<IdentityRef>,
    workspaces: HashMap<String, WorkspaceBinding>,  // workspace_uuid -> binding
}

struct IdentityRef {
    uuid: Uuid,
    display_name: String,
    file: String,               // relative: "identities/<uuid>.json"
    last_used: DateTime<Utc>,
}

struct WorkspaceBinding {
    db_path: String,
    identity_uuid: Uuid,
    db_password_enc: String,    // base64(nonce || ciphertext || tag)
}
```

## Crypto Chain

```
Passphrase
    |
    v
Argon2id(passphrase, salt, m=64MiB, t=3, p=1)
    |
    v
32-byte encryption key
    |
    v
AES-256-GCM decrypt(key, nonce, ciphertext)
    |
    v
Ed25519 seed (32 bytes)
    |
    +---> SigningKey::from_bytes(seed)     -> sign operations
    +---> signing_key.verifying_key()      -> verify signatures
    |
    +---> HKDF-SHA256(seed, workspace_uuid, "krillnotes-db-password-v1")
              |
              v
         32-byte DB password key
              |
              v
         AES-256-GCM decrypt(key, nonce, encrypted_db_password)
              |
              v
         Plaintext DB password -> PRAGMA key
```

## IdentityManager API

```rust
pub struct IdentityManager {
    config_dir: PathBuf,  // ~/.config/krillnotes/
}

pub struct UnlockedIdentity {
    pub identity_uuid: Uuid,
    pub display_name: String,
    pub signing_key: SigningKey,
    pub verifying_key: VerifyingKey,
}
```

| Method | Purpose |
|--------|---------|
| `new(config_dir)` | Constructor; ensures `identities/` subdirectory exists |
| `create_identity(display_name, passphrase)` | Generate keypair, encrypt seed, write file, register in settings |
| `list_identities()` | Read from identity_settings.json |
| `unlock_identity(uuid, passphrase)` | Read file, Argon2id derive key, decrypt seed, return UnlockedIdentity |
| `change_passphrase(uuid, old, new)` | Unlock with old, re-encrypt with new salt + key, rewrite file |
| `delete_identity(uuid)` | Remove file + deregister. Fails if workspaces still bound |
| `bind_workspace(identity_uuid, workspace_uuid, db_path, db_password, seed)` | HKDF from seed -> encrypt DB password -> store in settings |
| `unbind_workspace(workspace_uuid)` | Remove binding from settings |
| `decrypt_db_password(workspace_uuid, seed)` | HKDF from seed -> decrypt stored blob |
| `get_workspaces_for_identity(identity_uuid)` | Filter settings by identity |

## New Cargo Dependencies

```toml
ed25519-dalek = { version = "2", features = ["rand_core"] }
argon2 = "0.5"
aes-gcm = "0.10"
```

Already present: `hkdf`, `sha2`, `rand`, `base64`, `uuid`, `serde`, `serde_json`, `chrono`.

## New Error Variants

```rust
IdentityNotFound(Uuid),
IdentityAlreadyExists(String),
IdentityLocked,
IdentityWrongPassphrase,
IdentityCorrupt(String),
IdentityHasBoundWorkspaces(Uuid),
WorkspaceNotBound(String),
```

## File Layout

```
~/.config/krillnotes/
├── settings.json              # Tauri AppSettings (unchanged)
├── identity_settings.json     # NEW — IdentityManager owns this
└── identities/
    ├── <uuid-1>.json
    └── <uuid-2>.json
```

## Testing Strategy

All tests in `identity.rs` `#[cfg(test)]` module using `TempDir` for the config directory.

| Test | Validates |
|------|-----------|
| `test_create_identity` | Keygen, file write, settings update, valid JSON |
| `test_unlock_identity` | Create -> unlock -> valid SigningKey |
| `test_wrong_passphrase` | Wrong passphrase -> error |
| `test_sign_and_verify` | Unlock -> sign -> verify with public key from file |
| `test_change_passphrase` | Change -> old fails -> new works -> same keypair |
| `test_list_identities` | Create 3 -> list returns all 3 |
| `test_delete_identity` | Delete -> file gone -> deregistered |
| `test_delete_with_bound_workspaces` | Bound workspace -> delete fails |
| `test_bind_workspace` | Bind -> decrypt -> matches original password |
| `test_db_password_roundtrip` | Encrypt -> decrypt -> match |
| `test_unbind_workspace` | Unbind -> decrypt fails |
| `test_multiple_identities_isolation` | Two identities -> each decrypts only own bindings |
| `test_identity_file_format` | Create -> read raw JSON -> matches spec structure |

Argon2id test params: `m_cost=1024`, `t_cost=1` via `#[cfg(test)]` const override for speed.
