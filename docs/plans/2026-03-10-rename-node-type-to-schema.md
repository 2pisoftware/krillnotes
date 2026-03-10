# Rename `node_type` → `schema` Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Rename the `node_type` field on `Note` to `schema` across every layer — SQLite column, Rust struct, Rhai map keys, TypeScript types, and documentation — while preserving backward compatibility for old `.krillnotes` archives.

**Architecture:** Rename `Note.node_type` in Rust first (compile errors guide all field-access fixes), then update SQL string literals and Rhai map key strings (compiler-invisible), then TypeScript, then docs. A new SQLite migration renames the DB column. `#[serde(alias = "nodeType")]` preserves archive import compat.

**Tech Stack:** Rust / rusqlite / Rhai / TypeScript / React 19 / Tauri v2

---

## Before you start

Create a worktree and branch:

```bash
git -C /Users/careck/Source/Krillnotes worktree add .worktrees/feat/rename-node-type-to-schema -b feat/rename-node-type-to-schema
cd /Users/careck/Source/Krillnotes/.worktrees/feat/rename-node-type-to-schema
```

All remaining steps run from this worktree.

---

### Task 1: Write the backward-compat serde test (TDD first)

**Files:**
- Modify: `krillnotes-core/src/core/note.rs` (test section)

**Step 1: Add a failing test for archive backward compat**

In the `#[cfg(test)]` block at the bottom of `note.rs`, add:

```rust
#[test]
fn test_note_deserializes_legacy_node_type_key() {
    // Old archives use "nodeType" (camelCase). Must still deserialize.
    let json = r#"{
        "id": "abc",
        "title": "Old Note",
        "nodeType": "TextNote",
        "parentId": null,
        "position": 0.0,
        "createdAt": 0,
        "modifiedAt": 0,
        "createdBy": "",
        "modifiedBy": "",
        "fields": {},
        "isExpanded": true
    }"#;
    let note: Note = serde_json::from_str(json).expect("should deserialize legacy archive");
    assert_eq!(note.schema, "TextNote");
}

#[test]
fn test_note_serializes_new_schema_key() {
    // New exports must use "schema", not "nodeType".
    let note = Note {
        id: "x".into(), title: "T".into(), schema: "TextNote".into(),
        parent_id: None, position: 0.0, created_at: 0, modified_at: 0,
        created_by: String::new(), modified_by: String::new(),
        fields: BTreeMap::new(), is_expanded: true, tags: vec![], schema_version: 1,
    };
    let json = serde_json::to_string(&note).unwrap();
    assert!(json.contains(r#""schema":"TextNote""#), "must use new key");
    assert!(!json.contains("nodeType"), "must not contain old key");
}
```

**Step 2: Run tests — expect compile error (field `schema` does not exist yet)**

```bash
cargo test -p krillnotes-core --lib 2>&1 | head -30
```

Expected: compile error `no field 'schema' on type 'Note'`

---

### Task 2: Rename `Note.node_type` → `Note.schema` + add serde alias

**Files:**
- Modify: `krillnotes-core/src/core/note.rs`

**Step 1: Update the `Note` struct**

Replace the field declaration and its doc comment:

```rust
// OLD:
/// Schema name governing this note's `fields` (e.g. `"TextNote"`).
pub node_type: String,

// NEW:
/// Schema name governing this note's `fields` (e.g. `"TextNote"`).
#[serde(alias = "nodeType")]
pub schema: String,
```

Also update the module-level doc comment on the struct (line 58):

```rust
// OLD:
/// each note has a `node_type` that maps to a [`crate::Schema`]
// NEW:
/// each note has a `schema` that maps to a [`crate::Schema`]
```

**Step 2: Try to compile — collect all errors**

```bash
cargo build -p krillnotes-core 2>&1 | grep "error\[" | head -40
```

Expected: many errors pointing to `.node_type` field accesses across the codebase — this output is your todo list for Task 3+.

**Step 3: Update the `test_create_note` test in note.rs**

```rust
// OLD:
node_type: "TextNote".to_string(),
// ...
assert_eq!(note.node_type, "TextNote");

// NEW:
schema: "TextNote".to_string(),
// ...
assert_eq!(note.schema, "TextNote");
```

**Step 4: Run the two new tests**

```bash
cargo test -p krillnotes-core --lib note::tests 2>&1
```

Expected: the two new serde tests pass; compile errors in other files are expected at this stage.

---

### Task 3: Fix `operation.rs` and `save_transaction.rs`

**Files:**
- Modify: `krillnotes-core/src/core/operation.rs`
- Modify: `krillnotes-core/src/core/save_transaction.rs`

**Step 1: Update `operation.rs` — `CreateNote` variant field**

Find the `CreateNote` variant (around line 38). Rename `node_type` → `schema`:

```rust
// In the Operation enum, CreateNote variant:
// OLD:
CreateNote { ..., node_type: String, ... }
// NEW:
CreateNote { ..., schema: String, ... }
```

Also update every struct-literal construction of `CreateNote { ..., node_type: ... }` — there are two test instances (around lines 580, 649). Change `node_type:` → `schema:`.

**Step 2: Update `save_transaction.rs` — `PendingNote` struct**

Rename the three `node_type` fields in the `PendingNote` struct and its constructors (lines 33, 79–88, 104–112, 125–133):

```rust
// In PendingNote struct:
pub node_type: String,  →  pub schema: String,

// In all impl blocks constructing PendingNote:
node_type,  →  schema,
node_type: String  →  schema: String  (parameter names)
```

**Step 3: Compile check**

```bash
cargo build -p krillnotes-core 2>&1 | grep "error\[" | wc -l
```

Error count should decrease.

---

### Task 4: Fix `workspace.rs` field accesses and SQL

**Files:**
- Modify: `krillnotes-core/src/core/workspace.rs`

**Step 1: Replace all `.node_type` field accesses with `.schema`**

There are ~50 occurrences. Use a targeted replace — every instance of `.node_type` (a field access) becomes `.schema`:

```
.node_type  →  .schema
```

Also replace struct-literal constructions `node_type:` → `schema:` and local variable names `node_type` where they are bound from the struct (e.g. `let node_type = note.node_type` → `let schema = note.schema`, but be careful with `node_type` used as a *parameter name* in function signatures — those are separate and should also be renamed to `schema` for clarity, but are not required by the compiler).

**Step 2: Update SQL string literals in workspace.rs**

Every SQL query in workspace.rs that references the column name needs updating. These are NOT caught by the compiler. Do a search-replace on SQL strings:

```
"node_type" (inside SQL strings)  →  "schema"
```

Key queries to update (examples — search for all):
- `INSERT INTO notes (id, title, node_type, ...)` → `INSERT INTO notes (id, title, schema, ...)`
- `SELECT n.id, n.title, n.node_type, ...` → `SELECT n.id, n.title, n.schema, ...`
- `SELECT node_type FROM notes WHERE id = ?1` → `SELECT schema FROM notes WHERE id = ?1`
- `FROM notes WHERE node_type = ?1` → `FROM notes WHERE schema = ?1`

There are approximately 10 distinct SQL strings. Check every one.

**Step 3: Update `NoteRow` tuple destructuring (bottom of workspace.rs ~line 4528)**

```rust
// OLD:
(id, title, node_type, parent_id, ...) : NoteRow,
// and struct construction:
node_type,

// NEW:
(id, title, schema, parent_id, ...) : NoteRow,
// and struct construction:
schema,
```

Also update the comment on `row.get::<_, String>(2)?` (line ~4512):
```rust
row.get::<_, String>(2)?,  // schema  (was: node_type)
```

Also update the `note_map.insert` near line 4570:
```rust
// OLD:
note_map.insert("node_type".into(), Dynamic::from(note.node_type.clone()));
// NEW:
note_map.insert("schema".into(), Dynamic::from(note.schema.clone()));
```

**Step 4: Compile check**

```bash
cargo build -p krillnotes-core 2>&1 | grep "error\[" | wc -l
```

---

### Task 5: Fix `export.rs`, `operation_log.rs`, `display_helpers.rs`

**Files:**
- Modify: `krillnotes-core/src/core/export.rs`
- Modify: `krillnotes-core/src/core/operation_log.rs`
- Modify: `krillnotes-core/src/core/scripting/display_helpers.rs`

**Step 1: `export.rs` — field access + SQL**

Two locations (lines ~445–450):
```rust
// Field access:
note.node_type  →  note.schema

// SQL string:
INSERT INTO notes (id, title, node_type, ...)  →  INSERT INTO notes (id, title, schema, ...)
```

**Step 2: `operation_log.rs` — test Note constructors**

Three test Note constructions (lines ~321, 358, 426). In each:
```rust
node_type: "TextNote".to_string()  →  schema: "TextNote".to_string()
```

**Step 3: `display_helpers.rs` — test Note constructors**

Six test Note constructions. In each:
```rust
node_type: "T".into()  →  schema: "T".into()
```

**Step 4: Compile check — expect clean or near-clean**

```bash
cargo build -p krillnotes-core 2>&1 | grep "error\["
```

---

### Task 6: Fix `scripting/mod.rs` and `scripting/schema.rs`

**Files:**
- Modify: `krillnotes-core/src/core/scripting/mod.rs`
- Modify: `krillnotes-core/src/core/scripting/schema.rs`

These files have two categories of changes:
1. `.node_type` field accesses (caught by compiler)
2. `"node_type"` Rhai map key strings (NOT caught by compiler — must be done manually)

**Step 1: Fix `.node_type` field accesses in `scripting/mod.rs`**

Every `.node_type` access (there are ~15) becomes `.schema`. Key locations from the grep:
- Line 62: `pending.node_type.clone()` → `pending.schema.clone()`
- Line 528: `node_type.clone()` (this is a local from the `create_child` closure — rename the param too)
- Line 543: `map.insert("node_type"..., Dynamic::from(node_type))` — the string key AND the value
- Line 644–653: `tx.pending_notes.get(&note_id).map(|p| p.node_type.clone())` → `.schema`
- Lines 968, 980, 1079, 1088: `.node_type` accesses
- All test Note constructors (lines ~1303, 2309, 2338, 2351, 2657, 2720, 2747, 2769, 2796, 2818, 2847, 2861, 2986, 3186, 3282, 3313)

**Step 2: Replace all Rhai map key strings in `scripting/mod.rs`**

Replace every occurrence of the string `"node_type"` used as a map key with `"schema"`:

```rust
// OLD:
note_map.insert("node_type".into(), Dynamic::from(...));
map.insert("node_type".into(), Dynamic::from(node_type));

// NEW:
note_map.insert("schema".into(), Dynamic::from(...));
map.insert("schema".into(), Dynamic::from(schema));
```

There are approximately 5 such string insertions. The test at line 2847 that asserts `t.node_type != "Task"` — this is a Rhai script string inside a test; update it:
```rhai
if t.schema != "Task" { throw "schema must be Task"; }
```

**Step 3: Fix `scripting/schema.rs` — map key strings and `.get()` calls**

Lines ~715–863 in `schema.rs`:

```rust
// Map inserts (lines ~736, 932, 943):
note_map.insert("node_type".into(), ...)  →  note_map.insert("schema".into(), ...)
parent_map.insert("node_type".into(), ...)  →  parent_map.insert("schema".into(), ...)
child_map.insert("node_type".into(), ...)  →  child_map.insert("schema".into(), ...)

// Map lookups (lines ~799, 831, 863):
.get("node_type")  →  .get("schema")
```

Also the function parameter name `node_type: &str` at line 715 — rename to `schema: &str` for clarity (update all references in that function).

**Step 4: Full compile + test run**

```bash
cargo test -p krillnotes-core 2>&1 | tail -20
```

Expected: all tests pass. If any tests fail, the error message will point to remaining `node_type` string literals in test Rhai scripts inside `#[test]` blocks — fix those too.

**Step 5: Commit**

```bash
git add krillnotes-core/
git commit -m "refactor: rename Note.node_type → schema in Rust + Rhai map keys"
```

---

### Task 7: Add SQLite migration in `storage.rs`

**Files:**
- Modify: `krillnotes-core/src/core/storage.rs`
- Modify: `krillnotes-core/src/core/schema.sql`

**Step 1: Write a failing migration test**

In `storage.rs` test section, add:

```rust
#[test]
fn test_migration_renames_node_type_to_schema() {
    // Create old-style DB with node_type column
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE notes (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            node_type TEXT NOT NULL,
            parent_id TEXT,
            position REAL NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL DEFAULT 0,
            modified_at INTEGER NOT NULL DEFAULT 0,
            created_by TEXT NOT NULL DEFAULT '',
            modified_by TEXT NOT NULL DEFAULT '',
            fields_json TEXT NOT NULL DEFAULT '{}',
            is_expanded INTEGER DEFAULT 1,
            schema_version INTEGER NOT NULL DEFAULT 1
        );
        INSERT INTO notes (id, title, node_type, position, created_at, modified_at)
        VALUES ('id1', 'T', 'TextNote', 0, 0, 0);",
    ).unwrap();

    // Run migrations
    Storage::run_migrations(&conn).unwrap();

    // Column should now be named 'schema'
    let schema_name: String = conn.query_row(
        "SELECT schema FROM notes WHERE id = 'id1'",
        [],
        |row| row.get(0),
    ).unwrap();
    assert_eq!(schema_name, "TextNote");

    // Old column name should be gone
    let old_exists: bool = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('notes') WHERE name='node_type'",
        [],
        |row| row.get::<_, i64>(0),
    ).unwrap() > 0;
    assert!(!old_exists, "node_type column should no longer exist");
}
```

**Step 2: Run test — expect failure**

```bash
cargo test -p krillnotes-core --lib storage::tests::test_migration_renames_node_type_to_schema 2>&1
```

Expected: FAIL (column doesn't exist yet, or migration not present).

**Step 3: Add the migration to `run_migrations` in `storage.rs`**

At the end of the `run_migrations` function (after all existing conditional migrations), add:

```rust
// Migration: rename node_type column to schema.
let node_type_exists: bool = conn.query_row(
    "SELECT COUNT(*) FROM pragma_table_info('notes') WHERE name='node_type'",
    [],
    |row| row.get(0),
)?;
if node_type_exists {
    conn.execute_batch(
        "ALTER TABLE notes RENAME COLUMN node_type TO schema;",
    )?;
}
```

**Step 4: Update all SQL string literals in `storage.rs`**

The compiler doesn't catch SQL strings. Search for every `node_type` inside SQL strings in `storage.rs` and replace with `schema`. Key locations (from the earlier grep — lines ~267, 339, 352, 482, 509, 559, 593, 636, 685):

- Every `CREATE TABLE notes (... node_type TEXT NOT NULL ...)` → `schema TEXT NOT NULL`
- Every `INSERT INTO notes (id, title, node_type, ...)` → `schema`
- Every `SELECT ... node_type ...` → `schema`
- Every `WHERE node_type = ?1` → `WHERE schema = ?1`

These appear in both the main migration chain and the identity migration rebuild. Update all of them.

**Step 5: Update `schema.sql`**

In `krillnotes-core/src/core/schema.sql`, change the column:

```sql
-- OLD:
node_type TEXT NOT NULL,
-- NEW:
schema TEXT NOT NULL,
```

**Step 6: Run migration test + full test suite**

```bash
cargo test -p krillnotes-core --lib storage::tests::test_migration_renames_node_type_to_schema 2>&1
cargo test -p krillnotes-core 2>&1 | tail -20
```

Expected: all pass.

**Step 7: Commit**

```bash
git add krillnotes-core/src/core/storage.rs krillnotes-core/src/core/schema.sql
git commit -m "feat: migrate notes.node_type column → schema"
```

---

### Task 8: Update Tauri backend (`lib.rs`)

**Files:**
- Modify: `krillnotes-desktop/src-tauri/src/lib.rs`

**Step 1: Update function parameter names and doc comments**

`lib.rs` uses `node_type` as a parameter name in several Tauri commands. Rename parameters for consistency (the compiler won't force this since they're just parameter names, but it's important for readability):

- `create_note` command (line ~676): `node_type: String` → `schema: String`; update the body `create_note(..., &node_type)` → `create_note(..., &schema)` and `create_note_root(&node_type)` → `create_note_root(&schema)`
- `get_schema_info` command (line ~816): `node_type: String` → `schema: String`; update body references

The function `get_node_types` (line ~616) is a command name exposed to TypeScript — **do not rename this function** or you'd need to update all callers. The name refers to "types of notes" generically and is fine as-is.

**Step 2: Build check**

```bash
cd krillnotes-desktop && npx tauri build --no-bundle 2>&1 | grep "error\[" | head -20
```

**Step 3: Commit**

```bash
git add krillnotes-desktop/src-tauri/
git commit -m "refactor: rename node_type param → schema in Tauri commands"
```

---

### Task 9: Update TypeScript types and frontend components

**Files:**
- Modify: `krillnotes-desktop/src/types.ts`
- Modify: `krillnotes-desktop/src/components/AddNoteDialog.tsx`
- Modify: `krillnotes-desktop/src/components/InfoPanel.tsx`
- Modify: `krillnotes-desktop/src/components/TreeNode.tsx`
- Modify: `krillnotes-desktop/src/components/WorkspaceView.tsx`
- Modify: `krillnotes-desktop/src/components/TreeView.tsx`
- Modify: `krillnotes-desktop/src/utils/tree.ts`
- Modify: `krillnotes-desktop/src/utils/noteTypes.ts`

**Step 1: Update `types.ts`**

```typescript
// OLD:
nodeType: string;
// NEW:
schema: string;
```

**Step 2: TypeScript compile check — let the compiler find all usages**

```bash
cd krillnotes-desktop && npx tsc --noEmit 2>&1 | head -40
```

Expected: errors pointing to every `.noteType` access — this is your todo list.

**Step 3: Fix `AddNoteDialog.tsx`**

```typescript
// OLD:
const [nodeType, setNodeType] = useState<string>('');
// ... availableTypes.includes(nodeType) ...
// ... nodeType, ...
// ... value={nodeType} ...
// ... !nodeType || ...

// NEW: rename all occurrences of nodeType/setNodeType in this file
const [schema, setSchema] = useState<string>('');
// ... availableTypes.includes(schema) ...
// ... schema, ...
// ... value={schema} ...
// ... !schema || ...
```

**Step 4: Fix remaining components**

For each remaining TypeScript error from Step 2, rename `.noteType` → `.schema` and any local variable `noteType` → `schema`. The pattern is consistent across all components — every reference to the `Note.noteType` property becomes `Note.schema`.

**Step 5: TypeScript compile check — must be clean**

```bash
cd krillnotes-desktop && npx tsc --noEmit 2>&1
```

Expected: no errors.

**Step 6: Commit**

```bash
git add krillnotes-desktop/src/
git commit -m "refactor: rename Note.nodeType → schema in TypeScript"
```

---

### Task 10: Update documentation

**Files:**
- Modify: `SCRIPTING.md`
- Modify: `CHANGELOG.md`

**Step 1: Update `SCRIPTING.md`**

Replace both occurrences of `note.node_type` in the properties tables:

```markdown
<!-- Line ~336 and ~1064 -->
<!-- OLD: -->
| `note.node_type` | String | — |
<!-- NEW: -->
| `note.schema` | String | — |
```

**Step 2: Add `CHANGELOG.md` entry**

Add at the top of `CHANGELOG.md` under a new version section:

```markdown
## [Unreleased]

### Changed
- **Breaking (Rhai scripts):** `note.node_type` renamed to `note.schema` in all Rhai script contexts.
  Update any user scripts that reference `note.node_type` → `note.schema`.
- `Note` JSON key changed from `nodeType` to `schema` in workspace exports.
  Old `.krillnotes` archives with `nodeType` are still importable (backward compat preserved).
```

**Step 3: Commit**

```bash
git add SCRIPTING.md CHANGELOG.md
git commit -m "docs: update SCRIPTING.md and CHANGELOG for node_type → schema rename"
```

---

### Task 11: Final verification

**Step 1: Full Rust test suite**

```bash
cargo test -p krillnotes-core 2>&1 | tail -5
```

Expected: `test result: ok. N passed; 0 failed`

**Step 2: TypeScript type check**

```bash
cd krillnotes-desktop && npx tsc --noEmit 2>&1
```

Expected: no output (clean).

**Step 3: Grep for any remaining `node_type` in source (not docs/history)**

```bash
grep -r "node_type\|nodeType" \
  krillnotes-core/src \
  krillnotes-desktop/src \
  krillnotes-desktop/src-tauri/src \
  --include="*.rs" --include="*.ts" --include="*.tsx" \
  -l
```

Expected: no output. If any files appear, fix them.

**Step 4: Push and open PR**

```bash
git push github-https feat/rename-node-type-to-schema
gh pr create --title "refactor: rename node_type → schema everywhere" \
  --body "Renames the Note schema field from node_type to schema across SQLite, Rust, Rhai, TypeScript, and docs. Old .krillnotes archives still import via serde alias. See docs/plans/2026-03-10-rename-node-type-to-schema-design.md."
```
