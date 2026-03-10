# Design: Rename `node_type` → `schema`

**Date:** 2026-03-10
**Status:** Approved

## Problem

The `node_type` field on `Note` is exposed to users in Rhai scripts as `note.node_type`, which is an odd internal name. Semantically it is the note's *schema* — the Rhai script that defines its fields and behaviour. Renaming it to `schema` makes every layer more self-explanatory.

## Approach

Full rename across every layer (Approach A):

- **SQLite column** renamed from `node_type` → `schema` via a new migration. `schema` is not a reserved keyword in SQLite so no quoting is required.
- **Rust struct** field renamed `Note.node_type` → `Note.schema`.
- **Serde** backward compat: `#[serde(alias = "nodeType")]` on the renamed field so old `.krillnotes` archives (which serialise the key as `nodeType`) still import correctly. New exports will serialise as `schema`.
- **Rhai map keys** in `scripting/mod.rs`: string literals `"node_type"` replaced with `"schema"` wherever a note map is built and passed into Rhai scripts.
- **TypeScript** interface: `noteType: string` → `schema: string` in `types.ts`, and all component references updated.
- **Documentation**: `SCRIPTING.md` and `.rhai` templates updated; `CHANGELOG.md` entry added.

## Backward Compatibility

| Consumer | Before | After | Compat mechanism |
|----------|--------|-------|-----------------|
| Old `.krillnotes` archives | `"nodeType"` JSON key | `"schema"` JSON key | `#[serde(alias = "nodeType")]` on field |
| Existing workspaces (SQLite) | `node_type` column | `schema` column | Migration runs at workspace open |
| Rhai scripts written by users | `note.node_type` | `note.schema` | **Breaking for user scripts** — document in CHANGELOG |

User-written Rhai scripts that reference `note.node_type` will need to be updated manually. This is unavoidable and is the only true breaking change.

## Files Affected

### krillnotes-core
- `src/core/note.rs` — field rename + serde alias
- `src/core/schema.sql` — DDL column rename
- `src/core/storage.rs` — migration + all SQL queries
- `src/core/workspace.rs` — `.node_type` → `.schema`, struct init
- `src/core/operation.rs` — field references
- `src/core/operation_log.rs` — field references
- `src/core/save_transaction.rs` — field references
- `src/core/export.rs` — field references
- `src/core/scripting/mod.rs` — `"node_type"` map key strings + `.node_type` accesses
- `src/core/scripting/schema.rs` — any field references
- `src/core/scripting/display_helpers.rs` — any field references

### krillnotes-desktop
- `src/types.ts` — `nodeType` → `schema`
- `src/components/InfoPanel.tsx`
- `src/components/TreeNode.tsx`
- `src/components/WorkspaceView.tsx`
- `src/components/TreeView.tsx`
- `src/components/AddNoteDialog.tsx`
- `src/utils/tree.ts`
- `src/utils/noteTypes.ts`

### Documentation & Templates
- `SCRIPTING.md`
- `templates/*.rhai`
- `CHANGELOG.md`

## Migration

```sql
ALTER TABLE notes RENAME COLUMN node_type TO schema;
```

This is a single non-destructive DDL statement. It runs inside the existing `storage.rs` migration chain. The migration version will be incremented.

## Testing

- Existing `cargo test -p krillnotes-core` must pass with all `node_type` identifiers renamed.
- TypeScript type-check (`npx tsc --noEmit`) must pass.
- Manual smoke test: open an existing workspace, verify notes load; import a pre-rename `.krillnotes` archive, verify notes appear with correct schema names.
