# Design: Rhai Display Helper API Redesign

**Context:** Follow-up to the image embedding feature (`feat/image-embedding`). The original `display_image` / `display_download_link` helpers used a global `NoteRunContext` to resolve `field:` and `attach:` source strings. This breaks when iterating child notes — the helpers silently operate on the wrong note.

---

## Problem

```rhai
// BROKEN: resolves against the parent note's run_context, not each child
children.map(|c| display_image("field:photo", 200, c.title))
```

The `run_context` is a single global slot populated by the workspace before running the hook. Any iteration over multiple notes hits the same context, so `field:photo` always resolves against the note that triggered the hook.

---

## New Rhai API

```rhai
// File field — UUID is already the field value:
display_image(note.fields["photo"], 480, "alt text")

// Attachment by filename:
let atts = get_attachments(note.id);
let hero = atts.find(|a| a.filename == "hero.jpg");
display_image(hero.id, 480, "Hero")

// Download link:
display_download_link(hero.id, hero.filename)

// Iteration over children — now correct:
children.map(|c| display_image(c.fields["photo"], 200, c.title))
```

---

## Signature Changes

| Function | Old signature | New signature |
|---|---|---|
| `display_image` | `(source: String, width: i64, alt: String)` | `(uuid: Dynamic, width: i64, alt: String)` |
| `display_download_link` | `(source: String, label: String)` | `(uuid: Dynamic, label: String)` |
| `get_attachments` | *(not exposed to Rhai)* | `(note_id: String) → Array` |

**Why `Dynamic` for uuid:** `FieldValue::File(None)` serialises to Rhai `()` (unit). Registering the parameter as `String` would throw a runtime dispatch error when the field is unset. `Dynamic` handles both the set (String UUID) and unset (`()`) cases gracefully — the helper renders a `kn-image-error` span for `()`.

**`get_attachments` return shape:** Array of maps, each with keys `id` (String), `filename` (String), `mime_type` (String or `()`), `size_bytes` (i64).

---

## Rust Implementation

### `display_image` and `display_download_link`

Become pure HTML generators — no storage access, no `Arc` capture. Inspect the `Dynamic` first argument:
- Non-empty String → emit `<img data-kn-attach-id="uuid" ...>` / `<a data-kn-download-id="uuid" ...>` sentinel
- `()` or empty String → emit `<span class="kn-image-error">No image set</span>`

### `get_attachments(note_id: String)`

Registered as a closure capturing `storage_arc` (same pattern as `get_children`). Calls `workspace.get_attachments(note_id)` at runtime and maps the result to a `rhai::Array` of `rhai::Map` objects.

### `NoteRunContext`

No longer needed by `display_image` / `display_download_link`. Kept in place for `markdown()`, which still uses it to pre-process `{{image: field:xxx}}` blocks in textarea content.

---

## What Is Removed

- `resolve_attachment_source` call sites inside `display_image` / `display_download_link` (the function itself stays — still used by `preprocess_image_blocks` for the markdown `{{image:}}` syntax)
- `field:` / `attach:` prefix string parsing inside the two display helpers
- `ctx_for_display_image` and `ctx_for_download_link` `Arc::clone` calls in `ScriptRegistry::new()`

---

## Files Touched

- `krillnotes-core/src/core/scripting/mod.rs` — rewrite `display_image` and `display_download_link` closures; add `get_attachments` registration
- `krillnotes-core/src/core/scripting/display_helpers.rs` — simplify `make_display_image_html` / `make_download_link_html` (drop `fields` / `attachments` params); update unit tests
- `templates/photo_note.rhai` — update to new API
