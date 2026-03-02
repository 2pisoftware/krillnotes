# Design: Binary IPC for Drag-and-Drop File Attachments

**Date:** 2026-03-02
**Issue:** #55
**Branch:** `perf/binary-ipc-attachments`

## Problem

The drag-and-drop attachment path reads a `File` as an `ArrayBuffer`, then converts it to a JSON number array via `Array.from(new Uint8Array(buffer))` before passing it to the Tauri IPC bridge. A 10 MB file becomes ~30 MB of JSON text. This wastes IPC bandwidth and CPU for large files.

## Solution

Tauri v2 supports raw binary IPC: you can pass a `Uint8Array` (or `ArrayBuffer`) directly as the `args` parameter to `invoke()`, and the Rust command receives it as `InvokeBody::Raw(Vec<u8>)` via `tauri::ipc::Request<'_>`.

Metadata that would normally be named parameters (`note_id`, `filename`) must come from HTTP headers when the body is raw binary — Tauri's parameter deserialization fails on raw bodies for named keys.

## Architecture

```
Frontend                     IPC Bridge                 Rust
--------                     ----------                 ----
file.arrayBuffer()
  → new Uint8Array(bytes)
  → invoke(cmd, bytes, {      → InvokeBody::Raw(bytes)  → request.body()
      headers: {              → HeaderMap                → request.headers()
        x-note-id: ...,
        x-filename: <b64>,
      }
    })
```

The filename is base64-encoded in the header to safely handle non-ASCII characters (Japanese, emoji, etc.) — `http::HeaderValue` only accepts ASCII bytes.

## Tauri v2 API facts

- `Request<'a>` implements `CommandArg<'a, R>` — fully compatible alongside `Window` and `State<'_>`
- When `InvokeBody::Raw`, any attempt to deserialise a named arg from the body returns an error → metadata goes in headers
- `invoke(cmd, Uint8Array, options)` — second arg typed as `InvokeArgs` which accepts `Uint8Array`
- Android note: `InvokeBody::Raw` is not supported on Android; the fallback would be base64 string in JSON. Not relevant here (desktop-only app).

## Scope

| File | Change |
|------|--------|
| `krillnotes-desktop/src-tauri/src/lib.rs` | `attach_file_bytes`: replace `data: Vec<u8>` parameter with `request: tauri::ipc::Request<'_>`; extract bytes from body, `note_id`/`filename` from headers |
| `krillnotes-desktop/src/components/AttachmentsSection.tsx` | `handleDrop`: pass `new Uint8Array(buffer)` as invoke args; add `x-note-id` and base64 `x-filename` headers |

No changes to: encryption, storage, `attach_file` (file-picker path), export/import.
