# Design: Undo/Redo for Note and Script Operations (issue #45)

**Date:** 2026-03-02
**Approach:** Event-sourced operation log with `RetractOperation`

## Summary

Add Cmd+Z / Ctrl+Z undo and Cmd+Shift+Z / Ctrl+Y redo for all workspace data
mutations: note create, edit, delete, move, and script create, edit, delete.
Undo is in-session only (stack cleared on workspace close). Theme changes are
out of scope — they are application-wide, not workspace data, and will be
handled separately.

## Core Idea: Retract as a First-Class Operation

Undo is not a side-channel. Each undo appends a `RetractOperation` to the
operation log — the same log used for future `.swarm` peer sync. This means:

- Peers that receive a `.swarm` diff can apply the retract and converge correctly.
- The operation log is always the ground truth; undo history is derivable from it.
- Retracts are themselves revertible (redo appends a new forward operation).

```
op-1: CreateNote { note_id: "abc" }
op-2: UpdateField { note_id: "abc", field: "title", value: "Draft" }
op-3: RetractOperation { retracted_ids: ["op-2"], inverse: NoteRestore { old_title: "Untitled", ... } }
                        ↑ undo of op-2
op-4: UpdateField { note_id: "abc", field: "title", value: "Draft" }
                        ↑ redo (new forward op, new operation_id)
```

## Changes to `Operation` Enum

One new variant added; all existing variants are untouched.

```rust
/// Reverses one or more previously logged operations (undo).
RetractOperation {
    operation_id: String,
    timestamp: i64,
    device_id: String,
    /// All operation_ids this retract covers.
    /// A note save emits title + N field ops; one retract covers them all.
    retracted_ids: Vec<String>,
    /// Inverse data needed to reverse the original operations locally.
    inverse: RetractInverse,
    /// Whether this retract should be included in .swarm sync diffs.
    /// false for textarea (CRDT) field retracts — local only.
    propagate: bool,
}
```

### `RetractInverse` Enum

```rust
pub enum RetractInverse {
    /// Inverse of CreateNote — delete the created note.
    DeleteNote { note_id: String },

    /// Inverse of DeleteNote — restore the full note subtree and its
    /// attachment metadata rows. Attachment .enc files remain on disk
    /// (they are not eagerly deleted on note deletion) so only the DB
    /// rows need to be re-inserted.
    SubtreeRestore {
        notes: Vec<NoteSnapshot>,
        attachments: Vec<AttachmentMeta>,
    },

    /// Inverse of UpdateField / update_note — restore the full note
    /// state (title + all fields + tags) as one atomic unit.
    NoteRestore {
        note_id: String,
        old_title: String,
        old_fields: HashMap<String, FieldValue>,
        old_tags: Vec<String>,
    },

    /// Inverse of MoveNote — return note to its previous position.
    PositionRestore {
        note_id: String,
        old_parent_id: Option<String>,
        old_position: i32,
    },

    /// Inverse of CreateUserScript — delete the created script.
    DeleteScript { script_id: String },

    /// Inverse of DeleteUserScript / UpdateUserScript — restore full
    /// script state.
    ScriptRestore {
        script_id: String,
        name: String,
        description: String,
        source_code: String,
        load_order: i32,
        enabled: bool,
    },

    /// Inverse of a tree_hook or other compound action that created or
    /// modified multiple entities. Applied in reverse order (LIFO).
    Batch(Vec<RetractInverse>),
}
```

### `NoteSnapshot` Struct

```rust
pub struct NoteSnapshot {
    pub id: String,
    pub parent_id: Option<String>,
    pub position: i32,
    pub node_type: String,
    pub title: String,
    pub fields: HashMap<String, FieldValue>,
    pub tags: Vec<String>,
}
```

## Operation Log Always Enabled

`Workspace.operation_log` changes from `Option<OperationLog>` to
`OperationLog` (always `Some`). The `PurgeStrategy::LocalOnly { keep_last }`
value is derived from the `undo_limit` workspace setting (default 50). The
sync gate that currently prevents logging is removed.

## In-Memory Undo / Redo Stacks

```rust
struct UndoEntry {
    retracted_ids: Vec<String>,   // always non-empty (themes are out of scope)
    inverse: RetractInverse,
    propagate: bool,
}
```

`Workspace` gains:

```rust
undo_stack: Vec<UndoEntry>   // in-memory, cleared on workspace close/switch
redo_stack: Vec<UndoEntry>
undo_limit: usize            // from workspace_meta key "undo_limit", default 50
```

Every mutation:
1. Captures `UndoEntry` (before-state) before applying the DB change.
2. Applies the mutation.
3. Pushes `UndoEntry` to `undo_stack`; clears `redo_stack`.
4. Trims `undo_stack` to `undo_limit`.

### Undo Flow

1. Pop `UndoEntry` from `undo_stack`.
2. Apply `inverse` to SQLite (restore subtree / restore fields / etc.).
3. Write `RetractOperation { retracted_ids, inverse, propagate }` to the log.
4. Push a redo entry (captures the current state before the undo was applied)
   onto `redo_stack`.

### Redo Flow

1. Pop redo entry from `redo_stack`.
2. Re-apply the forward operation to SQLite.
3. Write a new forward `Operation` to the log (new `operation_id`, current
   timestamp).
4. Push a new `UndoEntry` back onto `undo_stack`.

## Undo Groups (for tree_hooks and compound actions)

A tree_hook can create many child notes in one user action. All resulting
mutations must collapse to a single undo step.

`Workspace` gains:

```rust
fn begin_undo_group(&mut self)
fn end_undo_group(&mut self)   // flushes one batched UndoEntry with Batch inverse
```

While a group is open, individual mutations append to a staging buffer instead
of pushing separate entries. `end_undo_group()` bundles them into one
`UndoEntry` with `RetractInverse::Batch(...)`, `retracted_ids` containing all
accumulated op-ids, applied LIFO on undo (children before parents).

The tree_hook executor wraps its Rhai call:

```rust
workspace.begin_undo_group();
execute_rhai_hook(&mut workspace, script);
workspace.end_undo_group();
```

Other compound actions that use groups: note save (`update_note`), workspace
import.

## New Tauri Commands

| Command | Signature | Notes |
|---|---|---|
| `undo` | `() -> Result<UndoResult>` | Returns affected note_id for UI re-selection |
| `redo` | `() -> Result<UndoResult>` | Returns affected note_id for UI re-selection |
| `can_undo` | `() -> bool` | |
| `can_redo` | `() -> bool` | |

`UndoResult` carries enough context for the frontend to re-select the right
note after applying the inverse (e.g. the restored note_id for a
`SubtreeRestore`).

## Frontend Changes

### Keyboard Shortcuts

- `Cmd+Z` / `Ctrl+Z` → `undo`
- `Cmd+Shift+Z` / `Ctrl+Y` → `redo`

Registered as global shortcuts in `WorkspaceView.tsx` via `useEffect` +
`keydown` listener. Only active when a workspace is open.

### Toolbar Buttons

Small Undo / Redo icon buttons added to the workspace toolbar (disabled when
respective stacks are empty). State polled after every mutation and after
undo/redo via `can_undo` / `can_redo`.

### Post-undo Behaviour

After `undo` or `redo`, the frontend:
1. Re-fetches the workspace tree.
2. Selects the note returned in `UndoResult` (if any).
3. Re-renders the InfoPanel.

### Workspace Switch

On workspace close or switch, `undo_stack` and `redo_stack` are cleared
server-side (called from the Tauri workspace-open command).

## Settings Dialog

New "Undo history" number field in the existing `SettingsDialog.tsx`:

- Label: "Undo history limit"
- Input: number, min 1, max 500, default 50
- Stored in `workspace_meta` under key `"undo_limit"`
- Changing it trims the current `undo_stack` if the new limit is smaller

## Sync Compatibility

### LWW fields (notes, scripts, non-textarea)

`propagate: true`. When a peer receives a `RetractOperation`, it applies the
inverse only if the retracted operation is still the current LWW winner for
that field/note. If a newer operation from any device has superseded it, the
retract is a silent no-op. This is future sync-layer work; for now all retracts
are applied locally without the LWW guard.

### Textarea fields (CRDT)

`propagate: false`. The retract is applied locally (restoring the old textarea
value in the current session), but it is excluded from `.swarm` diffs. Peers
never receive it. This avoids blowing away concurrent CRDT edits on peers.

### Summary Table

| Entity | In op log? | `propagate` | Peer LWW guard needed |
|---|---|---|---|
| Note create / delete / move | Yes | `true` | Yes (future) |
| Note field (non-textarea) | Yes | `true` | Yes (future) |
| Note field (textarea) | Yes | `false` | N/A |
| Script create / edit / delete | Yes | `true` | Yes (future) |
| Theme create / edit / delete | **No** | N/A | N/A — out of scope |

## Out of Scope

- **Themes** — application-wide, file-based, not workspace data. Separate
  issue.
- **Attachment-only changes** — `delete_attachment` (individual file removal)
  is not undoable. Attachment restoration is supported only as part of
  `SubtreeRestore` (undoing a note delete).
- **LWW guard on peers** — the sync-layer check "is this still the LWW winner?"
  is deferred to the sync implementation milestone.
- **Persistent undo across restarts** — in-session only by design.
