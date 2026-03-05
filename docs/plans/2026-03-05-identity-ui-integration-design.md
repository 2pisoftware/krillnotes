# Identity UI Integration — Design Document

**Date:** 2026-03-05
**Status:** Approved
**Depends on:** Identity Model Foundation (PR #69, merged to `swarm`)
**Spec reference:** `docs/swarm/section_11_identity_model.md`

## Summary

Wire the `IdentityManager` foundation into the Tauri layer and React frontend. After this PR, identities replace per-workspace passwords entirely. Users authenticate with an identity passphrase; DB passwords are auto-generated, encrypted to the identity seed, and invisible. Multiple identities can be unlocked simultaneously — a personal workspace and a work workspace can be open side by side.

**Scope:** Tauri commands, AppState changes, identity manager UI, workspace creation/open flow rework, first-launch onboarding.

**Out of scope:** `.krillid` export/import (follow-up PR), OS keychain integration (commercial/future), identity switching (not needed — multi-identity replaces it).

## Design Decisions

### Multi-identity over single-identity

The spec (§11.5) describes "one active identity at a time" with explicit switching. We deviate: multiple identities can be unlocked concurrently in `AppState`. This is architecturally simpler (no switch/wipe/reopen cycle) and better UX (work + personal side by side).

`AppState` stores `HashMap<Uuid, UnlockedIdentity>` instead of `Option<UnlockedIdentity>`. The crypto layer (`IdentityManager`) is already stateless — each `unlock_identity` call returns an independent `UnlockedIdentity`.

### On-demand identity unlock (no startup picker)

Instead of a mandatory identity picker at app launch, identities unlock on demand when the user opens a workspace whose identity isn't yet unlocked. This avoids a speed bump for the common case (one identity, already unlocked from a previous workspace open).

First launch (zero identities) is the exception: forces identity creation before anything else.

### Workspace UUID in `info.json`

Workspaces need a stable UUID for identity bindings (the key in `identity_settings.json`). Storing it in the SQLCipher DB would require unlocking to read — but the workspace manager must show workspace-to-identity mappings before any identity is unlocked. `info.json` is already read by `list_workspace_files` without opening the DB.

### Clean break — no backward compatibility

No migration from password-only workspaces. All workspaces must be bound to an identity. Acceptable because there are no production users yet.

### DB passwords are invisible

Users never see, type, or choose a DB password. At workspace creation: generate random 32 bytes → open SQLCipher → bind to identity (HKDF encrypt). The identity passphrase is the only credential.

## AppState Changes

```rust
pub struct AppState {
    // NEW
    pub identity_manager: Arc<Mutex<IdentityManager>>,
    pub unlocked_identities: Arc<Mutex<HashMap<Uuid, UnlockedIdentity>>>,

    // EXISTING (unchanged)
    pub workspaces: Arc<Mutex<HashMap<String, Workspace>>>,
    pub workspace_paths: Arc<Mutex<HashMap<String, PathBuf>>>,
    pub focused_window: Arc<Mutex<Option<String>>>,
    pub paste_menu_items: Arc<Mutex<HashMap<String, (MenuItem, MenuItem)>>>,
    pub workspace_menu_items: Arc<Mutex<HashMap<String, Vec<MenuItem>>>>,
    pub pending_file_open: Arc<Mutex<Option<PathBuf>>>,

    // REMOVED
    // workspace_passwords — replaced by identity system
}
```

`IdentityManager` is initialized once at Tauri setup with the platform config directory. `unlocked_identities` starts empty; identities are added as users unlock them.

## Tauri Commands

### New commands (10)

| Command | Params | Returns | Purpose |
|---------|--------|---------|---------|
| `list_identities` | — | `Vec<IdentityRef>` | Populate identity manager |
| `create_identity` | `display_name, passphrase` | `IdentityRef` | Create + auto-unlock |
| `unlock_identity` | `identity_uuid, passphrase` | `()` | Unlock → store in AppState |
| `lock_identity` | `identity_uuid` | `()` | Wipe seed, close associated windows |
| `delete_identity` | `identity_uuid` | `()` | Must be locked, no bound workspaces |
| `rename_identity` | `identity_uuid, new_name` | `()` | Update display name in file + settings |
| `change_passphrase` | `identity_uuid, old_passphrase, new_passphrase` | `()` | Re-encrypt seed |
| `get_unlocked_identities` | — | `Vec<Uuid>` | Frontend checks unlock state |
| `get_workspaces_for_identity` | `identity_uuid` | `Vec<WorkspaceBinding>` | Filter workspace list |
| `is_identity_unlocked` | `identity_uuid` | `bool` | Quick check before workspace open |

### Modified commands

- **`create_workspace(path, identity_uuid)`** — removes `password` param. Generates random 32-byte DB password, creates workspace, binds to identity.
- **`open_workspace(path)`** — removes `password` param. Looks up workspace UUID from `info.json`, finds identity binding, decrypts DB password from unlocked identity's seed. Errors if identity is locked.

### Removed commands

- **`get_cached_password`** — replaced by identity system.

## New Core API: `rename_identity`

Add to `IdentityManager`:

```rust
pub fn rename_identity(&self, identity_uuid: &Uuid, new_name: &str) -> Result<()>
```

Updates `display_name` in both the identity file and the settings registry.

## Workspace UUID

Generated at workspace creation, stored in `info.json`:

```json
{
  "workspace_uuid": "550e8400-e29b-41d4-a716-446655440000",
  "created_at": 1709312400,
  "note_count": 42,
  "attachment_count": 7
}
```

`list_workspace_files` already reads `info.json` for every workspace. The UUID is the key used in `identity_settings.json` to map workspace → identity → encrypted DB password.

## Frontend Components

### New

| Component | Purpose |
|-----------|---------|
| `IdentityManagerDialog.tsx` | Identity list with create/delete/rename/change-passphrase/lock/unlock. Accessed from launcher menu + app menu. |
| `CreateIdentityDialog.tsx` | Name + passphrase + confirm. First-launch welcome screen + identity manager "new" action. |
| `UnlockIdentityDialog.tsx` | Passphrase prompt for a specific identity. Shown when opening a workspace whose identity is locked. |

### Modified

| Component | Changes |
|-----------|---------|
| `App.tsx` | Startup: `list_identities()` → if empty, show `CreateIdentityDialog`. Add "Manage Identities" menu handler. |
| `WorkspaceManagerDialog.tsx` | Add identity filter dropdown. Show identity display name + lock/unlock icon per workspace. Replace `EnterPasswordDialog` with `UnlockIdentityDialog`. |
| `NewWorkspaceDialog.tsx` | Remove `SetPasswordDialog` step. Add identity selector (defaults to most recently used unlocked identity). |

### Removed

| Component | Why |
|-----------|-----|
| `EnterPasswordDialog.tsx` | Replaced by `UnlockIdentityDialog` |
| `SetPasswordDialog.tsx` | DB passwords are auto-generated |

## App Flow

### First launch (no identities)

```
App launches → main window → CreateIdentityDialog (forced)
  → User enters display name + passphrase
  → create_identity() → auto-unlock
  → Launcher shows empty workspace list
  → User creates first workspace (bound to new identity)
```

### Normal launch

```
App launches → main window → launcher
  → WorkspaceManager shows all workspaces (filterable by identity)
  → User clicks "Open"
    → Identity already unlocked? → open_workspace() → done
    → Identity locked? → UnlockIdentityDialog → passphrase → unlock → open_workspace()
```

### Creating a workspace

```
NewWorkspaceDialog: name + identity selector (unlocked identities only)
  → create_workspace(name, identity_uuid)
  → Backend: random password → SQLCipher → bind_workspace → open window
  → User never sees a password
```

### Locking an identity

```
IdentityManagerDialog → click "Lock" on an identity
  → lock_identity(uuid)
  → Backend: wipe seed from HashMap, close all workspace windows bound to this identity
  → Frontend: workspace windows disappear, workspace list updates lock icons
```

## Identity Manager Dialog

```
┌─────────────────────────────────────┐
│  Identities                    [+]  │
│─────────────────────────────────────│
│  🔓 Carsten @ 2pi        ⋮ menu    │
│  🔒 Carsten K             ⋮ menu    │
│  🔓 Treasurer, RC         ⋮ menu    │
│─────────────────────────────────────│
│                          [Close]    │
└─────────────────────────────────────┘

⋮ menu options:
  - Unlock (if locked) / Lock (if unlocked)
  - Rename
  - Change Passphrase
  - Delete (disabled if workspaces bound)
```

## Error Handling

| Scenario | Behavior |
|----------|----------|
| Wrong passphrase | `UnlockIdentityDialog` shows inline error, user retries |
| Open workspace with locked identity | Frontend intercepts, shows `UnlockIdentityDialog` first |
| Delete identity with bound workspaces | Error from core; frontend shows message |
| Lock identity with open workspaces | Close windows first, then wipe seed |
| Corrupt identity file | Error surfaced in identity manager list |

## i18n

New translation keys for all identity-related strings (identity manager dialog title, create/unlock/lock/delete/rename labels, error messages, first-launch welcome text). Added to all 7 locale files.
