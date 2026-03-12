# Identity Storage Refactor Design

**Date:** 2026-03-13
**Status:** Approved
**Branch:** `feat/phase-d-snapshot` (implement in new worktree)

## Problem

`~/.config/krillnotes/` has two structural issues:

1. **Split identity layout** — each identity's key file (`identities/<uuid>.json`) lives alongside its data folder (`identities/<uuid>/contacts/`, `invites/`). The JSON and folder should be one unit.
2. **Workspace bindings in the wrong place** — `identity_settings.json` has a `workspaces` section mapping workspace UUIDs to encrypted DB passwords and identity links. This registry grows stale (entries for deleted workspaces persist), and conceptually the binding belongs with the workspace, not the identity registry.

## On-Disk Layout

### Before

```
~/.config/krillnotes/
├── identity_settings.json       ← identity list + workspace bindings (mixed concerns)
├── identities/
│   ├── <uuid>.json              ← key material (flat file)
│   ├── <uuid>/
│   │   ├── contacts/
│   │   └── invites/
│   └── ...
├── contacts/                    ← top-level, always empty (vestigial)
└── themes/

~/Documents/Krillnotes/<workspace>/
├── notes.db
├── info.json
└── attachments/
```

### After

```
~/.config/krillnotes/
├── identity_settings.json       ← identity list only (no workspaces key)
├── identities/
│   └── <uuid>/
│       ├── identity.json        ← key material (moved inside folder)
│       ├── contacts/
│       └── invites/
└── themes/

~/Documents/Krillnotes/<workspace>/
├── notes.db
├── info.json
├── attachments/
└── binding.json                 ← NEW: {identity_uuid, db_password_enc}
```

**Key changes:**
- `identities/<uuid>.json` → `identities/<uuid>/identity.json`
- `identity_settings.json.workspaces` → per-workspace `binding.json`
- Top-level `contacts/` folder deleted (empty, vestigial)
- `IdentityRef.file` updated from `"identities/<uuid>.json"` to `"identities/<uuid>/identity.json"`

## Migration

Runs once in `IdentityManager::new()` before any other operations. Idempotent — safe to re-run.

**Pass 1 — Identity files:**
For each `identities/<uuid>.json` flat file found:
1. Ensure `identities/<uuid>/` directory exists
2. Move `<uuid>.json` → `<uuid>/identity.json`
3. Update the `file` field in `identity_settings.json`

**Pass 2 — Workspace bindings:**
For each entry in `identity_settings.json.workspaces`:
1. Derive workspace folder from `db_path` (parent directory of the `.db` file)
2. If folder exists → write `binding.json` there: `{identity_uuid, db_password_enc}`
3. If folder is missing → drop silently (stale entry)
4. Remove `workspaces` key from `identity_settings.json` and save

## Data Structures

### `WorkspaceBinding` (simplified)

```rust
pub struct WorkspaceBinding {
    pub identity_uuid: Uuid,
    pub db_password_enc: String,  // base64(nonce || AES-256-GCM ciphertext)
}
```

`db_path` is removed — the folder is the lookup key, not the UUID.

### `IdentitySettings` transition

The `workspaces` field is kept in the struct during migration but annotated `#[serde(default, skip_serializing)]` so it is readable from old files but never written back. Once migration has run and the field is always empty, it can be removed from the struct in a follow-up cleanup. This avoids a deserialization failure if the app starts on a pre-migration `identity_settings.json` that still has a `workspaces` key.

```rust
pub struct IdentitySettings {
    pub identities: Vec<IdentityRef>,
    #[serde(default, skip_serializing)]   // readable from old files; never written
    pub workspaces: HashMap<String, LegacyWorkspaceBinding>,  // migration only
}
```

After migration runs, `workspaces` is always empty in memory and never written, so `identity_settings.json` on disk will no longer contain the key.

## IdentityManager API Changes

Methods whose **signatures change**:

```rust
// bind_workspace: workspace_dir replaces db_path
fn bind_workspace(
    identity_uuid: &Uuid,
    workspace_uuid: &str,
    workspace_dir: &Path,   // was: db_path: &str
    db_password: &str,
    seed: &[u8; 32],
) -> Result<()>

// get_workspace_binding: takes folder path, not UUID string
fn get_workspace_binding(workspace_dir: &Path) -> Result<Option<WorkspaceBinding>>

// decrypt_db_password: takes folder path, not UUID string
fn decrypt_db_password(workspace_dir: &Path, seed: &[u8; 32]) -> Result<String>

// get_workspaces_for_identity: scans workspace_base_dir/*/binding.json
// returns (folder_path, WorkspaceBinding) — UUID derivable from info.json if needed
fn get_workspaces_for_identity(
    identity_uuid: &Uuid,
    workspace_base_dir: &Path,
) -> Result<Vec<(PathBuf, WorkspaceBinding)>>

// unbind_workspace: deletes <workspace_dir>/binding.json
fn unbind_workspace(workspace_dir: &Path) -> Result<()>
```

Methods whose **internals change but signatures stay the same:**
`create_identity`, `unlock_identity`, `delete_identity`, `change_passphrase`, `rename_identity`, `export_swarmid`, `import_swarmid`, `import_swarmid_overwrite` — all update internal path from `identities/<uuid>.json` to `identities/<uuid>/identity.json`.

**New helper (eliminates raw `file` field usage in `lib.rs`):**
```rust
// Returns the absolute path to the identity key file for a given UUID.
// Replaces the pattern: config_dir.join(&identity_ref.file)
fn identity_file_path(&self, identity_uuid: &Uuid) -> PathBuf {
    self.config_dir.join("identities").join(identity_uuid.to_string()).join("identity.json")
}
```
Three call sites in `lib.rs` currently do `config_dir().join(&identity_ref.file)` directly (lines 2075, 3155, 3199 — in `get_identity_key_info` and `open_swarm_file_cmd`). These should be replaced with `mgr.identity_file_path(&identity_uuid)` to remove the dependency on the `file` field string.

## `lib.rs` Call Site Changes

All call sites already have the workspace folder path available:

| Call site | Change |
|-----------|--------|
| `create_workspace` | Pass `folder` to `bind_workspace` |
| `open_workspace` | Pass `folder` to `get_workspace_binding` and `decrypt_db_password` |
| `execute_import` | Pass `folder` to `bind_workspace` |
| `apply_swarm_snapshot` | Pass `folder` to `bind_workspace` |
| `list_workspace_files` | Pass `folder` to `get_workspace_binding` (already iterates folders) |
| `get_workspace_info_internal` (line 435) | Pass folder path from `state.workspace_paths` to `get_workspace_binding` instead of `workspace.workspace_id()` |
| `list_workspace_peers` (line 2248) | Pass `folder` from `state.workspace_paths` to `get_workspace_binding` |
| `lock_identity` | Pass `settings::load_settings().workspace_directory` to `get_workspaces_for_identity`; return type changes to `Vec<(PathBuf, WorkspaceBinding)>` so the UUID-based `HashSet` match becomes a folder-path-based match against `state.workspace_paths` |
| `get_identity_key_info` (line 2075) | Replace `config_dir().join(&identity_ref.file)` with `mgr.identity_file_path(&uuid)` |
| `open_swarm_file_cmd` (lines 3155, 3199) | Replace `config_dir().join(&identity_ref.file)` with `mgr.identity_file_path(&uuid)` (two branches: Invite and Snapshot) |
| `delete_workspace` | No `unbind_workspace` call needed — binding is deleted with the folder |
| `duplicate_workspace` | New binding created with new UUID/password — not copied from source |

## Error Handling

- Migration failures are non-fatal per pass: if a file move fails (e.g. permissions), log and continue
- `get_workspace_binding` returns `Ok(None)` if `binding.json` is missing (same as today for unbound workspaces)
- `decrypt_db_password` returns an error if `binding.json` is missing (workspace must be bound to be opened)

## Testing

Existing tests that use `bind_workspace`, `get_workspace_binding`, `decrypt_db_password`, and `get_workspaces_for_identity` need to be updated to pass folder paths. Migration logic needs a dedicated test: create a legacy config, call `IdentityManager::new()`, assert new layout exists and `workspaces` key is gone.
