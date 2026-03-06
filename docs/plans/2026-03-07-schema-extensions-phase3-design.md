# Schema Extensions Phase 3 — Design Document

**Spec:** `docs/swarm/KrillNotes_Schema_Extensions_Spec_v0_4.docx` (Sections 9.6-9.9, 8.1-8.2, 3.6 step 7, 7.5)
**Parent plan:** `docs/plans/2026-03-05-schema-extensions-v04-overview.md`
**Date:** 2026-03-07
**Branch base:** `master`

Phase 3 delivers: schema versioning, batch migration on schema load, version
stamping on create/save, the `UpdateSchema` operation variant, and a frontend
migration notification toast.

---

## Key Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Migration trigger | Batch on schema load (Phase D) | Avoids stale DB and compounding migrations from lazy approaches |
| Migration scope | Fields + title, direct mutation | Covers 99% of real migrations without dangerous side effects |
| Operations model | Bypass gated ops + validation | Migrations are trusted schema-author code; avoids log bloat since migration is deterministic and runs on every peer |
| Operation log | Single `UpdateSchema` per schema type | The log is a sync primitive, not an audit trail; the core library guarantees migrated data on read |
| Forward compatibility | Deferred | Only relevant once sync is live; sync design v0.5 may have its own approach |
| Backward compatibility | Clean break | No legacy workspaces to migrate; all scripts get `version: 1` |
| Same-version updates | Allowed | Version is a data contract version, not a code revision; authors can update validations/hooks freely |
| Version downgrade | Hard error | Prevents accidental old schema uploads; enforced at registration time |

---

## 1. Data Model Changes

### 1.1 Schema Struct

Add two fields:

```rust
pub struct Schema {
    // ... existing fields ...
    pub version: u32,                               // required key in schema()
    pub migrations: BTreeMap<u32, rhai::FnPtr>,     // target_version -> closure
}
```

- `version` is **required** -- omitting it is a hard error at registration time
- `migrations` map is optional (empty if no migrations needed yet)
- Keyed by **target version** -- the closure at key `2` migrates from v1 to v2

### 1.2 Note Struct

```rust
pub struct Note {
    // ... existing fields ...
    pub schema_version: u32,
}
```

Stamped with the schema's current `version` on `create_note`. Updated after
successful save and after migration.

### 1.3 Database

Update `schema.sql` DDL directly (clean break, no migration needed):

```sql
-- In notes table definition:
schema_version INTEGER NOT NULL DEFAULT 1
```

Update all `INSERT` and `SELECT` queries to include `schema_version`.

### 1.4 Operation Enum — `UpdateSchema` Variant

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
}
```

Logged once per schema type when batch migration completes.

---

## 2. Version Guard on Schema Registration

When `schema()` is called and a schema with that name already exists:

| Condition | Behavior |
|-----------|----------|
| new version < existing version | **Hard error.** Script fails to load. Warning in Script Manager: "Schema 'X' version N cannot replace existing version M -- downgrade not allowed" |
| new version == existing version | **Allowed.** Schema re-registers with updated definition. No migration runs. |
| new version > existing version | **Allowed.** Phase D migration kicks in after loading completes. |

This applies to:
- Script loading on workspace open
- Saving scripts in the Script Manager
- Importing scripts from file

---

## 3. Migration Pipeline (Phase D)

After script loading completes (phases A/B/C from phase 2), a new Phase D runs
in `workspace.rs`:

```
Phase A: Load presentation scripts
Phase B: Load schema scripts
Phase C: Resolve deferred bindings
Phase D: Run schema migrations        <-- NEW
```

### 3.1 Phase D Steps

1. **Collect versioned schemas** -- iterate all registered schemas, get each
   `(name, version, migrations)`
2. **Query stale notes** -- for each schema:
   `SELECT id, title, fields_json, schema_version FROM notes WHERE node_type = ? AND schema_version < ?`
3. **Skip if none** -- no stale notes means no work for this schema type
4. **Chain closures** -- for a note at v1 and schema at v3, run migration
   closures for keys 2 then 3 in order
5. **Write back** -- single transaction per schema type: update each note's
   `title`, `fields_json`, and `schema_version`
6. **Log operation** -- one `UpdateSchema` operation per schema type migrated
7. **Emit notification** -- Tauri event `schema-migrated` with payload

### 3.2 Migration Closure Contract

```rhai
schema("Contact", #{
    version: 2,
    fields: [...],
    migrate: #{
        2: |note| {
            // note.title -- readable and writable
            // note.fields -- mutable map of field values
            note.fields["mobile"] = note.fields["phone"];
            note.fields.remove("phone");
        }
    }
});
```

The closure receives a Rhai map with `title` (String) and `fields` (Map).
Mutates in place, returns nothing. No gated operations, no validation, no
per-note operation log entries.

### 3.3 Multi-Version Jump Example

```rhai
schema("Contact", #{
    version: 3,
    fields: [...],
    migrate: #{
        2: |note| {
            note.fields["mobile"] = note.fields["phone"];
            note.fields.remove("phone");
        },
        3: |note| {
            let parts = note.fields["name"].split(" ");
            note.fields["first_name"] = parts[0];
            note.fields["last_name"] = if parts.len() > 1 { parts[1] } else { "" };
            note.fields.remove("name");
        }
    }
});
```

A note at v1 runs closures 2 then 3 in sequence. A note at v2 runs only
closure 3.

### 3.4 Error Handling

- Migration closure failure (runtime error) -> **entire batch for that schema
  type rolls back**. No partial migrations.
- Error surfaced as a `ScriptWarning` in the Script Manager.
- Other schema types continue migrating independently.

---

## 4. Stamping on Create and Save

### 4.1 On `create_note`

Look up the schema for the chosen `node_type` and stamp `schema_version` with
the schema's current `version`.

### 4.2 On `update_note` (save pipeline)

After a successful save (validation passes, `on_save` runs, no rejects),
re-stamp `schema_version` to the current schema version. Handles the edge case
where a note was loaded before a schema upgrade but saved after.

### 4.3 No stamping during migration

Phase D writes `schema_version` directly. The save pipeline stamping is only
for normal user edits.

---

## 5. Frontend — Notification Toast

### 5.1 Tauri Event

Event name: `schema-migrated`

```typescript
interface SchemaMigratedEvent {
    schemaName: string;
    fromVersion: number;
    toVersion: number;
    notesMigrated: number;
}
```

Emitted once per schema type after Phase D migration completes.

### 5.2 Toast Notification

`WorkspaceView.tsx` listens for `schema-migrated` events and shows a transient
toast:

> **"Contact" schema updated** -- 12 notes migrated to version 3

Auto-dismisses after a few seconds. No user action required.

### 5.3 No Other Frontend Changes

No version skew banners, no migration preview, no version display in Script
Manager UI. The `version` field is visible in script source code.

---

## 6. Rhai `schema()` Parsing Changes

Two new keys in `parse_from_rhai()`:

| Key | Type | Required | Default |
|-----|------|----------|---------|
| `version` | `u32` (via `INT`) | **Yes** | Hard error if missing |
| `migrate` | Map of `INT -> FnPtr` | No | Empty `BTreeMap` |

Validation at parse time:
- `version` must be >= 1
- Each key in `migrate` must be > 1 and <= `version`
- Migration keys must be contiguous from some start to `version` (no gaps)

---

## 7. Files Changed

### Modified (Rust -- krillnotes-core)

| File | Changes |
|------|---------|
| `scripting/schema.rs` | Add `version` and `migrations` to `Schema`. Parse `version` (required) and `migrate` map in `parse_from_rhai()`. Version guard on re-registration. Add `get_migrations()` on `SchemaRegistry`. |
| `scripting/mod.rs` | Pass migration closures' AST context for evaluation. |
| `workspace.rs` | Phase D migration step after script loading. Stamp `schema_version` on `create_note` and `update_note`. |
| `note.rs` | Add `schema_version: u32` to `Note`. |
| `operation.rs` | Add `UpdateSchema` variant. |
| `operation_log.rs` | Handle `UpdateSchema` in log/replay. |
| `storage.rs` | Update `schema.sql` DDL -- add `schema_version` to `notes` table. Update INSERT/SELECT queries. |

### Modified (Rust -- krillnotes-desktop)

| File | Changes |
|------|---------|
| `lib.rs` | Emit `schema-migrated` Tauri event after Phase D. |

### Modified (TypeScript)

| File | Changes |
|------|---------|
| `types.ts` | Add `schemaVersion` to `Note`. Add `SchemaMigratedEvent` interface. |
| `WorkspaceView.tsx` | Listen for `schema-migrated` event, render toast notification. |

### Modified (Scripts -- all get `version: 1`)

| File | Schemas |
|------|---------|
| `00_text_note.schema.rhai` | TextNote |
| `01_contact.schema.rhai` | ContactsFolder, Contact |
| `02_task.schema.rhai` | Task |
| `03_project.schema.rhai` | Project |
| `05_recipe.schema.rhai` | Recipe |
| `06_product.schema.rhai` | Product |
| `templates/zettelkasten.schema.rhai` | Zettel, Kasten |
| `templates/book_collection.schema.rhai` | BookCollection, Book |
| `templates/photo_note.schema.rhai` | PhotoNote |

### No new files

All changes fit within existing files.
