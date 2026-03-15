# Folder Sync Addressing — Design Spec

**Date:** 2026-03-15
**Status:** Approved

## Problem

The folder sync channel (`FolderChannel`) uses a flat shared directory where all peers read and write `.swarm` bundle files. Two bugs exist:

1. **No inbox filtering:** Every device in the shared folder tries to decrypt every bundle it didn't write itself. When Alice and Charlie share the same folder, Alice reads Charlie's bundles and gets "no recipient entry matched our key" errors. These bundles are never deleted (correctly), so the error repeats on every poll cycle forever.

2. **Silent channel config failure:** `update_channel_config` issues a SQL `UPDATE … WHERE peer_device_id = ?`. If the device ID is stale (e.g. the peer row was recently consolidated from a placeholder `identity:<pubkey>` to a real device ID via `upsert_peer_from_delta`), the UPDATE matches 0 rows and silently does nothing. The caller receives no error and the config change is lost. This is intentional only for internal watermark bookkeeping — not for user-facing channel configuration.

## Design

### Fix 1 — Recipient-prefixed filenames

**New filename format:**

```
{RECIPIENT_identity_short}_{timestamp}_{uuid_short}.swarm
```

where `RECIPIENT_identity_short` = first 8 chars of the recipient peer's `peer_identity_id` (base64 Ed25519 public key), with `/`→`-` and `+`→`_` applied (URL-safe base64 mapping) to make the prefix filesystem-safe. `timestamp` = `YYYYMMDDHHmmss` (14 digits), `uuid_short` = first 8 chars of a new UUIDv4.

**Sender side (`send_bundle`):**
Derive `recipient_short` from `peer.peer_identity_id.chars().take(8)`, with `/`→`-` and `+`→`_` to avoid path-separator issues (standard base64 contains `/`). If `peer_identity_id` is empty, return `KrillnotesError::Swarm("folder channel peer has no identity key")` — this is a programming error; all folder-channel peers must have a known identity. Write the bundle to `dir/{recipient_short}_{timestamp}_{uuid_short}.swarm`.

`device_short` is no longer used in the filename. The `FolderChannel` struct field and `FolderChannel::new` parameter are kept but the field becomes unused. The callsite in `krillnotes-desktop/src-tauri/src/commands/sync.rs` (`FolderChannel::new(identity_pubkey, device_id)`) requires no change; `device_id` continues to be passed but is now a no-op in the filename. A `#[allow(dead_code)]` or prefixed `_device_short` can suppress the compiler warning.

**Receiver side (`receive_bundles_from_dir`):**
Replace the old sender-prefix filter with an inbox filter:

```
inbox_prefix = "{MY_identity_short}_"
```

Only collect files whose filename **starts with `inbox_prefix`**. Files not matching are silently skipped — this handles both bundles addressed to other peers and old-format files.

There is one transition-period ambiguity: old-format files written by this device begin with the same `{MY_identity_short}_` prefix. To distinguish new-format from old-format, check the segment immediately after the first `_`. In new-format files this is a 14-digit decimal timestamp; in old-format files this is an 8-char device short (alphanumeric, never 14 digits). Concretely:

```rust
// After confirming starts_with(inbox_prefix), extract next segment
let rest = &filename[inbox_prefix.len()..];
let next_segment = rest.split('_').next().unwrap_or("");
if next_segment.len() != 14 || !next_segment.chars().all(|c| c.is_ascii_digit()) {
    // old-format file — skip silently
    continue;
}
```

There is no "skip own files" logic in the new design — since a device never addresses a bundle to itself, its outbound files will never match its own inbox prefix.

**Delete on success:**
A device that successfully applies a bundle deletes it via `acknowledge()`. This is safe: the file was addressed to this device, so no other peer needs it.

**Why this works for shared folders:**
Alice reads only files starting with `ALICE_SHORT_`. Charlie reads only `CHARLIE_SHORT_`. No cross-decryption, no "not for us" errors. Old-format files are silently skipped by the format check above.

### Fix 2 — Detect silent channel config failure

In `PeerRegistry::update_channel_config`, check the row count returned by `conn.execute()`. If 0 rows were affected, return `KrillnotesError::Sync("peer not found: {peer_device_id}")`.

**Rationale for asymmetry:** Other peer-registry updaters (`update_last_sent`, `update_last_received`, `update_sync_status`, `reset_last_sent`) are called from internal sync bookkeeping paths where a missing row is non-fatal and expected during race conditions. `update_channel_config` is called only from a user-facing UI action — a 0-row result there always indicates a stale device ID in the frontend, which must be surfaced so the UI can reload peers and retry.

## Files Changed

| File | Change |
|------|--------|
| `krillnotes-core/src/core/sync/folder.rs` | New filename format in `send_bundle`; inbox-prefix + format-check filter in `receive_bundles_from_dir`; `device_short` field kept but prefixed `_device_short` to suppress dead-code warning |
| `krillnotes-core/src/core/peer_registry.rs` | Check row count in `update_channel_config`, return error on 0 rows |
| `krillnotes-desktop/src-tauri/src/commands/sync.rs` | No change — `FolderChannel::new(identity_pubkey, device_id)` callsite unchanged |

## Non-Changes

- No new error variants needed.
- No subdirectory creation logic.
- No changes to bundle encryption, headers, or the sync engine dispatch loop.
- The `acknowledge` (delete) behaviour on successful apply is unchanged.

## Testing

- **Update** `test_folder_channel_send_creates_file`: verify filename matches `{8chars}_{14digits}_{8chars}.swarm` pattern.
- **Rewrite** `test_folder_channel_receive_filters_own_bundles` → rename to `test_folder_channel_inbox_prefix_filtering`: place files with own identity prefix (new format) and files with another identity prefix — verify only own-inbox files are returned. There is no "skip own files" concept; the test should reflect inbox filtering only.
- **New** `test_folder_channel_ignores_other_recipient_files`: place a new-format file with a different identity prefix, verify it is not returned by `receive_bundles_from_dir`.
- **New** `test_folder_channel_ignores_old_format_files`: place an old-format file (`{MY_identity_short}_{device_short}_{ts}_{uuid}.swarm`) whose prefix matches the local identity, verify the format check skips it.
- **New** `test_update_channel_config_unknown_peer_returns_error`: call `update_channel_config` with a non-existent device ID, verify `Err` is returned.
