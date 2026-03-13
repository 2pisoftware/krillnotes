# Desktop Rust Split: lib.rs → commands/ Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split `krillnotes-desktop/src-tauri/src/lib.rs` (4,421 lines, 117 Tauri commands) into a `commands/` subdirectory of 9 focused domain modules with zero functional changes.

**Architecture:** Create `src-tauri/src/commands/` mirroring the `krillnotes-core/src/core/workspace/` pattern. `lib.rs` becomes a thin orchestration layer holding only `AppState`, `run()`, and module declarations. Each domain module is independently `pub(crate)` and re-exported via `commands/mod.rs`. Private helpers currently in `lib.rs` move to `commands/workspace.rs` (where their callers live) and become `pub(crate)`.

**Tech Stack:** Rust, Tauri v2, `tauri::State<'_, AppState>`, `pub(crate)` visibility

**Spec:** `docs/superpowers/specs/2026-03-13-desktop-rust-split-design.md`

---

## Important Conventions

- Every `#[tauri::command]` function must be `pub` (for re-export through `commands/mod.rs`)
- Every module needs: `use crate::AppState;` and `use tauri::State;`
- Since `lib.rs` does `pub use krillnotes_core::*;`, core types are available as `crate::TypeName` from within command modules
- After each task, run `cargo build -p krillnotes-desktop` to verify — do not batch moves
- The 4 existing tests live at the bottom of `lib.rs` and use `super::read_file_content_impl`; they move to `commands/workspace.rs` as `mod tests { ... }` — `super::` will still resolve correctly there because `read_file_content_impl` is the private impl helper for the `read_file_content` command, and it moves to `commands/workspace.rs` with that command
- `read_info_json` at line 2663 is dead code — remove it during Task 12
- **Private helpers move to `commands/workspace.rs`**: The spec's "Access Pattern" section mentions helpers staying in `lib.rs`, but this plan takes the cleaner approach of moving ALL private helpers to `commands/workspace.rs` (where their callers live). `lib.rs`'s `run()` calls them as `commands::workspace::handle_file_opened(...)` etc. This is consistent with making `lib.rs` a truly thin orchestration layer.
- **`handle_menu_event` and `MENU_MESSAGES` stay in `lib.rs`**: This function only reads `state.focused_window` and emits events. It uses no private helpers and belongs logically with `run()`. Do not move it.
- **`generate_handler!` does not need changes**: `use commands::*;` in `lib.rs` brings all `pub` command functions into scope by their flat names, so the `generate_handler![create_workspace, list_notes, ...]` list is unchanged.
- **Wildcard re-export collision risk**: `commands/mod.rs` uses `pub use module::*` for all 9 modules. If two modules export a name that collides, the build fails. If this occurs, replace the offending `pub use module::*` line in `mod.rs` with explicit `pub use module::{TypeA, TypeB, fn_name};` re-exports for that module.

---

## Chunk 1: Setup and Scaffold

### Task 1: Create worktree

- [ ] **Create the worktree and branch**

```bash
git -C /Users/careck/Source/Krillnotes worktree add .worktrees/feat/split-desktop-rust -b feat/split-desktop-rust
```

- [ ] **Verify worktree exists**

```bash
ls /Users/careck/Source/Krillnotes/.worktrees/feat/split-desktop-rust/krillnotes-desktop/src-tauri/src/lib.rs
```

Expected: file path printed, no error.

All subsequent steps run from within the worktree:
```bash
cd /Users/careck/Source/Krillnotes/.worktrees/feat/split-desktop-rust
```

---

### Task 2: Create commands/ scaffold

**Files:**
- Create: `krillnotes-desktop/src-tauri/src/commands/mod.rs`
- Create: `krillnotes-desktop/src-tauri/src/commands/workspace.rs` (empty stub)
- Create: `krillnotes-desktop/src-tauri/src/commands/notes.rs` (empty stub)
- Create: `krillnotes-desktop/src-tauri/src/commands/scripting.rs` (empty stub)
- Create: `krillnotes-desktop/src-tauri/src/commands/scripts.rs` (empty stub)
- Create: `krillnotes-desktop/src-tauri/src/commands/identity.rs` (empty stub)
- Create: `krillnotes-desktop/src-tauri/src/commands/invites.rs` (empty stub)
- Create: `krillnotes-desktop/src-tauri/src/commands/contacts.rs` (empty stub)
- Create: `krillnotes-desktop/src-tauri/src/commands/attachments.rs` (empty stub)
- Create: `krillnotes-desktop/src-tauri/src/commands/swarm.rs` (empty stub)
- Modify: `krillnotes-desktop/src-tauri/src/lib.rs` (add `mod commands; use commands::*;`)

- [ ] **Create `commands/mod.rs`**

```rust
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2024-2026 TripleACS Pty Ltd t/a 2pi Software

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

- [ ] **Create 9 empty stub files** — each with just the license header:

```rust
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2024-2026 TripleACS Pty Ltd t/a 2pi Software
```

Create this content in each of: `commands/workspace.rs`, `commands/notes.rs`, `commands/scripting.rs`, `commands/scripts.rs`, `commands/identity.rs`, `commands/invites.rs`, `commands/contacts.rs`, `commands/attachments.rs`, `commands/swarm.rs`.

- [ ] **Add `mod commands;` to `lib.rs`**

In `lib.rs`, the existing module declarations are at the top (lines 13-16):
```rust
pub mod locales;
pub mod menu;
pub mod settings;
pub mod themes;
```

Add `mod commands;` and `use commands::*;` after them:
```rust
pub mod locales;
pub mod menu;
pub mod settings;
pub mod themes;

mod commands;
use commands::*;
```

- [ ] **Verify the scaffold compiles**

```bash
cargo build -p krillnotes-desktop 2>&1 | head -30
```

Expected: build succeeds (lib.rs still has all the functions; the empty stubs add nothing yet).

- [ ] **Commit the scaffold**

```bash
git add krillnotes-desktop/src-tauri/src/commands/
git add krillnotes-desktop/src-tauri/src/lib.rs
git commit -m "refactor(desktop): scaffold commands/ module directory"
```

---

## Chunk 2: Leaf Modules (scripting, scripts, attachments)

These three modules have no dependency on the private helpers that currently live in `lib.rs`. They are the safest to move first.

### Task 3: Move scripting commands

**Files:**
- Populate: `krillnotes-desktop/src-tauri/src/commands/scripting.rs`
- Modify: `krillnotes-desktop/src-tauri/src/lib.rs` (remove moved functions)

The 12 commands to move: `get_schema_fields`, `get_all_schemas`, `get_tree_action_map`, `invoke_tree_action`, `get_note_view`, `get_note_hover`, `get_views_for_type`, `render_view`, `get_script_warnings`, `validate_field`, `validate_fields`, `evaluate_group_visibility`.

- [ ] **Find the line ranges for scripting commands in `lib.rs`**

Use `#[tauri::command]` as the anchor — each command is preceded by this attribute:
```bash
grep -n "#\[tauri::command\]" krillnotes-desktop/src-tauri/src/lib.rs
```

Then read the block around each of the 12 scripting command names. Alternatively, search by function name:
```bash
grep -n "fn get_schema_fields\|fn get_all_schemas\|fn get_tree_action_map\|fn invoke_tree_action\|fn get_note_view\|fn get_note_hover\|fn get_views_for_type\|fn render_view\|fn get_script_warnings\|fn validate_field\b\|fn validate_fields\|fn evaluate_group_visibility" krillnotes-desktop/src-tauri/src/lib.rs
```

- [ ] **Cut all 12 scripting functions from `lib.rs` and paste into `commands/scripting.rs`**

Add these imports at the top of `commands/scripting.rs`:
```rust
use crate::AppState;
use tauri::State;
```

All functions in this module only need access to `state.workspaces` — no private helpers from `lib.rs` are used.

Make each function `pub` (add `pub` keyword before `async fn`).

- [ ] **Verify build**

```bash
cargo build -p krillnotes-desktop 2>&1 | grep -E "^error" | head -20
```

Expected: zero errors. If duplicate symbol errors appear, the functions were not fully removed from `lib.rs`. If wildcard name collision errors appear, see the "Wildcard re-export collision risk" note in Important Conventions.

- [ ] **Commit**

```bash
git add krillnotes-desktop/src-tauri/src/commands/scripting.rs
git add krillnotes-desktop/src-tauri/src/lib.rs
git commit -m "refactor(desktop): move scripting commands to commands/scripting.rs"
```

---

### Task 4: Move scripts commands

**Files:**
- Populate: `krillnotes-desktop/src-tauri/src/commands/scripts.rs`
- Modify: `krillnotes-desktop/src-tauri/src/lib.rs`

The 8 commands to move: `list_user_scripts`, `get_user_script`, `create_user_script`, `update_user_script`, `delete_user_script`, `toggle_user_script`, `reorder_user_script`, `reorder_all_user_scripts`.

- [ ] **Find line ranges in `lib.rs`**

```bash
grep -n "user_script\|reorder_user_script\|reorder_all_user_scripts" krillnotes-desktop/src-tauri/src/lib.rs | grep "#\[tauri::command\]\|^async fn\|^pub async fn" | head -20
```

- [ ] **Cut all 8 functions from `lib.rs`, paste into `commands/scripts.rs`**

Add imports:
```rust
use crate::AppState;
use tauri::State;
```

Make each function `pub`.

- [ ] **Verify build**

```bash
cargo build -p krillnotes-desktop 2>&1 | grep "^error" | head -20
```

- [ ] **Commit**

```bash
git add krillnotes-desktop/src-tauri/src/commands/scripts.rs
git add krillnotes-desktop/src-tauri/src/lib.rs
git commit -m "refactor(desktop): move user script commands to commands/scripts.rs"
```

---

### Task 5: Move attachments commands

**Files:**
- Populate: `krillnotes-desktop/src-tauri/src/commands/attachments.rs`
- Modify: `krillnotes-desktop/src-tauri/src/lib.rs`

The 7 commands to move: `attach_file`, `attach_file_bytes`, `get_attachments`, `get_attachment_data`, `delete_attachment`, `restore_attachment`, `open_attachment`.

- [ ] **Find line ranges**

```bash
grep -n "attach_file\|get_attachments\|get_attachment_data\|delete_attachment\|restore_attachment\|open_attachment" krillnotes-desktop/src-tauri/src/lib.rs | grep "#\[tauri\|^async fn\|^pub async" | head -20
```

- [ ] **Cut all 7 functions from `lib.rs`, paste into `commands/attachments.rs`**

Add imports:
```rust
use crate::AppState;
use tauri::State;
use tauri::AppHandle;
```

Check if any attachment command uses `AppHandle` (e.g., `open_attachment` likely calls `app.shell()` or similar). Add imports as needed.

Make each function `pub`.

- [ ] **Verify build**

```bash
cargo build -p krillnotes-desktop 2>&1 | grep "^error" | head -20
```

- [ ] **Commit**

```bash
git add krillnotes-desktop/src-tauri/src/commands/attachments.rs
git add krillnotes-desktop/src-tauri/src/lib.rs
git commit -m "refactor(desktop): move attachment commands to commands/attachments.rs"
```

---

## Chunk 3: Identity Domain (identity, invites, contacts)

### Task 6: Move identity commands + WorkspaceBindingInfo type

**Files:**
- Populate: `krillnotes-desktop/src-tauri/src/commands/identity.rs`
- Modify: `krillnotes-desktop/src-tauri/src/lib.rs`

The 15 commands to move: `list_identities`, `resolve_identity_name`, `create_identity`, `unlock_identity`, `lock_identity`, `delete_identity`, `rename_identity`, `change_identity_passphrase`, `get_unlocked_identities`, `is_identity_unlocked`, `get_workspaces_for_identity`, `export_swarmid_cmd`, `get_identity_public_key`, `import_swarmid_cmd`, `import_swarmid_overwrite_cmd`.

The type to move: `WorkspaceBindingInfo` struct (currently at lines 93-99 in `lib.rs`).

- [ ] **Find line ranges for identity commands**

```bash
grep -n "list_identities\|resolve_identity_name\|create_identity\|unlock_identity\|lock_identity\|delete_identity\|rename_identity\|change_identity_passphrase\|get_unlocked_identities\|is_identity_unlocked\|get_workspaces_for_identity\|export_swarmid_cmd\|get_identity_public_key\|import_swarmid_cmd\|import_swarmid_overwrite_cmd" krillnotes-desktop/src-tauri/src/lib.rs | grep "#\[tauri\|^async fn\|^pub async" | head -30
```

- [ ] **Cut `WorkspaceBindingInfo` from `lib.rs`, paste into `commands/identity.rs`**

The struct definition (from `lib.rs` lines 93-99):
```rust
/// Information about a workspace bound to an identity, returned to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceBindingInfo {
    pub workspace_uuid: String,
    pub folder_path: String,
}
```

- [ ] **Cut all 15 identity command functions from `lib.rs`, paste into `commands/identity.rs`**

Add imports at the top of `commands/identity.rs`:
```rust
use crate::AppState;
use tauri::State;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use std::path::PathBuf;
```

Make each function `pub`.

- [ ] **Verify build**

```bash
cargo build -p krillnotes-desktop 2>&1 | grep "^error" | head -20
```

Expected: zero errors. `WorkspaceBindingInfo` is already in scope everywhere in `lib.rs` via `use commands::*` (which re-exports `identity::*`). No additional re-export is needed.

- [ ] **Commit**

```bash
git add krillnotes-desktop/src-tauri/src/commands/identity.rs
git add krillnotes-desktop/src-tauri/src/lib.rs
git commit -m "refactor(desktop): move identity commands to commands/identity.rs"
```

---

### Task 7: Move invites commands

**Files:**
- Populate: `krillnotes-desktop/src-tauri/src/commands/invites.rs`
- Modify: `krillnotes-desktop/src-tauri/src/lib.rs`

The 7 commands to move: `list_invites`, `create_invite`, `revoke_invite`, `import_invite_response`, `import_invite`, `respond_to_invite`, `accept_peer`.

- [ ] **Find line ranges**

```bash
grep -n "list_invites\|create_invite\|revoke_invite\|import_invite_response\|import_invite\b\|respond_to_invite\|accept_peer" krillnotes-desktop/src-tauri/src/lib.rs | grep "#\[tauri\|^async fn\|^pub async" | head -20
```

- [ ] **Cut all 7 functions from `lib.rs`, paste into `commands/invites.rs`**

Add imports:
```rust
use crate::AppState;
use tauri::State;
use uuid::Uuid;
```

Make each function `pub`.

- [ ] **Verify build**

```bash
cargo build -p krillnotes-desktop 2>&1 | grep "^error" | head -20
```

- [ ] **Commit**

```bash
git add krillnotes-desktop/src-tauri/src/commands/invites.rs
git add krillnotes-desktop/src-tauri/src/lib.rs
git commit -m "refactor(desktop): move invite commands to commands/invites.rs"
```

---

### Task 8: Move contacts commands + ContactInfo type

**Files:**
- Populate: `krillnotes-desktop/src-tauri/src/commands/contacts.rs`
- Modify: `krillnotes-desktop/src-tauri/src/lib.rs`

The 10 commands to move: `list_contacts`, `get_contact`, `create_contact`, `update_contact`, `delete_contact`, `get_fingerprint`, `list_workspace_peers`, `get_workspace_peers`, `remove_workspace_peer`, `add_contact_as_peer`.

The type + impl to move: `ContactInfo` struct (lines 101-128 in `lib.rs`) and its `from_contact` method. Also move `trust_level_to_str` helper function (used only by `ContactInfo::from_contact` — find it with `grep -n trust_level_to_str lib.rs`).

- [ ] **Find `trust_level_to_str` and `ContactInfo` in `lib.rs`**

```bash
grep -n "trust_level_to_str\|ContactInfo\|fn from_contact" krillnotes-desktop/src-tauri/src/lib.rs | head -20
```

- [ ] **Cut `ContactInfo` + `impl ContactInfo` + `trust_level_to_str` from `lib.rs`, paste into `commands/contacts.rs`**

Add imports:
```rust
use crate::AppState;
use tauri::State;
use serde::Serialize;
use uuid::Uuid;
```

- [ ] **Cut all 10 contact/peer command functions from `lib.rs`, paste into `commands/contacts.rs`**

Make each function `pub`.

- [ ] **Verify build**

```bash
cargo build -p krillnotes-desktop 2>&1 | grep "^error" | head -20
```

- [ ] **Commit**

```bash
git add krillnotes-desktop/src-tauri/src/commands/contacts.rs
git add krillnotes-desktop/src-tauri/src/lib.rs
git commit -m "refactor(desktop): move contact + peer commands to commands/contacts.rs"
```

---

## Chunk 4: Notes and Swarm

### Task 9: Move notes commands (note CRUD + tags + operations log + undo/redo)

**Files:**
- Populate: `krillnotes-desktop/src-tauri/src/commands/notes.rs`
- Modify: `krillnotes-desktop/src-tauri/src/lib.rs`

The 31 commands to move: `list_notes`, `get_note`, `get_node_types`, `toggle_note_expansion`, `set_selected_note`, `create_note_with_type`, `update_note`, `save_note`, `search_notes`, `count_children`, `delete_note`, `move_note`, `deep_copy_note_cmd`, `update_note_tags`, `get_all_tags`, `get_notes_for_tag`, `list_operations`, `get_operation_detail`, `purge_operations`, `undo`, `redo`, `can_undo`, `can_redo`, `get_undo_limit`, `set_undo_limit`, `begin_undo_group`, `end_undo_group`, `script_undo`, `script_redo`, `can_script_undo`, `can_script_redo`.

- [ ] **Find the line ranges for note/undo/operations commands**

```bash
grep -n "^async fn list_notes\|^async fn get_note\b\|^async fn get_node_types\|^async fn toggle_note\|^async fn set_selected_note\|^async fn create_note\|^async fn update_note\|^async fn save_note\|^async fn search_notes\|^async fn count_children\|^async fn delete_note\|^async fn move_note\|^async fn deep_copy\|^async fn update_note_tags\|^async fn get_all_tags\|^async fn get_notes_for_tag\|^async fn list_operations\|^async fn get_operation_detail\|^async fn purge_operations\|^async fn undo\b\|^async fn redo\b\|^async fn can_undo\|^async fn can_redo\|^async fn get_undo_limit\|^async fn set_undo_limit\|^async fn begin_undo_group\|^async fn end_undo_group\|^async fn script_undo\|^async fn script_redo\|^async fn can_script_undo\|^async fn can_script_redo" krillnotes-desktop/src-tauri/src/lib.rs
```

- [ ] **Cut all 31 functions from `lib.rs`, paste into `commands/notes.rs`**

Add imports:
```rust
use crate::AppState;
use tauri::State;
use serde_json::Value;
```

Check if any note command uses `AppHandle` — add if needed.

Make each function `pub`.

- [ ] **Verify build**

```bash
cargo build -p krillnotes-desktop 2>&1 | grep "^error" | head -20
```

- [ ] **Commit**

```bash
git add krillnotes-desktop/src-tauri/src/commands/notes.rs
git add krillnotes-desktop/src-tauri/src/lib.rs
git commit -m "refactor(desktop): move note/tag/operations/undo commands to commands/notes.rs"
```

---

### Task 10: Move swarm commands + SwarmFileInfo type

**Files:**
- Populate: `krillnotes-desktop/src-tauri/src/commands/swarm.rs`
- Modify: `krillnotes-desktop/src-tauri/src/lib.rs`

The 5 commands to move: `open_swarm_file_cmd`, `create_snapshot_for_peers`, `apply_swarm_snapshot`, `apply_swarm_delta`, `generate_deltas_for_peers`.

The type to move: `SwarmFileInfo` enum (find with `grep -n "SwarmFileInfo" lib.rs`).

- [ ] **Find `SwarmFileInfo` and swarm command line ranges**

```bash
grep -n "SwarmFileInfo\|open_swarm_file_cmd\|create_snapshot_for_peers\|apply_swarm_snapshot\|apply_swarm_delta\|generate_deltas_for_peers" krillnotes-desktop/src-tauri/src/lib.rs | head -20
```

- [ ] **Cut `SwarmFileInfo` enum from `lib.rs`, paste into `commands/swarm.rs`**

`SwarmFileInfo` likely derives `Serialize`/`Deserialize` — check the definition and copy the `#[derive(...)]` attribute.

- [ ] **Cut all 5 swarm command functions from `lib.rs`, paste into `commands/swarm.rs`**

Add imports:
```rust
use crate::AppState;
use tauri::{State, AppHandle};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;
```

Make each function `pub`.

- [ ] **Verify build**

```bash
cargo build -p krillnotes-desktop 2>&1 | grep "^error" | head -20
```

- [ ] **Commit**

```bash
git add krillnotes-desktop/src-tauri/src/commands/swarm.rs
git add krillnotes-desktop/src-tauri/src/lib.rs
git commit -m "refactor(desktop): move swarm/sync commands to commands/swarm.rs"
```

---

## Chunk 5: Workspace Module + Final Cleanup

### Task 11: Move workspace commands, private helpers, and WorkspaceInfo type

This is the largest and final command move. All remaining `#[tauri::command]` functions and all private helpers (lines 134-456) move to `commands/workspace.rs`.

**Files:**
- Populate: `krillnotes-desktop/src-tauri/src/commands/workspace.rs`
- Modify: `krillnotes-desktop/src-tauri/src/lib.rs`

**Commands to move (22):** `create_workspace`, `open_workspace`, `get_workspace_info`, `get_workspace_metadata`, `set_workspace_metadata`, `get_app_version`, `consume_pending_file_open`, `consume_pending_swarm_file`, `set_paste_menu_enabled`, `read_file_content`, `list_workspace_files`, `delete_workspace`, `duplicate_workspace`, `export_workspace_cmd`, `peek_import_cmd`, `execute_import`, `get_settings`, `update_settings`, `list_themes`, `read_theme`, `write_theme`, `delete_theme`.

**Private helpers to move (as `pub(crate)`):** `generate_unique_label`, `find_window_for_path`, `focus_window`, `handle_file_opened`, `handle_krillnotes_open`, `handle_swarm_open`, `create_main_window`, `create_workspace_window`, `rebuild_menus`, `store_workspace` (private fn, not command), `get_workspace_info_internal`.

**Type to move:** `WorkspaceInfo` struct (lines 77-91 in `lib.rs`).

**Tests to move:** The 4 tests currently in `lib.rs:4385-4421` that test `read_file_content_impl` — move them as a `mod tests { ... }` block at the bottom of `commands/workspace.rs`. `read_file_content_impl` is the private implementation helper called by the `read_file_content` command; it moves as part of that command's block, so `super::read_file_content_impl` resolves correctly from the tests.

**Stays in `lib.rs`:** `handle_menu_event`, `MENU_MESSAGES` constant — these are event-routing code used only in `run()`, have no dependency on private helpers, and belong with the Tauri application entry point.

- [ ] **Find all remaining functions in `lib.rs` after Tasks 3–10**

At this point, `lib.rs` should only contain: `AppState` struct, `WorkspaceInfo`, `WorkspaceBindingInfo` (if not already moved), `ContactInfo` (if not already moved), private helpers (lines 134-456), the 22 workspace commands, and `run()`. Verify:

```bash
grep -n "#\[tauri::command\]" krillnotes-desktop/src-tauri/src/lib.rs | wc -l
```

Expected: 22 (just the workspace commands remain).

- [ ] **Cut `WorkspaceInfo` from `lib.rs`, paste into `commands/workspace.rs`**

```rust
/// Serialisable summary of an open workspace, returned to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceInfo {
    pub filename: String,
    pub path: String,
    pub note_count: usize,
    pub selected_note_id: Option<String>,
    pub identity_uuid: Option<String>,
}
```

- [ ] **Cut all 11 private helper functions from `lib.rs`, paste into `commands/workspace.rs`**

Change their visibility from `fn` to `pub(crate) fn`:
- `pub(crate) fn generate_unique_label(...)`
- `pub(crate) fn find_window_for_path(...)`
- `pub(crate) fn focus_window(...)`
- `pub(crate) fn handle_file_opened(...)`
- `pub(crate) fn handle_krillnotes_open(...)`
- `pub(crate) fn handle_swarm_open(...)`
- `pub(crate) fn create_main_window(...)`
- `pub(crate) fn create_workspace_window(...)`
- `pub(crate) fn rebuild_menus(...)`
- `pub(crate) fn store_workspace(...)`
- `pub(crate) fn get_workspace_info_internal(...)`

- [ ] **Update `run()` in `lib.rs` to use the moved helpers**

In `run()`, the calls to `handle_file_opened(...)` and `create_main_window(...)` must become:
```rust
commands::workspace::handle_file_opened(app_handle, &state, path);
```
and
```rust
commands::workspace::create_main_window(app_handle);
```

Also, in `handle_menu_event` (if it calls any helper), update similarly.

- [ ] **Cut all 22 workspace command functions from `lib.rs`, paste into `commands/workspace.rs`**

Add imports at top of `commands/workspace.rs`:
```rust
use crate::AppState;
use crate::{locales, menu, settings, themes};
use tauri::{AppHandle, Emitter, Manager, State};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;
```

Make each command function `pub`.

- [ ] **Move the 4 tests to `commands/workspace.rs`**

Cut the `#[cfg(test)] mod tests { ... }` block from `lib.rs` (lines 4385–4421) and paste it at the bottom of `commands/workspace.rs`. The tests use `super::read_file_content_impl` — `super::` will resolve correctly since the tests are inside `commands/workspace.rs`.

- [ ] **Verify build**

```bash
cargo build -p krillnotes-desktop 2>&1 | grep "^error" | head -30
```

Expected: zero errors. Common issues to fix:
- If `WorkspaceInfo` is referenced in `lib.rs` somewhere that didn't move, add `use commands::workspace::WorkspaceInfo;` in `lib.rs` or rely on `use commands::*;` (it should already be re-exported).
- If any helper in `lib.rs::run()` calls a function that moved, update the call path.

- [ ] **Commit**

```bash
git add krillnotes-desktop/src-tauri/src/commands/workspace.rs
git add krillnotes-desktop/src-tauri/src/lib.rs
git commit -m "refactor(desktop): move workspace commands and helpers to commands/workspace.rs"
```

---

### Task 12: Final cleanup — strip lib.rs, remove dead code, verify tests

**Files:**
- Modify: `krillnotes-desktop/src-tauri/src/lib.rs`

At this point `lib.rs` should only contain:
- License header and module doc comment
- `pub mod locales; pub mod menu; pub mod settings; pub mod themes;`
- `mod commands; use commands::*;`
- `pub use krillnotes_core::*;`
- Standard library imports
- `AppState` struct
- `run()` function (with `generate_handler!` and event setup)
- `handle_menu_event` function (if it still lives here)

- [ ] **Remove `read_info_json` dead function**

Find and delete the `read_info_json` function at line 2663 of the original file (now at a different line after moves). Verify it's dead:

```bash
grep -n "read_info_json" krillnotes-desktop/src-tauri/src/lib.rs
```

If it appears only as a definition (not called anywhere), delete the function.

- [ ] **Run all tests**

```bash
cargo test -p krillnotes-desktop 2>&1
```

Expected output: 4 tests pass:
- `tests::read_file_content_impl_rejects_disallowed_extension`
- `tests::read_file_content_impl_errors_on_missing_rhai_file`
- `tests::read_file_content_impl_rejects_nonexistent_allowed_file` (if present)
- `tests::read_file_content_impl_accepts_rhai_file` (if present)

- [ ] **Check for new warnings**

```bash
cargo build -p krillnotes-desktop 2>&1 | grep "^warning" | grep -v "inactive-code\|cfg\|target_os"
```

Expected: no new warnings beyond the pre-existing `inactive-code` warnings from `#[cfg(target_os = "macos")]` blocks (those are normal for cross-platform code).

- [ ] **Count lines in `lib.rs`**

```bash
wc -l krillnotes-desktop/src-tauri/src/lib.rs
```

Expected: under 500 lines (success criterion from spec).

- [ ] **Final commit**

```bash
git add krillnotes-desktop/src-tauri/src/lib.rs
git commit -m "refactor(desktop): strip lib.rs, remove dead code, all commands in commands/"
```

---

## Chunk 6: PR

### Task 13: Push and open pull request

- [ ] **Push the branch**

```bash
git push -u github-https feat/split-desktop-rust
```

- [ ] **Open PR**

```bash
gh pr create \
  --title "refactor(desktop): split lib.rs into commands/ domain modules" \
  --base master \
  --body "$(cat <<'EOF'
## Summary

- Splits `src-tauri/src/lib.rs` (4,421 lines, 117 Tauri commands) into `commands/` subdirectory of 9 focused domain modules: `workspace`, `notes`, `scripting`, `scripts`, `identity`, `invites`, `contacts`, `attachments`, `swarm`
- Zero functional changes — all command names and signatures unchanged
- `lib.rs` reduced to under 500 lines (AppState + run() + module wiring)
- Removes dead `read_info_json` function
- Mirrors the module structure applied to `krillnotes-core` in PR #97

## Test plan

- [ ] `cargo test -p krillnotes-desktop` — all 4 tests pass
- [ ] `cargo build -p krillnotes-desktop` — no new warnings
- [ ] `wc -l krillnotes-desktop/src-tauri/src/lib.rs` — under 500 lines
- [ ] App launches and functions correctly (smoke test workspace open/close)
EOF
)"
```

- [ ] **Update `CHANGELOG.md`** (after PR merges, per project convention)

---

## Quick Reference: Command → Module Mapping

| Module | Commands |
|--------|----------|
| `workspace.rs` | `create_workspace` `open_workspace` `get_workspace_info` `get_workspace_metadata` `set_workspace_metadata` `get_app_version` `consume_pending_file_open` `consume_pending_swarm_file` `set_paste_menu_enabled` `read_file_content` `list_workspace_files` `delete_workspace` `duplicate_workspace` `export_workspace_cmd` `peek_import_cmd` `execute_import` `get_settings` `update_settings` `list_themes` `read_theme` `write_theme` `delete_theme` |
| `notes.rs` | `list_notes` `get_note` `get_node_types` `toggle_note_expansion` `set_selected_note` `create_note_with_type` `update_note` `save_note` `search_notes` `count_children` `delete_note` `move_note` `deep_copy_note_cmd` `update_note_tags` `get_all_tags` `get_notes_for_tag` `list_operations` `get_operation_detail` `purge_operations` `undo` `redo` `can_undo` `can_redo` `get_undo_limit` `set_undo_limit` `begin_undo_group` `end_undo_group` `script_undo` `script_redo` `can_script_undo` `can_script_redo` |
| `scripting.rs` | `get_schema_fields` `get_all_schemas` `get_tree_action_map` `invoke_tree_action` `get_note_view` `get_note_hover` `get_views_for_type` `render_view` `get_script_warnings` `validate_field` `validate_fields` `evaluate_group_visibility` |
| `scripts.rs` | `list_user_scripts` `get_user_script` `create_user_script` `update_user_script` `delete_user_script` `toggle_user_script` `reorder_user_script` `reorder_all_user_scripts` |
| `identity.rs` | `list_identities` `resolve_identity_name` `create_identity` `unlock_identity` `lock_identity` `delete_identity` `rename_identity` `change_identity_passphrase` `get_unlocked_identities` `is_identity_unlocked` `get_workspaces_for_identity` `export_swarmid_cmd` `get_identity_public_key` `import_swarmid_cmd` `import_swarmid_overwrite_cmd` |
| `invites.rs` | `list_invites` `create_invite` `revoke_invite` `import_invite_response` `import_invite` `respond_to_invite` `accept_peer` |
| `contacts.rs` | `list_contacts` `get_contact` `create_contact` `update_contact` `delete_contact` `get_fingerprint` `list_workspace_peers` `get_workspace_peers` `remove_workspace_peer` `add_contact_as_peer` |
| `attachments.rs` | `attach_file` `attach_file_bytes` `get_attachments` `get_attachment_data` `delete_attachment` `restore_attachment` `open_attachment` |
| `swarm.rs` | `open_swarm_file_cmd` `create_snapshot_for_peers` `apply_swarm_snapshot` `apply_swarm_delta` `generate_deltas_for_peers` |
