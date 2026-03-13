# Design: Split `lib.rs` into Domain Command Modules

**Date:** 2026-03-13
**Scope:** `krillnotes-desktop/src-tauri/src/` — Rust backend only
**Phase:** 1 of 2 (command-only split; AppState decomposition deferred to a later phase)

## Problem

`src-tauri/src/lib.rs` is 4,421 lines containing 117 Tauri commands spanning unrelated domains (notes, identity, swarm/sync, scripts, attachments, workspace lifecycle, undo/redo, invites, contacts). This is the same pattern that was just resolved in `krillnotes-core` by splitting `workspace.rs` into a `workspace/` directory of focused modules.

## Goal

Split `lib.rs` into a `commands/` subdirectory of 9 domain modules while making zero functional changes. `lib.rs` becomes a thin orchestration layer. All existing Tauri commands retain their exact names and signatures.

## Non-Goals (Explicitly Deferred)

- **AppState decomposition**: The `AppState` struct (11 `Arc<Mutex>` fields) remains untouched in `lib.rs`. A future phase will decompose it into domain-specific state structs.
- **React / TypeScript refactoring**: Separate effort.
- **New tests**: The 4 existing tests in `lib.rs` will move to their new module; no new tests are written.

## Approach

**Option A — `commands/` subdirectory** (chosen)

Mirrors the `workspace/` directory pattern from `krillnotes-core`. A `commands/mod.rs` re-exports all command functions so `lib.rs` can use a single `use commands::*` and keep `generate_handler![...]` unchanged.

## Final Structure

```
src-tauri/src/
  lib.rs              # AppState + run() + generate_handler! (~300-400 lines)
  commands/
    mod.rs            # pub mod + pub use for all 9 domain modules
    workspace.rs      # workspace lifecycle + import/export + settings + themes
    notes.rs          # note CRUD + tags + operations log + undo/redo
    scripting.rs      # schema, tree actions, view rendering, field validation
    scripts.rs        # user script CRUD
    identity.rs       # identity lifecycle + passphrase + swarmid + binding info
    invites.rs        # invite flow + accept_peer
    contacts.rs       # contacts CRUD + peer management
    attachments.rs    # file attachment CRUD
    swarm.rs          # swarm/sync: open file, apply snapshot/delta, generate deltas
  locales.rs          # (unchanged)
  menu.rs             # (unchanged)
  settings.rs         # (unchanged — business logic only, no Tauri commands)
  themes.rs           # (unchanged — business logic only, no Tauri commands)
  main.rs             # (unchanged)
```

Note: `lib.rs` will be ~300-400 lines after the split due to `AppState` (~40 lines), its `Default` impl, private helper functions (`create_workspace_window`, `handle_file_opened`, `rebuild_menus`, etc. ~300 lines), and the `generate_handler!` block (~120 lines).

## Module Ownership — Complete Command List

### `commands/workspace.rs`
Commands:
`create_workspace`, `open_workspace`, `get_workspace_info`, `get_workspace_metadata`, `set_workspace_metadata`, `get_app_version`, `consume_pending_file_open`, `consume_pending_swarm_file`, `set_paste_menu_enabled`, `read_file_content`, `list_workspace_files`, `delete_workspace`, `duplicate_workspace`, `export_workspace_cmd`, `peek_import_cmd`, `execute_import`, `get_settings`, `update_settings`, `list_themes`, `read_theme`, `write_theme`, `delete_theme`

Types moved here: `WorkspaceInfo`

Note: `get_settings`/`update_settings` are thin wrappers around `crate::settings`; `list_themes`/`read_theme`/`write_theme`/`delete_theme` are thin wrappers around `crate::themes`. Both `settings.rs` and `themes.rs` remain unchanged.

### `commands/notes.rs`
Commands:
`list_notes`, `get_note`, `get_node_types`, `toggle_note_expansion`, `set_selected_note`, `create_note_with_type`, `update_note`, `save_note`, `search_notes`, `count_children`, `delete_note`, `move_note`, `deep_copy_note_cmd`, `update_note_tags`, `get_all_tags`, `get_notes_for_tag`, `list_operations`, `get_operation_detail`, `purge_operations`, `undo`, `redo`, `can_undo`, `can_redo`, `get_undo_limit`, `set_undo_limit`, `begin_undo_group`, `end_undo_group`, `script_undo`, `script_redo`, `can_script_undo`, `can_script_redo`

Rationale: undo/redo commands are note-mutation bookkeeping (~105 lines); not worth a separate file.

### `commands/scripting.rs`
Commands:
`get_schema_fields`, `get_all_schemas`, `get_tree_action_map`, `invoke_tree_action`, `get_note_view`, `get_note_hover`, `get_views_for_type`, `render_view`, `get_script_warnings`, `validate_field`, `validate_fields`, `evaluate_group_visibility`

### `commands/scripts.rs`
Commands:
`list_user_scripts`, `get_user_script`, `create_user_script`, `update_user_script`, `delete_user_script`, `toggle_user_script`, `reorder_user_script`, `reorder_all_user_scripts`

### `commands/identity.rs`
Commands:
`list_identities`, `resolve_identity_name`, `create_identity`, `unlock_identity`, `lock_identity`, `delete_identity`, `rename_identity`, `change_identity_passphrase`, `get_unlocked_identities`, `is_identity_unlocked`, `get_workspaces_for_identity`, `export_swarmid_cmd`, `get_identity_public_key`, `import_swarmid_cmd`, `import_swarmid_overwrite_cmd`

Types moved here: `WorkspaceBindingInfo`

### `commands/invites.rs`
Commands:
`list_invites`, `create_invite`, `revoke_invite`, `import_invite_response`, `import_invite`, `respond_to_invite`, `accept_peer`

Rationale: Invite flow is distinct from contact management — it handles the handshake protocol (creation, transport, acceptance) that *results* in a contact/peer relationship. `accept_peer` lives here rather than in `contacts.rs` because it is the final step of the invite flow.

### `commands/contacts.rs`
Commands:
`list_contacts`, `get_contact`, `create_contact`, `update_contact`, `delete_contact`, `get_fingerprint`, `list_workspace_peers`, `get_workspace_peers`, `remove_workspace_peer`, `add_contact_as_peer`

Types moved here: `ContactInfo`

### `commands/attachments.rs`
Commands:
`attach_file`, `attach_file_bytes`, `get_attachments`, `get_attachment_data`, `delete_attachment`, `restore_attachment`, `open_attachment`

### `commands/swarm.rs`
Commands:
`open_swarm_file_cmd`, `create_snapshot_for_peers`, `apply_swarm_snapshot`, `apply_swarm_delta`, `generate_deltas_for_peers`

Types moved here: `SwarmFileInfo` enum

## `lib.rs` After Split

```rust
// lib.rs — AppState + wiring + private helpers only

mod commands;
mod locales;
mod menu;
mod settings;
mod themes;

use commands::*;

pub struct AppState {
    // all 11 Arc<Mutex> fields — unchanged
}

// Private helper functions remain here (used by multiple command modules
// via crate::helper_name or passed as closures):
//   create_workspace_window(), handle_file_opened(), rebuild_menus(), etc.

pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            // all 117 commands, exactly as today — no names change
        ])
        .run(...)
}
```

## `commands/mod.rs`

```rust
pub mod attachments;
pub mod contacts;
pub mod identity;
pub mod invites;
pub mod notes;
pub mod scripting;
pub mod scripts;
pub mod swarm;
pub mod workspace;

pub use attachments::*;
pub use contacts::*;
pub use identity::*;
pub use invites::*;
pub use notes::*;
pub use scripting::*;
pub use scripts::*;
pub use swarm::*;
pub use workspace::*;
```

## Access Pattern

Each domain module accesses `AppState` via `crate::AppState`. Private helper functions in `lib.rs` that are needed by command modules are either:
- Made `pub(crate)` and called as `crate::helper_fn()`
- Or inlined into the command module if only used in one place

No circular dependencies — modules only reach *up* to the crate root, never sideways to each other.

```rust
// example: commands/notes.rs
use crate::AppState;
use tauri::State;

#[tauri::command]
pub async fn list_notes(state: State<'_, AppState>, ...) -> Result<Vec<Note>, String> {
    ...
}
```

## Cleanup Included in Scope

- Remove `read_info_json` (dead function, flagged by compiler at `lib.rs:2663`)
- The 4 existing tests at the bottom of `lib.rs` move to `commands/workspace.rs` (they test `read_file_content_impl` via `super::`)

## Success Criteria

1. `cargo test -p krillnotes-desktop` passes (all 4 existing tests)
2. `cargo build` succeeds with no new warnings
3. No command names or signatures change (zero frontend impact)
4. `read_info_json` dead code warning is gone
5. `lib.rs` is under 500 lines (accounting for AppState, private helpers, and `generate_handler!`)

## Out of Scope

- `menu.rs`, `settings.rs`, `themes.rs`, `locales.rs` — already focused, no changes
- Any React/TypeScript files
- AppState restructuring
