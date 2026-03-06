# Schema Extensions Phase 3 — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add schema versioning with batch migration on schema load, version stamping on create/save, and a frontend migration notification toast.

**Architecture:** Schema gets a required `version: u32` and optional `migrate` map. After script loading (phases A-C), a new Phase D queries stale notes per schema type and chains migration closures in a single transaction. One `UpdateSchema` operation is logged per schema. Frontend shows a transient toast on migration.

**Tech Stack:** Rust (krillnotes-core), Rhai (scripting engine), Tauri v2 (IPC + events), React 19 + TypeScript (frontend)

---

### Task 1: Add `version` and `migrations` to Schema struct

**Files:**
- Modify: `krillnotes-core/src/core/scripting/schema.rs:126-150` (Schema struct)
- Test: `krillnotes-core/src/core/scripting/schema.rs` (existing tests)

**Step 1: Add fields to Schema struct**

In `schema.rs`, add two fields to the `Schema` struct (after `ast`):

```rust
pub struct Schema {
    pub name: String,
    pub fields: Vec<FieldDefinition>,
    pub title_can_view: bool,
    pub title_can_edit: bool,
    pub children_sort: String,
    pub allowed_parent_types: Vec<String>,
    pub allowed_children_types: Vec<String>,
    pub allow_attachments: bool,
    pub attachment_types: Vec<String>,
    pub field_groups: Vec<FieldGroup>,
    pub ast: Option<rhai::AST>,
    pub version: u32,
    pub migrations: std::collections::BTreeMap<u32, rhai::FnPtr>,
}
```

**Step 2: Update `parse_from_rhai()` to parse `version` (required) and `migrate` (optional)**

In `parse_from_rhai()` (line ~309), add parsing before the final `Ok(Schema { ... })`:

```rust
// version is required — hard error if missing
let version = def
    .get("version")
    .and_then(|v| v.clone().try_cast::<i64>())
    .ok_or_else(|| KrillnotesError::Scripting(
        format!("Schema '{}' missing required 'version' key", name)
    ))?;
if version < 1 {
    return Err(KrillnotesError::Scripting(
        format!("Schema '{}' version must be >= 1, got {}", name, version)
    ));
}
let version = version as u32;

// migrate map is optional — keyed by target version, values are closures
let mut migrations = std::collections::BTreeMap::new();
if let Some(migrate_map) = def
    .get("migrate")
    .and_then(|v| v.clone().try_cast::<rhai::Map>())
{
    for (key, val) in migrate_map.iter() {
        let target_ver = key.to_string().parse::<u32>().map_err(|_| {
            KrillnotesError::Scripting(
                format!("Schema '{}' migrate key '{}' must be an integer", name, key)
            )
        })?;
        if target_ver < 2 || target_ver > version {
            return Err(KrillnotesError::Scripting(
                format!(
                    "Schema '{}' migrate key {} out of range (must be 2..={})",
                    name, target_ver, version
                )
            ));
        }
        let fn_ptr = val.clone().try_cast::<rhai::FnPtr>().ok_or_else(|| {
            KrillnotesError::Scripting(
                format!("Schema '{}' migrate[{}] must be a closure", name, target_ver)
            )
        })?;
        migrations.insert(target_ver, fn_ptr);
    }
}
```

Update the final `Ok(Schema { ... })` to include `version, migrations`.

**Step 3: Fix all existing tests that construct Schema manually**

Any test creating a `Schema` directly must add `version: 1, migrations: BTreeMap::new()`.

**Step 4: Run tests**

Run: `cargo test -p krillnotes-core`
Expected: All existing tests pass (scripts will fail in next task).

**Step 5: Commit**

```bash
git add -A && git commit -m "feat(schema): add version and migrations fields to Schema struct"
```

---

### Task 2: Add version guard on schema re-registration

**Files:**
- Modify: `krillnotes-core/src/core/scripting/mod.rs:290-343` (schema() Rhai function)

**Step 1: Add version downgrade check**

In the `schema()` closure in `mod.rs` (line ~318, after `parse_from_rhai` call), before inserting into the schemas map, add a version guard:

```rust
let mut s = Schema::parse_from_rhai(&name, &def)
    .map_err(|e| -> Box<EvalAltResult> { e.to_string().into() })?;

// Version guard: prevent downgrades
{
    let schemas = schemas_arc.lock().unwrap();
    if let Some(existing) = schemas.get(&name) {
        if s.version < existing.version {
            return Err(format!(
                "Schema '{}' version {} cannot replace existing version {} — downgrade not allowed",
                name, s.version, existing.version
            ).into());
        }
    }
}
```

**Step 2: Run tests**

Run: `cargo test -p krillnotes-core`
Expected: PASS (no scripts loaded yet with version).

**Step 3: Commit**

```bash
git add -A && git commit -m "feat(schema): version downgrade guard on re-registration"
```

---

### Task 3: Add `version: 1` to all system scripts and templates

**Files:**
- Modify: `krillnotes-core/src/system_scripts/00_text_note.schema.rhai`
- Modify: `krillnotes-core/src/system_scripts/01_contact.schema.rhai`
- Modify: `krillnotes-core/src/system_scripts/02_task.schema.rhai`
- Modify: `krillnotes-core/src/system_scripts/03_project.schema.rhai`
- Modify: `krillnotes-core/src/system_scripts/05_recipe.schema.rhai`
- Modify: `krillnotes-core/src/system_scripts/06_product.schema.rhai`
- Modify: `templates/zettelkasten.schema.rhai`
- Modify: `templates/book_collection.schema.rhai`
- Modify: `templates/photo_note.schema.rhai`

**Step 1: Add `version: 1` to every `schema()` call**

For each file, add `version: 1,` as the first key inside the `schema()` map. Example for `text_note.schema.rhai`:

```rhai
schema("TextNote", #{
    version: 1,
    fields: [
        #{ name: "body", type: "textarea", required: false },
    ]
});
```

For schemas with no fields (e.g. `Library`, `Kasten`):

```rhai
schema("Library", #{
    version: 1,
    allowed_children_types: ["Book"],
    fields: [],
});
```

Every `schema()` call in every `.schema.rhai` file gets `version: 1`.

**Step 2: Run tests**

Run: `cargo test -p krillnotes-core`
Expected: PASS — all scripts now provide the required `version` key.

**Step 3: Commit**

```bash
git add -A && git commit -m "feat(schema): add version: 1 to all system scripts and templates"
```

---

### Task 4: Add `schema_version` to Note struct and database

**Files:**
- Modify: `krillnotes-core/src/core/note.rs:39-68` (Note struct)
- Modify: `krillnotes-core/src/core/schema.sql:2-15` (notes table DDL)
- Modify: `krillnotes-desktop/src/types.ts:14-27` (Note interface)

**Step 1: Add `schema_version` to Rust Note struct**

In `note.rs`, add the field after `tags`:

```rust
pub struct Note {
    // ... existing fields ...
    #[serde(default)]
    pub tags: Vec<String>,
    /// Schema version this note was created/migrated with.
    pub schema_version: u32,
}
```

**Step 2: Update `schema.sql` DDL**

Add `schema_version` column to the notes table definition:

```sql
CREATE TABLE IF NOT EXISTS notes (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    node_type TEXT NOT NULL,
    parent_id TEXT,
    position REAL NOT NULL DEFAULT 0.0,
    created_at INTEGER NOT NULL,
    modified_at INTEGER NOT NULL,
    created_by INTEGER NOT NULL DEFAULT 0,
    modified_by INTEGER NOT NULL DEFAULT 0,
    fields_json TEXT NOT NULL DEFAULT '{}',
    is_expanded INTEGER DEFAULT 1,
    schema_version INTEGER NOT NULL DEFAULT 1,
    FOREIGN KEY (parent_id) REFERENCES notes(id) ON DELETE CASCADE
);
```

**Step 3: Update TypeScript Note interface**

In `types.ts`, add:

```typescript
export interface Note {
  // ... existing fields ...
  tags: string[];
  schemaVersion: number;
}
```

**Step 4: Run type check**

Run: `cd krillnotes-desktop && npx tsc --noEmit`
Expected: FAIL — `schemaVersion` not provided in places that construct Note objects. Fix in next tasks.

**Step 5: Commit**

```bash
git add -A && git commit -m "feat(schema): add schema_version to Note struct, DDL, and TS types"
```

---

### Task 5: Update all SQL queries and row mapping for `schema_version`

**Files:**
- Modify: `krillnotes-core/src/core/workspace.rs` — multiple locations

This is the largest task. Every INSERT and SELECT for `notes` must include `schema_version`.

**Step 1: Update NoteRow type (line ~4063)**

Add `u32` for schema_version as column index 11 (shift `tags_csv` to index 12):

```rust
type NoteRow = (String, String, String, Option<String>, f64, i64, i64, i64, i64, String, i64, u32, Option<String>);
```

**Step 2: Update `map_note_row()` (line ~4069)**

Add `row.get::<_, u32>(11)?` for schema_version. Shift `is_expanded` stays at index 10, schema_version at 11, tags_csv at 12:

```rust
fn map_note_row(row: &rusqlite::Row) -> rusqlite::Result<NoteRow> {
    Ok((
        row.get::<_, String>(0)?,          // id
        row.get::<_, String>(1)?,          // title
        row.get::<_, String>(2)?,          // node_type
        row.get::<_, Option<String>>(3)?,  // parent_id
        row.get::<_, f64>(4)?,             // position
        row.get::<_, i64>(5)?,             // created_at
        row.get::<_, i64>(6)?,             // modified_at
        row.get::<_, i64>(7)?,             // created_by
        row.get::<_, i64>(8)?,             // modified_by
        row.get::<_, String>(9)?,          // fields_json
        row.get::<_, i64>(10)?,            // is_expanded
        row.get::<_, u32>(11)?,            // schema_version
        row.get::<_, Option<String>>(12)?, // tags_csv
    ))
}
```

**Step 3: Update `note_from_row_tuple()` (line ~4087)**

Destructure the new field and set it on Note:

```rust
fn note_from_row_tuple(
    (id, title, node_type, parent_id, position, created_at, modified_at,
     created_by, modified_by, fields_json, is_expanded_int, schema_version, tags_csv): NoteRow,
) -> Result<Note> {
    // ... existing tag parsing ...
    Ok(Note {
        // ... existing fields ...
        is_expanded: is_expanded_int == 1,
        tags,
        schema_version,
    })
}
```

**Step 4: Update ALL SELECT queries to include `n.schema_version`**

Every SELECT that reads from notes and feeds into `map_note_row` must add `n.schema_version` after `n.is_expanded` (before the `GROUP_CONCAT` for tags). There are at least 6 locations:

- `get_note()` (~line 926)
- `list_all_notes()` (~line 1622)
- `get_children()` (~line 2383)
- Notes by tags (~line 1480)
- `collect_subtree_notes()` recursive CTE (~line 3848)
- Any other SELECT feeding into `map_note_row`

Pattern — change every SELECT from:
```sql
SELECT n.id, n.title, n.node_type, n.parent_id, n.position,
       n.created_at, n.modified_at, n.created_by, n.modified_by,
       n.fields_json, n.is_expanded,
       GROUP_CONCAT(nt.tag, ',') AS tags_csv
```
to:
```sql
SELECT n.id, n.title, n.node_type, n.parent_id, n.position,
       n.created_at, n.modified_at, n.created_by, n.modified_by,
       n.fields_json, n.is_expanded, n.schema_version,
       GROUP_CONCAT(nt.tag, ',') AS tags_csv
```

**Step 5: Update ALL INSERT queries to include `schema_version`**

Three INSERT locations in workspace.rs:

1. Root note insert (~line 232): Add `schema_version` column + value `1` (root is always TextNote v1)
2. `create_note()` insert (~line 1038): Add `schema_version` column + value from schema lookup (done in Task 6)
3. `duplicate_note_tree()` insert (~line 1241): Add `schema_version` column + copy from source note

Pattern — change column list from:
```sql
INSERT INTO notes (id, title, node_type, parent_id, position, created_at, modified_at, created_by, modified_by, fields_json, is_expanded)
VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
```
to:
```sql
INSERT INTO notes (id, title, node_type, parent_id, position, created_at, modified_at, created_by, modified_by, fields_json, is_expanded, schema_version)
VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
```

For root note and duplicate, pass `1` or `note.schema_version` respectively.

**Step 6: Run tests**

Run: `cargo test -p krillnotes-core`
Expected: PASS — all queries now include schema_version.

**Step 7: Commit**

```bash
git add -A && git commit -m "feat(schema): update all SQL queries and row mapping for schema_version"
```

---

### Task 6: Stamp `schema_version` on create and save

**Files:**
- Modify: `krillnotes-core/src/core/workspace.rs` — `create_note()` and `update_note()`

**Step 1: Stamp on create**

In `create_note()` (~line 1010), after building the Note struct, set schema_version from the schema:

```rust
let note = Note {
    // ... existing fields ...
    schema_version: schema.version,
};
```

The schema is already fetched at line ~958 (`let schema = self.script_registry.get_schema(note_type)?;`).

**Step 2: Re-stamp on save**

In `update_note()` (~line 2919), add `schema_version` to the UPDATE query:

```sql
UPDATE notes SET title = ?1, fields_json = ?2, modified_at = ?3, modified_by = ?4, schema_version = ?5 WHERE id = ?6
```

Look up the current schema version:

```rust
let current_schema_version = self.script_registry
    .get_schema(&node_type)
    .map(|s| s.version)
    .unwrap_or(1);
```

Pass it as the 5th parameter.

**Step 3: Write a test for version stamping on create**

```rust
#[test]
fn create_note_stamps_schema_version() {
    let mut ws = test_workspace();
    let root_id = ws.list_all_notes().unwrap()[0].id.clone();
    let note_id = ws.create_note(&root_id, AddPosition::LastChild, "TextNote").unwrap();
    let note = ws.get_note(&note_id).unwrap();
    assert_eq!(note.schema_version, 1);
}
```

**Step 4: Run tests**

Run: `cargo test -p krillnotes-core`
Expected: PASS

**Step 5: Commit**

```bash
git add -A && git commit -m "feat(schema): stamp schema_version on create and save"
```

---

### Task 7: Add `UpdateSchema` operation variant

**Files:**
- Modify: `krillnotes-core/src/core/operation.rs:20-218` (Operation enum + helper methods)
- Modify: `krillnotes-core/src/core/operation_log.rs:194-207` (operation_type_name)

**Step 1: Add the variant**

In the `Operation` enum, add after `DeleteUserScript`:

```rust
UpdateSchema {
    operation_id: String,
    timestamp: HlcTimestamp,
    device_id: String,
    signature: String,
    updated_by: String,
    schema_name: String,
    from_version: u32,
    to_version: u32,
    notes_migrated: u32,
},
```

**Step 2: Update all match arms in Operation helper methods**

Add `UpdateSchema` to every match in `operation_id()`, `timestamp()`, `device_id()`, `author_key()`, `set_author_key()`, `set_signature()`, `get_signature()`:

```rust
// operation_id:
| Self::UpdateSchema { operation_id, .. } => operation_id,

// timestamp:
| Self::UpdateSchema { timestamp, .. } => *timestamp,

// device_id:
| Self::UpdateSchema { device_id, .. } => device_id,

// author_key:
Self::UpdateSchema { updated_by, .. } => updated_by,

// set_author_key:
Self::UpdateSchema { updated_by, .. } => *updated_by = key,

// set_signature:
| Self::UpdateSchema { signature, .. } => *signature = sig,

// get_signature:
| Self::UpdateSchema { signature, .. } => signature,
```

**Step 3: Update `operation_type_name` in operation_log.rs**

Add to the match at line ~194:

```rust
Operation::UpdateSchema { .. } => "UpdateSchema",
```

**Step 4: Update `extract_target_name` in operation_log.rs**

Add `schema_name` extraction (check for "schema_name" key in JSON):

```rust
// After existing checks for title/name/note_id/script_id:
if let Some(schema_name) = value.get("schema_name").and_then(|v| v.as_str()) {
    return schema_name.to_string();
}
```

**Step 5: Run tests**

Run: `cargo test -p krillnotes-core`
Expected: PASS

**Step 6: Commit**

```bash
git add -A && git commit -m "feat(schema): add UpdateSchema operation variant"
```

---

### Task 8: Implement Phase D migration pipeline

**Files:**
- Modify: `krillnotes-core/src/core/workspace.rs` — new method + call site
- Modify: `krillnotes-core/src/core/scripting/schema.rs` — expose migration data

**Step 1: Add `get_versioned_schemas()` to SchemaRegistry**

In `schema.rs`, add a method that returns the data Phase D needs:

```rust
/// Returns (name, version, migrations BTreeMap, AST) for all schemas that have migrations.
pub(super) fn get_versioned_schemas(&self) -> Vec<(String, u32, std::collections::BTreeMap<u32, FnPtr>, Option<rhai::AST>)> {
    let schemas = self.schemas.lock().unwrap();
    schemas.values().map(|s| {
        (s.name.clone(), s.version, s.migrations.clone(), s.ast.clone())
    }).collect()
}
```

**Step 2: Add `run_schema_migrations()` method to Workspace**

This is the Phase D entry point. Add it as a method on `Workspace`:

```rust
/// Phase D: batch-migrate notes whose schema_version is behind the current schema version.
/// Returns a Vec of (schema_name, from_version, to_version, notes_migrated) for each
/// schema type that had migrations to run.
fn run_schema_migrations(&mut self) -> Result<Vec<(String, u32, u32, u32)>> {
    let versioned_schemas = self.script_registry.get_versioned_schemas();
    let mut results = Vec::new();

    for (schema_name, schema_version, migrations, ast) in versioned_schemas {
        // Query stale notes
        let stale_notes: Vec<(String, String, String, u32)> = {
            let conn = self.storage.connection();
            let mut stmt = conn.prepare(
                "SELECT id, title, fields_json, schema_version FROM notes WHERE node_type = ? AND schema_version < ?"
            )?;
            stmt.query_map(
                rusqlite::params![&schema_name, schema_version],
                |row| Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, u32>(3)?,
                ))
            )?.collect::<rusqlite::Result<Vec<_>>>()?
        };

        if stale_notes.is_empty() {
            continue;
        }

        let min_version = stale_notes.iter().map(|n| n.3).min().unwrap_or(1);
        let notes_count = stale_notes.len() as u32;

        // Run migration closures on each note
        let ast = match &ast {
            Some(a) => a.clone(),
            None => {
                self.script_registry.add_warning(&schema_name,
                    &format!("Schema '{}' has no AST for migration closures", schema_name));
                continue;
            }
        };

        let tx = self.storage.connection_mut().transaction()?;
        let mut migration_failed = false;

        for (note_id, title, fields_json, note_version) in &stale_notes {
            let fields: std::collections::BTreeMap<String, crate::core::note::FieldValue> =
                serde_json::from_str(fields_json)?;

            // Build a Rhai map: #{ title: "...", fields: #{...} }
            let mut note_map = rhai::Map::new();
            note_map.insert("title".into(), rhai::Dynamic::from(title.clone()));

            let mut fields_rhai = rhai::Map::new();
            for (k, v) in &fields {
                fields_rhai.insert(k.as_str().into(), crate::core::scripting::field_value_to_dynamic(v));
            }
            note_map.insert("fields".into(), rhai::Dynamic::from(fields_rhai));

            // Chain closures from note_version+1 to schema_version
            for target_ver in (*note_version + 1)..=schema_version {
                if let Some(fn_ptr) = migrations.get(&target_ver) {
                    let result = self.script_registry.engine().call_fn_raw(
                        rhai::Scope::new(),
                        &ast,
                        true,   // eval AST
                        false,  // not method call
                        fn_ptr.fn_name(),
                        None,
                        [rhai::Dynamic::from(note_map.clone())],
                    );
                    match result {
                        Ok(returned) => {
                            // The closure mutates note_map in place (Rhai pass-by-ref for maps)
                            // But if it returns a map, use that instead
                            if let Some(m) = returned.try_cast::<rhai::Map>() {
                                note_map = m;
                            }
                        }
                        Err(e) => {
                            self.script_registry.add_warning(&schema_name,
                                &format!("Migration to v{} failed for note '{}': {}", target_ver, note_id, e));
                            migration_failed = true;
                            break;
                        }
                    }
                }
                // If no closure for this version, skip (allow gaps for versions that need no migration)
            }

            if migration_failed {
                break;
            }

            // Extract migrated title and fields
            let new_title = note_map.get("title")
                .and_then(|v| v.clone().try_cast::<String>())
                .unwrap_or_else(|| title.clone());
            let new_fields_map = note_map.get("fields")
                .and_then(|v| v.clone().try_cast::<rhai::Map>());

            let new_fields_json = if let Some(fm) = new_fields_map {
                let converted = crate::core::scripting::rhai_map_to_fields(&fm, &schema_name, &self.script_registry)?;
                serde_json::to_string(&converted)?
            } else {
                fields_json.clone()
            };

            tx.execute(
                "UPDATE notes SET title = ?1, fields_json = ?2, schema_version = ?3 WHERE id = ?4",
                rusqlite::params![new_title, new_fields_json, schema_version, note_id],
            )?;
        }

        if migration_failed {
            // Rollback is automatic when tx is dropped without commit
            continue;
        }

        // Log UpdateSchema operation
        let ts = self.hlc.lock().unwrap().now();
        Self::save_hlc(&ts, &tx)?;
        let mut op = Operation::UpdateSchema {
            operation_id: uuid::Uuid::new_v4().to_string(),
            timestamp: ts,
            device_id: self.device_id.clone(),
            signature: String::new(),
            updated_by: String::new(),
            schema_name: schema_name.clone(),
            from_version: min_version,
            to_version: schema_version,
            notes_migrated: notes_count,
        };
        if let Some(ref key) = self.signing_key {
            Self::sign_op_with(key, &mut op);
        }
        Self::log_op(&self.operation_log, &tx, &op)?;

        tx.commit()?;
        results.push((schema_name, min_version, schema_version, notes_count));
    }

    Ok(results)
}
```

**Note:** The helper functions `field_value_to_dynamic` and `rhai_map_to_fields` may already exist in `scripting/mod.rs`. If not, they need to be exposed/created. Check existing code for how `on_save` builds the Rhai note map — reuse the same conversion logic.

**Step 3: Call Phase D after script loading**

In `new()` (~line 197) and `open_existing()` (~line 401), after `resolve_bindings()`, add:

```rust
// Phase D: run schema migrations
let migration_results = workspace.run_schema_migrations()?;
// Store results for later Tauri event emission
workspace.pending_migration_results = migration_results;
```

Add `pending_migration_results: Vec<(String, u32, u32, u32)>` field to `Workspace` struct (or return it from the constructor).

**Step 4: Run tests**

Run: `cargo test -p krillnotes-core`
Expected: PASS

**Step 5: Commit**

```bash
git add -A && git commit -m "feat(schema): implement Phase D batch migration pipeline"
```

---

### Task 9: Write migration tests

**Files:**
- Modify: `krillnotes-core/src/core/workspace.rs` (test module)

**Step 1: Test — migration runs on schema version bump**

Create a test that:
1. Opens a workspace, creates a note with schema v1
2. Updates the schema script to v2 with a migration closure that renames a field
3. Reloads scripts (simulating workspace reopen)
4. Asserts the note's field was renamed and `schema_version` is now 2

```rust
#[test]
fn migration_renames_field_on_version_bump() {
    // 1. Create workspace with a custom schema v1
    // 2. Create a note of that type with field "phone" = "123"
    // 3. Update the script to v2 with migrate: #{ 2: |note| { note.fields["mobile"] = note.fields["phone"]; note.fields.remove("phone"); } }
    // 4. Reload scripts + run_schema_migrations()
    // 5. Assert note.fields["mobile"] == "123" and "phone" is gone
    // 6. Assert note.schema_version == 2
}
```

**Step 2: Test — multi-version chained migration**

```rust
#[test]
fn migration_chains_across_multiple_versions() {
    // Create note at v1, upgrade schema to v3 with migrate keys 2 and 3
    // Assert both closures ran in order
}
```

**Step 3: Test — migration failure rolls back entire batch**

```rust
#[test]
fn migration_failure_rolls_back_batch() {
    // Create 3 notes at v1
    // Schema v2 migration closure that fails on the 2nd note
    // Assert all 3 notes remain at v1 (rollback)
    // Assert ScriptWarning was recorded
}
```

**Step 4: Test — version downgrade rejected**

```rust
#[test]
fn schema_version_downgrade_rejected() {
    // Register schema v2, then try to register v1
    // Assert error
}
```

**Step 5: Test — same-version re-registration allowed**

```rust
#[test]
fn schema_same_version_reregistration_allowed() {
    // Register schema v1, then re-register v1 with different fields
    // Assert no error, schema updated
}
```

**Step 6: Run tests**

Run: `cargo test -p krillnotes-core`
Expected: ALL PASS

**Step 7: Commit**

```bash
git add -A && git commit -m "test(schema): migration pipeline tests"
```

---

### Task 10: Emit Tauri event and frontend toast

**Files:**
- Modify: `krillnotes-desktop/src-tauri/src/lib.rs` — emit event after workspace open
- Modify: `krillnotes-desktop/src/types.ts` — add `SchemaMigratedEvent` interface
- Modify: `krillnotes-desktop/src/components/WorkspaceView.tsx` — listen + toast

**Step 1: Add `SchemaMigratedEvent` to types.ts**

```typescript
export interface SchemaMigratedEvent {
  schemaName: string;
  fromVersion: number;
  toVersion: number;
  notesMigrated: number;
}
```

**Step 2: Emit Tauri event from lib.rs**

In the Tauri command that opens/creates a workspace, after the workspace is loaded and stored in AppState, emit events for any pending migration results:

```rust
// After workspace is opened and stored in state:
for (schema_name, from_version, to_version, notes_migrated) in &migration_results {
    let _ = window.emit("schema-migrated", serde_json::json!({
        "schemaName": schema_name,
        "fromVersion": from_version,
        "toVersion": to_version,
        "notesMigrated": notes_migrated,
    }));
}
```

The migration results need to be returned from the workspace open path. Approach: have `Workspace::new()` / `Workspace::open_existing()` return the migration results alongside the workspace, or store them as a field and drain them after opening.

**Step 3: Add toast listener in WorkspaceView.tsx**

Add a `useEffect` that listens for `schema-migrated` events:

```typescript
import { listen } from '@tauri-apps/api/event';

// Inside WorkspaceView component:
const [toasts, setToasts] = useState<SchemaMigratedEvent[]>([]);

useEffect(() => {
  const unlisten = listen<SchemaMigratedEvent>('schema-migrated', (event) => {
    setToasts(prev => [...prev, event.payload]);
    // Auto-dismiss after 5 seconds
    setTimeout(() => {
      setToasts(prev => prev.filter(t => t !== event.payload));
    }, 5000);
  });
  return () => { unlisten.then(f => f()); };
}, []);
```

**Step 4: Render toast**

Add a simple toast overlay at the bottom of WorkspaceView:

```tsx
{toasts.length > 0 && (
  <div className="fixed bottom-4 right-4 flex flex-col gap-2 z-50">
    {toasts.map((t, i) => (
      <div key={i} className="bg-blue-600 text-white px-4 py-2 rounded-lg shadow-lg text-sm animate-fade-in">
        <strong>"{t.schemaName}" schema updated</strong> — {t.notesMigrated} note{t.notesMigrated !== 1 ? 's' : ''} migrated to version {t.toVersion}
      </div>
    ))}
  </div>
)}
```

**Step 5: Run type check**

Run: `cd krillnotes-desktop && npx tsc --noEmit`
Expected: PASS

**Step 6: Commit**

```bash
git add -A && git commit -m "feat(schema): frontend migration toast notification"
```

---

### Task 11: Update Script Manager starter templates

**Files:**
- Modify: `krillnotes-desktop/src/components/ScriptManagerDialog.tsx`

**Step 1: Update schema starter template**

Find the schema starter template string in ScriptManagerDialog.tsx and add `version: 1`:

```rhai
schema("MyType", #{
    version: 1,
    fields: [
        #{ name: "title_field", type: "text", required: true },
    ],
    on_save: |note| {
        commit();
    }
});
```

**Step 2: Run type check**

Run: `cd krillnotes-desktop && npx tsc --noEmit`
Expected: PASS

**Step 3: Commit**

```bash
git add -A && git commit -m "feat(schema): add version: 1 to Script Manager schema starter template"
```

---

### Task 12: Final integration test and cleanup

**Files:**
- All modified files

**Step 1: Run full test suite**

Run: `cargo test -p krillnotes-core`
Expected: ALL PASS

**Step 2: Run TypeScript type check**

Run: `cd krillnotes-desktop && npx tsc --noEmit`
Expected: PASS

**Step 3: Manual smoke test checklist**

- [ ] `cargo test -p krillnotes-core` passes
- [ ] `npx tsc --noEmit` passes
- [ ] Create a workspace — notes get `schema_version: 1`
- [ ] Save a note — `schema_version` re-stamped
- [ ] Version downgrade in script editor shows error
- [ ] Same-version re-save of script works

**Step 4: Commit any remaining fixes**

```bash
git add -A && git commit -m "chore: phase 3 integration cleanup"
```

---

## Task Dependency Graph

```
Task 1 (Schema struct) ──→ Task 2 (Version guard) ──→ Task 3 (Scripts v1)
                        │
                        ├──→ Task 4 (Note + DDL) ──→ Task 5 (SQL queries) ──→ Task 6 (Stamp on create/save)
                        │
                        └──→ Task 7 (UpdateSchema op)
                                                                              │
Task 6 + Task 7 ──→ Task 8 (Phase D pipeline) ──→ Task 9 (Migration tests)  │
                                                                              │
                                               Task 10 (Frontend toast) ←────┘
                                                       │
                                               Task 11 (Starter template)
                                                       │
                                               Task 12 (Integration)
```

**Parallelizable groups:**
- Tasks 2, 4, 7 can run in parallel after Task 1
- Tasks 3, 5 can run in parallel (scripts vs queries)
- Task 10 and Task 11 can run in parallel after Task 9
