# Implementation Plan: Binary IPC for Drag-and-Drop File Attachments

**Date:** 2026-03-02
**Issue:** #55
**Design:** `2026-03-02-binary-ipc-attachments-design.md`

## Tasks

### 1. Update `attach_file_bytes` in `lib.rs`

**File:** `krillnotes-desktop/src-tauri/src/lib.rs` (~line 1422)

Replace:
```rust
fn attach_file_bytes(
    window: tauri::Window,
    state: State<'_, AppState>,
    note_id: String,
    filename: String,
    data: Vec<u8>,
) -> std::result::Result<AttachmentMeta, String> {
    let label = window.label();
    let mut workspaces = state.workspaces.lock().expect("Mutex poisoned");
    let workspace = workspaces.get_mut(label).ok_or("No workspace open")?;
    let mime_type = mime_guess::from_path(&filename)
        .first()
        .map(|m| m.to_string());
    workspace
        .attach_file(&note_id, &filename, mime_type.as_deref(), &data)
        .map_err(|e| e.to_string())
}
```

With:
```rust
fn attach_file_bytes(
    request: tauri::ipc::Request<'_>,
    window: tauri::Window,
    state: State<'_, AppState>,
) -> std::result::Result<AttachmentMeta, String> {
    // Extract binary body.
    let tauri::ipc::InvokeBody::Raw(data) = request.body() else {
        return Err("attach_file_bytes: expected raw binary body".to_string());
    };
    // note_id comes through as a plain ASCII header.
    let note_id = request
        .headers()
        .get("x-note-id")
        .and_then(|v| v.to_str().ok())
        .ok_or("attach_file_bytes: missing x-note-id header")?
        .to_owned();
    // filename is base64(UTF-8 bytes) to safely handle non-ASCII characters.
    let filename_b64 = request
        .headers()
        .get("x-filename")
        .and_then(|v| v.to_str().ok())
        .ok_or("attach_file_bytes: missing x-filename header")?;
    let filename_bytes = {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode(filename_b64)
            .map_err(|e| format!("attach_file_bytes: invalid filename encoding: {e}"))?
    };
    let filename = String::from_utf8(filename_bytes)
        .map_err(|e| format!("attach_file_bytes: invalid UTF-8 in filename: {e}"))?;

    let label = window.label();
    let mut workspaces = state.workspaces.lock().expect("Mutex poisoned");
    let workspace = workspaces.get_mut(label).ok_or("No workspace open")?;
    let mime_type = mime_guess::from_path(&filename)
        .first()
        .map(|m| m.to_string());
    workspace
        .attach_file(&note_id, &filename, mime_type.as_deref(), data)
        .map_err(|e| e.to_string())
}
```

Note: `data` is now `&Vec<u8>` from the request (not owned), but `attach_file` takes `&[u8]` so it deref-coerces fine.

### 2. Update `handleDrop` in `AttachmentsSection.tsx`

**File:** `krillnotes-desktop/src/components/AttachmentsSection.tsx` (~line 57)

Replace:
```typescript
const buffer = await file.arrayBuffer();
const data = Array.from(new Uint8Array(buffer));
await invoke('attach_file_bytes', { noteId, filename: file.name, data });
```

With:
```typescript
const buffer = await file.arrayBuffer();
// Encode filename as base64 UTF-8 bytes to safely pass through ASCII-only headers.
const nameBytes = new TextEncoder().encode(file.name);
let nameBinary = '';
for (const b of nameBytes) nameBinary += String.fromCharCode(b);
const filenameB64 = btoa(nameBinary);
await invoke('attach_file_bytes', new Uint8Array(buffer), {
    headers: {
        'x-note-id': noteId,
        'x-filename': filenameB64,
    },
});
```

### 3. Build & test

```bash
cd .worktrees/perf/binary-ipc-attachments/krillnotes-desktop
cargo test -p krillnotes-core 2>&1 | tail -5
cargo build --manifest-path src-tauri/Cargo.toml 2>&1 | tail -10
npm run check 2>&1 | tail -10
```
