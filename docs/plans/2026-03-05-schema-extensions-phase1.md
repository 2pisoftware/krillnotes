# Schema Extensions Phase 1 — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace direct-mutation on_save with a gated operations model (set_field/set_title/reject/commit), add field-level validation closures, note-level reject(), and field groups with conditional visibility.

**Architecture:** A new `SaveTransaction` struct manages pending writes and soft errors across all Rhai write paths (on_save, tree actions, on_add_child). Four Rhai-native functions operate on it via thread-local storage. The 7-step save pipeline replaces the current validate-then-apply flow. Field groups and validate closures extend `Schema` and `FieldDefinition` with `FnPtr` storage.

**Tech Stack:** Rust (krillnotes-core), Rhai scripting engine, Tauri v2, React 19, TypeScript

**Design doc:** `docs/plans/2026-03-05-schema-extensions-phase1-design.md`

---

## Task 1: Create SaveTransaction module

**Files:**
- Create: `krillnotes-core/src/core/save_transaction.rs`
- Modify: `krillnotes-core/src/core/mod.rs:12-26`
- Modify: `krillnotes-core/src/lib.rs:19-38`

**Step 1: Create the SaveTransaction module with structs**

Create `krillnotes-core/src/core/save_transaction.rs`:

```rust
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2024-2026 TripleACS Pty Ltd t/a 2pi Software

//! Gated operations model for Rhai write paths.
//!
//! [`SaveTransaction`] collects pending field/title writes and soft errors
//! from `set_field()`, `set_title()`, `reject()`, and `commit()` calls
//! during on_save hooks, tree actions, and on_add_child hooks.

use std::collections::BTreeMap;
use crate::core::note::FieldValue;

/// A soft validation error accumulated by `reject()`.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SoftError {
    /// `None` = note-level error, `Some(name)` = field-pinned error.
    pub field: Option<String>,
    pub message: String,
}

/// Pending state for a single note within a [`SaveTransaction`].
#[derive(Debug, Clone)]
pub struct PendingNote {
    pub note_id: String,
    /// True if created via `create_child()` within this transaction.
    pub is_new: bool,
    /// Parent ID (only meaningful when `is_new` is true).
    pub parent_id: Option<String>,
    pub node_type: String,
    pub original_fields: BTreeMap<String, FieldValue>,
    pub pending_fields: BTreeMap<String, FieldValue>,
    pub original_title: String,
    pub pending_title: Option<String>,
}

impl PendingNote {
    /// Returns the current effective title (pending or original).
    pub fn effective_title(&self) -> &str {
        self.pending_title.as_deref().unwrap_or(&self.original_title)
    }

    /// Returns the current effective fields (original merged with pending).
    pub fn effective_fields(&self) -> BTreeMap<String, FieldValue> {
        let mut fields = self.original_fields.clone();
        for (k, v) in &self.pending_fields {
            fields.insert(k.clone(), v.clone());
        }
        fields
    }
}

/// Collects pending writes and soft errors during a Rhai write-path hook.
///
/// Supports single-note (on_save) and multi-note (tree actions) transactions.
#[derive(Debug, Clone)]
pub struct SaveTransaction {
    pub pending_notes: BTreeMap<String, PendingNote>,
    pub soft_errors: Vec<SoftError>,
    pub committed: bool,
}

impl SaveTransaction {
    /// Creates an empty transaction.
    pub fn new() -> Self {
        Self {
            pending_notes: BTreeMap::new(),
            soft_errors: Vec::new(),
            committed: false,
        }
    }

    /// Creates a transaction pre-loaded with one existing note (for on_save).
    pub fn for_existing_note(
        note_id: String,
        node_type: String,
        title: String,
        fields: BTreeMap<String, FieldValue>,
    ) -> Self {
        let mut tx = Self::new();
        tx.pending_notes.insert(note_id.clone(), PendingNote {
            note_id,
            is_new: false,
            parent_id: None,
            node_type,
            original_fields: fields,
            pending_fields: BTreeMap::new(),
            original_title: title,
            pending_title: None,
        });
        tx
    }

    /// Registers a newly created child note in the transaction.
    pub fn add_new_note(
        &mut self,
        note_id: String,
        parent_id: String,
        node_type: String,
        title: String,
        fields: BTreeMap<String, FieldValue>,
    ) {
        self.pending_notes.insert(note_id.clone(), PendingNote {
            note_id,
            is_new: true,
            parent_id: Some(parent_id),
            node_type,
            original_fields: fields.clone(),
            pending_fields: fields,
            original_title: title,
            pending_title: None,
        });
    }

    /// Queues a field write. Returns the note's effective fields after the write.
    ///
    /// # Errors
    ///
    /// Returns an error if `note_id` is not in this transaction.
    pub fn set_field(&mut self, note_id: &str, field: String, value: FieldValue) -> Result<(), String> {
        let pending = self.pending_notes.get_mut(note_id)
            .ok_or_else(|| format!("Note '{}' is not in this transaction", note_id))?;
        pending.pending_fields.insert(field, value);
        Ok(())
    }

    /// Queues a title write.
    ///
    /// # Errors
    ///
    /// Returns an error if `note_id` is not in this transaction.
    pub fn set_title(&mut self, note_id: &str, title: String) -> Result<(), String> {
        let pending = self.pending_notes.get_mut(note_id)
            .ok_or_else(|| format!("Note '{}' is not in this transaction", note_id))?;
        pending.pending_title = Some(title);
        Ok(())
    }

    /// Accumulates a note-level soft error.
    pub fn reject_note(&mut self, message: String) {
        self.soft_errors.push(SoftError { field: None, message });
    }

    /// Accumulates a field-pinned soft error.
    pub fn reject_field(&mut self, field: String, message: String) {
        self.soft_errors.push(SoftError { field: Some(field), message });
    }

    /// Returns true if any soft errors have been accumulated.
    pub fn has_errors(&self) -> bool {
        !self.soft_errors.is_empty()
    }

    /// Marks the transaction as committed (caller must still apply to DB).
    ///
    /// # Errors
    ///
    /// Returns an error if soft errors exist (commit blocked).
    pub fn commit(&mut self) -> Result<(), Vec<SoftError>> {
        if self.has_errors() {
            Err(self.soft_errors.clone())
        } else {
            self.committed = true;
            Ok(())
        }
    }
}

/// Result of the save pipeline returned to the frontend.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SaveResult {
    /// Save succeeded — returns the updated note.
    Ok(crate::core::note::Note),
    /// Validation or reject errors blocked the save.
    ValidationErrors {
        /// Field-pinned errors: field_name -> error message.
        field_errors: BTreeMap<String, String>,
        /// Note-level errors from reject().
        note_errors: Vec<String>,
        /// Preview title from set_title() (if any).
        preview_title: Option<String>,
        /// Preview fields from set_field() calls.
        preview_fields: BTreeMap<String, FieldValue>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_transaction_is_empty() {
        let tx = SaveTransaction::new();
        assert!(tx.pending_notes.is_empty());
        assert!(tx.soft_errors.is_empty());
        assert!(!tx.committed);
    }

    #[test]
    fn test_for_existing_note_populates_pending() {
        let mut fields = BTreeMap::new();
        fields.insert("body".to_string(), FieldValue::Text("hello".to_string()));
        let tx = SaveTransaction::for_existing_note(
            "n1".to_string(), "TextNote".to_string(), "Title".to_string(), fields,
        );
        assert_eq!(tx.pending_notes.len(), 1);
        let pn = tx.pending_notes.get("n1").unwrap();
        assert!(!pn.is_new);
        assert_eq!(pn.effective_title(), "Title");
    }

    #[test]
    fn test_set_field_updates_pending() {
        let tx_fields = BTreeMap::new();
        let mut tx = SaveTransaction::for_existing_note(
            "n1".to_string(), "T".to_string(), "T".to_string(), tx_fields,
        );
        tx.set_field("n1", "x".to_string(), FieldValue::Number(42.0)).unwrap();
        let eff = tx.pending_notes.get("n1").unwrap().effective_fields();
        assert_eq!(eff.get("x"), Some(&FieldValue::Number(42.0)));
    }

    #[test]
    fn test_set_title_updates_pending() {
        let mut tx = SaveTransaction::for_existing_note(
            "n1".to_string(), "T".to_string(), "Old".to_string(), BTreeMap::new(),
        );
        tx.set_title("n1", "New".to_string()).unwrap();
        assert_eq!(tx.pending_notes.get("n1").unwrap().effective_title(), "New");
    }

    #[test]
    fn test_set_field_unknown_note_errors() {
        let mut tx = SaveTransaction::new();
        let result = tx.set_field("missing", "x".to_string(), FieldValue::Number(1.0));
        assert!(result.is_err());
    }

    #[test]
    fn test_reject_accumulates_errors() {
        let mut tx = SaveTransaction::new();
        tx.reject_note("bad".to_string());
        tx.reject_field("f".to_string(), "invalid".to_string());
        assert_eq!(tx.soft_errors.len(), 2);
        assert!(tx.has_errors());
    }

    #[test]
    fn test_commit_blocked_by_errors() {
        let mut tx = SaveTransaction::new();
        tx.reject_note("nope".to_string());
        let result = tx.commit();
        assert!(result.is_err());
        assert!(!tx.committed);
    }

    #[test]
    fn test_commit_succeeds_when_clean() {
        let mut tx = SaveTransaction::new();
        let result = tx.commit();
        assert!(result.is_ok());
        assert!(tx.committed);
    }

    #[test]
    fn test_add_new_note() {
        let mut tx = SaveTransaction::new();
        let mut fields = BTreeMap::new();
        fields.insert("body".to_string(), FieldValue::Text(String::new()));
        tx.add_new_note("c1".to_string(), "p1".to_string(), "TextNote".to_string(), "".to_string(), fields);
        assert_eq!(tx.pending_notes.len(), 1);
        let pn = tx.pending_notes.get("c1").unwrap();
        assert!(pn.is_new);
        assert_eq!(pn.parent_id.as_deref(), Some("p1"));
    }

    #[test]
    fn test_effective_fields_merges_original_and_pending() {
        let mut orig = BTreeMap::new();
        orig.insert("a".to_string(), FieldValue::Text("original".to_string()));
        orig.insert("b".to_string(), FieldValue::Text("keep".to_string()));
        let mut tx = SaveTransaction::for_existing_note(
            "n1".to_string(), "T".to_string(), "T".to_string(), orig,
        );
        tx.set_field("n1", "a".to_string(), FieldValue::Text("updated".to_string())).unwrap();
        let eff = tx.pending_notes.get("n1").unwrap().effective_fields();
        assert_eq!(eff.get("a"), Some(&FieldValue::Text("updated".to_string())));
        assert_eq!(eff.get("b"), Some(&FieldValue::Text("keep".to_string())));
    }
}
```

**Step 2: Wire the module into the crate**

Add `pub mod save_transaction;` to `krillnotes-core/src/core/mod.rs` after the existing modules.

Add re-exports to `krillnotes-core/src/lib.rs`:
```rust
pub use core::save_transaction::{SaveResult, SaveTransaction, SoftError};
```

**Step 3: Run tests**

Run: `cargo test -p krillnotes-core save_transaction`
Expected: All 9 new tests pass.

**Step 4: Commit**

```
feat: add SaveTransaction module for gated operations model
```

---

## Task 2: Extend FieldDefinition with validate and Schema with field_groups

**Files:**
- Modify: `krillnotes-core/src/core/scripting/schema.rs:37-61` (FieldDefinition)
- Modify: `krillnotes-core/src/core/scripting/schema.rs:65-83` (Schema)
- Modify: `krillnotes-core/src/core/scripting/schema.rs:150-295` (parse_from_rhai)

**Step 1: Add validate to FieldDefinition and FieldGroup + field_groups to Schema**

In `schema.rs`, add to `FieldDefinition` struct after `allowed_types`:
```rust
    /// Field-level validation closure. Receives the field value, returns ()
    /// for valid or a String error message for invalid.
    #[serde(skip)]
    pub validate: Option<rhai::FnPtr>,
```

Add new struct after `FieldDefinition`:
```rust
/// A named group of fields with optional conditional visibility.
#[derive(Debug, Clone)]
pub struct FieldGroup {
    pub name: String,
    pub fields: Vec<FieldDefinition>,
    /// Visibility closure: |fields_map| -> bool. None means always visible.
    pub visible: Option<rhai::FnPtr>,
    /// Initial collapsed state in the UI.
    pub collapsed: bool,
}
```

Add to `Schema` struct after `attachment_types`:
```rust
    /// Named field groups with optional visibility rules.
    pub field_groups: Vec<FieldGroup>,
```

Add convenience methods to `impl Schema`:
```rust
    /// All fields in declaration order: top-level first, then each group's fields.
    pub fn all_fields(&self) -> Vec<&FieldDefinition> {
        self.fields.iter()
            .chain(self.field_groups.iter().flat_map(|g| g.fields.iter()))
            .collect()
    }
```

**Step 2: Update parse_from_rhai to extract validate and field_groups**

In the field parsing loop (around line 233), after extracting `allowed_types`, add:
```rust
            let validate: Option<rhai::FnPtr> = field_map
                .get("validate")
                .and_then(|v| v.clone().try_cast::<rhai::FnPtr>());
```

And include `validate` in the `FieldDefinition` construction.

After the existing `attachment_types` parsing (around line 293), add field_groups parsing:
```rust
        let mut field_groups: Vec<FieldGroup> = Vec::new();
        if let Some(groups_array) = def
            .get("field_groups")
            .and_then(|v| v.clone().try_cast::<rhai::Array>())
        {
            let mut all_field_names: std::collections::HashSet<String> =
                fields.iter().map(|f| f.name.clone()).collect();

            for group_item in groups_array {
                let group_map = group_item
                    .try_cast::<Map>()
                    .ok_or_else(|| KrillnotesError::Scripting("field_groups entry must be a map".to_string()))?;

                let group_name = group_map
                    .get("name")
                    .and_then(|v| v.clone().try_cast::<String>())
                    .ok_or_else(|| KrillnotesError::Scripting("field group missing 'name'".to_string()))?;

                let collapsed = group_map
                    .get("collapsed")
                    .and_then(|v| v.clone().try_cast::<bool>())
                    .unwrap_or(false);

                let visible: Option<rhai::FnPtr> = group_map
                    .get("visible")
                    .and_then(|v| v.clone().try_cast::<rhai::FnPtr>());

                // Parse fields within the group (same logic as top-level fields).
                let group_fields_array = group_map
                    .get("fields")
                    .and_then(|v| v.clone().try_cast::<rhai::Array>())
                    .ok_or_else(|| KrillnotesError::Scripting(
                        format!("field group '{}' missing 'fields' array", group_name)
                    ))?;

                let mut group_fields = Vec::new();
                for field_item in group_fields_array {
                    // Re-use the same field parsing logic as top-level fields.
                    // (Extract this into a helper function to avoid duplication.)
                    let field = Self::parse_field_def(&field_item)?;

                    // Enforce field name uniqueness across entire schema.
                    if !all_field_names.insert(field.name.clone()) {
                        return Err(KrillnotesError::Scripting(format!(
                            "Duplicate field name '{}' in schema '{}'", field.name, name
                        )));
                    }
                    group_fields.push(field);
                }

                field_groups.push(FieldGroup { name: group_name, fields: group_fields, visible, collapsed });
            }
        }
```

**Important refactor:** Extract the field-parsing loop body into a `Schema::parse_field_def(field_item: &Dynamic) -> Result<FieldDefinition>` helper method so both top-level and group fields use the same code. This avoids duplicating ~50 lines.

Update the `Ok(Schema { ... })` return to include `field_groups`.

**Step 3: Update validate_required_fields to use all_fields()**

Replace `for field_def in &self.fields` with `for field_def in self.all_fields()` in `validate_required_fields()`.

**Step 4: Update default_fields() to use all_fields()**

Replace `for field_def in &self.fields` with `for field_def in self.all_fields()` in `default_fields()`.

**Step 5: Run tests**

Run: `cargo test -p krillnotes-core`
Expected: All existing tests still pass (no scripts use `validate` or `field_groups` yet, so these are additive).

**Step 6: Commit**

```
feat: extend FieldDefinition with validate closure and Schema with field_groups
```

---

## Task 3: Add FieldGroup and validate to the SchemaInfo Tauri response

**Files:**
- Modify: `krillnotes-desktop/src-tauri/src/lib.rs` (get_schema_info command)
- Modify: `krillnotes-desktop/src/types.ts:45-74`

**Step 1: Find and update the get_schema_info Tauri command**

Search for the `get_schema_info` command in `lib.rs`. It currently constructs a response object from `Schema`. Add:
- `hasValidate: bool` to each field definition in the response
- `fieldGroups: Vec<FieldGroupInfo>` to the schema info response

Define a serializable `FieldGroupInfo`:
```rust
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct FieldGroupInfo {
    name: String,
    fields: Vec<FieldDefInfo>,  // same shape as top-level field defs
    collapsed: bool,
    has_visible_closure: bool,
}
```

**Step 2: Update TypeScript types**

In `types.ts`, add to `FieldDefinition`:
```typescript
  hasValidate: boolean;
```

Add new interface:
```typescript
export interface FieldGroup {
  name: string;
  fields: FieldDefinition[];
  collapsed: boolean;
  hasVisibleClosure: boolean;
}
```

Add to `SchemaInfo`:
```typescript
  fieldGroups: FieldGroup[];
```

**Step 3: Run type check**

Run: `cd krillnotes-desktop && npx tsc --noEmit`
Expected: No errors (the new fields are additive; existing code doesn't reference them yet).

**Step 4: Commit**

```
feat: expose field groups and validate flag in SchemaInfo
```

---

## Task 4: Register set_field, set_title, reject, commit as Rhai native functions

**Files:**
- Modify: `krillnotes-core/src/core/scripting/mod.rs:103-215` (ScriptRegistry::new)

**Step 1: Add thread-local SaveTransaction storage**

At the top of `mod.rs`, add:
```rust
use std::cell::RefCell;
use crate::core::save_transaction::SaveTransaction;

thread_local! {
    static SAVE_TX: RefCell<Option<SaveTransaction>> = RefCell::new(None);
}

/// Sets the active SaveTransaction for the current thread (used by hook runners).
pub(crate) fn set_save_tx(tx: SaveTransaction) {
    SAVE_TX.with(|cell| *cell.borrow_mut() = Some(tx));
}

/// Takes the active SaveTransaction from the current thread (used after hook returns).
pub(crate) fn take_save_tx() -> Option<SaveTransaction> {
    SAVE_TX.with(|cell| cell.borrow_mut().take())
}

/// Accesses the active SaveTransaction (used by Rhai native functions).
fn with_save_tx<F, R>(f: F) -> std::result::Result<R, Box<rhai::EvalAltResult>>
where
    F: FnOnce(&mut SaveTransaction) -> std::result::Result<R, String>,
{
    SAVE_TX.with(|cell| {
        let mut borrow = cell.borrow_mut();
        let tx = borrow.as_mut().ok_or_else(|| -> Box<rhai::EvalAltResult> {
            "set_field/set_title/reject/commit called outside a write context".to_string().into()
        })?;
        f(tx).map_err(|e| -> Box<rhai::EvalAltResult> { e.into() })
    })
}
```

**Step 2: Register the four Rhai functions in ScriptRegistry::new()**

After the existing `schema()` registration (around line 214), add:

```rust
        // ── Gated operations API ──────────────────────────────────────────────
        engine.register_fn("set_field",
            |note_id: String, field_name: String, value: Dynamic|
            -> std::result::Result<Dynamic, Box<EvalAltResult>>
            {
                let fv = dynamic_to_field_value(value.clone(), "text")
                    .map_err(|e| -> Box<EvalAltResult> { e.to_string().into() })?;
                with_save_tx(|tx| tx.set_field(&note_id, field_name, fv))?;
                Ok(Dynamic::UNIT)
            }
        );

        engine.register_fn("set_title",
            |note_id: String, title: String|
            -> std::result::Result<Dynamic, Box<EvalAltResult>>
            {
                with_save_tx(|tx| tx.set_title(&note_id, title))?;
                Ok(Dynamic::UNIT)
            }
        );

        // reject() with one arg = note-level
        engine.register_fn("reject",
            |message: String| -> std::result::Result<Dynamic, Box<EvalAltResult>> {
                with_save_tx(|tx| { tx.reject_note(message); Ok(()) })?;
                Ok(Dynamic::UNIT)
            }
        );

        // reject() with two args = field-pinned
        engine.register_fn("reject",
            |field: String, message: String| -> std::result::Result<Dynamic, Box<EvalAltResult>> {
                with_save_tx(|tx| { tx.reject_field(field, message); Ok(()) })?;
                Ok(Dynamic::UNIT)
            }
        );

        engine.register_fn("commit",
            || -> std::result::Result<Dynamic, Box<EvalAltResult>> {
                with_save_tx(|tx| {
                    tx.commit().map_err(|errors| {
                        let msgs: Vec<String> = errors.iter().map(|e| {
                            match &e.field {
                                Some(f) => format!("{}: {}", f, e.message),
                                None => e.message.clone(),
                            }
                        }).collect();
                        format!("Validation failed: {}", msgs.join("; "))
                    })
                })?;
                Ok(Dynamic::UNIT)
            }
        );
```

**Note on set_field type inference:** The `dynamic_to_field_value` call needs the schema's field type for correct coercion. For now, we'll use a simplified approach that infers the type from the Dynamic value (string→Text, float→Number, bool→Boolean). We'll refine this in a later step when we wire validate closures into set_field. The existing `dynamic_to_field_value` helper already handles this when given the field_type string.

**Step 3: Run tests**

Run: `cargo test -p krillnotes-core`
Expected: All existing tests pass. The new functions are registered but not yet called by any scripts.

**Step 4: Commit**

```
feat: register set_field, set_title, reject, commit Rhai native functions
```

---

## Task 5: Rewrite run_on_save_hook to use SaveTransaction

**Files:**
- Modify: `krillnotes-core/src/core/scripting/mod.rs:647-658` (run_on_save_hook)
- Modify: `krillnotes-core/src/core/scripting/schema.rs:378-465` (SchemaRegistry::run_on_save_hook)

**Step 1: Update SchemaRegistry::run_on_save_hook**

The current `run_on_save_hook` in `schema.rs` calls the on_save closure and expects it to return a map `(title, fields)`. Rewrite it to:

1. Create a `SaveTransaction::for_existing_note(...)`.
2. Store it via `set_save_tx()`.
3. Call the on_save closure (which now uses set_field/set_title/reject/commit internally).
4. Retrieve the transaction via `take_save_tx()`.
5. Check the return value: if the closure returned a Map (old style), raise a hard error with the migration message.
6. Return the SaveTransaction (or errors) to the caller.

Change the return type from `Result<Option<(String, BTreeMap<String, FieldValue>)>>` to `Result<SaveTransaction>`.

**Step 2: Update ScriptRegistry::run_on_save_hook wrapper**

In `mod.rs:647-658`, update the signature to match the new return type.

**Step 3: Update workspace.rs callers**

The main caller is `workspace.rs:update_note()` (line ~2628-2635). Update it to read the SaveTransaction results:
- If `tx.committed` is true, extract `effective_title()` and `effective_fields()` from the pending note.
- If `tx.has_errors()`, convert soft errors into `SaveResult::ValidationErrors`.
- If the hook didn't call commit(), treat as no-op (preserve original values).

**Step 4: Write a test for gated on_save**

```rust
#[test]
fn test_on_save_gated_model() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::create(dir.path(), "", None).unwrap();
    ws.load_script(r#"
        schema("GatedTest", #{
            fields: [
                #{ name: "body", type: "text", required: false },
            ],
            on_save: |note| {
                set_title(note.id, "Computed: " + note.fields["body"]);
                commit();
            },
        });
    "#, "gated_test").unwrap();

    let note = ws.create_note(None, "GatedTest").unwrap();
    let mut fields = BTreeMap::new();
    fields.insert("body".to_string(), FieldValue::Text("hello".to_string()));
    let updated = ws.update_note(&note.id, "ignored".to_string(), fields).unwrap();
    assert_eq!(updated.title, "Computed: hello");
}
```

**Step 5: Write a test for old-style on_save detection**

```rust
#[test]
fn test_old_style_on_save_raises_error() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::create(dir.path(), "", None).unwrap();
    ws.load_script(r#"
        schema("OldStyle", #{
            fields: [
                #{ name: "body", type: "text", required: false },
            ],
            on_save: |note| {
                note.title = "Old Style";
                note
            },
        });
    "#, "old_style_test").unwrap();

    let note = ws.create_note(None, "OldStyle").unwrap();
    let result = ws.update_note(&note.id, "test".to_string(), BTreeMap::new());
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("set_field"));
}
```

**Step 6: Run tests**

Run: `cargo test -p krillnotes-core`
Expected: New tests pass. Existing tests that use on_save will FAIL because system scripts still use the old pattern — that's expected and fixed in Task 8.

**Step 7: Commit**

```
feat: rewrite on_save hook to use SaveTransaction gated model
```

---

## Task 6: Rewrite tree action execution to use SaveTransaction

**Files:**
- Modify: `krillnotes-core/src/core/scripting/mod.rs:810-865` (invoke_tree_action_hook)
- Modify: `krillnotes-core/src/core/scripting/hooks.rs` (ActionTxContext removal)
- Modify: `krillnotes-core/src/core/workspace.rs:1788-1956` (run_tree_action_inner)

**Step 1: Register create_child() Rhai function**

Add `create_child` to the engine registration in `ScriptRegistry::new()`. It needs access to the schema registry to generate default fields:

```rust
        let create_child_schemas = schema_registry.schemas_arc();
        engine.register_fn("create_child",
            move |parent_id: String, node_type: String|
            -> std::result::Result<Dynamic, Box<EvalAltResult>>
            {
                let schemas = create_child_schemas.lock().unwrap();
                let schema = schemas.get(&node_type).ok_or_else(|| -> Box<EvalAltResult> {
                    format!("Schema '{}' not found", node_type).into()
                })?;
                let default_fields = schema.default_fields();
                let note_id = uuid::Uuid::new_v4().to_string();

                with_save_tx(|tx| {
                    tx.add_new_note(
                        note_id.clone(),
                        parent_id.clone(),
                        node_type.clone(),
                        String::new(),
                        default_fields.clone(),
                    );
                    Ok(())
                }).map_err(|e| -> Box<EvalAltResult> { e.to_string().into() })?;

                // Return a note map to the script.
                let mut map = rhai::Map::new();
                map.insert("id".into(), Dynamic::from(note_id));
                map.insert("parent_id".into(), Dynamic::from(parent_id));
                map.insert("node_type".into(), Dynamic::from(node_type));
                map.insert("title".into(), Dynamic::from(String::new()));
                // ... fields map ...
                Ok(Dynamic::from_map(map))
            }
        );
```

**Step 2: Update invoke_tree_action_hook**

Replace the `ActionTxContext` mechanism with SaveTransaction. Before calling the closure, `set_save_tx(SaveTransaction::new())`. After, `take_save_tx()`.

Remove the old `create_note` and `update_note` Rhai registrations that wrote to `ActionTxContext`.

**Step 3: Update run_tree_action_inner in workspace.rs**

Replace the `result.creates` / `result.updates` application with reading from the SaveTransaction's `pending_notes`. For each `PendingNote` where `is_new` is true, INSERT into the DB. For each where `is_new` is false and has pending changes, UPDATE.

**Step 4: Remove ActionCreate, ActionUpdate, ActionTxContext from hooks.rs**

These are replaced by SaveTransaction. The `TreeActionResult` struct simplifies to just the reorder field.

**Step 5: Write test for tree action with gated model**

```rust
#[test]
fn test_tree_action_create_child_gated() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::create(dir.path(), "", None).unwrap();
    ws.load_script(r#"
        schema("TAFolder", #{ fields: [] });
        schema("TAItem", #{
            fields: [
                #{ name: "value", type: "text", required: false },
            ],
        });
        add_tree_action("Add Item", ["TAFolder"], |note| {
            let child = create_child(note.id, "TAItem");
            set_title(child.id, "New Item");
            set_field(child.id, "value", "default");
            commit();
        });
    "#, "ta_test").unwrap();

    let folder = ws.create_note(None, "TAFolder").unwrap();
    ws.run_tree_action(&folder.id, "Add Item").unwrap();
    let children = ws.get_children(&folder.id).unwrap();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].title, "New Item");
}
```

**Step 6: Run tests**

Run: `cargo test -p krillnotes-core`
Expected: New test passes. Old tree action tests will need updating to new syntax — fix them in this step.

**Step 7: Commit**

```
feat: rewrite tree actions to use SaveTransaction with create_child/set_field/commit
```

---

## Task 7: Add validate_field and evaluate_group_visibility to ScriptRegistry

**Files:**
- Modify: `krillnotes-core/src/core/scripting/mod.rs` (new public methods)
- Modify: `krillnotes-core/src/core/scripting/schema.rs` (add run_validate)

**Step 1: Add validate_field method**

In `ScriptRegistry`, add:
```rust
    /// Runs a field's validate closure. Returns None if valid, Some(error) if invalid.
    pub fn validate_field(
        &self,
        schema_name: &str,
        field_name: &str,
        value: &FieldValue,
    ) -> Result<Option<String>> { ... }
```

Implementation: look up the schema, find the field (in top-level or groups), check if it has a validate FnPtr, call it with the value converted to Dynamic, check return type (unit = valid, string = error).

**Step 2: Add validate_fields method (batch)**

```rust
    pub fn validate_fields(
        &self,
        schema_name: &str,
        fields: &BTreeMap<String, FieldValue>,
    ) -> Result<BTreeMap<String, String>> { ... }
```

Iterates all fields in the schema, calls validate on each that has a closure and a value.

**Step 3: Add evaluate_group_visibility method**

```rust
    pub fn evaluate_group_visibility(
        &self,
        schema_name: &str,
        fields: &BTreeMap<String, FieldValue>,
    ) -> Result<BTreeMap<String, bool>> { ... }
```

For each FieldGroup, evaluate its `visible` FnPtr (if any) with the fields map. Return group_name → visible.

**Step 4: Write tests**

```rust
#[test]
fn test_validate_field_returns_error() {
    // Script with validate: |v| if v < 0.0 { "Must be positive" } else { () }
    // Call validate_field with -1.0, expect Some("Must be positive")
}

#[test]
fn test_validate_field_returns_none_on_valid() {
    // Call with 5.0, expect None
}

#[test]
fn test_evaluate_group_visibility() {
    // Script with visible: |fields| fields["type"] == "special"
    // Call with type="special" → true, type="other" → false
}
```

**Step 5: Run tests**

Run: `cargo test -p krillnotes-core`

**Step 6: Commit**

```
feat: add validate_field, validate_fields, evaluate_group_visibility to ScriptRegistry
```

---

## Task 8: Migrate all system scripts to gated on_save

**Files:**
- Modify: `krillnotes-core/src/system_scripts/00_text_note.rhai`
- Modify: `krillnotes-core/src/system_scripts/01_contact.rhai`
- Modify: `krillnotes-core/src/system_scripts/02_task.rhai`
- Modify: `krillnotes-core/src/system_scripts/03_project.rhai`
- Modify: `krillnotes-core/src/system_scripts/05_recipe.rhai`
- Modify: `krillnotes-core/src/system_scripts/06_product.rhai`

**Step 1: Migrate each script's on_save hook**

Pattern for each script:
```rhai
// BEFORE
on_save: |note| {
    note.title = "...";
    note.fields["x"] = "...";
    note
}

// AFTER
on_save: |note| {
    set_title(note.id, "...");
    set_field(note.id, "x", "...");
    commit();
}
```

Also migrate any `add_tree_action` closures that use `create_note()` / `update_note()` to the `create_child()` / `set_field()` / `set_title()` / `commit()` pattern.

**Step 2: Add validate closures to a few fields as examples**

- Task: `due_date` validate (cannot be in the past, optional)
- Contact: `email` validate (basic format check)
- Recipe: `servings` validate (must be positive)

**Step 3: Run all tests**

Run: `cargo test -p krillnotes-core`
Expected: ALL existing tests now pass again (system scripts use the new pattern).

**Step 4: Commit**

```
feat: migrate all system scripts to gated operations model
```

---

## Task 9: Migrate templates to gated on_save

**Files:**
- Modify: `templates/book_collection.rhai`
- Modify: `templates/zettelkasten.rhai`
- Modify: `templates/photo_note.rhai`

**Step 1: Update each template's on_save and tree action closures**

Same pattern as Task 8.

**Step 2: Run tests**

Run: `cargo test -p krillnotes-core`

**Step 3: Commit**

```
feat: migrate templates to gated operations model
```

---

## Task 10: Implement save_note_with_pipeline in workspace.rs

**Files:**
- Modify: `krillnotes-core/src/core/workspace.rs`

**Step 1: Implement the 7-step pipeline**

Add a new public method:

```rust
pub fn save_note_with_pipeline(
    &mut self,
    note_id: &str,
    title: String,
    fields: BTreeMap<String, FieldValue>,
) -> Result<SaveResult> {
    let note = self.get_note(note_id)?;
    let schema = self.script_registry.get_schema(&note.node_type)?;

    // Step 1: Evaluate group visibility.
    let visibility = self.script_registry.evaluate_group_visibility(
        &note.node_type, &fields,
    )?;

    // Step 2: Run validate closures on visible fields.
    let visible_field_names: HashSet<String> = schema.fields.iter()
        .map(|f| f.name.clone())
        .chain(schema.field_groups.iter()
            .filter(|g| visibility.get(&g.name).copied().unwrap_or(true))
            .flat_map(|g| g.fields.iter().map(|f| f.name.clone())))
        .collect();

    let validation_errors = self.script_registry.validate_fields(
        &note.node_type, &fields,
    )?;
    let visible_errors: BTreeMap<String, String> = validation_errors.into_iter()
        .filter(|(k, _)| visible_field_names.contains(k))
        .collect();

    if !visible_errors.is_empty() {
        return Ok(SaveResult::ValidationErrors {
            field_errors: visible_errors,
            note_errors: vec![],
            preview_title: None,
            preview_fields: BTreeMap::new(),
        });
    }

    // Step 3: Required check on visible required fields.
    // (Use visible_fields logic — skip hidden group fields)
    // ... required check returning field_errors ...

    // Steps 4-7: Delegate to update_note which now uses SaveTransaction internally.
    let updated_note = self.update_note(note_id, title, fields)?;
    Ok(SaveResult::Ok(updated_note))
}
```

This is the orchestration layer. The existing `update_note` handles steps 4-7 internally (it already calls run_on_save_hook which creates the SaveTransaction).

**Step 2: Handle SaveTransaction errors in update_note**

When `run_on_save_hook` returns a SaveTransaction with soft errors, `update_note` should return `SaveResult::ValidationErrors` instead of proceeding. This requires changing `update_note`'s return type or adding a separate internal path.

**Step 3: Write tests**

```rust
#[test]
fn test_save_pipeline_validation_error() {
    // Schema with validate: |v| if v < 0.0 { "Must be positive" } else { () }
    // Save with value = -1.0
    // Expect SaveResult::ValidationErrors with field_errors containing the field
}

#[test]
fn test_save_pipeline_reject_error() {
    // Schema with on_save: |note| { reject("bad"); commit(); }
    // Save → expect SaveResult::ValidationErrors with note_errors
}

#[test]
fn test_save_pipeline_success() {
    // Normal save with valid data → expect SaveResult::Ok(note)
}
```

**Step 4: Run tests**

Run: `cargo test -p krillnotes-core`

**Step 5: Commit**

```
feat: implement 7-step save pipeline in workspace.rs
```

---

## Task 11: Add Tauri commands for validation and save

**Files:**
- Modify: `krillnotes-desktop/src-tauri/src/lib.rs`

**Step 1: Add validate_field command**

```rust
#[tauri::command]
fn validate_field(
    window: Window,
    state: State<'_, AppState>,
    schema_name: String,
    field_name: String,
    value: serde_json::Value,
) -> Result<Option<String>, String> {
    // Convert JSON value to FieldValue, look up workspace, call validate_field
}
```

**Step 2: Add validate_fields command**

```rust
#[tauri::command]
fn validate_fields(
    window: Window,
    state: State<'_, AppState>,
    schema_name: String,
    fields: HashMap<String, serde_json::Value>,
) -> Result<HashMap<String, String>, String> { ... }
```

**Step 3: Add evaluate_group_visibility command**

```rust
#[tauri::command]
fn evaluate_group_visibility(
    window: Window,
    state: State<'_, AppState>,
    schema_name: String,
    fields: HashMap<String, serde_json::Value>,
) -> Result<HashMap<String, bool>, String> { ... }
```

**Step 4: Add save_note command (replacing update_note)**

```rust
#[tauri::command]
fn save_note(
    window: Window,
    state: State<'_, AppState>,
    note_id: String,
    title: String,
    fields: BTreeMap<String, FieldValue>,
) -> Result<SaveResult, String> { ... }
```

**Step 5: Register all new commands in generate_handler**

Add `validate_field`, `validate_fields`, `evaluate_group_visibility`, `save_note` to the handler list.

**Step 6: Run build check**

Run: `cd krillnotes-desktop && cargo check -p krillnotes-desktop`

**Step 7: Commit**

```
feat: add Tauri commands for validation, group visibility, and save pipeline
```

---

## Task 12: Frontend — Field groups in InfoPanel edit mode

**Files:**
- Modify: `krillnotes-desktop/src/components/InfoPanel.tsx`
- Modify: `krillnotes-desktop/src/components/FieldEditor.tsx`

**Step 1: Fetch group visibility on field changes**

When the user changes a field value in edit mode, call `evaluate_group_visibility` to determine which groups are visible. Store the result in component state.

**Step 2: Render field groups as collapsible sections**

After rendering top-level fields, render each visible field group as a collapsible section:
- Group header with name and collapse toggle
- Fields inside the group use the same `FieldEditor` components
- Hidden groups with existing data show a greyed-out header with "(hidden — data exists)" indicator

**Step 3: Run dev mode and test manually**

Run: `cd krillnotes-desktop && npm run tauri dev`
Test: Create a schema with field_groups and verify groups render, collapse, and visibility toggling works.

**Step 4: Commit**

```
feat: render field groups as collapsible sections in InfoPanel
```

---

## Task 13: Frontend — Inline validation errors

**Files:**
- Modify: `krillnotes-desktop/src/components/FieldEditor.tsx`
- Modify: `krillnotes-desktop/src/components/InfoPanel.tsx`

**Step 1: Add on-blur validation to FieldEditor**

When a field with `hasValidate: true` loses focus, call `validate_field`. If an error is returned, display it as red text below the field.

**Step 2: Display save pipeline errors**

When `save_note` returns `SaveResult.validationErrors`:
- Display field-pinned errors below each named field
- Display note-level errors as a red banner at the top of the edit panel
- Show preview values (previewTitle, previewFields) in the form

**Step 3: Update save handler**

Replace the existing `invoke('update_note', ...)` call with `invoke('save_note', ...)` and handle the `SaveResult` response.

**Step 4: Run dev mode and test**

Run: `cd krillnotes-desktop && npm run tauri dev`
Test: Create a schema with a validate closure that rejects values. Verify inline error appears on blur and on save.

**Step 5: Run TypeScript check**

Run: `cd krillnotes-desktop && npx tsc --noEmit`

**Step 6: Commit**

```
feat: inline validation errors and save pipeline error display
```

---

## Task 14: Frontend — Field groups in view mode

**Files:**
- Modify: `krillnotes-desktop/src/components/InfoPanel.tsx`
- Modify: `krillnotes-desktop/src/components/FieldDisplay.tsx`

**Step 1: Render field groups in view mode**

When displaying a note in view mode (default field grid, not custom on_view), render field groups as named sections. Hidden groups are not shown. Use the same collapsible section UI as edit mode but in read-only form.

**Step 2: Run dev mode and test**

**Step 3: Commit**

```
feat: render field groups in view mode
```

---

## Task 15: Wire validate into set_field() for hard errors

**Files:**
- Modify: `krillnotes-core/src/core/scripting/mod.rs` (set_field registration)

**Step 1: Enhance set_field to run validate closure**

When `set_field()` is called from within a hook, look up the field's schema and validate closure. If a validate closure exists and returns an error, throw a hard error (abort the hook immediately).

This requires the set_field Rhai function to have access to the schema registry. Pass it via a captured Arc.

**Step 2: Write test**

```rust
#[test]
fn test_set_field_validate_hard_error() {
    // Schema with validate: |v| if v < 0.0 { "Negative!" } else { () }
    // on_save: |note| { set_field(note.id, "x", -1.0); commit(); }
    // Save → expect hard error (scripting error), not soft error
}
```

**Step 3: Run tests**

Run: `cargo test -p krillnotes-core`

**Step 4: Commit**

```
feat: validate closure runs as hard error inside set_field()
```

---

## Task 16: Update on_add_child hook to use SaveTransaction

**Files:**
- Modify: `krillnotes-core/src/core/scripting/schema.rs` (run_on_add_child_hook)
- Modify: `krillnotes-core/src/core/workspace.rs` (callers of on_add_child)

**Step 1: Rewrite run_on_add_child_hook**

Similar to on_save: create a SaveTransaction with both parent and child notes, set the thread-local, call the closure, take the transaction back.

The on_add_child closure uses `set_field()` / `set_title()` / `commit()` on either note.

**Step 2: Update workspace.rs callers**

The on_add_child hook is called from `create_note` and `move_note`. Update these to read results from the SaveTransaction.

**Step 3: Write test**

```rust
#[test]
fn test_on_add_child_gated_model() {
    // Schema with on_add_child that sets a field on the child
    // Create a child → verify the field was set
}
```

**Step 4: Run tests**

Run: `cargo test -p krillnotes-core`

**Step 5: Commit**

```
feat: update on_add_child hook to use SaveTransaction
```

---

## Task 17: Integration tests and cleanup

**Files:**
- Modify: `krillnotes-core/src/core/workspace.rs` (test module)

**Step 1: Write integration test for full pipeline with groups + validation + reject**

```rust
#[test]
fn test_full_pipeline_groups_validation_reject() {
    // Schema with:
    //   - top-level field "type" (select: ["A", "B"])
    //   - field_group "B Details" visible only when type == "B"
    //     - field "b_value" (number, required, validate: must be > 0)
    //   - on_save: reject if type == "B" and b_value > 100
    //
    // Test 1: Save with type="A" → succeeds (B Details hidden, b_value not required)
    // Test 2: Save with type="B", b_value=-1 → validation error
    // Test 3: Save with type="B", b_value=200 → reject error
    // Test 4: Save with type="B", b_value=50 → success
}
```

**Step 2: Write integration test for tree action with validation**

```rust
#[test]
fn test_tree_action_validates_created_notes() {
    // Tree action that creates a child with a required field left empty
    // Expect: commit fails, no note created
}
```

**Step 3: Run full test suite**

Run: `cargo test -p krillnotes-core`
Expected: ALL tests pass.

**Step 4: Run TypeScript check**

Run: `cd krillnotes-desktop && npx tsc --noEmit`
Expected: No errors.

**Step 5: Commit**

```
test: integration tests for save pipeline with groups, validation, and reject
```

---

## Summary

| Task | Description | Key files |
|------|------------|-----------|
| 1 | SaveTransaction module | `save_transaction.rs` (new) |
| 2 | FieldDefinition.validate + FieldGroup + Schema.field_groups | `schema.rs` |
| 3 | SchemaInfo Tauri response + TS types | `lib.rs`, `types.ts` |
| 4 | Register set_field/set_title/reject/commit in Rhai | `mod.rs` |
| 5 | Rewrite on_save to use SaveTransaction | `mod.rs`, `schema.rs`, `workspace.rs` |
| 6 | Rewrite tree actions to use SaveTransaction | `mod.rs`, `hooks.rs`, `workspace.rs` |
| 7 | validate_field + evaluate_group_visibility methods | `mod.rs`, `schema.rs` |
| 8 | Migrate system scripts to gated model | 6 `.rhai` files |
| 9 | Migrate templates to gated model | 3 `.rhai` files |
| 10 | save_note_with_pipeline orchestration | `workspace.rs` |
| 11 | Tauri commands for validation + save | `lib.rs` |
| 12 | Frontend: field groups in edit mode | `InfoPanel.tsx` |
| 13 | Frontend: inline validation errors | `FieldEditor.tsx`, `InfoPanel.tsx` |
| 14 | Frontend: field groups in view mode | `InfoPanel.tsx`, `FieldDisplay.tsx` |
| 15 | Wire validate into set_field() for hard errors | `mod.rs` |
| 16 | on_add_child with SaveTransaction | `schema.rs`, `workspace.rs` |
| 17 | Integration tests and cleanup | `workspace.rs` tests |
