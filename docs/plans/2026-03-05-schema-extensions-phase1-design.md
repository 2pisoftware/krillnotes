# Schema Extensions Phase 1 — Design Document

**Spec:** `docs/swarm/KrillNotes_Schema_Extensions_Spec_v0_4.docx` (Sections 2–7, Appendices A–D)
**Parent plan:** `docs/plans/2026-03-05-schema-extensions-v04-overview.md`
**Date:** 2026-03-05
**Branch base:** `master`

Phase 1 delivers: the gated operations model, field-level validation, note-level
validation (reject), and field groups with conditional visibility. This is a
**clean break** — no backward compatibility with the current direct-mutation
on_save pattern.

---

## 1. Gated Operations Model

### 1.1 Current Model (being replaced)

The on_save hook receives a mutable note map, mutates it directly, and returns
the modified map. The caller (`run_on_save_hook`) applies the returned map to
the database. Tree actions use a separate mechanism: `ActionCreate` /
`ActionUpdate` structs queued during closure execution and applied after the
closure returns.

```rhai
// OLD — direct mutation (being removed)
on_save: |note| {
    note.title = "New Title";
    note.fields["x"] = 42;
    note
}
```

### 1.2 New Model: SaveTransaction

All write paths use a shared transactional API with four Rhai-native functions:

- **`set_field(note_id, field_name, value)`** — Queues a field write. Runs the
  field's validate closure (hard error on failure). Updates the in-scope note
  map for read-your-writes semantics.
- **`set_title(note_id, title)`** — Queues a title change. Updates the in-scope
  note map.
- **`reject(message)`** — Accumulates a note-level soft error.
- **`reject(field_name, message)`** — Accumulates a field-pinned soft error.
- **`commit()`** — Validates all pending notes (required checks on visible
  fields). If any reject() calls were made, aborts. Otherwise applies all
  pending writes atomically.
- **`create_child(parent_id, node_type)`** — Creates a new note with schema
  defaults and registers it in the transaction. Returns a note map. Available
  in tree action contexts and on_add_child.

```rhai
// NEW — gated operations
on_save: |note| {
    set_title(note.id, "New Title");
    set_field(note.id, "x", 42);
    commit();
}
```

### 1.3 SaveTransaction Struct

New file: `krillnotes-core/src/core/save_transaction.rs`

```rust
pub struct SaveTransaction {
    /// Pending notes keyed by note ID (supports multi-note tree actions).
    pub pending_notes: BTreeMap<String, PendingNote>,
    /// Accumulated soft errors from reject() calls.
    pub soft_errors: Vec<SoftError>,
    /// Set to true after a successful commit().
    pub committed: bool,
}

pub struct PendingNote {
    pub note_id: String,
    pub is_new: bool,                                // created via create_child()
    pub parent_id: Option<String>,                   // for new notes
    pub node_type: String,
    pub original_fields: BTreeMap<String, FieldValue>,
    pub pending_fields: BTreeMap<String, FieldValue>,
    pub original_title: String,
    pub pending_title: Option<String>,
}

pub struct SoftError {
    pub field: Option<String>,   // None = note-level, Some(name) = field-pinned
    pub message: String,
}
```

The `SaveTransaction` is stored in a thread-local `RefCell` during hook
execution. The four Rhai-native functions access it via the thread-local.
After the hook returns, the orchestrator reads the transaction state and
either commits to DB or returns errors.

### 1.4 Read-Your-Writes

When `set_field()` or `set_title()` is called, the pending value is also
written into the Rhai scope's note map. This means subsequent reads of
`note.fields["x"]` return the pending value, not the original. This is
implemented by mutating the Dynamic map in the Rhai scope — no custom
Rhai types needed.

For multi-note transactions (tree actions), `create_child()` returns a fresh
note map that is also stored in the SaveTransaction. Subsequent `set_field()`
calls on the new note's ID update both the transaction and the Rhai-side map.

### 1.5 Hard vs Soft Errors

| Error type | Trigger | Behavior | Display |
|---|---|---|---|
| Hard | set_field() validate closure fails | Hook aborts immediately | Script error in Script Manager |
| Soft | reject() call | Accumulates; blocks commit() | Inline (field) or banner (note) |

### 1.6 Write Paths Unified

| Context | Available functions | Notes |
|---|---|---|
| on_save | set_field, set_title, reject, commit | Single-note transaction |
| Tree actions | set_field, set_title, reject, commit, create_child | Multi-note transaction |
| on_add_child | set_field, set_title, commit | Receives parent + child; can modify both |

### 1.7 Detection of Old-Style Scripts

If an on_save closure returns a Map (the old direct-mutation pattern), the
engine raises a hard error: "on_save must use set_field()/set_title()/commit()
instead of returning a modified note. See the Krillnotes scripting guide."
This is detected by checking the return type of the closure invocation.

---

## 2. Field-Level Validation

### 2.1 Schema Syntax

```rhai
#{ name: "latitude", type: "number", required: true,
   validate: |v| if v < -90.0 || v > 90.0 { "Must be -90 to 90" } else { () } },
```

### 2.2 Storage

`FieldDefinition` gains:
```rust
pub validate: Option<rhai::FnPtr>,
```

Parsed from the Rhai map in `Schema::parse_from_rhai()`. If the `validate`
key is present, it is extracted as an `FnPtr`.

### 2.3 Execution Contexts

1. **UI on blur** — Frontend calls `validate_field(schema_name, field_name,
   value)` via Tauri IPC. Returns `Option<String>`.

2. **Save pipeline step 2** — `validate_fields(schema_name, fields_map)` runs
   all validate closures on visible fields with values. Returns
   `BTreeMap<String, String>` of field_name → error_message.

3. **Inside set_field()** — Validate closure runs with the proposed value.
   If it returns an error string, this is a hard error (hook aborts). This
   catches script bugs where a hook writes invalid data.

### 2.4 Interaction with required

Independent checks, evaluated in order:
1. Required check first — empty required field produces a "required" error.
2. Validate closure only runs if a value is present.

### 2.5 Type Coercion

The validate closure receives the value after type coercion:
- `number` → f64
- `text`, `textarea`, `email`, `select` → String
- `date` → String (ISO 8601)
- `boolean` → bool
- `rating` → f64
- `note_link`, `file` → String (UUID) or () if empty

---

## 3. Note-Level Validation via reject()

### 3.1 The reject() Function

Two overloads registered as Rhai native functions:

```rhai
reject("Save failed: dates are inconsistent")       // note-level
reject("end_date", "Must be after start date")       // field-pinned
```

Both accumulate errors in `SaveTransaction.soft_errors`. The on_save hook
runs to completion — `reject()` does NOT abort. After the hook finishes,
if any soft errors exist, `commit()` is blocked and all errors are returned
to the caller.

### 3.2 Accumulation

Multiple `reject()` calls in a single on_save produce multiple errors, all
displayed simultaneously. The user fixes everything in one pass.

### 3.3 reject() Outside on_save

`reject()` is also available in tree action closures (same SaveTransaction
context). Calling `reject()` in any other context (on_view, on_hover,
validate closures) produces a runtime error.

### 3.4 Preview of Computed Values

Even when reject() blocks commit(), the preview of computed values (titles,
derived fields) is visible in the UI because set_title()/set_field() calls
still execute and update the Rhai scope's note map. The frontend receives
both the soft errors AND the preview values so the user sees what the result
will look like once errors are fixed.

### 3.5 Return Type

The save pipeline returns a result type to the frontend:

```rust
pub enum SaveResult {
    Ok(Note),
    ValidationErrors {
        field_errors: BTreeMap<String, String>,   // field-pinned errors
        note_errors: Vec<String>,                  // note-level errors
        preview_title: Option<String>,             // from set_title()
        preview_fields: BTreeMap<String, FieldValue>, // from set_field()
    },
}
```

The Tauri command serialises this as a JSON object. The frontend distinguishes
success from validation failure and renders accordingly.

---

## 4. Field Groups

### 4.1 Schema Syntax

```rhai
schema("SpotFireReport", #{
    fields: [
        #{ name: "report_type", type: "select", required: true,
           options: ["Initial Report", "Update", "Final Assessment"] },
    ],
    field_groups: [
        #{
            name: "Fire Behaviour",
            visible: |fields| fields["report_type"] != "Final Assessment",
            collapsed: false,
            fields: [
                #{ name: "wind_speed_kph", type: "number", required: false },
            ],
        },
    ],
});
```

### 4.2 Rust Structs

```rust
pub struct FieldGroup {
    pub name: String,
    pub fields: Vec<FieldDefinition>,
    pub visible: Option<rhai::FnPtr>,
    pub collapsed: bool,
}
```

`Schema` gains:
```rust
pub field_groups: Vec<FieldGroup>,
```

### 4.3 Convenience Methods

```rust
impl Schema {
    /// All fields in declaration order: top-level first, then each group.
    pub fn all_fields(&self) -> Vec<&FieldDefinition> { ... }

    /// Only fields from top-level + visible groups.
    pub fn visible_fields(&self, field_values: &BTreeMap<String, FieldValue>) -> Vec<&FieldDefinition> { ... }
}
```

`visible_fields()` evaluates each group's visibility closure against the
current field values. Groups without a `visible` closure are always visible.

### 4.4 Field Name Uniqueness

Enforced at schema registration time in `parse_from_rhai()`. Duplicate field
names across top-level and groups cause the schema to fail to load with a
clear error message.

### 4.5 Storage

Groups are a schema/UI concept only. Fields inside groups are stored flat
in the note's JSON `fields` column, identical to top-level fields. No DB
schema changes for groups.

### 4.6 Hidden Group Behavior

When a group's `visible` closure returns false:
- Required constraints on the group's fields are suspended.
- Validate closures on the group's fields are not executed (at save time).
- The group's fields are not rendered in edit or view mode.
- Existing values in hidden fields are preserved in the DB.
- on_save CAN still read/write hidden fields via set_field(). If set_field()
  targets a hidden field, the validate closure IS executed (hard error).

### 4.7 Hidden Group Data Indicator

When a group is hidden and any of its fields have non-empty values, the UI
displays a subtle greyed-out group header with an info icon, signaling that
data exists but is currently hidden.

### 4.8 Visibility Evaluation

New Tauri command: `evaluate_group_visibility(schema_name, fields_map)`.
Returns `BTreeMap<String, bool>` (group_name → visible). Called by the
frontend whenever a field value changes in edit mode.

---

## 5. The 7-Step Save Pipeline

Replaces the current `validate_required → run_on_save → apply` flow.

### 5.1 Steps

| Step | Action | Failure |
|------|--------|---------|
| 1 | Evaluate group visibility | — |
| 2 | Run validate closures on visible fields with pending changes | Field errors → return to UI |
| 3 | Required check on visible required fields | Field errors → return to UI |
| 4 | Open SaveTransaction (create PendingNote from current note state) | — |
| 5 | Run on_save hook (set_field, set_title, reject, commit) | Hard error → abort, script error |
| 6 | Check reject errors from SaveTransaction | Soft errors → return to UI |
| 7 | Commit: apply pending writes to DB atomically, log operations | — |

Steps 2–3 run **before** the on_save hook so the user sees basic validation
errors without side effects. If steps 2–3 fail, the hook doesn't run.

Steps 5–6 handle cross-field validation via reject(). The hook runs to
completion (unless a hard error occurs), and soft errors are collected
afterward.

### 5.2 Implementation Location

New method in `workspace.rs`:

```rust
pub fn save_note_with_pipeline(
    &mut self,
    note_id: &str,
    title: &str,
    fields: &BTreeMap<String, FieldValue>,
) -> Result<SaveResult> { ... }
```

This replaces the current `update_note` / `update_note_title` as the primary
save entry point from the frontend. The existing methods remain for
programmatic use but are not called from the Tauri save command.

### 5.3 Frontend Flow

```
User clicks Save
  → Frontend calls save_note(note_id, title, fields) Tauri command
  → Backend runs full 7-step pipeline
  → Returns SaveResult::Ok(note) or SaveResult::ValidationErrors { ... }
  → Frontend displays field errors inline and/or reject banner
  → On success, frontend exits edit mode and refreshes the note
```

The frontend MAY also call `validate_field()` on blur for immediate feedback,
but this is optional UX polish — the backend pipeline is the authoritative
validation.

### 5.4 Tree Action Pipeline

Tree actions use the same SaveTransaction but with a slightly different flow:

| Step | Action |
|------|--------|
| 1 | Create SaveTransaction (empty, multi-note capable) |
| 2 | Run tree action closure (create_child, set_field, set_title, reject, commit) |
| 3 | On commit(): validate all pending notes (visible fields, required checks) |
| 4 | If reject errors or validation failures → abort, return errors |
| 5 | Apply creates + updates to DB atomically, log operations |

The key difference: tree actions don't run steps 2–3 (pre-hook validation)
because the closure IS the mutation source. Validation runs inside commit().

---

## 6. New Tauri Commands

| Command | Parameters | Returns |
|---|---|---|
| `validate_field` | schema_name, field_name, value | `Option<String>` |
| `validate_fields` | schema_name, fields (map) | `BTreeMap<String, String>` |
| `evaluate_group_visibility` | schema_name, fields (map) | `BTreeMap<String, bool>` |
| `save_note` | note_id, title, fields | `SaveResult` (Ok or ValidationErrors) |

The existing `update_note` and `update_note_title` Tauri commands are removed.
The frontend uses `save_note` as the single save entry point.

---

## 7. Frontend Changes

### 7.1 Types (types.ts)

```typescript
interface FieldDef {
    // existing fields...
    hasValidate: boolean;     // true if a validate closure exists
}

interface FieldGroup {
    name: string;
    fields: FieldDef[];
    collapsed: boolean;
    hasVisibleClosure: boolean;
}

interface SchemaInfo {
    // existing fields...
    fieldGroups: FieldGroup[];
}

interface SaveResult {
    ok?: Note;
    validationErrors?: {
        fieldErrors: Record<string, string>;
        noteErrors: string[];
        previewTitle?: string;
        previewFields?: Record<string, FieldValue>;
    };
}
```

### 7.2 InfoPanel Edit Mode

- Top-level fields render as before.
- Each field group renders as a collapsible section with a header.
- When a field value changes, call `evaluate_group_visibility()` to update
  which groups are shown.
- On field blur, call `validate_field()` and display inline error (red text
  below the field).
- Hidden groups with data show a greyed-out header with info icon.

### 7.3 Save Error Display

- Field-pinned errors (from validate or reject) appear as red text below
  the named field.
- Note-level errors (from reject) appear as a red error banner at the top
  of the edit panel.
- Preview values (from set_title/set_field in on_save) are shown even when
  errors block the save, so the user sees the computed result.

---

## 8. System Script Migration

All 6 system scripts and 3 templates are migrated from direct mutation to
the gated API. Example (Task):

```rhai
// BEFORE
on_save: |note| {
    note.title = "[" + symbol + "] " + name;
    note.fields["priority_label"] = "High";
    note
}

// AFTER
on_save: |note| {
    set_title(note.id, "[" + symbol + "] " + name);
    set_field(note.id, "priority_label", "High");
    commit();
}
```

Tree action scripts (sort, create) are migrated to use create_child() +
set_field() + commit().

---

## 9. Rhai Sandbox Constraints

All closures execute within the existing Rhai sandbox. The spec defines:
- Max execution time: 100ms per closure invocation
- Max stack depth: 64
- Max operations: 10,000 per invocation

These limits apply to validate, visible, on_save, on_add_child, and tree
action closures. The gated API functions (set_field, set_title, reject,
commit, create_child) are Rhai-native functions registered on the engine
and do not count toward the operation limit.

---

## 10. Resolved Open Questions

| # | Question | Decision |
|---|----------|----------|
| 1 | Validation errors for hidden groups | Show as note-level banner (don't auto-reveal) |
| 2 | get_groups() in on_save | Not in Phase 1 — add in Phase 2 with presentation functions |
| 3 | set_field() on non-existent fields | Hard error |
| 6 | register_menu() transaction scope | Same model as on_save (unified SaveTransaction) |

---

## 11. Files Changed (Summary)

### New files
- `krillnotes-core/src/core/save_transaction.rs`

### Modified (Rust)
- `krillnotes-core/src/core/scripting/schema.rs` — FieldDefinition.validate, FieldGroup, Schema.field_groups, all_fields(), visible_fields()
- `krillnotes-core/src/core/scripting/mod.rs` — register set_field/set_title/reject/commit/create_child; update run_on_save_hook; update invoke_tree_action_hook; old-style detection
- `krillnotes-core/src/core/scripting/hooks.rs` — remove ActionCreate/ActionUpdate/ActionTxContext (replaced by SaveTransaction)
- `krillnotes-core/src/core/workspace.rs` — save_note_with_pipeline(), update run_tree_action to use SaveTransaction, update on_add_child flow
- `krillnotes-core/src/core/mod.rs` — pub mod save_transaction
- `krillnotes-core/src/lib.rs` — re-export SaveResult, SoftError
- `krillnotes-desktop/src-tauri/src/lib.rs` — validate_field, validate_fields, evaluate_group_visibility, save_note commands

### Modified (TypeScript)
- `krillnotes-desktop/src/types.ts` — FieldDef, FieldGroup, SchemaInfo, SaveResult
- `krillnotes-desktop/src/components/InfoPanel.tsx` — groups, validation display, save flow
- `krillnotes-desktop/src/components/FieldEditor.tsx` — on-blur validation, error display

### Modified (Scripts)
- All 6 system scripts in `krillnotes-core/src/system_scripts/`
- All 3 templates in `templates/`
