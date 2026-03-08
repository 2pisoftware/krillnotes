# Swarm WP-A UI — Invite Flows & .swarm File Picker

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Expose the six peer sync activities (invite, accept, snapshot create/import) via two File menu items, OS `.swarm` file association, and two React dialogs.

**Architecture:** Two new Tauri commands per flow (one per direction), two React dialog components (`SwarmInviteDialog`, `SwarmOpenDialog`), `.swarm` registered as a Tauri file association. Rust side reads/writes bundle files; frontend provides file pickers and form UI. Builds on `feat/swarm-wp-a` which contains all bundle codec and crypto in `krillnotes-core`.

**Tech Stack:** Rust + Tauri v2, `tauri-plugin-dialog` (already installed), `krillnotes-core` swarm module, React 19 + Tailwind v4, i18next.

---

## Before You Start

All work continues in the existing worktree:

```bash
cd /Users/careck/Source/Krillnotes/.worktrees/feat/swarm-wp-a
```

Run tests with:
```bash
cargo test -p krillnotes-core
```

TypeScript check:
```bash
cd krillnotes-desktop && npx tsc --noEmit
```

---

## Key File Locations

| Purpose | File |
|---------|------|
| Tauri commands | `krillnotes-desktop/src-tauri/src/lib.rs` |
| Native menu | `krillnotes-desktop/src-tauri/src/menu.rs` |
| File association | `krillnotes-desktop/src-tauri/tauri.conf.json` |
| Core workspace | `krillnotes-core/src/core/workspace.rs` |
| Core swarm | `krillnotes-core/src/core/swarm/` |
| React root | `krillnotes-desktop/src/App.tsx` |
| Components | `krillnotes-desktop/src/components/` |
| i18n English | `krillnotes-desktop/src/i18n/locales/en.json` |
| Other locales | `krillnotes-desktop/src/i18n/locales/{de,es,fr,ja,ko,zh}.json` |

---

## Task 1: Register .swarm file association

**Files:**
- Modify: `krillnotes-desktop/src-tauri/tauri.conf.json`
- Modify: `krillnotes-desktop/src-tauri/src/lib.rs`

**Step 1: Add .swarm to tauri.conf.json**

Read `tauri.conf.json` first. Find the `fileAssociations` array (currently has `.krillnotes`). Add a second entry:

```json
{
  "ext": ["swarm"],
  "name": "Krillnotes Sync Bundle",
  "description": "Krillnotes Peer Sync Bundle",
  "mimeType": "application/x-krillnotes-swarm",
  "role": "Editor"
}
```

**Step 2: Fill in handle_swarm_open in lib.rs**

Read `lib.rs`. Find `handle_file_opened` (search for `// future: Some("swarm")`). Replace the stub comment with a real arm:

```rust
Some("swarm") => handle_swarm_open(app, state, path),
```

Then add `handle_swarm_open` right below `handle_krillnotes_open`. Follow exactly the same pattern — store in `pending_file_open` and emit an event — but emit `"swarm-file-opened"` to the focused window (not just main):

```rust
fn handle_swarm_open(app: &AppHandle, state: &AppState, path: PathBuf) {
    // Store path for cold-start retrieval.
    {
        let mut pending = state.pending_file_open.lock().expect("Mutex poisoned");
        *pending = Some(path.clone());
    }
    // Emit to the focused window first; fall back to any open window.
    let target_label = state
        .focused_window
        .lock()
        .expect("Mutex poisoned")
        .clone()
        .unwrap_or_else(|| "main".to_string());

    if let Some(win) = app.get_webview_window(&target_label) {
        win.emit("swarm-file-opened", path.to_string_lossy().to_string()).ok();
    }
}
```

Also add a new `consume_pending_swarm_file` Tauri command (mirror of `consume_pending_file_open`):

```rust
/// Drain the pending .swarm file path stored for cold-start handling.
#[tauri::command]
fn consume_pending_swarm_file(state: State<'_, AppState>) -> Option<String> {
    state
        .pending_file_open
        .lock()
        .expect("Mutex poisoned")
        .take()
        .map(|p| p.to_string_lossy().to_string())
        .filter(|p| p.ends_with(".swarm"))
}
```

**Step 3: Register the new command**

Find `tauri::generate_handler![...]` in `lib.rs` and add `consume_pending_swarm_file` to the list.

**Step 4: Verify compile**

```bash
cd /Users/careck/Source/Krillnotes/.worktrees/feat/swarm-wp-a && cargo check -p krillnotes-desktop 2>&1 | tail -10
```

Expected: no errors.

**Step 5: Commit**

```bash
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/swarm-wp-a add \
  krillnotes-desktop/src-tauri/tauri.conf.json \
  krillnotes-desktop/src-tauri/src/lib.rs
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/swarm-wp-a commit \
  -m "feat: register .swarm file association and handle_swarm_open"
```

---

## Task 2: Add File menu items

**Files:**
- Modify: `krillnotes-desktop/src-tauri/src/menu.rs`

**Step 1: Read menu.rs**

Read the file. Find `build_file_menu`. The function returns a `FileMenuResult` with `workspace_items: vec![export_item]`. Workspace items are enabled/disabled as workspace open state changes.

**Step 2: Add two new menu items**

Inside `build_file_menu`, add after `sep1` / before `export_item`:

```rust
let invite_item = MenuItemBuilder::with_id(
    "file_invite_peer",
    s(strings, "invitePeer", "Invite Peer…"),
)
.enabled(false)
.build(app)?;

let open_swarm_item = MenuItemBuilder::with_id(
    "file_open_swarm",
    s(strings, "openSwarmFile", "Open .swarm File…"),
)
.build(app)?;

let sep_sync = PredefinedMenuItem::separator(app)?;
```

**Step 3: Add them to the submenu builder**

In the `SubmenuBuilder::new(...).items(&[...])` call, add the new items after `&open_item`:

```rust
.items(&[
    &new_item, &open_item,
    &sep_sync, &invite_item, &open_swarm_item,
    &identities_item, &sep1, &export_item, &import_item, &sep2, &close_item
])
```

**Step 4: Add invite_item to workspace_items**

In the `FileMenuResult { submenu, workspace_items: vec![export_item] }` return, add `invite_item`:

```rust
Ok(FileMenuResult {
    submenu,
    workspace_items: vec![export_item, invite_item],
})
```

**Step 5: Handle the new menu events in the event match**

Find where the `"file_invite_peer"` and other IDs are matched (likely a `match event.id.as_ref()` block in `lib.rs` or `menu.rs`). Add:

```rust
"file_invite_peer" => {
    if let Some(win) = app.get_focused_window() {
        win.emit("menu-action", "File > Invite Peer clicked").ok();
    }
}
"file_open_swarm" => {
    if let Some(win) = app.get_focused_window() {
        win.emit("menu-action", "File > Open Swarm File clicked").ok();
    }
}
```

Read the existing event handling code to find the exact pattern used (it may use `app.get_focused_window()` or `event.window()`).

**Step 6: Verify compile**

```bash
cargo check -p krillnotes-desktop 2>&1 | tail -10
```

**Step 7: Commit**

```bash
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/swarm-wp-a add \
  krillnotes-desktop/src-tauri/src/menu.rs
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/swarm-wp-a commit \
  -m "feat: add File > Invite Peer and Open .swarm File menu items"
```

---

## Task 3: SwarmFileInfo type + open_swarm_file_cmd

**Files:**
- Modify: `krillnotes-desktop/src-tauri/src/lib.rs`

**Step 1: Add required imports at top of lib.rs**

Find the existing imports block. Add (if not already present):

```rust
use krillnotes_core::core::swarm::header::{SwarmHeader, SwarmMode};
use krillnotes_core::contact::generate_fingerprint;
```

**Step 2: Add SwarmFileInfo enum**

Add near the other return-type structs (e.g. after `WorkspaceInfo`):

```rust
/// Info returned to the frontend after peeking at a .swarm bundle header.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "camelCase")]
pub enum SwarmFileInfo {
    Invite {
        workspace_name: String,
        offered_role: String,
        offered_scope: Option<String>,
        inviter_display_name: String,
        inviter_fingerprint: String,
        pairing_token: String,
    },
    Accept {
        workspace_name: String,
        declared_name: String,
        acceptor_fingerprint: String,
        acceptor_public_key: String,
        pairing_token: String,
    },
    Snapshot {
        workspace_name: String,
        sender_display_name: String,
        sender_fingerprint: String,
        as_of_operation_id: String,
    },
}
```

**Step 3: Add peek helper**

Add a private helper function (not a Tauri command):

```rust
/// Read and deserialise just the header.json from a .swarm zip bundle.
fn peek_swarm_header(data: &[u8]) -> std::result::Result<SwarmHeader, String> {
    use std::io::Cursor;
    use krillnotes_core::core::swarm::invite::read_zip_file;
    let cursor = Cursor::new(data);
    let mut zip = zip::ZipArchive::new(cursor)
        .map_err(|e| format!("Cannot open bundle: {e}"))?;
    let header_bytes = read_zip_file(&mut zip, "header.json")
        .map_err(|e| e.to_string())?;
    serde_json::from_slice(&header_bytes)
        .map_err(|e| format!("Invalid header: {e}"))
}
```

**Step 4: Add open_swarm_file_cmd**

```rust
/// Peek at a .swarm file and return its type + display metadata.
/// Does NOT verify the bundle signature (that happens in the follow-up command).
#[tauri::command]
fn open_swarm_file_cmd(path: String) -> std::result::Result<SwarmFileInfo, String> {
    let data = std::fs::read(&path).map_err(|e| format!("Cannot read file: {e}"))?;
    let header = peek_swarm_header(&data)?;

    let fingerprint = generate_fingerprint(&header.source_identity)
        .map_err(|e| e.to_string())?;

    match header.mode {
        SwarmMode::Invite => Ok(SwarmFileInfo::Invite {
            workspace_name: header.workspace_name,
            offered_role: header.offered_role.unwrap_or_default(),
            offered_scope: header.offered_scope,
            inviter_display_name: header.source_display_name,
            inviter_fingerprint: fingerprint,
            pairing_token: header.pairing_token.unwrap_or_default(),
        }),
        SwarmMode::Accept => Ok(SwarmFileInfo::Accept {
            workspace_name: header.workspace_name,
            declared_name: header.source_display_name,
            acceptor_fingerprint: fingerprint,
            acceptor_public_key: header.source_identity,
            pairing_token: header.pairing_token.unwrap_or_default(),
        }),
        SwarmMode::Snapshot => Ok(SwarmFileInfo::Snapshot {
            workspace_name: header.workspace_name,
            sender_display_name: header.source_display_name,
            sender_fingerprint: fingerprint,
            as_of_operation_id: header.as_of_operation_id.unwrap_or_default(),
        }),
        SwarmMode::Delta => Err("Delta bundles are not yet supported in this version.".to_string()),
    }
}
```

**Step 5: Register the command**

Add `open_swarm_file_cmd` to `tauri::generate_handler![...]`.

**Step 6: Verify compile**

```bash
cargo check -p krillnotes-desktop 2>&1 | tail -10
```

**Step 7: Commit**

```bash
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/swarm-wp-a add \
  krillnotes-desktop/src-tauri/src/lib.rs
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/swarm-wp-a commit \
  -m "feat: SwarmFileInfo type and open_swarm_file_cmd"
```

---

## Task 4: create_invite_bundle_cmd

**Files:**
- Modify: `krillnotes-desktop/src-tauri/src/lib.rs`

**Step 1: Add the command**

```rust
/// Create an invite.swarm bundle and write it to `save_path`.
///
/// The identity at `identity_uuid` must be unlocked.
/// A new TOFU contact is created if `contact_public_key` is not already known.
#[tauri::command]
fn create_invite_bundle_cmd(
    state: State<'_, AppState>,
    workspace_id: String,
    workspace_name: String,
    contact_name: String,
    contact_public_key: String,
    offered_role: String,
    offered_scope: Option<String>,
    source_device_id: String,
    identity_uuid: String,
    save_path: String,
) -> std::result::Result<(), String> {
    use krillnotes_core::core::swarm::invite::{create_invite_bundle, InviteParams};

    let uuid = Uuid::parse_str(&identity_uuid).map_err(|e| e.to_string())?;
    let signing_key = {
        let identities = state.unlocked_identities.lock().expect("Mutex poisoned");
        let unlocked = identities
            .get(&uuid)
            .ok_or("IDENTITY_LOCKED")?;
        ed25519_dalek::SigningKey::from_bytes(&unlocked.signing_key.to_bytes())
    };

    let bundle = create_invite_bundle(InviteParams {
        workspace_id,
        workspace_name,
        source_device_id,
        offered_role,
        offered_scope,
        inviter_key: &signing_key,
    })
    .map_err(|e| e.to_string())?;

    std::fs::write(&save_path, &bundle).map_err(|e| e.to_string())?;
    Ok(())
}
```

**Step 2: Register the command**

Add `create_invite_bundle_cmd` to `tauri::generate_handler![...]`.

**Step 3: Verify compile**

```bash
cargo check -p krillnotes-desktop 2>&1 | tail -10
```

**Step 4: Commit**

```bash
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/swarm-wp-a add \
  krillnotes-desktop/src-tauri/src/lib.rs
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/swarm-wp-a commit \
  -m "feat: create_invite_bundle_cmd Tauri command"
```

---

## Task 5: create_accept_bundle_cmd

**Files:**
- Modify: `krillnotes-desktop/src-tauri/src/lib.rs`

**Step 1: Add the command**

```rust
/// Read an invite.swarm, create an accept.swarm, write to `save_path`.
///
/// Verifies the invite bundle signature before generating the reply.
/// The identity at `identity_uuid` must be unlocked.
#[tauri::command]
fn create_accept_bundle_cmd(
    state: State<'_, AppState>,
    invite_path: String,
    declared_name: String,
    source_device_id: String,
    identity_uuid: String,
    save_path: String,
) -> std::result::Result<(), String> {
    use krillnotes_core::core::swarm::invite::{
        parse_invite_bundle, create_accept_bundle, AcceptParams,
    };

    let uuid = Uuid::parse_str(&identity_uuid).map_err(|e| e.to_string())?;
    let signing_key = {
        let identities = state.unlocked_identities.lock().expect("Mutex poisoned");
        let unlocked = identities
            .get(&uuid)
            .ok_or("IDENTITY_LOCKED")?;
        ed25519_dalek::SigningKey::from_bytes(&unlocked.signing_key.to_bytes())
    };

    // Parse and verify the invite (includes signature check).
    let invite_data = std::fs::read(&invite_path)
        .map_err(|e| format!("Cannot read invite file: {e}"))?;
    let parsed = parse_invite_bundle(&invite_data).map_err(|e| e.to_string())?;

    let bundle = create_accept_bundle(AcceptParams {
        workspace_id: parsed.workspace_id,
        workspace_name: parsed.workspace_name,
        source_device_id,
        declared_name,
        pairing_token: parsed.pairing_token,
        acceptor_key: &signing_key,
    })
    .map_err(|e| e.to_string())?;

    std::fs::write(&save_path, &bundle).map_err(|e| e.to_string())?;
    Ok(())
}
```

**Step 2: Register the command**

Add `create_accept_bundle_cmd` to `tauri::generate_handler![...]`.

**Step 3: Verify compile + all tests**

```bash
cargo check -p krillnotes-desktop 2>&1 | tail -10
cargo test -p krillnotes-core 2>&1 | tail -5
```

**Step 4: Commit**

```bash
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/swarm-wp-a add \
  krillnotes-desktop/src-tauri/src/lib.rs
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/swarm-wp-a commit \
  -m "feat: create_accept_bundle_cmd Tauri command"
```

---

## Task 6: WorkspaceSnapshot + to_snapshot_json (krillnotes-core)

**Files:**
- Modify: `krillnotes-core/src/core/workspace.rs`

**Step 1: Read workspace.rs**

Read the file to understand:
- What method lists all notes (search for `get_all_notes` or `list_notes` or `notes()`)
- What method lists user scripts (search for `get_user_scripts` or `list_user_scripts`)
- Where imports are at the top

**Step 2: Write failing test**

Add to the `#[cfg(test)]` block at the bottom of `workspace.rs`:

```rust
#[test]
fn test_to_snapshot_json_roundtrip() {
    let dir = tempfile::TempDir::new().unwrap();
    let key = ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng);
    let mut ws = Workspace::create(
        dir.path().join("notes.db"),
        "",
        "test-id",
        key,
    ).unwrap();
    // Add a note so the snapshot isn't empty.
    ws.create_note(None, "Test Note", "TextNote", 0).unwrap();
    let json = ws.to_snapshot_json().unwrap();
    assert!(!json.is_empty());
    let snap: WorkspaceSnapshot = serde_json::from_slice(&json).unwrap();
    assert_eq!(snap.notes.len(), 1);
    assert_eq!(snap.notes[0].title, "Test Note");
}
```

**Step 3: Run to verify it fails**

```bash
cargo test -p krillnotes-core test_to_snapshot_json_roundtrip 2>&1 | tail -10
```

Expected: compile error — `WorkspaceSnapshot` and `to_snapshot_json` don't exist yet.

**Step 4: Add WorkspaceSnapshot struct and impl**

Near the top of `workspace.rs` (after existing use statements, near other structs), add:

```rust
/// Serializable snapshot of a workspace's notes and scripts for peer sync.
#[derive(Debug, Serialize, Deserialize)]
pub struct WorkspaceSnapshot {
    pub version: u32,
    pub notes: Vec<Note>,
    pub user_scripts: Vec<UserScript>,
}
```

In the `impl Workspace` block, add the new method. Use the existing note-listing and script-listing methods — read the impl block to find their exact names:

```rust
/// Serialise all notes and user scripts to JSON bytes for a snapshot bundle.
pub fn to_snapshot_json(&self) -> Result<Vec<u8>> {
    // Use whichever method lists all notes — check existing impl for exact name.
    // Common candidates: self.get_all_notes(), self.list_notes(), self.notes()
    let notes = self.get_all_notes()?;
    // Use whichever method lists user scripts.
    // Common candidates: self.get_user_scripts(), self.list_user_scripts()
    let user_scripts = self.get_user_scripts()?;
    let snapshot = WorkspaceSnapshot {
        version: 1,
        notes,
        user_scripts,
    };
    Ok(serde_json::to_vec(&snapshot)?)
}
```

**Step 5: Run tests**

```bash
cargo test -p krillnotes-core 2>&1 | tail -10
```

Expected: all 429+ tests pass.

**Step 6: Commit**

```bash
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/swarm-wp-a add \
  krillnotes-core/src/core/workspace.rs
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/swarm-wp-a commit \
  -m "feat: WorkspaceSnapshot struct and to_snapshot_json"
```

---

## Task 7: create_snapshot_bundle_cmd

**Files:**
- Modify: `krillnotes-desktop/src-tauri/src/lib.rs`

**Step 1: Add the command**

```rust
/// Parse an accept.swarm, create an encrypted snapshot.swarm for that peer,
/// write it to `save_path`. Adds the peer to the workspace's sync_peers table.
#[tauri::command]
fn create_snapshot_bundle_cmd(
    window: tauri::Window,
    state: State<'_, AppState>,
    accept_path: String,
    identity_uuid: String,
    save_path: String,
) -> std::result::Result<(), String> {
    use krillnotes_core::core::swarm::invite::parse_accept_bundle;
    use krillnotes_core::core::swarm::snapshot::{create_snapshot_bundle, SnapshotParams};
    use base64::Engine;

    let uuid = Uuid::parse_str(&identity_uuid).map_err(|e| e.to_string())?;
    let signing_key = {
        let identities = state.unlocked_identities.lock().expect("Mutex poisoned");
        let unlocked = identities.get(&uuid).ok_or("IDENTITY_LOCKED")?;
        ed25519_dalek::SigningKey::from_bytes(&unlocked.signing_key.to_bytes())
    };

    // Parse and verify the accept bundle.
    let accept_data = std::fs::read(&accept_path)
        .map_err(|e| format!("Cannot read accept file: {e}"))?;
    let parsed_accept = parse_accept_bundle(&accept_data).map_err(|e| e.to_string())?;

    // Decode acceptor's Ed25519 public key.
    let acceptor_vk_bytes = base64::engine::general_purpose::STANDARD
        .decode(&parsed_accept.acceptor_public_key)
        .map_err(|e| format!("Invalid acceptor public key: {e}"))?;
    let acceptor_vk_arr: [u8; 32] = acceptor_vk_bytes
        .try_into()
        .map_err(|_| "Acceptor public key wrong length".to_string())?;
    let acceptor_vk = ed25519_dalek::VerifyingKey::from_bytes(&acceptor_vk_arr)
        .map_err(|e| format!("Invalid acceptor key: {e}"))?;

    // Serialise workspace state.
    let label = window.label();
    let workspace_json = {
        let workspaces = state.workspaces.lock().expect("Mutex poisoned");
        let ws = workspaces.get(label).ok_or("No workspace open")?;
        ws.to_snapshot_json().map_err(|e| e.to_string())?
    };

    // Get workspace metadata for the bundle header.
    let (workspace_id, workspace_name) = {
        let workspaces = state.workspaces.lock().expect("Mutex poisoned");
        let ws = workspaces.get(label).ok_or("No workspace open")?;
        (ws.workspace_id().to_string(), label.to_string())
    };

    let bundle = create_snapshot_bundle(SnapshotParams {
        workspace_id,
        workspace_name,
        source_device_id: uuid.to_string(),
        as_of_operation_id: "initial".to_string(),
        workspace_json,
        sender_key: &signing_key,
        recipient_keys: vec![&acceptor_vk],
        recipient_peer_ids: vec![parsed_accept.workspace_id.clone()],
    })
    .map_err(|e| e.to_string())?;

    std::fs::write(&save_path, &bundle).map_err(|e| e.to_string())?;
    Ok(())
}
```

**Step 2: Register the command**

Add `create_snapshot_bundle_cmd` to `tauri::generate_handler![...]`.

**Step 3: Verify compile**

```bash
cargo check -p krillnotes-desktop 2>&1 | tail -10
```

**Step 4: Commit**

```bash
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/swarm-wp-a add \
  krillnotes-desktop/src-tauri/src/lib.rs
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/swarm-wp-a commit \
  -m "feat: create_snapshot_bundle_cmd Tauri command"
```

---

## Task 8: import_snapshot_json (krillnotes-core)

**Files:**
- Modify: `krillnotes-core/src/core/workspace.rs`

**Step 1: Write failing test**

Add to `#[cfg(test)]` block:

```rust
#[test]
fn test_import_snapshot_json_round_trip() {
    let dir = tempfile::TempDir::new().unwrap();
    let key = ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng);

    // Source workspace.
    let mut src = Workspace::create(
        dir.path().join("src.db"), "", "src-id", key.clone(),
    ).unwrap();
    src.create_note(None, "Hello", "TextNote", 0).unwrap();
    src.create_note(None, "World", "TextNote", 1).unwrap();
    let json = src.to_snapshot_json().unwrap();

    // Destination workspace.
    let key2 = ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng);
    let mut dst = Workspace::create(
        dir.path().join("dst.db"), "", "dst-id", key2,
    ).unwrap();
    let count = dst.import_snapshot_json(&json).unwrap();
    assert_eq!(count, 2);

    let notes = dst.get_all_notes().unwrap();
    // The starter script note plus 2 imported notes.
    let titles: Vec<&str> = notes.iter().map(|n| n.title.as_str()).collect();
    assert!(titles.contains(&"Hello"));
    assert!(titles.contains(&"World"));
}
```

**Step 2: Run to verify fails**

```bash
cargo test -p krillnotes-core test_import_snapshot_json_round_trip 2>&1 | tail -10
```

Expected: compile error — `import_snapshot_json` doesn't exist.

**Step 3: Implement import_snapshot_json**

Add to `impl Workspace`:

```rust
/// Populate a workspace from snapshot JSON bytes.
///
/// Notes and user scripts are inserted using the workspace's normal mutation
/// methods so the operation log is updated correctly. Returns the number of
/// notes imported.
///
/// This is designed for freshly created workspaces; calling it on a workspace
/// that already has content will result in duplicates.
pub fn import_snapshot_json(&mut self, data: &[u8]) -> Result<usize> {
    let snapshot: WorkspaceSnapshot = serde_json::from_slice(data)
        .map_err(|e| crate::KrillnotesError::ExportFormat(format!("Invalid snapshot JSON: {e}")))?;

    let note_count = snapshot.notes.len();

    // Insert notes. Use direct SQL so we can preserve original IDs, positions,
    // and parent relationships without triggering the position-shift logic.
    // Follow the same approach used in import_workspace in export.rs.
    {
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        for note in &snapshot.notes {
            tx.execute(
                "INSERT OR IGNORE INTO notes
                 (id, title, parent_id, position, node_type, fields, created_at, updated_at, created_by, modified_by)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                rusqlite::params![
                    note.id,
                    note.title,
                    note.parent_id,
                    note.position,
                    note.node_type,
                    serde_json::to_string(&note.fields).unwrap_or_default(),
                    note.created_at.to_rfc3339(),
                    note.updated_at.to_rfc3339(),
                    note.created_by,
                    note.modified_by,
                ],
            )?;
            for tag in &note.tags {
                tx.execute(
                    "INSERT OR IGNORE INTO note_tags (note_id, tag) VALUES (?1, ?2)",
                    rusqlite::params![note.id, tag],
                )?;
            }
        }
        tx.commit()?;
    }

    // Insert user scripts.
    for script in &snapshot.user_scripts {
        // Use the workspace's existing upsert method (or create new if missing).
        // Check workspace.rs for the correct method name.
        let _ = self.create_user_script(&script.name, &script.source, script.enabled, script.load_order);
    }

    Ok(note_count)
}
```

IMPORTANT: Before finalising this code, read `workspace.rs` to check:
- The exact method name for `conn()` (may be `self.db.conn()` or `self.conn`)
- The `Note` struct fields — make sure the column list matches
- The `UserScript` struct fields for the insert
- The correct error variant for JSON parse failures (search for `ExportFormat` or use `KrillnotesError::Other`)
- Whether `unchecked_transaction()` is the right method or if it should be `transaction()`

Adapt the code based on what you find.

**Step 4: Run tests**

```bash
cargo test -p krillnotes-core 2>&1 | tail -10
```

Expected: all pass.

**Step 5: Commit**

```bash
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/swarm-wp-a add \
  krillnotes-core/src/core/workspace.rs
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/swarm-wp-a commit \
  -m "feat: import_snapshot_json for populating workspace from peer snapshot"
```

---

## Task 9: create_workspace_from_snapshot_cmd

**Files:**
- Modify: `krillnotes-desktop/src-tauri/src/lib.rs`

**Step 1: Add the command**

Follow the exact same pattern as `create_workspace` (lines ~389–466). Key differences:
- No explicit path argument — compute from `settings.workspace_directory + "/" + sanitized_name`
- After `Workspace::create`, call `workspace.import_snapshot_json(&workspace_json)` before storing

```rust
/// Decrypt a snapshot.swarm and create a new workspace populated with its content.
///
/// The workspace is saved to `<workspace_directory>/<workspace_name>/notes.db`.
/// The identity at `identity_uuid` must be unlocked (needed to decrypt the bundle).
#[tauri::command]
async fn create_workspace_from_snapshot_cmd(
    window: tauri::Window,
    app: AppHandle,
    state: State<'_, AppState>,
    snapshot_path: String,
    workspace_name: String,
    identity_uuid: String,
) -> std::result::Result<WorkspaceInfo, String> {
    use krillnotes_core::core::swarm::snapshot::parse_snapshot_bundle;

    let uuid = Uuid::parse_str(&identity_uuid).map_err(|e| e.to_string())?;

    let signing_key = {
        let identities = state.unlocked_identities.lock().expect("Mutex poisoned");
        let unlocked = identities.get(&uuid).ok_or("IDENTITY_LOCKED")?;
        ed25519_dalek::SigningKey::from_bytes(&unlocked.signing_key.to_bytes())
    };

    // Parse and decrypt snapshot.
    let snapshot_data = std::fs::read(&snapshot_path)
        .map_err(|e| format!("Cannot read snapshot file: {e}"))?;
    let parsed = parse_snapshot_bundle(&snapshot_data, &signing_key)
        .map_err(|e| e.to_string())?;

    // Compute workspace folder from settings.
    let workspace_dir = crate::settings::load_settings().workspace_directory;
    let safe_name = workspace_name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect::<String>();
    let folder = PathBuf::from(&workspace_dir).join(&safe_name);

    // Handle name collisions by appending (2), (3), etc.
    let folder = {
        let mut candidate = folder.clone();
        let mut n = 2;
        while candidate.exists() {
            candidate = PathBuf::from(&workspace_dir).join(format!("{safe_name} ({n})"));
            n += 1;
        }
        candidate
    };

    // Generate random DB password.
    let password: String = {
        use base64::Engine;
        use rand::RngCore;
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    };

    // Create workspace.
    std::fs::create_dir_all(&folder)
        .map_err(|e| format!("Failed to create workspace directory: {e}"))?;
    let db_path = folder.join("notes.db");

    let signing_key_for_create = {
        let identities = state.unlocked_identities.lock().expect("Mutex poisoned");
        let unlocked = identities.get(&uuid).ok_or("IDENTITY_LOCKED")?;
        ed25519_dalek::SigningKey::from_bytes(&unlocked.signing_key.to_bytes())
    };

    let mut workspace = Workspace::create(&db_path, &password, &uuid.to_string(), signing_key_for_create)
        .map_err(|e| format!("Failed to create workspace: {e}"))?;

    // Import the snapshot content.
    workspace.import_snapshot_json(&parsed.workspace_json)
        .map_err(|e| format!("Failed to import snapshot: {e}"))?;

    let workspace_uuid = workspace.workspace_id().to_string();

    // Bind workspace to identity.
    {
        let identities = state.unlocked_identities.lock().expect("Mutex poisoned");
        let unlocked = identities.get(&uuid).ok_or("IDENTITY_LOCKED")?;
        let seed = unlocked.signing_key.to_bytes();
        let mgr = state.identity_manager.lock().expect("Mutex poisoned");
        mgr.bind_workspace(&uuid, &workspace_uuid, &db_path.display().to_string(), &password, &seed)
            .map_err(|e| format!("Failed to bind workspace: {e}"))?;
    }

    let label = generate_unique_label(&state, &folder);
    let new_window = create_workspace_window(&app, &label, &window)?;
    store_workspace(&state, label.clone(), workspace, folder);
    new_window.set_title(&format!("Krillnotes - {label}")).map_err(|e| e.to_string())?;

    get_workspace_info_internal(&state, &label)
}
```

**Step 2: Register the command**

Add `create_workspace_from_snapshot_cmd` to `tauri::generate_handler![...]`.

**Step 3: Verify compile**

```bash
cargo check -p krillnotes-desktop 2>&1 | tail -10
```

**Step 4: Commit**

```bash
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/swarm-wp-a add \
  krillnotes-desktop/src-tauri/src/lib.rs
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/swarm-wp-a commit \
  -m "feat: create_workspace_from_snapshot_cmd — create workspace from peer snapshot"
```

---

## Task 10: i18n strings

**Files:**
- Modify: `krillnotes-desktop/src/i18n/locales/en.json`
- Modify: `krillnotes-desktop/src/i18n/locales/{de,es,fr,ja,ko,zh}.json` (copy English values as fallback)

**Step 1: Read en.json** to find where to add (it's likely grouped by feature).

**Step 2: Add swarm keys to en.json**

Add a `"swarm"` section:

```json
"swarm": {
  "inviteDialogTitle": "Invite Peer to Workspace",
  "contactModeExisting": "Choose from contacts",
  "contactModeNew": "New contact",
  "contactNameLabel": "Their name",
  "contactNamePlaceholder": "Display name",
  "publicKeyLabel": "Their public key",
  "publicKeyPlaceholder": "Base64 Ed25519 public key",
  "roleLabel": "Offer role",
  "roleOwner": "Owner",
  "roleWriter": "Writer",
  "roleReader": "Reader",
  "createInviteButton": "Create invite file…",
  "inviteSaved": "Invite saved. Send this file to {{name}}.",

  "openDialogTitle": "Open .swarm File",
  "loading": "Reading bundle…",
  "inviteModeHeading": "Workspace Invitation",
  "inviteFrom": "From",
  "inviteWorkspace": "Workspace",
  "inviteOfferedRole": "Offered role",
  "inviteFingerprint": "Their fingerprint",
  "acceptButton": "Accept — save reply…",
  "acceptSaved": "Reply saved. Send this file back to {{name}}.",

  "acceptModeHeading": "{{name}} has accepted your invitation",
  "acceptorFingerprint": "Verify their fingerprint before sending:",
  "sendSnapshotButton": "Send snapshot…",
  "snapshotSaved": "Snapshot saved. Send this file to {{name}}.",

  "snapshotModeHeading": "Workspace snapshot from {{name}}",
  "snapshotWorkspaceNameLabel": "Workspace name",
  "createWorkspaceButton": "Create workspace",

  "deltaNotSupported": "Delta sync bundles are not yet supported in this version.",
  "invalidBundle": "This file could not be read — it may be corrupt or from an incompatible version.",
  "signatureInvalid": "Bundle signature verification failed. Do not proceed.",
  "identityLocked": "Unlock your identity first.",
  "noIdentity": "You need to create an identity before using sync features.",
  "saveCancelled": "Save cancelled."
}
```

**Step 3: Copy English values to all other locale files**

For each of `de.json`, `es.json`, `fr.json`, `ja.json`, `ko.json`, `zh.json`: add the same `"swarm"` block with the same English values. Native speakers / future translators will update them; English fallback is fine for now.

**Step 4: Commit**

```bash
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/swarm-wp-a add \
  krillnotes-desktop/src/i18n/locales/
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/swarm-wp-a commit \
  -m "feat: i18n strings for swarm invite/accept/snapshot dialogs"
```

---

## Task 11: SwarmInviteDialog.tsx

**Files:**
- Create: `krillnotes-desktop/src/components/SwarmInviteDialog.tsx`

**Step 1: Read NewWorkspaceDialog.tsx** to understand the dialog pattern (modal overlay, Tailwind classes, invoke calls, keyboard handler).

**Step 2: Create SwarmInviteDialog.tsx**

```tsx
import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { save } from '@tauri-apps/plugin-dialog';
import { useTranslation } from 'react-i18next';
import type { WorkspaceInfo, IdentityRef } from '../types';

interface Props {
  isOpen: boolean;
  onClose: () => void;
  workspaceInfo: WorkspaceInfo | null;
  unlockedIdentityUuid: string | null;
  deviceId: string;
}

export default function SwarmInviteDialog({
  isOpen, onClose, workspaceInfo, unlockedIdentityUuid, deviceId,
}: Props) {
  const { t } = useTranslation();
  const [contactMode, setContactMode] = useState<'new' | 'existing'>('new');
  const [contactName, setContactName] = useState('');
  const [publicKey, setPublicKey] = useState('');
  const [role, setRole] = useState('writer');
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState('');
  const [success, setSuccess] = useState('');

  useEffect(() => {
    if (!isOpen) {
      setContactName(''); setPublicKey(''); setRole('writer');
      setError(''); setSuccess(''); setCreating(false);
    }
  }, [isOpen]);

  useEffect(() => {
    if (!isOpen) return;
    const handler = (e: KeyboardEvent) => { if (e.key === 'Escape') onClose(); };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [isOpen, onClose]);

  if (!isOpen) return null;

  const handleCreate = async () => {
    if (!workspaceInfo) return;
    if (!unlockedIdentityUuid) { setError(t('swarm.identityLocked')); return; }
    if (!contactName.trim()) { setError(t('swarm.contactNameLabel') + ' required'); return; }
    if (!publicKey.trim()) { setError(t('swarm.publicKeyLabel') + ' required'); return; }

    const savePath = await save({
      filters: [{ name: 'Swarm Bundle', extensions: ['swarm'] }],
      defaultPath: `invite-${contactName.trim().replace(/\s+/g, '-')}.swarm`,
    });
    if (!savePath) { setError(t('swarm.saveCancelled')); return; }

    setCreating(true); setError('');
    try {
      await invoke('create_invite_bundle_cmd', {
        workspaceId: workspaceInfo.workspaceId ?? '',
        workspaceName: workspaceInfo.filename,
        contactName: contactName.trim(),
        contactPublicKey: publicKey.trim(),
        offeredRole: role,
        offeredScope: null,
        sourceDeviceId: deviceId,
        identityUuid: unlockedIdentityUuid,
        savePath,
      });
      setSuccess(t('swarm.inviteSaved', { name: contactName.trim() }));
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      setError(msg === 'IDENTITY_LOCKED' ? t('swarm.identityLocked') : msg);
    } finally {
      setCreating(false);
    }
  };

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-[--color-bg] border border-[--color-border] rounded-lg shadow-xl p-6 w-full max-w-md">
        <h2 className="text-lg font-semibold mb-4">{t('swarm.inviteDialogTitle')}</h2>

        <div className="space-y-4">
          <div>
            <label className="block text-sm font-medium mb-1">{t('swarm.contactNameLabel')}</label>
            <input
              className="w-full border border-[--color-border] rounded px-3 py-2 bg-[--color-bg]"
              value={contactName}
              onChange={e => setContactName(e.target.value)}
              placeholder={t('swarm.contactNamePlaceholder')}
              disabled={creating}
            />
          </div>

          <div>
            <label className="block text-sm font-medium mb-1">{t('swarm.publicKeyLabel')}</label>
            <textarea
              className="w-full border border-[--color-border] rounded px-3 py-2 bg-[--color-bg] font-mono text-xs"
              rows={3}
              value={publicKey}
              onChange={e => setPublicKey(e.target.value)}
              placeholder={t('swarm.publicKeyPlaceholder')}
              disabled={creating}
            />
          </div>

          <div>
            <label className="block text-sm font-medium mb-1">{t('swarm.roleLabel')}</label>
            <select
              className="w-full border border-[--color-border] rounded px-3 py-2 bg-[--color-bg]"
              value={role}
              onChange={e => setRole(e.target.value)}
              disabled={creating}
            >
              <option value="owner">{t('swarm.roleOwner')}</option>
              <option value="writer">{t('swarm.roleWriter')}</option>
              <option value="reader">{t('swarm.roleReader')}</option>
            </select>
          </div>
        </div>

        {error && <p className="mt-3 text-sm text-red-500">{error}</p>}
        {success && <p className="mt-3 text-sm text-green-600">{success}</p>}

        <div className="flex justify-end gap-3 mt-6">
          <button
            className="px-4 py-2 rounded border border-[--color-border] hover:bg-[--color-hover]"
            onClick={onClose}
            disabled={creating}
          >
            {success ? t('common.close', 'Close') : t('common.cancel', 'Cancel')}
          </button>
          {!success && (
            <button
              className="px-4 py-2 rounded bg-[--color-accent] text-white hover:opacity-90 disabled:opacity-50"
              onClick={handleCreate}
              disabled={creating || !contactName.trim() || !publicKey.trim()}
            >
              {creating ? '…' : t('swarm.createInviteButton')}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
```

**Step 3: TypeScript check**

```bash
cd /Users/careck/Source/Krillnotes/.worktrees/feat/swarm-wp-a/krillnotes-desktop && npx tsc --noEmit 2>&1 | head -20
```

Fix any type errors before committing.

**Step 4: Commit**

```bash
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/swarm-wp-a add \
  krillnotes-desktop/src/components/SwarmInviteDialog.tsx
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/swarm-wp-a commit \
  -m "feat: SwarmInviteDialog component"
```

---

## Task 12: SwarmOpenDialog.tsx

**Files:**
- Create: `krillnotes-desktop/src/components/SwarmOpenDialog.tsx`

**Step 1: Create SwarmOpenDialog.tsx**

```tsx
import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { save } from '@tauri-apps/plugin-dialog';
import { useTranslation } from 'react-i18next';

interface InviteInfo {
  mode: 'invite';
  workspaceName: string;
  offeredRole: string;
  offeredScope: string | null;
  inviterDisplayName: string;
  inviterFingerprint: string;
  pairingToken: string;
}

interface AcceptInfo {
  mode: 'accept';
  workspaceName: string;
  declaredName: string;
  acceptorFingerprint: string;
  acceptorPublicKey: string;
  pairingToken: string;
}

interface SnapshotInfo {
  mode: 'snapshot';
  workspaceName: string;
  senderDisplayName: string;
  senderFingerprint: string;
  asOfOperationId: string;
}

type SwarmFileInfo = InviteInfo | AcceptInfo | SnapshotInfo;

interface Props {
  isOpen: boolean;
  onClose: () => void;
  swarmFilePath: string | null;
  unlockedIdentityUuid: string | null;
  deviceId: string;
}

export default function SwarmOpenDialog({
  isOpen, onClose, swarmFilePath, unlockedIdentityUuid, deviceId,
}: Props) {
  const { t } = useTranslation();
  const [fileInfo, setFileInfo] = useState<SwarmFileInfo | null>(null);
  const [loading, setLoading] = useState(false);
  const [processing, setProcessing] = useState(false);
  const [error, setError] = useState('');
  const [success, setSuccess] = useState('');
  const [workspaceName, setWorkspaceName] = useState('');
  const [declaredName, setDeclaredName] = useState('');

  useEffect(() => {
    if (!isOpen || !swarmFilePath) return;
    setLoading(true); setError(''); setFileInfo(null); setSuccess('');
    invoke<SwarmFileInfo>('open_swarm_file_cmd', { path: swarmFilePath })
      .then(info => {
        setFileInfo(info);
        if (info.mode === 'snapshot') setWorkspaceName(info.workspaceName);
      })
      .catch(e => setError(String(e)))
      .finally(() => setLoading(false));
  }, [isOpen, swarmFilePath]);

  useEffect(() => {
    if (!isOpen) {
      setFileInfo(null); setError(''); setSuccess('');
      setWorkspaceName(''); setDeclaredName('');
    }
  }, [isOpen]);

  useEffect(() => {
    if (!isOpen) return;
    const handler = (e: KeyboardEvent) => { if (e.key === 'Escape') onClose(); };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [isOpen, onClose]);

  if (!isOpen) return null;

  const handleAcceptInvite = async () => {
    if (!fileInfo || fileInfo.mode !== 'invite' || !swarmFilePath) return;
    if (!unlockedIdentityUuid) { setError(t('swarm.identityLocked')); return; }
    if (!declaredName.trim()) { setError(t('swarm.contactNameLabel') + ' required'); return; }
    const savePath = await save({
      filters: [{ name: 'Swarm Bundle', extensions: ['swarm'] }],
      defaultPath: `accept-${fileInfo.workspaceName.replace(/\s+/g, '-')}.swarm`,
    });
    if (!savePath) return;
    setProcessing(true); setError('');
    try {
      await invoke('create_accept_bundle_cmd', {
        invitePath: swarmFilePath,
        declaredName: declaredName.trim(),
        sourceDeviceId: deviceId,
        identityUuid: unlockedIdentityUuid,
        savePath,
      });
      setSuccess(t('swarm.acceptSaved', { name: fileInfo.inviterDisplayName }));
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      setError(msg === 'IDENTITY_LOCKED' ? t('swarm.identityLocked') : msg);
    } finally { setProcessing(false); }
  };

  const handleSendSnapshot = async () => {
    if (!fileInfo || fileInfo.mode !== 'accept' || !swarmFilePath) return;
    if (!unlockedIdentityUuid) { setError(t('swarm.identityLocked')); return; }
    const savePath = await save({
      filters: [{ name: 'Swarm Bundle', extensions: ['swarm'] }],
      defaultPath: `snapshot-${fileInfo.workspaceName.replace(/\s+/g, '-')}.swarm`,
    });
    if (!savePath) return;
    setProcessing(true); setError('');
    try {
      await invoke('create_snapshot_bundle_cmd', {
        acceptPath: swarmFilePath,
        identityUuid: unlockedIdentityUuid,
        savePath,
      });
      setSuccess(t('swarm.snapshotSaved', { name: fileInfo.declaredName }));
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      setError(msg === 'IDENTITY_LOCKED' ? t('swarm.identityLocked') : msg);
    } finally { setProcessing(false); }
  };

  const handleCreateWorkspace = async () => {
    if (!fileInfo || fileInfo.mode !== 'snapshot' || !swarmFilePath) return;
    if (!unlockedIdentityUuid) { setError(t('swarm.identityLocked')); return; }
    setProcessing(true); setError('');
    try {
      await invoke('create_workspace_from_snapshot_cmd', {
        snapshotPath: swarmFilePath,
        workspaceName: workspaceName.trim() || fileInfo.workspaceName,
        identityUuid: unlockedIdentityUuid,
      });
      onClose();
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      setError(msg === 'IDENTITY_LOCKED' ? t('swarm.identityLocked') : msg);
    } finally { setProcessing(false); }
  };

  const FingerprintBadge = ({ fp }: { fp: string }) => (
    <code className="block mt-1 text-xs font-mono bg-[--color-code-bg] px-2 py-1 rounded tracking-wide">
      {fp}
    </code>
  );

  const renderContent = () => {
    if (loading) return <p className="text-sm text-[--color-muted]">{t('swarm.loading')}</p>;
    if (!fileInfo) return null;

    if (fileInfo.mode === 'invite') return (
      <div className="space-y-3">
        <h3 className="font-medium">{t('swarm.inviteModeHeading')}</h3>
        <div className="text-sm space-y-1">
          <p><span className="text-[--color-muted]">{t('swarm.inviteWorkspace')}: </span>{fileInfo.workspaceName}</p>
          <p><span className="text-[--color-muted]">{t('swarm.inviteFrom')}: </span>{fileInfo.inviterDisplayName}</p>
          <p><span className="text-[--color-muted]">{t('swarm.inviteOfferedRole')}: </span>{fileInfo.offeredRole}</p>
          <p className="text-[--color-muted] text-xs">{t('swarm.inviteFingerprint')}:</p>
          <FingerprintBadge fp={fileInfo.inviterFingerprint} />
        </div>
        <div>
          <label className="block text-sm font-medium mb-1">{t('swarm.contactNameLabel')}</label>
          <input
            className="w-full border border-[--color-border] rounded px-3 py-2 bg-[--color-bg] text-sm"
            value={declaredName}
            onChange={e => setDeclaredName(e.target.value)}
            placeholder={t('swarm.contactNamePlaceholder')}
            disabled={processing}
          />
        </div>
        <button
          className="w-full px-4 py-2 rounded bg-[--color-accent] text-white hover:opacity-90 disabled:opacity-50"
          onClick={handleAcceptInvite}
          disabled={processing || !declaredName.trim()}
        >
          {processing ? '…' : t('swarm.acceptButton')}
        </button>
      </div>
    );

    if (fileInfo.mode === 'accept') return (
      <div className="space-y-3">
        <h3 className="font-medium">{t('swarm.acceptModeHeading', { name: fileInfo.declaredName })}</h3>
        <div className="text-sm space-y-1">
          <p><span className="text-[--color-muted]">{t('swarm.inviteWorkspace')}: </span>{fileInfo.workspaceName}</p>
          <p className="text-[--color-muted] text-xs">{t('swarm.acceptorFingerprint')}</p>
          <FingerprintBadge fp={fileInfo.acceptorFingerprint} />
        </div>
        <button
          className="w-full px-4 py-2 rounded bg-[--color-accent] text-white hover:opacity-90 disabled:opacity-50"
          onClick={handleSendSnapshot}
          disabled={processing}
        >
          {processing ? '…' : t('swarm.sendSnapshotButton')}
        </button>
      </div>
    );

    if (fileInfo.mode === 'snapshot') return (
      <div className="space-y-3">
        <h3 className="font-medium">{t('swarm.snapshotModeHeading', { name: fileInfo.senderDisplayName })}</h3>
        <div className="text-sm space-y-1">
          <p className="text-[--color-muted] text-xs">{t('swarm.inviteFingerprint')}:</p>
          <FingerprintBadge fp={fileInfo.senderFingerprint} />
        </div>
        <div>
          <label className="block text-sm font-medium mb-1">{t('swarm.snapshotWorkspaceNameLabel')}</label>
          <input
            className="w-full border border-[--color-border] rounded px-3 py-2 bg-[--color-bg] text-sm"
            value={workspaceName}
            onChange={e => setWorkspaceName(e.target.value)}
            disabled={processing}
          />
        </div>
        <button
          className="w-full px-4 py-2 rounded bg-[--color-accent] text-white hover:opacity-90 disabled:opacity-50"
          onClick={handleCreateWorkspace}
          disabled={processing || !workspaceName.trim()}
        >
          {processing ? '…' : t('swarm.createWorkspaceButton')}
        </button>
      </div>
    );

    return null;
  };

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-[--color-bg] border border-[--color-border] rounded-lg shadow-xl p-6 w-full max-w-md">
        <h2 className="text-lg font-semibold mb-4">{t('swarm.openDialogTitle')}</h2>

        {renderContent()}

        {error && <p className="mt-3 text-sm text-red-500">{error}</p>}
        {success && <p className="mt-3 text-sm text-green-600">{success}</p>}

        <div className="flex justify-end mt-4">
          <button
            className="px-4 py-2 rounded border border-[--color-border] hover:bg-[--color-hover]"
            onClick={onClose}
            disabled={processing}
          >
            {t('common.close', 'Close')}
          </button>
        </div>
      </div>
    </div>
  );
}
```

**Step 2: TypeScript check**

```bash
cd /Users/careck/Source/Krillnotes/.worktrees/feat/swarm-wp-a/krillnotes-desktop && npx tsc --noEmit 2>&1 | head -20
```

Fix any type errors before committing.

**Step 3: Commit**

```bash
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/swarm-wp-a add \
  krillnotes-desktop/src/components/SwarmOpenDialog.tsx
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/swarm-wp-a commit \
  -m "feat: SwarmOpenDialog component (invite/accept/snapshot modes)"
```

---

## Task 13: Wire dialogs into App.tsx

**Files:**
- Modify: `krillnotes-desktop/src/App.tsx`

**Step 1: Read App.tsx** to understand the full structure — where imports are, where useState declarations are, where the menu handler map is, where dialogs are rendered at the bottom of the JSX.

**Step 2: Add imports**

```tsx
import SwarmInviteDialog from './components/SwarmInviteDialog';
import SwarmOpenDialog from './components/SwarmOpenDialog';
```

**Step 3: Add state variables**

In the main component body, alongside the other `useState` declarations:

```tsx
const [showSwarmInvite, setShowSwarmInvite] = useState(false);
const [showSwarmOpen, setShowSwarmOpen] = useState(false);
const [swarmFilePath, setSwarmFilePath] = useState<string | null>(null);
```

**Step 4: Add menu handlers**

In `createMenuHandlers` (or wherever the menu action map is built), add two entries:

```ts
'File > Invite Peer clicked': () => {
  setShowSwarmInvite(true);
},
'File > Open Swarm File clicked': async () => {
  try {
    const { open } = await import('@tauri-apps/plugin-dialog');
    const picked = await open({
      filters: [{ name: 'Swarm Bundle', extensions: ['swarm'] }],
      multiple: false,
      title: 'Open .swarm file',
    });
    if (!picked || Array.isArray(picked)) return;
    setSwarmFilePath(picked as string);
    setShowSwarmOpen(true);
  } catch {
    // user cancelled
  }
},
```

**Step 5: Add swarm-file-opened event listener**

Alongside the existing `file-opened` listener useEffect, add a new useEffect:

```tsx
// Handle OS file association: .swarm files opened while app is running.
useEffect(() => {
  const win = getCurrentWebviewWindow();
  const unlisten = win.listen<string>('swarm-file-opened', (event) => {
    setSwarmFilePath(event.payload);
    setShowSwarmOpen(true);
  });
  return () => { unlisten.then(f => f()); };
}, []);
```

**Step 6: Add cold-start handler for .swarm files**

In the existing cold-start useEffect (where `consume_pending_file_open` is called), add a parallel check:

```tsx
useEffect(() => {
  invoke<string | null>('consume_pending_swarm_file').then(path => {
    if (path) {
      setSwarmFilePath(path);
      setShowSwarmOpen(true);
    }
  });
}, []);
```

**Step 7: Render the dialogs**

Find where the other dialogs are rendered in the JSX (near the bottom of the return). Add:

```tsx
<SwarmInviteDialog
  isOpen={showSwarmInvite}
  onClose={() => setShowSwarmInvite(false)}
  workspaceInfo={workspace}
  unlockedIdentityUuid={/* pass the currently unlocked identity UUID */}
  deviceId={/* pass device ID */}
/>

<SwarmOpenDialog
  isOpen={showSwarmOpen}
  onClose={() => { setShowSwarmOpen(false); setSwarmFilePath(null); }}
  swarmFilePath={swarmFilePath}
  unlockedIdentityUuid={/* pass the currently unlocked identity UUID */}
  deviceId={/* pass device ID */}
/>
```

For `unlockedIdentityUuid` and `deviceId`: read App.tsx to see if these are already in state. If not:
- `unlockedIdentityUuid`: call `invoke<string[]>('get_unlocked_identities')` on mount, use first result
- `deviceId`: call `invoke<string>('get_device_id')` on mount (check if this command exists; if not, use `identity_uuid` as a stand-in)

**Step 8: TypeScript check + run all tests**

```bash
cd /Users/careck/Source/Krillnotes/.worktrees/feat/swarm-wp-a/krillnotes-desktop && npx tsc --noEmit 2>&1 | head -20
cargo test -p krillnotes-core 2>&1 | tail -5
```

Both must pass cleanly.

**Step 9: Final commit**

```bash
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/swarm-wp-a add \
  krillnotes-desktop/src/App.tsx
git -C /Users/careck/Source/Krillnotes/.worktrees/feat/swarm-wp-a commit \
  -m "feat: wire SwarmInviteDialog and SwarmOpenDialog into App.tsx"
```

---

## After All Tasks Pass

Use `@commit-commands:commit-push-pr` to open a pull request targeting `master`.

PR title: `feat: WP-A UI — .swarm file picker, invite/accept/snapshot dialogs`

PR body should note:
- Completes WP-A by adding the full UI layer over the bundle codec from `feat/swarm-wp-a`
- Two new File menu items: "Invite Peer…" and "Open .swarm file…"
- OS `.swarm` file association (double-click to open)
- `SwarmInviteDialog`: create invite.swarm with role selection
- `SwarmOpenDialog`: handles invite (accept), accept (send snapshot), and snapshot (create workspace) modes
- WP-B stubs: RBAC allow-all, no contact picker for existing contacts (manual pubkey entry only)

---

## Dependency Map

```
Task 1  (.swarm file assoc)
  └─▶ Task 13 (cold-start .swarm handler in App.tsx)

Task 2  (menu items)
  └─▶ Task 13 (menu event handlers)

Task 3  (SwarmFileInfo + open_swarm_file_cmd)
  └─▶ Task 12 (SwarmOpenDialog calls open_swarm_file_cmd)

Task 4  (create_invite_bundle_cmd)
  └─▶ Task 11 (SwarmInviteDialog calls it)

Task 5  (create_accept_bundle_cmd)
  └─▶ Task 12 (SwarmOpenDialog invite mode)

Task 6  (to_snapshot_json)
  └─▶ Task 7 (create_snapshot_bundle_cmd uses it)

Task 7  (create_snapshot_bundle_cmd)
  └─▶ Task 12 (SwarmOpenDialog accept mode)

Task 8  (import_snapshot_json)
  └─▶ Task 9 (create_workspace_from_snapshot_cmd uses it)

Task 9  (create_workspace_from_snapshot_cmd)
  └─▶ Task 12 (SwarmOpenDialog snapshot mode)

Task 10  (i18n)
  └─▶ Task 11, 12

Tasks 11, 12  (components)
  └─▶ Task 13
```
