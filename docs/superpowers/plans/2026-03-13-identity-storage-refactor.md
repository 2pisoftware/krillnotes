# Identity Storage Refactor Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Consolidate identity key files into identity folders and move workspace bindings from `identity_settings.json` into per-workspace `binding.json` files.

**Architecture:** All changes are in `krillnotes-core/src/core/identity.rs` (data structures + methods) and `krillnotes-desktop/src-tauri/src/lib.rs` (call site updates). Migration runs automatically in `IdentityManager::new()` and is idempotent. No changes to the frontend.

**Spec:** `docs/superpowers/specs/2026-03-13-identity-storage-refactor-design.md`

**Tech Stack:** Rust, serde_json, AES-256-GCM, HKDF-SHA256, tempfile (tests)

---

## Setup

- [ ] **Create worktree**

```bash
git -C /Users/careck/Source/Krillnotes worktree add .worktrees/feat/identity-storage-refactor -b feat/identity-storage-refactor feat/phase-d-snapshot
```

All subsequent work is in `/Users/careck/Source/Krillnotes/.worktrees/feat/identity-storage-refactor/`.

---

## Chunk 1: Data Structures

**Files:**
- Modify: `krillnotes-core/src/core/identity.rs`

### Task 1: Add `LegacyWorkspaceBinding`, replace `WorkspaceBinding`, update `IdentitySettings`

- [ ] **Step 1: Write failing tests for struct serialisation**

In `identity.rs`, inside the existing `#[cfg(test)]` block, add:

```rust
#[test]
fn old_identity_settings_with_workspaces_key_deserialises() {
    // Old format still deserialises (workspaces key is readable)
    let json = r#"{
        "identities": [],
        "workspaces": {
            "ws-uuid-1": {
                "db_path": "/tmp/foo/notes.db",
                "identity_uuid": "00000000-0000-0000-0000-000000000001",
                "db_password_enc": "aGVsbG8="
            }
        }
    }"#;
    let settings: IdentitySettings = serde_json::from_str(json).unwrap();
    assert_eq!(settings.workspaces.len(), 1);
    let binding = settings.workspaces.get("ws-uuid-1").unwrap();
    assert_eq!(binding.db_path, "/tmp/foo/notes.db");
}

#[test]
fn new_identity_settings_serialises_without_workspaces_key() {
    let settings = IdentitySettings::default();
    let json = serde_json::to_string(&settings).unwrap();
    assert!(!json.contains("workspaces"),
        "workspaces key must not appear in serialised output");
}

#[test]
fn workspace_binding_serialises_with_workspace_uuid() {
    let b = WorkspaceBinding {
        workspace_uuid: "ws-1".to_string(),
        identity_uuid: Uuid::nil(),
        db_password_enc: "enc".to_string(),
    };
    let json = serde_json::to_string(&b).unwrap();
    assert!(json.contains("workspace_uuid"));
    assert!(json.contains("identity_uuid"));
    assert!(!json.contains("db_path"));
}
```

- [ ] **Step 2: Run tests — expect compile errors (types don't exist yet)**

```bash
cd /Users/careck/Source/Krillnotes/.worktrees/feat/identity-storage-refactor
cargo test -p krillnotes-core -- identity 2>&1 | head -30
```

Expected: compile errors about missing `LegacyWorkspaceBinding` and wrong `WorkspaceBinding` shape.

- [ ] **Step 3: Rename existing `WorkspaceBinding` to `LegacyWorkspaceBinding`, add new `WorkspaceBinding`, update `IdentitySettings`**

In `identity.rs`, replace the `WorkspaceBinding` struct and `IdentitySettings` block (currently around lines 96-107):

```rust
/// Per-workspace binding stored in `<workspace_dir>/binding.json`.
/// `workspace_uuid` is included so callers can derive the HKDF key without
/// reading `info.json` separately.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceBinding {
    pub workspace_uuid: String,
    pub identity_uuid: Uuid,
    pub db_password_enc: String,
}

/// Legacy workspace binding as stored in `identity_settings.json.workspaces`.
/// Read-only during migration; never written after migration runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegacyWorkspaceBinding {
    pub db_path: String,
    pub identity_uuid: Uuid,
    pub db_password_enc: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IdentitySettings {
    #[serde(default)]
    pub identities: Vec<IdentityRef>,
    /// Migration-only: readable from old files, never written back.
    #[serde(default, skip_serializing)]
    pub workspaces: std::collections::HashMap<String, LegacyWorkspaceBinding>,
}
```

- [ ] **Step 4: Run tests — expect pass**

```bash
cargo test -p krillnotes-core -- identity 2>&1 | tail -20
```

Expected: all three new tests pass. Pre-existing tests may fail due to `WorkspaceBinding` shape changes — that is expected and will be fixed in later tasks.

- [ ] **Step 5: Commit**

```bash
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/identity-storage-refactor \
  add krillnotes-core/src/core/identity.rs && \
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/identity-storage-refactor \
  commit -m "refactor(identity): split WorkspaceBinding into new shape + LegacyWorkspaceBinding"
```

---

## Chunk 2: Identity File Path Migration (Pass 1)

**Files:**
- Modify: `krillnotes-core/src/core/identity.rs`

### Task 2: Add `identity_dir()` and `identity_file_path()` helpers

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn identity_dir_returns_uuid_subfolder() {
    let tmp = tempfile::tempdir().unwrap();
    let mgr = IdentityManager::new(tmp.path().to_path_buf()).unwrap();
    let uuid = Uuid::parse_str("aaaaaaaa-0000-0000-0000-000000000001").unwrap();
    assert_eq!(
        mgr.identity_dir(&uuid),
        tmp.path().join("identities").join("aaaaaaaa-0000-0000-0000-000000000001")
    );
}

#[test]
fn identity_file_path_returns_identity_json_inside_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let mgr = IdentityManager::new(tmp.path().to_path_buf()).unwrap();
    let uuid = Uuid::parse_str("aaaaaaaa-0000-0000-0000-000000000001").unwrap();
    assert_eq!(
        mgr.identity_file_path(&uuid),
        tmp.path().join("identities").join("aaaaaaaa-0000-0000-0000-000000000001").join("identity.json")
    );
}
```

- [ ] **Step 2: Run — expect compile errors**

```bash
cargo test -p krillnotes-core -- identity 2>&1 | head -20
```

- [ ] **Step 3: Add helpers to `IdentityManager` impl block**

Replace `identities_dir()` (currently around line 164) and add new helpers:

```rust
fn identities_dir(&self) -> PathBuf {
    self.config_dir.join("identities")
}

/// Returns the directory for a single identity (contains identity.json, contacts/, invites/).
pub fn identity_dir(&self, identity_uuid: &Uuid) -> PathBuf {
    self.identities_dir().join(identity_uuid.to_string())
}

/// Returns the absolute path to the identity key file for a given UUID.
/// Replaces the pattern: `config_dir.join(&identity_ref.file)` in lib.rs.
pub fn identity_file_path(&self, identity_uuid: &Uuid) -> PathBuf {
    self.identity_dir(identity_uuid).join("identity.json")
}
```

- [ ] **Step 4: Run — expect pass**

```bash
cargo test -p krillnotes-core -- identity 2>&1 | tail -10
```

- [ ] **Step 5: Commit**

```bash
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/identity-storage-refactor \
  add krillnotes-core/src/core/identity.rs && \
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/identity-storage-refactor \
  commit -m "refactor(identity): add identity_dir() and identity_file_path() helpers"
```

### Task 3: Migration Pass 1 — move flat `<uuid>.json` into `<uuid>/identity.json`

- [ ] **Step 1: Write failing test for Pass 1 migration**

```rust
#[test]
fn migration_pass1_moves_flat_json_into_identity_subfolder() {
    let tmp = tempfile::tempdir().unwrap();
    let config_dir = tmp.path().to_path_buf();
    let identities_dir = config_dir.join("identities");
    std::fs::create_dir_all(&identities_dir).unwrap();

    // Create legacy flat identity file
    let uuid = Uuid::new_v4();
    let legacy_path = identities_dir.join(format!("{uuid}.json"));
    let identity_file = serde_json::json!({
        "identity_uuid": uuid.to_string(),
        "display_name": "Test",
        "public_key": "dGVzdA==",
        "private_key_enc": {
            "ciphertext": "dGVzdA==",
            "nonce": "dGVzdA==",
            "kdf": "argon2id",
            "kdf_params": { "salt": "dGVzdA==", "m_cost": 1024, "t_cost": 1, "p_cost": 1 }
        }
    });
    std::fs::write(&legacy_path, serde_json::to_string(&identity_file).unwrap()).unwrap();

    // Create identity_settings.json referencing the flat file
    let settings = serde_json::json!({
        "identities": [{
            "uuid": uuid.to_string(),
            "displayName": "Test",
            "file": format!("identities/{uuid}.json"),
            "lastUsed": "2026-01-01T00:00:00Z"
        }]
    });
    std::fs::write(config_dir.join("identity_settings.json"),
        serde_json::to_string(&settings).unwrap()).unwrap();

    // Trigger migration
    let _mgr = IdentityManager::new(config_dir.clone()).unwrap();

    // Flat file must be gone
    assert!(!legacy_path.exists(), "flat file should be removed");

    // New path must exist
    let new_path = identities_dir.join(uuid.to_string()).join("identity.json");
    assert!(new_path.exists(), "identity.json inside folder must exist");

    // settings must be updated
    let raw = std::fs::read_to_string(config_dir.join("identity_settings.json")).unwrap();
    let updated: IdentitySettings = serde_json::from_str(&raw).unwrap();
    assert_eq!(updated.identities[0].file,
        format!("identities/{uuid}/identity.json"));
}

#[test]
fn migration_pass1_is_idempotent() {
    // Running new() twice must not fail or corrupt data
    let tmp = tempfile::tempdir().unwrap();
    let config_dir = tmp.path().to_path_buf();

    // First call (no legacy files) — should succeed silently
    let _m1 = IdentityManager::new(config_dir.clone()).unwrap();
    // Second call — must also succeed
    let _m2 = IdentityManager::new(config_dir.clone()).unwrap();
}
```

- [ ] **Step 2: Run — expect test to fail (migration not yet written)**

```bash
cargo test -p krillnotes-core -- migration_pass1 2>&1 | tail -15
```

- [ ] **Step 3: Add `migrate()` method with Pass 1 logic to `IdentityManager`**

Add this private method before `new()`:

```rust
/// Runs on-disk migrations. Called once from `new()`. Idempotent.
fn migrate(config_dir: &Path) {
    Self::migrate_pass1_identity_files(config_dir);
}

/// Pass 1: move flat `identities/<uuid>.json` → `identities/<uuid>/identity.json`.
fn migrate_pass1_identity_files(config_dir: &Path) {
    let identities_dir = config_dir.join("identities");
    let settings_path = config_dir.join("identity_settings.json");

    // Collect flat .json files (entries like `<uuid>.json` at root of identities/)
    let flat_files: Vec<(Uuid, std::path::PathBuf)> = match std::fs::read_dir(&identities_dir) {
        Ok(rd) => rd.flatten()
            .filter_map(|e| {
                let p = e.path();
                if p.is_file() && p.extension().map(|x| x == "json").unwrap_or(false) {
                    let stem = p.file_stem()?.to_str()?;
                    let uuid = Uuid::parse_str(stem).ok()?;
                    Some((uuid, p))
                } else {
                    None
                }
            })
            .collect(),
        Err(_) => return,
    };

    if flat_files.is_empty() { return; }

    // Load settings to update file refs
    let raw = match std::fs::read_to_string(&settings_path) {
        Ok(r) => r,
        Err(_) => return,
    };
    let mut settings: IdentitySettings = match serde_json::from_str(&raw) {
        Ok(s) => s,
        Err(_) => return,
    };

    let mut changed = false;
    for (uuid, src_path) in flat_files {
        let dest_dir = identities_dir.join(uuid.to_string());
        let dest_path = dest_dir.join("identity.json");

        if dest_path.exists() {
            // Already migrated — remove the now-orphaned flat file
            let _ = std::fs::remove_file(&src_path);
            changed = true;
            continue;
        }

        if let Err(e) = std::fs::create_dir_all(&dest_dir) {
            eprintln!("[migration] Cannot create {dest_dir:?}: {e}");
            continue;
        }
        if let Err(e) = std::fs::rename(&src_path, &dest_path) {
            eprintln!("[migration] Cannot move {src_path:?}: {e}");
            continue;
        }

        // Update IdentityRef.file in settings
        let new_file = format!("identities/{uuid}/identity.json");
        for id_ref in settings.identities.iter_mut() {
            if id_ref.uuid == uuid {
                id_ref.file = new_file.clone();
                break;
            }
        }
        changed = true;
    }

    if changed {
        if let Ok(json) = serde_json::to_string_pretty(&settings) {
            let _ = std::fs::write(&settings_path, json);
        }
    }
}
```

Update `new()` to call migrate:

```rust
pub fn new(config_dir: PathBuf) -> Result<Self> {
    let identities_dir = config_dir.join("identities");
    std::fs::create_dir_all(&identities_dir)?;
    Self::migrate(&config_dir);  // ← add this line
    Ok(Self { config_dir })
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p krillnotes-core -- migration_pass1 2>&1 | tail -10
```

Expected: both Pass 1 tests pass.

- [ ] **Step 5: Update all identity read/write methods to use new path**

Find every occurrence of the old path pattern (approximately `self.identities_dir().join(format!("{uuid}.json"))` or similar) in `identity.rs` and replace with `self.identity_file_path(&uuid)`. Also:

- `create_identity`: create `identities/<uuid>/` dir before writing key file, also create `contacts/` and `invites/` subdirs:

```rust
// In create_identity(), replace the write section:
let identity_dir = self.identity_dir(&identity.identity_uuid);
std::fs::create_dir_all(&identity_dir)?;
// Pre-create data subdirs so the identity folder is complete from the start
std::fs::create_dir_all(identity_dir.join("contacts"))?;
std::fs::create_dir_all(identity_dir.join("invites"))?;
let file_path = identity_dir.join("identity.json");
let json = serde_json::to_string_pretty(&identity)?;
std::fs::write(&file_path, json)?;

// Update the IdentityRef.file to new path
let file_ref = format!("identities/{}/identity.json", identity.identity_uuid);
```

- `delete_identity`: delete the entire identity dir (not just the `.json` file):

```rust
// Replace: std::fs::remove_file(&identity_file_path)?;
// With:
let identity_dir = self.identity_dir(identity_uuid);
if identity_dir.exists() {
    std::fs::remove_dir_all(&identity_dir)?;
}
```

Also update `delete_identity` signature to accept `workspace_base_dir: &Path` for the bound-workspace check (Pass 2 migration must run first — see Chunk 3):

```rust
pub fn delete_identity(&self, identity_uuid: &Uuid, workspace_base_dir: &Path) -> Result<()> {
    // Check no workspaces bound before proceeding
    let bound = self.get_workspaces_for_identity(identity_uuid, workspace_base_dir)?;
    if !bound.is_empty() {
        return Err(crate::KrillnotesError::IdentityHasBoundWorkspaces);
    }
    // ... rest unchanged except path fix above
}
```

- `change_passphrase`, `rename_identity`, `export_swarmid`, `unlock_identity`: replace old path with `self.identity_file_path(&uuid)`.

- `write_swarmid_to_store` (private helper used by `import_swarmid`/`import_swarmid_overwrite`):

```rust
// In write_swarmid_to_store(), create the identity dir before writing:
let identity_dir = self.identity_dir(&file.identity.identity_uuid);
std::fs::create_dir_all(&identity_dir)?;
std::fs::create_dir_all(identity_dir.join("contacts"))?;
std::fs::create_dir_all(identity_dir.join("invites"))?;
let file_path = identity_dir.join("identity.json");
// ... write json to file_path
let file_ref = format!("identities/{}/identity.json", file.identity.identity_uuid);
```

- [ ] **Step 6: Run core tests**

```bash
cargo test -p krillnotes-core 2>&1 | tail -20
```

Expected: all tests pass.

- [ ] **Step 7: Commit**

```bash
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/identity-storage-refactor \
  add krillnotes-core/src/core/identity.rs && \
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/identity-storage-refactor \
  commit -m "refactor(identity): migrate flat identity files into per-identity folders"
```

---

## Chunk 3: Workspace Binding Migration (Pass 2) and New Binding Methods

**Files:**
- Modify: `krillnotes-core/src/core/identity.rs`

### Task 4: Migration Pass 2 — move legacy bindings to `binding.json` files

- [ ] **Step 1: Write failing tests for Pass 2**

```rust
#[test]
fn migration_pass2_writes_binding_json_for_existing_workspace() {
    let tmp = tempfile::tempdir().unwrap();
    let config_dir = tmp.path().to_path_buf();

    // Create a fake workspace folder with notes.db
    let ws_dir = tmp.path().join("workspaces").join("my-workspace");
    std::fs::create_dir_all(&ws_dir).unwrap();
    std::fs::write(ws_dir.join("notes.db"), b"").unwrap();

    let ws_uuid = "aaaaaaaa-1111-0000-0000-000000000001";
    let identity_uuid = "bbbbbbbb-2222-0000-0000-000000000001";

    // Write legacy identity_settings.json with workspaces section
    let settings_json = serde_json::json!({
        "identities": [],
        "workspaces": {
            ws_uuid: {
                "db_path": ws_dir.join("notes.db").display().to_string(),
                "identity_uuid": identity_uuid,
                "db_password_enc": "dGVzdA=="
            }
        }
    });
    std::fs::write(
        config_dir.join("identity_settings.json"),
        serde_json::to_string(&settings_json).unwrap()
    ).unwrap();

    // Trigger migration
    std::fs::create_dir_all(config_dir.join("identities")).unwrap();
    let _mgr = IdentityManager::new(config_dir.clone()).unwrap();

    // binding.json must exist in workspace folder
    let binding_path = ws_dir.join("binding.json");
    assert!(binding_path.exists(), "binding.json must be written");

    let raw = std::fs::read_to_string(&binding_path).unwrap();
    let binding: WorkspaceBinding = serde_json::from_str(&raw).unwrap();
    assert_eq!(binding.workspace_uuid, ws_uuid);
    assert_eq!(binding.identity_uuid.to_string(), identity_uuid);
    assert_eq!(binding.db_password_enc, "dGVzdA==");

    // identity_settings.json must no longer have workspaces key
    let raw_settings = std::fs::read_to_string(config_dir.join("identity_settings.json")).unwrap();
    assert!(!raw_settings.contains("workspaces"),
        "workspaces key must be absent after migration");
}

#[test]
fn migration_pass2_drops_stale_entry_for_missing_workspace() {
    let tmp = tempfile::tempdir().unwrap();
    let config_dir = tmp.path().to_path_buf();
    std::fs::create_dir_all(config_dir.join("identities")).unwrap();

    // Stale binding — workspace folder does not exist
    let settings_json = serde_json::json!({
        "identities": [],
        "workspaces": {
            "dead-ws-uuid": {
                "db_path": "/nonexistent/workspace/notes.db",
                "identity_uuid": "00000000-0000-0000-0000-000000000001",
                "db_password_enc": "dGVzdA=="
            }
        }
    });
    std::fs::write(
        config_dir.join("identity_settings.json"),
        serde_json::to_string(&settings_json).unwrap()
    ).unwrap();

    // Must not panic
    let _mgr = IdentityManager::new(config_dir.clone()).unwrap();

    // No binding.json created anywhere
    // identity_settings.json cleaned up
    let raw = std::fs::read_to_string(config_dir.join("identity_settings.json")).unwrap();
    assert!(!raw.contains("workspaces"));
}
```

- [ ] **Step 2: Run — expect failures**

```bash
cargo test -p krillnotes-core -- migration_pass2 2>&1 | tail -15
```

- [ ] **Step 3: Add Pass 2 to `migrate()`**

Add this private method:

```rust
/// Pass 2: migrate workspace bindings from `identity_settings.json.workspaces`
/// into per-workspace `binding.json` files.
fn migrate_pass2_workspace_bindings(config_dir: &Path) {
    let settings_path = config_dir.join("identity_settings.json");
    let raw = match std::fs::read_to_string(&settings_path) {
        Ok(r) => r,
        Err(_) => return,
    };
    let mut settings: IdentitySettings = match serde_json::from_str(&raw) {
        Ok(s) => s,
        Err(_) => return,
    };

    if settings.workspaces.is_empty() { return; }

    for (ws_uuid, legacy) in &settings.workspaces {
        // Derive workspace folder from db_path (parent of the .db file)
        let workspace_dir = std::path::Path::new(&legacy.db_path)
            .parent()
            .map(|p| p.to_path_buf());

        let workspace_dir = match workspace_dir {
            Some(d) if d.is_dir() => d,
            _ => {
                eprintln!("[migration] Workspace folder missing for {ws_uuid}, dropping binding");
                continue;
            }
        };

        let binding = WorkspaceBinding {
            workspace_uuid: ws_uuid.clone(),
            identity_uuid: legacy.identity_uuid,
            db_password_enc: legacy.db_password_enc.clone(),
        };
        let binding_path = workspace_dir.join("binding.json");
        match serde_json::to_string_pretty(&binding) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&binding_path, json) {
                    eprintln!("[migration] Cannot write binding.json to {binding_path:?}: {e}");
                }
            }
            Err(e) => eprintln!("[migration] Cannot serialise binding for {ws_uuid}: {e}"),
        }
    }

    // Clear workspaces from settings regardless (stale entries are dropped)
    settings.workspaces.clear();
    if let Ok(json) = serde_json::to_string_pretty(&settings) {
        let _ = std::fs::write(&settings_path, json);
    }
}
```

Update `migrate()` to call Pass 2 after Pass 1:

```rust
fn migrate(config_dir: &Path) {
    Self::migrate_pass1_identity_files(config_dir);
    Self::migrate_pass2_workspace_bindings(config_dir);
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p krillnotes-core -- migration_pass2 2>&1 | tail -10
```

Expected: both Pass 2 tests pass.

- [ ] **Step 5: Commit**

```bash
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/identity-storage-refactor \
  add krillnotes-core/src/core/identity.rs && \
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/identity-storage-refactor \
  commit -m "refactor(identity): migration pass 2 — move workspace bindings to binding.json"
```

### Task 5: Replace workspace binding methods with folder-based API

- [ ] **Step 1: Write failing tests for new binding methods**

```rust
#[test]
fn bind_and_get_workspace_binding_round_trips() {
    let tmp = tempfile::tempdir().unwrap();
    let config_dir = tmp.path().to_path_buf();
    std::fs::create_dir_all(config_dir.join("identities")).unwrap();
    let mgr = IdentityManager::new(config_dir).unwrap();

    let identity_uuid = Uuid::new_v4();
    let workspace_uuid = Uuid::new_v4().to_string();
    let workspace_dir = tmp.path().join("ws");
    std::fs::create_dir_all(&workspace_dir).unwrap();

    let seed = [42u8; 32];
    let password = "hunter2";

    mgr.bind_workspace(&identity_uuid, &workspace_uuid, &workspace_dir, password, &seed).unwrap();

    // binding.json must exist
    assert!(workspace_dir.join("binding.json").exists());

    let binding = mgr.get_workspace_binding(&workspace_dir).unwrap().unwrap();
    assert_eq!(binding.workspace_uuid, workspace_uuid);
    assert_eq!(binding.identity_uuid, identity_uuid);

    // Decrypt round-trip
    let decrypted = mgr.decrypt_db_password(&workspace_dir, &seed).unwrap();
    assert_eq!(decrypted, password);
}

#[test]
fn get_workspace_binding_returns_none_when_no_binding_json() {
    let tmp = tempfile::tempdir().unwrap();
    let ws_dir = tmp.path().join("ws");
    std::fs::create_dir_all(&ws_dir).unwrap();

    let config_dir = tmp.path().join("config");
    std::fs::create_dir_all(config_dir.join("identities")).unwrap();
    let mgr = IdentityManager::new(config_dir).unwrap();

    assert!(mgr.get_workspace_binding(&ws_dir).unwrap().is_none());
}

#[test]
fn get_workspaces_for_identity_scans_workspace_base_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let config_dir = tmp.path().join("config");
    std::fs::create_dir_all(config_dir.join("identities")).unwrap();
    let mgr = IdentityManager::new(config_dir).unwrap();

    let identity_a = Uuid::new_v4();
    let identity_b = Uuid::new_v4();
    let ws_base = tmp.path().join("workspaces");

    // Two workspaces for identity_a, one for identity_b
    for (name, owner) in &[("ws1", identity_a), ("ws2", identity_a), ("ws3", identity_b)] {
        let ws_dir = ws_base.join(name);
        std::fs::create_dir_all(&ws_dir).unwrap();
        let binding = WorkspaceBinding {
            workspace_uuid: Uuid::new_v4().to_string(),
            identity_uuid: *owner,
            db_password_enc: "enc".to_string(),
        };
        std::fs::write(
            ws_dir.join("binding.json"),
            serde_json::to_string(&binding).unwrap()
        ).unwrap();
    }
    // ws4 has no binding.json — must be ignored
    std::fs::create_dir_all(ws_base.join("ws4")).unwrap();

    let results = mgr.get_workspaces_for_identity(&identity_a, &ws_base).unwrap();
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|(_, b)| b.identity_uuid == identity_a));
}

#[test]
fn unbind_workspace_removes_binding_json() {
    let tmp = tempfile::tempdir().unwrap();
    let ws_dir = tmp.path().join("ws");
    std::fs::create_dir_all(&ws_dir).unwrap();
    let binding_path = ws_dir.join("binding.json");
    std::fs::write(&binding_path, "{}").unwrap();

    let config_dir = tmp.path().join("config");
    std::fs::create_dir_all(config_dir.join("identities")).unwrap();
    let mgr = IdentityManager::new(config_dir).unwrap();

    mgr.unbind_workspace(&ws_dir).unwrap();
    assert!(!binding_path.exists());
}
```

- [ ] **Step 2: Run — expect compile errors / failures**

```bash
cargo test -p krillnotes-core -- bind_and_get_workspace 2>&1 | head -20
```

- [ ] **Step 3: Replace workspace binding methods**

Replace `bind_workspace`, `unbind_workspace`, `decrypt_db_password`, `get_workspace_binding`, `get_workspaces_for_identity` in `identity.rs`:

```rust
/// Encrypts `db_password` with `seed` and writes a `binding.json` into `workspace_dir`.
pub fn bind_workspace(
    &self,
    identity_uuid: &Uuid,
    workspace_uuid: &str,
    workspace_dir: &Path,
    db_password: &str,
    seed: &[u8; 32],
) -> Result<()> {
    let db_password_enc = self.encrypt_db_password(seed, workspace_uuid, db_password)?;
    let binding = WorkspaceBinding {
        workspace_uuid: workspace_uuid.to_string(),
        identity_uuid: *identity_uuid,
        db_password_enc,
    };
    let json = serde_json::to_string_pretty(&binding)?;
    std::fs::write(workspace_dir.join("binding.json"), json)?;
    Ok(())
}

/// Removes `binding.json` from `workspace_dir`. Returns `Ok(())` if already absent.
pub fn unbind_workspace(&self, workspace_dir: &Path) -> Result<()> {
    let path = workspace_dir.join("binding.json");
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

/// Reads `<workspace_dir>/binding.json`. Returns `None` if the file is absent.
pub fn get_workspace_binding(&self, workspace_dir: &Path) -> Result<Option<WorkspaceBinding>> {
    let path = workspace_dir.join("binding.json");
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)?;
    let binding: WorkspaceBinding = serde_json::from_str(&raw)
        .map_err(|e| crate::KrillnotesError::IdentityCorrupt(
            format!("binding.json in {:?}: {e}", workspace_dir)
        ))?;
    Ok(Some(binding))
}

/// Decrypts the DB password from `<workspace_dir>/binding.json`.
/// Uses `workspace_uuid` stored in the binding for HKDF key derivation.
pub fn decrypt_db_password(&self, workspace_dir: &Path, seed: &[u8; 32]) -> Result<String> {
    let binding = self.get_workspace_binding(workspace_dir)?
        .ok_or_else(|| crate::KrillnotesError::IdentityCorrupt(
            format!("no binding.json in {:?}", workspace_dir)
        ))?;

    let key = Self::derive_db_password_key(seed, &binding.workspace_uuid);
    let blob = BASE64.decode(&binding.db_password_enc)
        .map_err(|e| crate::KrillnotesError::IdentityCorrupt(format!("db_password_enc: {e}")))?;

    if blob.len() < 12 {
        return Err(crate::KrillnotesError::IdentityCorrupt("db_password_enc too short".into()));
    }
    let (nonce_bytes, ciphertext) = blob.split_at(12);
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| crate::KrillnotesError::IdentityCorrupt(e.to_string()))?;
    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = cipher.decrypt(nonce, ciphertext)
        .map_err(|_| crate::KrillnotesError::IdentityCorrupt("decrypt failed".into()))?;
    String::from_utf8(plaintext)
        .map_err(|e| crate::KrillnotesError::IdentityCorrupt(e.to_string()))
}

/// Scans all subdirectories of `workspace_base_dir` for `binding.json` files
/// that belong to `identity_uuid`. Returns `(workspace_folder, WorkspaceBinding)` pairs.
pub fn get_workspaces_for_identity(
    &self,
    identity_uuid: &Uuid,
    workspace_base_dir: &Path,
) -> Result<Vec<(std::path::PathBuf, WorkspaceBinding)>> {
    let mut results = Vec::new();
    let entries = match std::fs::read_dir(workspace_base_dir) {
        Ok(rd) => rd,
        Err(_) => return Ok(results), // directory doesn't exist yet
    };
    for entry in entries.flatten() {
        let folder = entry.path();
        if !folder.is_dir() { continue; }
        let binding_path = folder.join("binding.json");
        if !binding_path.exists() { continue; }
        if let Ok(raw) = std::fs::read_to_string(&binding_path) {
            if let Ok(b) = serde_json::from_str::<WorkspaceBinding>(&raw) {
                if b.identity_uuid == *identity_uuid {
                    results.push((folder, b));
                }
            }
        }
    }
    Ok(results)
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p krillnotes-core 2>&1 | tail -20
```

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/identity-storage-refactor \
  add krillnotes-core/src/core/identity.rs && \
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/identity-storage-refactor \
  commit -m "refactor(identity): folder-based workspace binding methods"
```

---

## Chunk 4: `lib.rs` Call Site Updates

**Files:**
- Modify: `krillnotes-desktop/src-tauri/src/lib.rs`

### Task 6: Update all call sites in lib.rs

- [ ] **Step 1: Check that lib.rs fails to compile (changed API surfaces)**

```bash
cargo check -p krillnotes-desktop 2>&1 | grep "^error" | head -20
```

Expected: multiple errors for wrong argument counts/types on `bind_workspace`, `get_workspace_binding`, `decrypt_db_password`, `get_workspaces_for_identity`, `delete_identity`.

- [ ] **Step 2: Fix `get_workspace_info_internal` (line ~435)**

```rust
// Before:
let identity_uuid = state.identity_manager.lock().expect("Mutex poisoned")
    .get_workspace_binding(workspace.workspace_id())
    .ok()
    .flatten()
    .map(|b| b.identity_uuid.to_string());

// After (path is already in scope as the workspace folder):
let identity_uuid = state.identity_manager.lock().expect("Mutex poisoned")
    .get_workspace_binding(path.as_path())
    .ok()
    .flatten()
    .map(|b| b.identity_uuid.to_string());
```

- [ ] **Step 3: Fix `create_workspace` (line ~510)**

```rust
// Before:
mgr.bind_workspace(
    &uuid,
    &workspace_uuid,
    &db_path.display().to_string(),   // ← db_path string
    &password,
    &seed,
)

// After:
mgr.bind_workspace(
    &uuid,
    &workspace_uuid,
    &folder,                           // ← workspace folder PathBuf
    &password,
    &seed,
)
```

- [ ] **Step 4: Fix `open_workspace` (lines ~576, ~595)**

```rust
// Step 1 — get identity_uuid from binding:
// Before:
let binding = mgr.get_workspace_binding(&workspace_uuid)
// After:
let binding = mgr.get_workspace_binding(&folder)

// Step 3 — decrypt:
// Before:
mgr.decrypt_db_password(&workspace_uuid, &seed)
// After:
mgr.decrypt_db_password(&folder, &seed)
```

Note: `open_workspace` reads `workspace_uuid` from `info.json` to error with `IDENTITY_REQUIRED` if absent. That check stays, but `workspace_uuid` is no longer passed to binding methods. The binding file itself contains the `workspace_uuid`.

- [ ] **Step 5: Fix `execute_import` (line ~1708)**

```rust
// Before:
mgr.bind_workspace(
    &uuid,
    &workspace_uuid,
    &db_path_buf.display().to_string(),   // ← db_path string
    &workspace_password,
    &seed,
)

// After:
mgr.bind_workspace(
    &uuid,
    &workspace_uuid,
    &folder,                              // ← workspace folder
    &workspace_password,
    &seed,
)
```

- [ ] **Step 6: Fix `apply_swarm_snapshot` (line ~3731)**

```rust
// Before:
mgr.bind_workspace(
    &identity_uuid_parsed,
    &workspace_uuid,
    &db_path.display().to_string(),   // ← db_path string
    &workspace_password,
    &seed,
)

// After:
mgr.bind_workspace(
    &identity_uuid_parsed,
    &workspace_uuid,
    &folder,                           // ← workspace folder (already in scope)
    &workspace_password,
    &seed,
)
```

- [ ] **Step 7: Fix `list_workspace_files` (line ~2690)**

```rust
// Before:
if let Ok(Some(binding)) = mgr.get_workspace_binding(ws_id) {

// After (pass folder instead of ws_id string — folder is already in scope):
if let Ok(Some(binding)) = mgr.get_workspace_binding(&folder) {
```

- [ ] **Step 8: Fix `list_workspace_peers` (line ~2248)**

```rust
// Before:
let workspace_uuid = ws_uuid_opt.ok_or("Workspace UUID missing from info.json")?;
let mgr = state.identity_manager.lock().expect("Mutex poisoned");
mgr.get_workspace_binding(&workspace_uuid)

// After (folder is already derived from workspace_paths above):
let mgr = state.identity_manager.lock().expect("Mutex poisoned");
mgr.get_workspace_binding(&folder)
```

- [ ] **Step 9: Fix `lock_identity` (line ~1915)**

```rust
// Before:
let bound_workspaces = mgr.get_workspaces_for_identity(&uuid)
    .map_err(|e| e.to_string())?;
let bound_workspace_ids: std::collections::HashSet<String> = bound_workspaces.into_iter()
    .map(|(ws_uuid, _)| ws_uuid)
    .collect();
drop(mgr);

let workspaces = state.workspaces.lock().expect("Mutex poisoned");
let labels_to_close: Vec<String> = workspaces.iter()
    .filter(|(_, ws)| bound_workspace_ids.contains(ws.workspace_id()))
    .map(|(label, _)| label.clone())
    .collect();

// After:
let workspace_base_dir = PathBuf::from(&settings::load_settings().workspace_directory);
let bound_folders: std::collections::HashSet<PathBuf> =
    mgr.get_workspaces_for_identity(&uuid, &workspace_base_dir)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|(folder, _)| folder)
        .collect();
drop(mgr);

let labels_to_close: Vec<String> = state.workspace_paths.lock()
    .expect("Mutex poisoned")
    .iter()
    .filter(|(_, path)| bound_folders.contains(*path))
    .map(|(label, _)| label.clone())
    .collect();
```

- [ ] **Step 9a: Fix `get_workspaces_for_identity` Tauri command and `WorkspaceBindingInfo` struct (lines ~89-95, ~2017-2029)**

`WorkspaceBindingInfo` currently exposes `db_path` (copied from the old `WorkspaceBinding.db_path`). After the refactor, `WorkspaceBinding` no longer has `db_path`; the workspace folder is the lookup key and `workspace_uuid` comes from the binding itself.

Update the struct (around line 89):

```rust
// Before:
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceBindingInfo {
    pub workspace_uuid: String,
    pub identity_uuid: String,
    pub db_path: String,
}

// After (db_path replaced with folder_path, which is what callers need):
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceBindingInfo {
    pub workspace_uuid: String,
    pub identity_uuid: String,
    pub folder_path: String,
}
```

Update the Tauri command body (around lines 2017-2029):

```rust
// Before (iterates Vec<(String, WorkspaceBinding)>, accesses binding.db_path):
let workspace_base_dir = PathBuf::from(&settings::load_settings().workspace_directory);
let result = mgr.get_workspaces_for_identity(&uuid, &workspace_uuid_str, ...)
    .map(|(ws_uuid, binding)| WorkspaceBindingInfo {
        workspace_uuid: ws_uuid,
        identity_uuid: binding.identity_uuid.to_string(),
        db_path: binding.db_path,
    })

// After (iterates Vec<(PathBuf, WorkspaceBinding)>):
let workspace_base_dir = PathBuf::from(&settings::load_settings().workspace_directory);
let bindings = mgr
    .get_workspaces_for_identity(&uuid, &workspace_base_dir)
    .map_err(|e| e.to_string())?;
let result: Vec<WorkspaceBindingInfo> = bindings
    .into_iter()
    .map(|(folder, binding)| WorkspaceBindingInfo {
        workspace_uuid: binding.workspace_uuid,
        identity_uuid: binding.identity_uuid.to_string(),
        folder_path: folder.display().to_string(),
    })
    .collect();
```

If the TypeScript side reads `WorkspaceBindingInfo.dbPath`, update `types.ts` to replace `dbPath: string` with `folderPath: string` and fix any usage.

- [ ] **Step 10: Fix `get_identity_public_key` (line ~2075)**

```rust
// Before:
let full_path = crate::settings::config_dir().join(&identity_ref.file);

// After:
let full_path = mgr.identity_file_path(&identity_ref.uuid);
```

- [ ] **Step 11: Fix `open_swarm_file_cmd` — Invite branch (line ~3154) and Snapshot branch (line ~3197)**

For each branch, replace:
```rust
// Before:
let full_path = crate::settings::config_dir().join(&identity_ref.file);

// After:
let full_path = mgr.identity_file_path(&identity_ref.uuid);
```

- [ ] **Step 12: Fix `duplicate_workspace` — source reads (lines ~2782, ~2801) and dest bind (line ~2862)**

`duplicate_workspace` reads the source binding to get the identity UUID and decrypt the source DB password before binding the new destination workspace. Both source-side calls and the destination bind call need updating.

```rust
// Source-side — line ~2782: read source workspace binding
// Before:
let binding = mgr.get_workspace_binding(&ws_uuid)  // ws_uuid: &str (old UUID key)
// After (source_folder is already a PathBuf in scope pointing to the source workspace directory):
let binding = mgr.get_workspace_binding(&source_folder)

// Source-side — line ~2801: decrypt source DB password
// Before:
let source_password = mgr.decrypt_db_password(&ws_uuid, &seed)?;
// After:
let source_password = mgr.decrypt_db_password(&source_folder, &seed)?;

// Destination bind — line ~2862: write new binding for duplicated workspace
// Before:
mgr.bind_workspace(
    &uuid,
    &new_ws_uuid,
    &dest_db.display().to_string(),   // ← db_path string
    &new_password,
    &seed,
)
// After (dest_folder is already a PathBuf in scope):
mgr.bind_workspace(
    &uuid,
    &new_ws_uuid,
    &dest_folder,                     // ← workspace folder
    &new_password,
    &seed,
)
```

- [ ] **Step 13: Fix `delete_identity` call in lib.rs**

Find the `delete_identity` call (search for `mgr.delete_identity`) and add `workspace_base_dir`:

```rust
// Before:
mgr.delete_identity(&uuid)

// After:
let workspace_base_dir = PathBuf::from(&settings::load_settings().workspace_directory);
mgr.delete_identity(&uuid, &workspace_base_dir)
```

- [ ] **Step 14: Verify compile**

```bash
cargo check -p krillnotes-desktop 2>&1 | grep "^error" | head -20
```

Expected: no errors. One pre-existing warning about `read_info_json` being unused is OK.

- [ ] **Step 15: Run core tests**

```bash
cargo test -p krillnotes-core 2>&1 | tail -10
```

Expected: all pass.

- [ ] **Step 16: Commit**

```bash
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/identity-storage-refactor \
  add krillnotes-desktop/src-tauri/src/lib.rs && \
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/identity-storage-refactor \
  commit -m "refactor(identity): update all lib.rs call sites for folder-based binding API"
```

---

## Chunk 5: Cleanup and PR

### Task 7: Remove vestigial top-level `contacts/` directory and verify end-to-end

- [ ] **Step 1: Delete the top-level `contacts/` directory from the config dir at runtime**

In `IdentityManager::new()`, add after migration:

```rust
// Remove vestigial top-level contacts/ dir (empty, superseded by per-identity folders)
let legacy_contacts = config_dir.join("contacts");
if legacy_contacts.is_dir() {
    let is_empty = std::fs::read_dir(&legacy_contacts)
        .map(|mut d| d.next().is_none())
        .unwrap_or(false);
    if is_empty {
        let _ = std::fs::remove_dir(&legacy_contacts);
    }
}
```

- [ ] **Step 2: Run all tests**

```bash
cargo test -p krillnotes-core 2>&1 | tail -15
```

Expected: all pass.

- [ ] **Step 3: TypeScript type check**

```bash
cd /Users/careck/Source/Krillnotes/.worktrees/feat/identity-storage-refactor/krillnotes-desktop && npx tsc --noEmit 2>&1
```

Expected: no errors.

- [ ] **Step 4: Full compile check**

```bash
cd /Users/careck/Source/Krillnotes/.worktrees/feat/identity-storage-refactor && cargo check -p krillnotes-desktop 2>&1 | grep "^error"
```

Expected: no errors.

- [ ] **Step 5: Final commit**

```bash
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/identity-storage-refactor \
  add krillnotes-core/src/core/identity.rs && \
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/identity-storage-refactor \
  commit -m "refactor(identity): remove vestigial top-level contacts/ dir on startup"
```

- [ ] **Step 6: Push and open PR**

```bash
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/identity-storage-refactor \
  push -u github-https feat/identity-storage-refactor

gh pr create \
  --base master \
  --head feat/identity-storage-refactor \
  --title "refactor(identity): consolidate identity folders and move workspace bindings to binding.json" \
  --body "$(cat <<'EOF'
## Summary
- Moves `identities/<uuid>.json` into `identities/<uuid>/identity.json` so each identity has a single folder containing key material, contacts, and invites
- Replaces `identity_settings.json.workspaces` with per-workspace `binding.json` files, eliminating stale entries
- Auto-migrates existing configs on first launch (idempotent)
- Adds `identity_file_path()` helper to `IdentityManager`, removing raw path construction from `lib.rs`

## Test plan
- [ ] All `cargo test -p krillnotes-core` pass
- [ ] `npx tsc --noEmit` clean
- [ ] `cargo check -p krillnotes-desktop` clean
- [ ] Manually launch app — existing identities and workspaces open correctly after migration
- [ ] Check `~/.config/krillnotes/`: flat `.json` files gone, `binding.json` present in each workspace folder
EOF
)"
```
