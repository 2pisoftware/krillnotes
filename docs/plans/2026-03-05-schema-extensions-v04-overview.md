# Schema Extensions v0.4 — Implementation Overview

**Spec:** `docs/swarm/KrillNotes_Schema_Extensions_Spec_v0_4.docx`
**Date:** 2026-03-05
**Branch base:** `master`

This document is the master plan for implementing the Schema Extensions Spec v0.4.
It covers field validation, note-level validation (reject), field groups, the
constructing/reading split, schema versioning, and migration. The work is split
into three phases, each independently shippable and testable.

---

## Current State

The existing schema system is a flat, single-registration model:

- `schema()` takes a name + Rhai map, registers field definitions and hooks
  (`on_save`, `on_view`, `on_hover`, `on_add_child`) in one call.
- `SchemaRegistry` stores schemas and hooks in parallel `Arc<Mutex<HashMap>>` tables.
- `FieldDefinition` has: name, field_type, required, can_view, can_edit,
  options, max, target_type, show_on_hover, allowed_types.
- `Schema` has: name, fields, title_can_view/edit, children_sort,
  allowed_parent/children_types, allow_attachments, attachment_types.
- No field groups, validate closures, reject(), schema versioning,
  schema_version on notes, or file-category distinction.
- All scripts are `.rhai` files loaded in `load_order` sequence.
- The on_save hook mutates a note map directly (`note.title = ...;
  note.fields["x"] = ...;`) and returns it. There is no gated operations
  model (set_field / set_title / commit / reject).

---

## Phase 1 — Field Validation, reject(), and Field Groups

**Spec sections:** 2, 3, 4, 5.1, 5.3, 6, 7.1–7.4, Appendix A (partial), B, C, D

### Goals

- `validate` closures on field definitions (evaluated in UI on blur + at save time)
- `reject()` function in on_save (soft errors that block commit)
- `field_groups` with `visible` closures and `collapsed` flag
- The 7-step save pipeline (group visibility -> field validation -> required
  check -> open tx -> on_save -> check rejects -> commit+stamp)
- Frontend: inline validation errors, collapsible field groups, hidden-group
  data indicator, reject error banner

### Key Design Decisions

- **Gated operations model:** The spec calls for `set_field()` / `set_title()`
  / `commit()` replacing direct note mutation in on_save. This is a breaking
  change to every existing script. We must decide whether to:
  (a) implement the full gated model now, or
  (b) keep the current direct-mutation model and layer reject() on top, deferring
      the gated model to Phase 2/3 when we split file categories anyway.
- **Validate closures stored as `rhai::FnPtr`:** The Rhai `sync` feature makes
  FnPtr Send+Sync, so they can live in the registry alongside field definitions.
- **Field name uniqueness across groups:** Enforced at schema registration time.
- **Hidden group data:** Existing values preserved; indicator shown in UI.

### Deliverables

- [ ] Extend `FieldDefinition` with `validate: Option<rhai::FnPtr>`
- [ ] Add `FieldGroup` struct and `field_groups: Vec<FieldGroup>` to `Schema`
- [ ] Parse validate/field_groups from Rhai map in `Schema::parse_from_rhai()`
- [ ] Implement `reject()` Rhai function (context-scoped to on_save)
- [ ] Update save pipeline in `workspace.rs` (validate -> required -> on_save -> reject check)
- [ ] New Tauri commands: `validate_field`, `validate_fields`, `evaluate_group_visibility`
- [ ] Frontend: FieldEditor inline validation, collapsible groups in InfoPanel,
      reject error banner, hidden-group indicator
- [ ] Update system scripts with validate examples (e.g. Task due_date, Contact email)
- [ ] Tests: field validation, reject accumulation, group visibility, save pipeline

### Estimated Scope

~15–20 tasks. Touches: `schema.rs`, `mod.rs`, `workspace.rs`, `lib.rs`,
`InfoPanel.tsx`, `FieldEditor.tsx`, `types.ts`, system scripts.

---

## Phase 2 — Constructing/Reading Split and Presentation Registration

**Spec sections:** 9.1–9.5, 9.10–9.11, 5.2, 7.3, 7.5

### Goals

- Two file categories: `.schema.rhai` (data type) and `.rhai` (library + presentation)
- `register_view()`, `register_hover()`, `register_menu()` deferred binding
- Two-phase script loading (`.rhai` first, then `.schema.rhai`, then resolve bindings)
- Tabbed view mode (multiple views per type, built-in "Fields" tab always last)
- Script Manager visual distinction (blue for schema, amber for library/presentation)
- Migrate existing on_view/on_hover/add_tree_action from schema() into register_*()

### Key Design Decisions

- **Backward compatibility:** Existing scripts use on_view/on_hover inside schema().
  We need a migration path. Options: (a) deprecation period where both work,
  (b) clean break (spec recommends this since v0.3 shape/behaviour was never deployed).
- **Deferred binding resolution:** register_*() calls queue bindings that are
  resolved after all schemas load. Failed bindings produce Script Manager warnings.
- **Multiple views per type:** Each register_view() becomes a tab. The `front: true`
  flag controls which tab is default.

### Deliverables

- [ ] `ViewRegistration`, `HoverRegistration`, `MenuRegistration` structs
- [ ] `TypeDefinition` struct composing `Schema` + presentation bindings
- [ ] Deferred binding queue + resolution in ScriptRegistry
- [ ] Two-phase loading: reorder script execution by file category
- [ ] `register_view()`, `register_hover()`, `register_menu()` Rhai functions
- [ ] Remove on_view/on_hover from schema() (breaking change)
- [ ] Frontend: tabbed view mode in InfoPanel, Script Manager category badges
- [ ] `get_views_for_type` Tauri command
- [ ] Migrate all system scripts and templates to the split format
- [ ] Tests: deferred binding, tab rendering, missing-schema warnings

### Estimated Scope

~12–15 tasks. Touches: `schema.rs`, `mod.rs`, `hooks.rs`, `workspace.rs`,
`lib.rs`, `InfoPanel.tsx`, `ScriptManagerDialog.tsx`, `types.ts`, all system
scripts, all templates.

---

## Phase 3 — Schema Versioning and Migration

**Spec sections:** 9.6–9.9, 8.1–8.2, 3.6 step 7, 7.5

### Goals

- `version: u32` required key in schema()
- `schema_version` column on `notes` table (DB migration)
- Stamping: schema_version set on create and on every successful save
- `migrate` map: version -> closure, chained execution for multi-version jumps
- Version skew indicator in UI (banner when note version < schema version)
- `UpdateSchema` operation type in the operation log
- Forward compatibility: notes with higher schema_version than local are
  stored but flagged; unknown fields preserved as opaque JSON

### Key Design Decisions

- **Latest-only policy:** Only the current schema version is held in the registry.
  Historical versions exist only in the operation log as `UpdateSchema` entries.
- **Migration trigger:** User-initiated (open for editing), not automatic.
  The user sees migrated values in the editor and must save to persist.
- **schema_version default:** 0 for notes created before versioning. Migration
  closures start from version 1.
- **UpdateSchema operation:** Only for .schema.rhai files. Regular .rhai scripts
  continue to use CreateUserScript/UpdateUserScript.

### Deliverables

- [ ] Add `version: u32` parsing in `Schema::parse_from_rhai()`
- [ ] DB migration: add `schema_version INTEGER DEFAULT 0` to notes table
- [ ] Stamp schema_version on create_note and update_note
- [ ] Parse `migrate` map from schema definition
- [ ] `run_migration()` method: chains closures from note's version to current
- [ ] `get_note_schema_version` and `get_schema_version` Tauri commands
- [ ] Frontend: version skew banner in InfoPanel, migration preview on edit
- [ ] `UpdateSchema` operation variant + logging on script save
- [ ] Forward-compat: store unknown fields, flag notes with future versions
- [ ] Tests: migration chaining, version stamping, skew detection

### Estimated Scope

~10–12 tasks. Touches: `schema.rs`, `operation.rs`, `operation_log.rs`,
`workspace.rs`, `storage.rs` (migration), `lib.rs`, `InfoPanel.tsx`, `types.ts`.

---

## Open Questions (from Spec Section 10)

These are deferred to the relevant phase:

| # | Question | Phase | Recommendation |
|---|----------|-------|----------------|
| 1 | Validation errors for hidden groups | 1 | Show as note-level banner (don't auto-reveal hidden group) |
| 2 | get_groups() in on_save | 1 | Yes — useful for conditional logic in save hooks |
| 3 | set_field() on non-existent fields | 1/2 | Hard error (safety first) |
| 4 | Bulk migration UX | 3 | Defer to post-v0.4 |
| 5 | Schema version conflict (concurrent edits) | 3 | Single-authority model for now |
| 6 | register_menu() transaction scope | 2 | Same model as on_save (with reject support) |
| 7 | Chart/graph display primitives | 2 | Defer — table/stack/field/text are sufficient initially |

---

## Sequencing

```
Phase 1 (validation + groups)
    |
    v
Phase 2 (constructing/reading split)
    |
    v
Phase 3 (versioning + migration)
```

Each phase produces its own design doc and implementation plan at
`docs/plans/2026-MM-DD-schema-extensions-phase-N-*.md`.

All work branches off `master`. PRs target `master`.
