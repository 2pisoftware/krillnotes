# SWARM PROTOCOL

## Unified Design Specification

**Version 0.6 — March 2026**

**Status: DRAFT — Design & Security Remediation**

This document unifies three companion documents into a single authoritative specification:

- *KrillNotes Sync & Multi-User Architecture Design v0.5* (February 2026) — the foundational sync protocol
- *Swarm Protocol Security Assessment v1.0* (March 2026) — threat analysis and findings
- *Swarm Server Design Proposal v0.1* (March 2026) — enterprise command infrastructure

It incorporates design decisions made during the security remediation process, including Hybrid Logical Clocks, a revised Operation enum, a gated-function scripting API, and the server-as-peer architecture. Security assessment findings are addressed inline where they affect the design, with a traceability matrix in the appendix.

---

## Table of Contents

1. Design Principles
2. File Formats — The Three Roles of Data
3. Architecture Overview
4. Hybrid Logical Clocks
5. The Operation Model
6. The Gated Function API (Scripting v2)
7. Conflict Resolution Strategy
8. The .swarm File Format
9. Schema Governance & Script Sync
10. File Attachments in Sync
11. Sync Peer Registry & Contacts
12. Invitation & Onboarding Flow
13. Transport Mechanisms
14. Identity Model
15. Trust & Verification
16. RBAC — Role-Based Access Control
17. Permission Enforcement Without a Server
18. Revocation & Edge Cases
19. Encryption Model
20. Replay Protection & Hash Chaining
21. Data Preservation & Conflict Visibility
22. The Swarm Server
23. The Intelligence Loop
24. Compliance & Audit Anchoring
25. User Experience
26. Implementation Roadmap
27. Open Questions
28. Appendix A: Security Finding Traceability

---

## 1. Design Principles

The sync architecture is guided by five core principles:

- **Local-first, always.** Every device is fully functional without any network connection. Sync is an enhancement, never a requirement. The .krillnotes file remains the authoritative source of truth on each device.

- **No infrastructure dependency.** The system does not require a hosted server, a cloud account, or an internet connection to sync. Any mechanism that moves a file from point A to point B is a valid sync transport. The Swarm Server (Section 22) enhances the system without creating a dependency on it.

- **Transport-agnostic.** The sync protocol is defined at the data level (operations and patches), not the network level. USB drives, email attachments, shared cloud folders, SFTP, LAN sockets, LoRa radio, satellite, and relay servers all use the same .swarm bundle format.

- **Cryptographic trust, not institutional trust.** Permissions, identity, and operation authenticity are verified using cryptographic signatures. Any device can independently validate any operation without contacting a central authority.

- **User sovereignty.** Users control their data, their identities, and how data moves between devices.

---

## 2. File Formats — The Three Roles of Data

Krill Notes uses three distinct file formats, each serving a different purpose in the data lifecycle.

**The .krillnotes database** is the local runtime format — a SQLCipher-encrypted SQLite database on the user's device. It is the authoritative local copy. Never shared directly between peers.

**The .krillnotes archive** is the public distribution format — a zip archive containing a JSON workspace snapshot and Rhai scripts. No identity, no access control, no ongoing relationship. Like putting a book on a library shelf.

**The .swarm bundle** is the collaborative sync format — an encrypted zip archive containing signed operations, attachments, and sync metadata. Carries identity, RBAC, and the expectation of ongoing exchange. Like passing a notebook back and forth between trusted colleagues.

|                    | .db (local)           | .krillnotes (public)    | .swarm (collaborative)  |
|--------------------|-----------------------|-------------------------|-------------------------|
| **Purpose**        | Local runtime storage | Public distribution     | Collaborative sync      |
| **Encryption**     | SQLCipher (personal)  | Optional zip password   | Per-recipient public key|
| **Identity**       | None needed           | None                    | Ed25519 signed ops      |
| **Permissions**    | N/A (single user)     | None                    | RBAC enforced per op    |
| **Relationship**   | N/A                   | One-shot                | Continuous peer sync    |
| **Attachments**    | Encrypted .enc sidecar| Included unencrypted    | Included, encrypted     |

---

## 3. Architecture Overview

The sync system builds on Krill Notes' existing operation log. Every mutation to a workspace — creating, editing, moving, or deleting notes and scripts — is recorded as an immutable Operation with a UUID, HLC timestamp, and device ID.

Sync adds three concepts on top of this foundation:

1. **.swarm bundles** — The encrypted sync transport format: a zip archive containing signed operations and optionally encrypted file attachments, transmittable by any means.

2. **Peer registry** — Each workspace tracks known sync peers (other devices/users), recording what operations have been exchanged with each peer.

3. **Cryptographic identity** — Per-workspace keypairs that sign operations and permission grants, enabling local verification of authenticity and authorisation.

### High-Level Data Flow

1. Alice makes local edits. Each edit passes through the gated function API (Section 6), which creates Operation records in the operation log with `synced = 0`.

2. Alice generates a .swarm bundle for Bob. The app queries all operations since the last operation sent to Bob's peer entry, collects any new attachments, encrypts everything for Bob's public key, and writes a .swarm file.

3. Alice transmits the bundle via any channel (shared folder, email, USB drive, LoRa radio, satellite link).

4. Bob receives and opens the .swarm file. His app decrypts, verifies every operation's signature and RBAC permissions, applies conflict resolution using HLC timestamps, and merges into his local database.

5. Bob's HLC clock observes Alice's timestamps, correcting for any clock drift (Section 4).

6. Bob generates a return .swarm for Alice containing his local changes.

7. Both workspaces converge to the same state.

---

## 4. Hybrid Logical Clocks

> **Security Finding SA-001 (Critical): Wall-Clock Timestamp Dependency**
>
> The original design relied on wall-clock timestamps for Last-Writer-Wins conflict resolution. In the target deployment environment (field teams, ruggedised devices, no internet for NTP), clock drift is the baseline condition. A device offline for days could drift by minutes, causing stale operations to silently overwrite more recent entries.
>
> **Resolution:** Hybrid Logical Clocks are a core protocol requirement from Phase 1, for both KrillNotes (open source) and Swarm (commercial). This ensures interoperability between all products in the ecosystem.

### 4.1 The HlcTimestamp Type

Every operation carries an HLC timestamp instead of a wall-clock timestamp. An HLC is a triple that preserves wall-clock readability while providing causal ordering guarantees:

```rust
/// Hybrid Logical Clock timestamp.
/// Total size: 16 bytes (128 bits).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HlcTimestamp {
    /// Wall-clock component: milliseconds since Unix epoch.
    /// Monotonically non-decreasing per node.
    /// Always >= the local wall clock at time of creation.
    pub wall_ms: u64,

    /// Logical counter: disambiguates events within the same
    /// millisecond on the same node, and preserves causal ordering
    /// when wall clocks are equal across nodes.
    /// Resets to 0 when wall_ms advances.
    pub counter: u32,

    /// Node identifier: deterministic tiebreak when wall_ms and
    /// counter are both equal across different nodes.
    /// Derived from device_id (first 4 bytes of BLAKE3 hash).
    pub node_id: u32,
}
```

Ordering is lexicographic: compare `wall_ms` first, then `counter`, then `node_id`. This gives a total ordering over all events in the swarm.

```rust
impl Ord for HlcTimestamp {
    fn cmp(&self, other: &Self) -> Ordering {
        self.wall_ms.cmp(&other.wall_ms)
            .then(self.counter.cmp(&other.counter))
            .then(self.node_id.cmp(&other.node_id))
    }
}
```

### 4.2 Node ID Derivation

The `node_id` is a stable u32 derived from the device UUID via BLAKE3 hash. This avoids duplicating the full device UUID in the HLC while providing sufficient uniqueness for tiebreaking (collision probability ~0.001% at 10,000 devices).

```rust
pub fn node_id_from_device(device_id: &Uuid) -> u32 {
    let hash = blake3::hash(device_id.as_bytes());
    u32::from_le_bytes(hash.as_bytes()[..4].try_into().unwrap())
}
```

### 4.3 Clock State Management

Each workspace maintains a persistent HLC clock state.

**Local event (issuing a timestamp):** Set `wall_ms = max(local_wall_clock, current_hlc.wall_ms)`. If the wall component advanced, reset counter to 0. Otherwise increment counter. Always set `node_id` to this node's identifier.

**Remote event (observing an incoming timestamp):** Set `wall_ms = max(local_wall_clock, current_hlc.wall_ms, remote.wall_ms)` and adjust the counter based on which components tied. This ensures the local clock accounts for everything it has observed.

The `observe()` call happens during .swarm bundle application, for every incoming operation that passes signature and RBAC verification, before applying it to the local database.

### 4.4 Implicit Clock Correction via Sync

The Swarm Server (Section 22), with its NTP-synced clock, acts as a clock correction anchor. When a field device syncs with the server, the HLC `max()` rule causes the field device's HLC to leap forward to match the server's physical time component. This correction propagates transitively: the ICP syncs with the server, forward bases sync with the ICP, strike teams sync with the forward base. Clock truth flows through the sync topology like a wave, one sync cycle at a time.

If a device is completely isolated (no sync at all), its HLC drifts with its local clock. But in that case it isn't conflicting with anyone — and the first sync will correct it.

### 4.5 Forward-Drift Protection (Server-Side)

A device with a wildly fast clock could push the entire swarm's HLC forward via the `max()` rule. For the commercial Swarm product, the server implements a **max-drift monitor**: when an incoming operation's `wall_ms` exceeds the server's NTP-synced clock by more than a configurable threshold (e.g., 5 minutes), the operation is accepted but flagged with a clock anomaly marker. The server's own HLC does not leap forward. An alert is raised for the operator.

This is a monitoring and operational concern, not a protocol-level enforcement. Open-source KrillNotes does not enforce max-drift — it relies on the natural clock correction that occurs through sync.

### 4.6 SQLite Schema

HLC timestamps are stored as three queryable columns:

```sql
-- On the operations table:
timestamp_wall_ms INTEGER NOT NULL,
timestamp_counter INTEGER NOT NULL,
timestamp_node_id INTEGER NOT NULL,

-- HLC clock state (singleton):
CREATE TABLE IF NOT EXISTS hlc_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    wall_ms INTEGER NOT NULL,
    counter INTEGER NOT NULL,
    node_id INTEGER NOT NULL
);
```

Three columns (rather than a packed BLOB) enable direct SQL ordering and range queries: `ORDER BY timestamp_wall_ms, timestamp_counter, timestamp_node_id` and `WHERE timestamp_wall_ms > ?`.

The HLC state is updated atomically within the same transaction as the operation it timestamps, using the existing BEGIN → apply mutation → log_op → COMMIT pattern.

### 4.7 Wire Format

**JSON (.swarm payload):** Compact array representation saves ~30 bytes per operation versus a named-object encoding:

```json
{
    "timestamp": [1709550720000, 0, 2918374621]
}
```

**Compact binary (LoRa / constrained transports):** Delta-encode `wall_ms` against a bundle-level epoch. In a single-author LoRa priority bundle, the node_id is sent once in the header, and each operation carries only the delta (u32, 4 bytes) and counter (u16, 2 bytes) — **6 bytes total**, less than the original 8-byte wall-clock timestamp.

### 4.8 Migration

Existing .krillnotes databases with wall-clock timestamps migrate cleanly: `wall_ms = existing_timestamp`, `counter = 0`, `node_id = derive_from_device_id()`. Ordering is preserved.

---

## 5. The Operation Model

Every mutation to a workspace is represented as an immutable, signed Operation record. The Operation enum is the wire format for sync and the source of truth for the audit trail.

### 5.1 Design Change: Operations as First-Class Emitters

> The original design recorded operations by diffing the note state before and after a save. This created ambiguity between user-initiated and hook-derived changes, and required snapshotting the full note state before every hook invocation.
>
> **New approach:** All mutations — whether from user edits, `on_save` hooks, `on_add_child` hooks, or tree actions — flow through gated functions (Section 6) that explicitly create Operation records. The operation log is complete by construction, not by reconstruction.

### 5.2 The Operation Enum

```rust
pub enum Operation {
    // === Note lifecycle ===
    CreateNote {
        operation_id: Uuid,
        timestamp: HlcTimestamp,
        device_id: Uuid,
        note_id: Uuid,
        parent_id: Uuid,
        position: f64,
        node_type: String,
        title: String,
        fields: Value,          // initial field values from schema defaults
        created_by: IdentityPublicKey,
        signature: Signature,
    },

    /// Note-level mutable properties: title, and future properties
    /// such as pinned, archived, colour.
    UpdateNote {
        operation_id: Uuid,
        timestamp: HlcTimestamp,
        device_id: Uuid,
        note_id: Uuid,
        title: Option<String>,
        // Future note-level properties:
        // pinned: Option<bool>,
        // archived: Option<bool>,
        modified_by: IdentityPublicKey,
        signature: Signature,
    },

    /// Schema-defined field changes (one field per operation).
    UpdateField {
        operation_id: Uuid,
        timestamp: HlcTimestamp,
        device_id: Uuid,
        note_id: Uuid,
        field: String,          // field name from schema definition
        value: Value,           // new value (typed per schema)
        modified_by: IdentityPublicKey,
        signature: Signature,
    },

    DeleteNote {
        operation_id: Uuid,
        timestamp: HlcTimestamp,
        device_id: Uuid,
        note_id: Uuid,
        deleted_by: IdentityPublicKey,
        signature: Signature,
    },

    MoveNote {
        operation_id: Uuid,
        timestamp: HlcTimestamp,
        device_id: Uuid,
        note_id: Uuid,
        new_parent_id: Uuid,
        new_position: f64,
        moved_by: IdentityPublicKey,
        signature: Signature,
    },

    // === Tags ===
    /// Full replacement of a note's tag set. LWW on the entire vector.
    SetTags {
        operation_id: Uuid,
        timestamp: HlcTimestamp,
        device_id: Uuid,
        note_id: Uuid,
        tags: Vec<String>,
        modified_by: IdentityPublicKey,
        signature: Signature,
    },

    // === Attachments (note-level and field-level) ===
    AddAttachment {
        operation_id: Uuid,
        timestamp: HlcTimestamp,
        device_id: Uuid,
        attachment_id: Uuid,    // matches sidecar filename on disk
        note_id: Uuid,          // parent note
        filename: String,       // original filename (e.g., "photo.jpg")
        mime_type: String,      // e.g., "image/jpeg"
        size_bytes: u64,
        hash_sha256: String,    // hash of original unencrypted content
        created_by: IdentityPublicKey,
        signature: Signature,
    },

    RemoveAttachment {
        operation_id: Uuid,
        timestamp: HlcTimestamp,
        device_id: Uuid,
        attachment_id: Uuid,
        note_id: Uuid,
        removed_by: IdentityPublicKey,
        signature: Signature,
    },

    // === Schema governance ===
    CreateUserScript {
        operation_id: Uuid,
        timestamp: HlcTimestamp,
        device_id: Uuid,
        script_id: Uuid,
        name: String,
        description: String,
        source: String,         // full Rhai source code
        enabled: bool,
        load_order: i32,
        created_by: IdentityPublicKey,  // must be root owner
        signature: Signature,           // must be root owner's
    },

    UpdateUserScript {
        operation_id: Uuid,
        timestamp: HlcTimestamp,
        device_id: Uuid,
        script_id: Uuid,
        name: String,
        description: String,
        source: String,
        enabled: bool,
        load_order: i32,
        modified_by: IdentityPublicKey, // must be root owner
        signature: Signature,           // must be root owner's
    },

    DeleteUserScript {
        operation_id: Uuid,
        timestamp: HlcTimestamp,
        device_id: Uuid,
        script_id: Uuid,
        deleted_by: IdentityPublicKey,  // must be root owner
        signature: Signature,           // must be root owner's
    },

    // === RBAC ===
    SetPermission {
        operation_id: Uuid,
        timestamp: HlcTimestamp,
        device_id: Uuid,
        target_user_id: IdentityPublicKey,
        note_id: Option<Uuid>,  // None = workspace-level
        role: Role,             // Owner, Writer, Reader
        granted_by: IdentityPublicKey,
        signature: Signature,
    },

    RevokePermission {
        operation_id: Uuid,
        timestamp: HlcTimestamp,
        device_id: Uuid,
        target_user_id: IdentityPublicKey,
        note_id: Option<Uuid>,
        revoked_by: IdentityPublicKey,
        signature: Signature,
    },

    // === Peer management ===
    JoinWorkspace {
        operation_id: Uuid,
        timestamp: HlcTimestamp,
        device_id: Uuid,
        identity: IdentityPublicKey,
        display_name: String,
        pairing_token: [u8; 32],
        signature: Signature,
    },
}
```

### 5.3 Common Fields

Every operation variant carries:

| Field | Type | Purpose |
|---|---|---|
| `operation_id` | UUID | Stable unique identifier. Used for deduplication and idempotency. |
| `timestamp` | HlcTimestamp | Causal ordering. Used for LWW conflict resolution. |
| `device_id` | UUID | Originating device. Used for peer tracking and diagnostics. |
| `*_by` | IdentityPublicKey | Author identity. Used for RBAC verification and attribution. |
| `signature` | Signature | Ed25519 signature over the operation's content. Verified on ingest. |

### 5.4 Field-Level Attachments

File attachments can be associated with a note as a whole (note-level) or with a specific field via the `"file"` field type. Both use the same sidecar storage and the same `AddAttachment`/`RemoveAttachment` operations. The field-level association is established through an `UpdateField` operation that sets the file field's value to the `attachment_id`:

```
Attaching a photo to a "site_photo" field:
1. AddAttachment { note_id, attachment_id: "abc", filename: "fire_front.jpg", ... }
2. UpdateField  { note_id, field: "site_photo", value: "abc" }
```

This mirrors how `note_link` fields store a UUID reference to another note. The attachment is an independent entity; the field is a pointer.

### 5.5 Re-Derived Side Effects

Certain state changes are deterministic consequences of an operation, not independent operations themselves. These are computed locally by every peer upon applying the triggering operation, bypassing the operation queue:

| Trigger | Side Effect | Re-derive Logic |
|---|---|---|
| `DeleteNote` | `note_link` fields referencing the deleted note are set to null | Scan all note_link fields for matching UUID |
| `RemoveAttachment` | `file` fields referencing the removed attachment are set to null | Scan all file fields for matching UUID |
| `DeleteNote` | Attachments on the deleted note are removed (cascade) | ON DELETE CASCADE in database |

Re-derived side effects write directly to the database during .swarm application. They do **not** produce operations and do **not** flow through the gated function API. Every honest peer arrives at the same result because the logic is deterministic and the triggering operation is identical.

### 5.6 Purge Strategies

| Strategy | Behaviour | Use Case |
|---|---|---|
| `LocalOnly { keep_last: N }` | Keep the N most recent operations; delete older ones. Default: 1000. | Single-device, no sync. |
| `WithSync { retention_days: D }` | Keep unsynced operations indefinitely; delete synced ones older than D days. | Active sync. |

> **Security Finding SA-006 (Medium): Operation Log Purge / Delta Fallback Gap**
>
> When a peer purges operations that another peer hasn't synced yet, delta generation fails. **Resolution:** Implement explicit delta-not-possible detection in bundle generation. When the earliest unpurged operation is newer than the peer's last-synced marker, automatically fall back to snapshot mode with user-visible signalling. Log the fallback for operational visibility.

---

## 6. The Gated Function API (Scripting v2)

> This is a breaking change from the v0.5 scripting API. The previous model allowed hooks to mutate the note map directly (`note.title = ...`, `note.fields["x"] = ...`) and relied on diffing the before/after state to reconstruct operations. The new model makes every mutation an explicit operation-emitting function call.

### 6.1 Motivation

The gated function model ensures:

- **Every mutation has a first-class operation from the moment it happens.** No diffing, no reconstruction, no ambiguity.
- **User-initiated and hook-derived changes use the same mutation path.** The frontend save handler and the `on_save` hook both call the same gated functions.
- **The operation log is complete by construction.** If it didn't go through a gated function, it didn't happen.
- **Operations can carry origin metadata** (User vs. Hook) for smarter conflict resolution.

### 6.2 The API

All gated functions take an explicit `note_id` as the first parameter. This works identically in `on_save`, `on_add_child`, tree actions, and any future hook context.

| Function | Operation Emitted | Description |
|---|---|---|
| `set_field(note_id, field_name, value)` | `UpdateField` | Set a schema-defined field value |
| `set_title(note_id, title)` | `UpdateNote` | Set the note's title |
| `set_tags(note_id, tags)` | `SetTags` | Replace the note's tag set |
| `create_note(parent_id, node_type)` | `CreateNote` | Create a child note; returns a read-only note handle |
| `delete_note(note_id)` | `DeleteNote` | Delete a note |
| `move_note(note_id, new_parent_id)` | `MoveNote` | Move a note to a new parent |
| `add_attachment(note_id, ...)` | `AddAttachment` | Attach a file to a note |
| `remove_attachment(attachment_id)` | `RemoveAttachment` | Remove a file attachment |
| `commit()` | — | Apply all queued operations atomically in one transaction |

### 6.3 Hook Examples

**`on_save` — Contact title derivation:**

```rhai
on_save: |note| {
    let last = note.fields["last_name"];
    let first = note.fields["first_name"];
    if last != "" || first != "" {
        set_title(note.id, last + ", " + first);
    }
    commit()
}
```

**`on_add_child` — Parent counter update:**

```rhai
on_add_child: |parent, child| {
    let count = (parent.fields["item_count"] ?? 0.0) + 1.0;
    set_field(parent.id, "item_count", count);
    set_title(parent.id, "Projects (" + count.to_int().to_string() + ")");
    commit()
}
```

**Tree action — Create sub-task:**

```rhai
add_tree_action("Create Sub-task", ["Project"], |note| {
    let child = create_note(note.id, "Task");
    set_field(child.id, "status", "TODO");
    set_title(child.id, "New Task");
    commit()
});
```

### 6.4 Frontend Save Flow

The frontend participates in the same gated function model via Tauri commands:

1. User changes fields in the edit form.
2. User hits Save.
3. Frontend sends a delta of changed fields as gated function calls: `set_field(note_id, "first_name", "Jane")`, etc.
4. Backend queues these as `UpdateField` operations.
5. Backend runs the `on_save` hook, which may queue additional operations (`UpdateNote` for derived title, etc.).
6. Backend calls `commit()` — all queued operations are applied in a single SQLite transaction with sequential HLC timestamps.

### 6.5 Hook Execution in Sync Context

Hooks fire **only on the originating peer**. When a remote peer applies incoming operations from a .swarm bundle, it applies the operations directly — it does not re-run hooks. This ensures:

- Hook side effects are explicit operations that propagate through sync.
- Different peers don't produce divergent hook outputs due to data timing differences.
- The operation log fully captures what happened, including hook-derived changes.

---

## 7. Conflict Resolution Strategy

All conflict resolution uses HLC timestamps (Section 4) for ordering. The `operation_id` UUID serves as a final tiebreaker if HLC timestamps are exactly equal (theoretically possible but practically never occurs).

### 7.1 Summary Table

| Data Type | Strategy | Behaviour on Conflict |
|---|---|---|
| Atomic fields (text, number, date, select, boolean, rating, email, note_link, file) | Field-level LWW | Later HLC timestamp wins per individual field |
| Textarea fields | Text CRDT (Phase 7) or loud LWW | CRDT merge when available; flag conflict and notify user otherwise |
| Note-level properties (title) | Property-level LWW | Later HLC timestamp wins per property within `UpdateNote` |
| Tags | Set-level LWW | Later HLC `SetTags` wins; entire tag set replaced |
| Tree moves (MoveNote) | Detect & surface | LWW applied as working state; user notified to review |
| Delete vs. edit | Configurable per schema | Default: delete wins. AIIMS schemas: soft-delete (see Section 21) |
| Permission grants | Append-only + LWW | All grants applied; conflicting roles resolved by latest HLC |
| User scripts (schemas) | Root owner only | Conflicts prevented by restricting authorship to root owner identity |
| File attachments | Atomic / no conflict | Each attachment has a unique UUID; add and delete are idempotent |

### 7.2 Field-Level LWW

For atomic field types, each `UpdateField` operation is compared independently per `(note_id, field)` pair. If device A updates field X and device B updates field Y on the same note, both changes are preserved — they are separate operations targeting different fields.

If both devices update the same field, the operation with the later HLC timestamp wins. The losing operation is preserved in the operation log for audit purposes but does not affect the working state.

### 7.3 Tree Conflict Detection

If two devices move the same note to different parents, the system applies LWW as the working state but flags the conflict for user review. Cycle detection is performed before applying any tree move — a move that would create a cycle is rejected.

### 7.4 Textarea Conflict Handling

> **Security Finding SA-009 (Medium): Deferred Text CRDT Creates Migration Risk**
>
> LWW for textarea fields means one person's entire update is silently dropped during concurrent editing. In AIIMS incident management, concurrent editing of situation reports is a common pattern.
>
> **Resolution (interim):** Until text CRDT integration (Phase 7), implement loud conflict detection for textarea LWW. When concurrent textarea edits are detected on the same field, flag the conflict and present both versions to the user for manual resolution. The working state uses LWW, but the losing version is preserved and the user is notified. This prevents silent data loss while deferring the CRDT complexity.

---

## 8. The .swarm File Format

A .swarm file is the universal transport format for all sync-related communication. It is a zip archive whose internal structure varies based on its mode.

### 8.1 Modes

| Mode | Purpose | Content |
|---|---|---|
| `invite` | Invite a new peer to a workspace | Workspace metadata, permission grant, pairing token. Signed but not encrypted. |
| `accept` | Accept an invitation, binding an identity | Recipient's public key, pairing token reference. Signed by the accepting identity. |
| `snapshot` | Initial workspace share with a known peer | Full workspace state as JSON, encrypted attachments. Encrypted for recipient. |
| `delta` | Ongoing incremental sync | Signed operations since last exchange, encrypted attachments. Encrypted for recipient(s). |

### 8.2 Common Header (Unencrypted)

Every .swarm file contains an unencrypted `header.json`:

| Field | Description |
|---|---|
| `format_version` | Swarm format version for forward compatibility |
| `mode` | One of: invite, accept, snapshot, delta |
| `workspace_id` | Identifies which workspace this bundle belongs to |
| `workspace_name` | Human-readable workspace name (for display before decryption) |
| `source_device_id` | Device ID that generated this bundle |
| `source_identity` | Public key of the identity that generated the bundle |
| `source_display_name` | Human-readable name of the sender |
| `created_at` | ISO 8601 timestamp of bundle creation |

Additional header fields vary by mode (see v0.5 design document for full specification of invite, accept, snapshot, and delta mode headers).

### 8.3 HLC Timestamps in Bundles

Operations within .swarm bundles carry HLC timestamps in compact array format: `[wall_ms, counter, node_id]`. For the commercial product's compact binary encoding (MessagePack/CBOR), delta-encoding against a bundle-level epoch reduces per-operation overhead to 6 bytes for single-author bundles.

---

## 9. Schema Governance & Script Sync

Note types are defined by Rhai scripts that register schemas with the Schema Registry. Schema changes affect every note of that type on every peer's device, making them fundamentally different from note edits.

### 9.1 The Root Owner Rule

Only the root owner of a workspace may create, modify, enable, disable, reorder, or delete user scripts. This is enforced at three levels: application (Script Manager UI is read-only for non-owners), operation (script operations must be signed by root owner key), and sync (script operations from non-root identities are rejected during .swarm application).

> **Security Finding SA-004 (Critical): Root Owner Single Point of Failure**
>
> The root owner identity is bound to a single Ed25519 keypair. If the root owner's device is destroyed or the person rotates off shift, workspace schemas are frozen permanently.
>
> **Resolution:** The Swarm Server (Section 22) holds the root owner keypair in an HSM. For open-source KrillNotes, multi-signature ownership or a documented break-glass procedure for transferring root authority should be considered as a future enhancement.

### 9.2 The System Script Exception

The built-in TextNote system script is embedded in the application binary. It is always present, always loaded first, and cannot be modified or deleted by any user. This ensures every workspace has at least one functional note type.

### 9.3 Schema Change Propagation

Schema changes propagate via `CreateUserScript`, `UpdateUserScript`, and `DeleteUserScript` operations in .swarm bundles. On receipt, the peer verifies the root owner signature, applies the script change, and reloads the Schema Registry. Existing notes with dormant fields (fields removed by the schema change) preserve their data — the values remain in the database but are hidden from the UI until a compatible schema is restored.

---

## 10. File Attachments in Sync

Notes can have file attachments at two levels:

- **Note-level attachments** — files associated with the note as a whole, managed through the attachment panel.
- **Field-level attachments** — files associated with a specific `"file"` field, where the field value stores the `attachment_id` (UUID reference to the sidecar).

Both use identical sidecar storage (individually encrypted `.enc` files alongside the database) and the same `AddAttachment`/`RemoveAttachment` operations. The field-level association is established through an `UpdateField` operation on the file field.

### 10.1 Attachment Encryption in Transit

Attachments undergo a decrypt/re-encrypt cycle:

1. Sender decrypts from local at-rest storage (ChaCha20-Poly1305, keyed from workspace password).
2. Sender re-encrypts for transit using the per-recipient X25519 + AES-256-GCM scheme.
3. Recipient decrypts from transit encryption using their private identity key.
4. Recipient re-encrypts for local at-rest storage using their own workspace password.

At no point is key material from one peer's local encryption transmitted to another peer.

### 10.2 Attachment Conflict Resolution

Attachments are atomic — there is no meaningful way to merge two file versions:

- Two users attach different files to the same note: no conflict (each has a unique UUID).
- Two users delete the same attachment: no conflict (idempotent).
- Two users attach different files to the same `"file"` field: the `UpdateField` operations resolve via LWW — one attachment_id wins. Both sidecar files exist; the losing reference becomes an orphan for cleanup.
- A note is deleted while it has attachments: cascade removes attachment metadata; orphan cleanup removes the .enc files.

---

## 11. Sync Peer Registry & Contacts

*(Unchanged from v0.5 design. See companion document for full specification of the per-workspace peer registry and cross-workspace contacts address book.)*

---

## 12. Invitation & Onboarding Flow

*(Unchanged from v0.5 design. See companion document for full specification of invite, accept, and onboarding flows for both known contacts and new peers.)*

> **Security Finding SA-005 (Medium): Snapshot Onboarding Leaks Full Workspace**
>
> A snapshot-mode .swarm bundle contains the full workspace state. A new sector commander who should only see Sector 3 data receives everything.
>
> **Resolution:** Implement subtree-scoped snapshots for the commercial product. The snapshot generator evaluates RBAC permissions before including content, producing a snapshot containing only the notes within the recipient's permitted subtrees. This is a planned feature for the Swarm product, promoted from open question to roadmap item.

---

## 13. Transport Mechanisms

*(Transport mechanism catalogue unchanged from v0.5 design.)*

> **Security Finding SA-008 (Low): LoRa Transport Throughput Claims**
>
> At realistic LoRa data rates (SF12, 300 bps), a 10KB bundle takes over 4 minutes before overhead.
>
> **Resolution:** Reframe LoRa as critical-priority-only: team status updates, hazard alerts, evacuation orders (~500 bytes each). Full workspace deltas require higher-bandwidth channels. The priority queuing system is the primary LoRa operating mode, not an optimisation.

---

## 14. Identity Model

*(Unchanged from v0.5 design. Per-workspace cryptographic identities, Ed25519 keypairs, no global account, no linkability between identities across workspaces unless the user chooses.)*

---

## 15. Trust & Verification

*(Unchanged from v0.5 design. Key fingerprints via BIP-39 word rendering, QR code verification, vouching mechanism, three trust levels.)*

---

## 16. RBAC — Role-Based Access Control

*(Role definitions and permission inheritance model unchanged from v0.5 design.)*

---

## 17. Permission Enforcement Without a Server

*(Unchanged from v0.5 design. Every peer independently validates every operation's signature and RBAC permissions during .swarm application. The security boundary is the receiver's client, not the sender's.)*

### Modified Client Threat Model

The security assessment analysed four sub-threats:

| Threat | Risk | Mitigation |
|---|---|---|
| **A:** Modified client ignores RBAC locally | Contained to their device | Cannot be prevented; physical access reality |
| **B:** Modified client generates unauthorised operations | Primary threat | Every receiving peer validates signature + RBAC on ingest; unauthorised ops rejected |
| **C:** Two colluding modified clients | Contained between them | Cannot infect honest peers; honest peer perimeter is the security boundary |
| **D:** Authorised liar (valid access, false data) | Human process problem | Non-repudiation: every entry permanently signed with author's identity key |

---

## 18. Revocation & Edge Cases

> **Security Finding SA-002 (High): Revocation Propagation Gap**
>
> During the propagation window (hours over LoRa/sneakernet), peers continue accepting operations from revoked users. The original design specified retroactive rollback, which creates accountability problems — data that informed decisions disappears.
>
> **Resolution (proposed):** Implement a quarantine model for the commercial product. See Section 21 for the data preservation design.

---

## 19. Encryption Model

*(Three-layer encryption model unchanged from v0.5 design: SQLCipher at rest, per-recipient transport encryption via X25519 + AES-256-GCM, Ed25519 operation signatures.)*

> **Security Finding SA-007 (Medium): Per-Recipient Encryption Scaling**
>
> In a 40-device incident with watched-folder sync, generating N-1 separate encrypted bundles per device is impractical.
>
> **Resolution:** The server-as-relay-hub architecture (Section 22) reduces the topology from N² to 2N bundles. For P2P without a server, multi-recipient key wrapping (single encrypted payload, per-recipient AES key wrappers) should be the default for shared folder transports.

---

## 20. Replay Protection & Hash Chaining

> **Security Finding SA-010 (High): No Replay Attack or Bundle Freshness Protection**
>
> The design relies on operation UUID idempotency but does not address deliberate replay attacks. A captured .swarm containing a RevokePermission could be replayed after re-grant.

### 20.1 Proposed Mitigations (Design In Progress)

**Per-identity operation sequence counters:** Each operation from an identity includes a monotonically increasing sequence number. Gaps or overlaps signal fabrication or replay. The Swarm Server maintains authoritative counters for all identities.

**Per-identity hash chaining:** Each operation includes the BLAKE3 hash of the previous operation from that identity, creating a tamper-evident chain. A modified client fabricating operations would break the chain, detectable on ingest by any peer.

**Bundle manifest signatures:** Each .swarm bundle includes a signed hash of its complete contents, making bundles atomic — operations cannot be extracted, added, or reordered without breaking the signature.

**Trade-offs for constrained transports:** Hash chains add ~32 bytes per operation. On LoRa priority payloads, sequence counters alone (4 bytes) may be sufficient, with full hash chains on higher-bandwidth transports.

> *Status: Design in progress. Sequence counters and hash chaining interact with the HLC and server architecture and require detailed specification before implementation.*

---

## 21. Data Preservation & Conflict Visibility

> **Security Findings SA-002 (High) and SA-003 (High)** both concern silent data loss in life-safety operations: revocation rollback removes data that informed decisions, and delete-wins discards concurrent edits.

### 21.1 Proposed: Quarantine Model for Revocations (SA-002)

Instead of retroactive rollback, operations from revoked users that post-date the revocation are flagged as **contested** rather than removed. The UI marks them visually, the provenance chain is preserved for post-incident review, and the data that informed real-time decisions remains visible.

Three operation states: **valid**, **rejected** (failed signature or RBAC — never applied), **contested** (applied, then retroactively invalidated by a revocation — preserved but flagged).

> *Status: Proposed. Detailed interaction with conflict resolution and audit trail requires specification.*

### 21.2 Proposed: Configurable Conflict Policies (SA-003)

The delete-vs-edit behaviour becomes configurable per schema, aligning with the "the schema is the application" philosophy. An AIIMS Situation Report schema can declare `on_delete_conflict: preserve` while a personal recipe schema keeps `on_delete_conflict: delete_wins`.

This could be implemented as a declarative property in the Rhai schema definition:

```rhai
schema("SituationReport", #{
    on_delete_conflict: "preserve",   // soft-delete: edit wins, delete becomes pending
    fields: [ /* ... */ ],
});
```

> *Status: Proposed. Mechanism for schema-level conflict policy declaration requires specification.*

---

## 22. The Swarm Server

The Swarm Server is a headless krillnotes-core peer — an instance of the core library running without a user interface, holding the root owner identity for one or more workspaces, and continuously processing .swarm bundles across multiple transport channels.

From the protocol's perspective, the server is indistinguishable from any other peer. It generates and consumes .swarm bundles using the same format, applies the same signature verification and RBAC enforcement, and participates in the same conflict resolution. The difference is operational: the server is highly available, institutionally managed, backed by HSM key storage, and connected to integration services.

> *Design principle: The Swarm Server enhances the decentralised protocol without creating a dependency on it. If the server goes offline, every field device continues operating exactly as designed. When it returns, it catches up like any peer rejoining after an absence.*

### 22.1 Architecture Layers

| Layer | Technology | Responsibility |
|---|---|---|
| **Core Engine** | krillnotes-core (Rust) | Workspace management, operation log, .swarm bundle generation/application, signature verification, RBAC, conflict resolution. Unmodified from the open-source library. |
| **Server Shell** | Rust binary (no UI) | Headless process wrapping krillnotes-core. Multiple workspaces. Root owner identities. Merge-and-fan-out loop. Management API. |
| **Transport Layer** | Pluggable adapters | HTTPS endpoint, SFTP drop folder, watched directory, LoRa gateway, satellite modem. Each adapter converts its transport into .swarm bundles. |
| **Integration Layer** | REST/gRPC API | Feeds operations to knowledge graph, GIS, and external systems. Accepts derived intelligence and converts it into signed operations. |

### 22.2 Root Authority Model

When an incident workspace is created on the server, the server generates the root owner Ed25519 keypair and stores it in an HSM or managed key vault. The private key never leaves the secure enclave.

Human administrators interact with schema governance through a management interface that requests signatures from the HSM. The audit trail records which human authorised each schema change, while the cryptographic signature is always the server's root key.

Because the root owner identity is institutional rather than personal, shift rotations have no impact on workspace governance. The keypair is backed up using standard HSM procedures (split-key ceremony, offline backup tokens, geographically distributed key shares).

### 22.3 Relay Hub Function

The server converts the sync topology from mesh to hub-and-spoke. Each field device syncs with the server (one bundle); the server fans out to all peers. For N devices, this produces 2N bundles per cycle instead of N×(N-1) in a full mesh.

The server uses multi-recipient key wrapping for shared transports: a random AES-256 key encrypts the payload once, wrapped individually per recipient's public key. The server also implements transport-aware routing — HTTPS push for internet-connected peers, priority-filtered LoRa for radio-connected peers, queued USB bundles for intermittent peers.

### 22.4 Operational Monitoring

The server monitors: sync freshness per peer (alert when silent), clock drift detection (compare incoming HLC wall_ms against NTP-synced server clock), rejected operation patterns (signal compromised or modified client), operation volume anomalies, and transport channel health.

### 22.5 Multi-Incident Architecture

Each incident runs in an isolated workspace with its own root identity, RBAC rules, schemas, and peer registry. The server manages multiple workspaces concurrently. Cross-incident analysis is enabled through the integration layer feeding multiple workspaces into a shared knowledge graph.

### 22.6 Graceful Degradation

| Failure | Impact | Recovery |
|---|---|---|
| Server goes offline | P2P sync continues. Intelligence loop stops. | Server syncs with any peer on return. |
| Server destroyed | Root key at risk if not backed up. | Provision replacement with HSM-backed key. |
| Internet link lost | LoRa and local sync unaffected. | Server queues bundles for reconnection. |
| LoRa gateway fails | Radio-dependent peers lose server sync. | USB sneakernet. Protocol unchanged. |
| Knowledge graph fails | Intelligence loop stops. Core sync unaffected. | Server queues operations for backlog processing. |

In every scenario, the system degrades from "AI-augmented situational awareness" to "structured manual data sharing" — still vastly superior to voice radio and paper.

---

## 23. The Intelligence Loop

The server's integration layer creates a closed-loop intelligence cycle between the field edge and analytical engines at the command post:

1. Field operator creates a structured observation (e.g., spot fire report with coordinates).
2. Observation travels via .swarm bundle to the Swarm Server.
3. Server feeds the operation to the partner knowledge graph / GIS engine.
4. Reasoning engine produces derived outputs (fire spread prediction, risk assessment).
5. Integration layer converts outputs into new Swarm operations signed by a server analytical identity.
6. Server distributes derived intelligence to field peers via normal .swarm bundles.
7. Field operator's device displays the analysis through the same interface.

Every link carries full provenance metadata. The analytical identity has scoped RBAC: permitted to create analysis notes and recommendations, prohibited from creating directives/orders or modifying field observations.

---

## 24. Compliance & Audit Anchoring

The server exports the complete operation log as a hash-chained, signed archive. Each operation's hash includes the hash of the previous operation, creating a tamper-evident chain. The archive is signed with the server's root key and timestamped.

The archive contains: every operation from every peer with original signatures, every sync exchange record, every RBAC event, every rejected operation with rejection reason, every intelligence cycle output, and the complete schema history.

This is the primary evidence package for coronial inquiries, royal commissions, agency reviews, and insurance assessments.

---

## 25. User Experience

*(UI specification unchanged from v0.5 design: Sync menu, auto-sync via watched folder, identity selector, peer management, conflict resolution UI. The gated function API change (Section 6) does not affect end-user experience — it is an internal implementation change to the scripting engine.)*

---

## 26. Implementation Roadmap

### Core Protocol Phases

| Phase | Delivers | Dependencies | Security Findings Addressed |
|---|---|---|---|
| **1 — Swarm Infrastructure** | Manual sync (delta + snapshot), HLC timestamps, gated function API, revised Operation enum | None (builds on existing operation log) | SA-001 (HLC), SA-006 (delta fallback), SA-009 (loud textarea LWW) |
| **2 — Cryptographic Identity** | Signed operations, identities, invite/accept handshake | Phase 1 | — |
| **3 — RBAC & Schema Governance** | Multi-user access control, root-owner script authority | Phase 2 | SA-002 (quarantine), SA-003 (configurable delete policy) |
| **4 — Trust & Verification** | Fingerprints, QR verification, vouching | Phase 2 | — |
| **5 — Automated Transport** | Watched-folder sync, LAN auto-discovery | Phase 1 | — |
| **6 — Attachment Sync** | File attachments (note-level and field-level) in .swarm bundles | Phase 1 | — |
| **7 — Text CRDT** | Concurrent textarea editing via Yrs | Phase 1 | SA-009 (full resolution) |
| **8 — Replay Protection** | Sequence counters, hash chaining, bundle manifests | Phase 2 | SA-010 |

### Server Phases

| Phase | Delivers | Dependencies |
|---|---|---|
| **S1** | Headless shell, single workspace, file system transport | Core Phase 1 |
| **S2** | Multi-workspace, HTTPS transport, root identity with HSM, management API | Core Phases 2–3 |
| **S3** | LoRa gateway adapter, priority-aware routing, monitoring dashboard | LoRa hardware prototype |
| **S4** | Integration layer: knowledge graph adapter, inbound intelligence pipeline | Partner integration prototype |
| **S5** | Compliance export, hash-chained audit archive, multi-incident coordination | S2 + agency requirements |

### Phase Dependencies

Core Phases 4–8 and Server Phases can proceed in parallel after Core Phase 3 is complete. Server Phase S1 can run in parallel with Core Phases 1–3.

---

## 27. Open Questions

- **Schema migration hooks:** Should Rhai scripts support an `on_migrate` hook for automatic field mapping during schema changes?

- **Workspace forking:** Should the system support a formal "fork" operation creating an independent workspace from a shared one?

- **Selective sync (SA-005):** Subtree-scoped snapshots are planned for commercial Swarm. Should selective ongoing delta sync (peer only receives operations for their RBAC subtree) also be supported?

- **CRDT selection:** Yrs (Rust port of Yjs) is the current candidate for textarea CRDT. Should alternatives (Automerge, diamond-types) be evaluated?

- **Mobile / web client:** Should krillnotes-core be compiled to WASM for browser use?

- **Quarantine interaction with conflict resolution (SA-002):** Does a quarantined operation participate in conflict resolution, or is it excluded from LWW comparison?

- **Hash chain granularity (SA-010):** Should sequence counters use a lighter format on LoRa while full hash chains are used on higher-bandwidth transports? Or should the protocol mandate one approach universally?

- **Gated function error handling:** What happens if a gated function call fails mid-sequence before `commit()`? Should the entire queue be discarded, or should partial application be possible?

- **Hook-derived vs. user-initiated conflict precedence:** Should operations emitted by hooks carry an `origin: Hook` marker that gives user-initiated changes priority in LWW conflicts?

---

## Appendix A: Security Finding Traceability

| ID | Finding | Severity | Resolution | Section | Status |
|---|---|---|---|---|---|
| **SA-001** | Wall-clock timestamp dependency (LWW) | **Critical** | HLC timestamps in core protocol from Phase 1 | §4 | **Resolved** |
| **SA-002** | Revocation propagation gap | **High** | Quarantine model (contested state) proposed | §18, §21.1 | Design in progress |
| **SA-003** | Delete-wins conflict policy | **High** | Configurable per-schema conflict policy proposed | §21.2 | Design in progress |
| **SA-004** | Root owner single point of failure | **Critical** | Server-side root authority with HSM | §22.2 | **Resolved** (server design complete) |
| **SA-005** | Snapshot onboarding leaks full workspace | **Medium** | Subtree-scoped snapshots planned | §12 | Planned |
| **SA-006** | Operation log purge / delta fallback gap | **Medium** | Explicit delta-not-possible detection with snapshot fallback | §5.6 | Specified |
| **SA-007** | Per-recipient encryption scaling | **Medium** | Server relay hub (2N topology) + multi-recipient key wrapping | §19, §22.3 | **Resolved** |
| **SA-008** | LoRa transport throughput claims | **Low** | Reframed as critical-priority-only | §13 | **Resolved** |
| **SA-009** | Deferred text CRDT creates migration risk | **Medium** | Loud conflict detection for textarea LWW (interim); CRDT Phase 7 | §7.4 | Interim mitigation specified |
| **SA-010** | No replay attack or bundle freshness protection | **High** | Sequence counters + hash chaining + bundle manifests proposed | §20 | Design in progress |

---

*End of Unified Design Specification*

*Swarm Protocol — Unified Design v0.6*
