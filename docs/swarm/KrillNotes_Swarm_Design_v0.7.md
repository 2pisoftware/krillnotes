# SWARM PROTOCOL

## Unified Design Specification

**Version 0.7 — March 2026**

**Status: DRAFT — Design & Security Remediation**

This document is the single authoritative specification for the Swarm sync protocol. It merges and supersedes:

- *KrillNotes Sync & Multi-User Architecture Design v0.5* (February 2026) — foundational sync protocol
- *Swarm Protocol Security Assessment v1.0* (March 2026) — threat analysis and findings
- *Swarm Server Design Proposal v0.1* (March 2026) — enterprise command infrastructure
- *Identity Model Detail v1.0* (March 2026) — passphrase unlock, multi-device, OS keychain
- *The .cloud Broadcast Format Design* (March 2026) — signed broadcast extension to the Swarm protocol

Security assessment findings are addressed inline where they affect the design. A traceability matrix is in Appendix A.

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
8b. The .cloud Broadcast Format
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

- **Local-first, always.** Every device is fully functional without any network connection. Sync is an enhancement, never a requirement. The local database remains the authoritative source of truth on each device.

- **No infrastructure dependency.** The system does not require a hosted server, a cloud account, or an internet connection to sync. Any mechanism that moves a file from point A to point B is a valid sync transport. The Swarm Server (Section 22) enhances the system without creating a dependency on it.

- **Transport-agnostic.** The sync protocol is defined at the data level (operations and patches), not the network level. USB drives, email attachments, shared cloud folders, SFTP, LAN sockets, LoRa radio, satellite, and relay servers all use the same .swarm bundle format.

- **Cryptographic trust, not institutional trust.** Permissions, identity, and operation authenticity are verified using cryptographic signatures. Any device can independently validate any operation without contacting a central authority.

- **User sovereignty.** Users control their data, their identities, and how data moves between devices. There is no single point of control, no vendor lock-in on the sync layer, and no mandatory account.

---

## 2. File Formats — The Three Roles of Data

Krill Notes uses three distinct file formats, each serving a fundamentally different purpose. Understanding this distinction is central to the architecture.

**The local database (.db)** is the SQLCipher-encrypted SQLite database on the user's device. It is the authoritative local copy, encrypted with the user's personal password, and is never transmitted to anyone. It is not a transport format — it is a local runtime artifact. Each peer maintains their own independent database with their own encryption password.

**The open archive (.krillnotes)** is the public distribution format — a zip archive containing a workspace snapshot (notes, fields, tags, scripts) as structured JSON data. No identity, no access control, no ongoing relationship. Like putting a book on a library shelf.

**The broadcast bundle (.cloud)** is the signed broadcast format — a cleartext-signed zip archive containing operations and attachments. The sender has a cryptographic identity; the receiver is anonymous. Authenticity and tamper-evidence without confidentiality. Like a signed bulletin board: trusted, readable by anyone, but not theirs to republish.

**The sync bundle (.swarm)** is the collaborative sync format — an encrypted zip archive containing signed operations, attachments, and sync metadata. Both sender and receiver have cryptographic identity. Full confidentiality, RBAC, and the expectation of ongoing bidirectional exchange. Like passing a notebook between trusted colleagues.

|                    | .db (local)              | .krillnotes (public)     | .cloud (broadcast)          | .swarm (collaborative)      |
|--------------------|--------------------------|--------------------------|-----------------------------|-----------------------------|
| **Purpose**        | Local runtime storage    | Public distribution      | Signed broadcast            | Collaborative sync          |
| **Audience**       | Device owner only        | Anyone — public/casual   | Anyone who trusts sender    | Known, identified peers     |
| **Encryption**     | SQLCipher (personal)     | Optional zip password    | None (cleartext)            | Per-recipient public key    |
| **Sender identity**| None needed              | None                     | Ed25519 signed              | Ed25519 signed              |
| **Receiver identity**| None needed            | None                     | None (anonymous)            | Ed25519 per peer            |
| **Permissions**    | N/A (single user)        | None                     | RBAC; default: reader       | RBAC enforced per operation |
| **Relationship**   | N/A                      | One-shot                 | Subscribe (feed)            | Continuous peer sync        |
| **Attachments**    | Encrypted .enc sidecar   | Included unencrypted     | Included unencrypted, signed| Included, encrypted         |
| **Export**         | N/A                      | Unrestricted             | Root owner only             | Root owner only             |

The trust progression is: no identity → sender identity only → mutual identity. Each step adds a layer of cryptographic trust. Encryption is only present when the sender knows the receiver — you cannot encrypt for someone you do not know, and a shared symmetric key creates an illusion of confidentiality without actually providing it. The cleartext-signed .cloud model is honest about what a broadcast actually is: publishing with provenance.

**A .krillnotes file is a broadcast.** Like putting a book on a library shelf — the author shares openly, the recipient takes what they want, and neither party has any obligation. No identity, no access control, no expectation of ongoing exchange.

**A .cloud file is a signed bulletin board.** Like a notice pinned with a verifiable stamp — read it, trust the authorship, but it is not yours to republish. Anyone can read it; only the owner can update it.

**A .swarm file is a conversation.** Like passing a notebook between trusted colleagues. Each contribution is signed, each exchange builds on the last, and both parties maintain a shared understanding of who said what and who can do what.

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

The Swarm Server (Section 22), with its NTP-synced clock, acts as a clock correction anchor. When a field device syncs with the server, the HLC `max()` rule causes the field device's HLC to leap forward to match the server's physical time component. This correction propagates transitively through the sync topology — clock truth flows like a wave, one sync cycle at a time.

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

Certain state changes are deterministic consequences of an operation, not independent operations themselves. These are computed locally by every peer upon applying the triggering operation:

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
> When a peer purges operations that another peer hasn't synced yet, delta generation fails. **Resolution:** Implement explicit delta-not-possible detection in bundle generation. When the earliest unpurged operation is newer than the peer's last-synced marker, automatically fall back to snapshot mode with user-visible signalling.

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

### 6.6 Schema Change Impact on Existing Data

When a schema change arrives via sync, existing note data is handled gracefully following a principle of data preservation — schema changes never destroy data silently.

- **New field added:** Existing notes of the affected type gain the new field with a null or default value.
- **Field deleted from schema:** The field's data remains in the database, stored in the note's JSON fields column, but is no longer displayed in the UI. The data can be recovered by restoring the field definition.
- **Field renamed:** A rename is a deletion of the old field and an addition of a new one. The old field's data goes dormant (preserved but hidden), and the new field starts empty. The root owner should communicate schema changes to peers before applying them.
- **Field type changed:** The old data is preserved as-is; the new type applies going forward. Incompatible existing values display gracefully (raw value or type mismatch indicator) rather than crashing.
- **Hook changes (on_save, on_view):** `on_save` changes take effect the next time a note of that type is saved. `on_view` changes take effect immediately on the next view of any affected note.

### 6.7 The System Script Exception

The built-in TextNote system script (`text_note.rhai`) is embedded in the application binary via `include_dir!` and is not subject to the root owner rule. It is always present, always loaded first, and cannot be modified or deleted by any user. If a future version ships an updated system script, the update takes effect on all devices when they upgrade — this is a software update, not a sync event.

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

A .swarm file is the universal transport format for all sync-related communication. It is a zip archive whose internal structure varies based on its mode. Every sync interaction — inviting a peer, accepting an invitation, sharing a full workspace, or exchanging incremental updates — uses this single format.

### 8.1 Modes

| Mode | Purpose | Content |
|---|---|---|
| `invite` | Invite a new peer to a workspace | Workspace metadata, permission grant, pairing token. Signed but not encrypted. |
| `accept` | Accept an invitation, binding an identity | Recipient's public key, pairing token reference. Signed by the accepting identity. |
| `snapshot` | Initial workspace share with a known peer | Full workspace state as JSON, encrypted attachments. Encrypted for recipient. |
| `delta` | Ongoing incremental sync | Signed operations since last exchange, encrypted attachments. Encrypted for recipient(s). |

This unified approach means the app only needs to handle one file type. Double-clicking a .swarm file always opens Krill Notes, which reads the mode and takes the appropriate action.

### 8.2 Common Header (Unencrypted)

Every .swarm file contains an unencrypted `header.json` that allows the app to identify the file's purpose and workspace without attempting decryption:

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

Additional header fields vary by mode, as described in sections 8.3–8.6.

### 8.3 Invite Mode

An invite .swarm is signed by the inviter but not encrypted, because the recipient's public key is not yet known. It contains no workspace data — only the metadata needed for the recipient to decide whether to accept and to bind their identity.

**Additional header fields:** `pairing_token` (random 256-bit token), `offered_role` (owner/writer/reader), `offered_scope` (note_id of the subtree root, or null for entire workspace), `inviter_fingerprint` (human-readable key fingerprint).

**Payload:** The signed `SetPermission` operation granting the (as yet unknown) recipient the specified role. The `target_user_id` is set to a placeholder that will be resolved when the acceptance binds a real identity.

**Security note:** Because invite .swarm files are not encrypted, they should not be treated as sensitive. They reveal the workspace name, the inviter's identity, and the offered role — but no workspace content. Anyone who intercepts an invitation cannot use it without also intercepting the subsequent acceptance and snapshot exchange.

### 8.4 Accept Mode

An accept .swarm is the recipient's response to an invitation. It binds their chosen identity to the invitation's pairing token.

**Additional header fields:** `pairing_token` (matching the invite), `accepted_identity` (the recipient's public key), `accepted_display_name`, `accepted_fingerprint`.

**Payload:** A signed `JoinWorkspace` operation containing the recipient's public key and the pairing token. Signed by the recipient's private key, proving they control the identity they're binding.

### 8.5 Snapshot Mode

A snapshot .swarm contains the complete current state of the workspace, encrypted for a specific recipient. It is used for initial workspace sharing after the invite/accept handshake is complete, or when a peer needs a full resync.

**Additional header fields:** `as_of_operation_id` (the last operation reflected in the snapshot), `recipients[]` (per-recipient encrypted symmetric key), `has_attachments` (boolean).

**Encrypted payload:** `workspace.json` containing the full resolved workspace state (notes with all fields, tags, tree structure, user scripts, resolved permission state, known users with public keys). This is the current state — not a replay of operations. No conflict resolution is needed on import.

**Attachments:** All attachment files, encrypted per-recipient, in the `attachments/` directory with a manifest for integrity verification.

The recipient creates a fresh local database, imports the snapshot directly, and sets their peer tracking to `as_of_operation_id`. From this point forward, all sync is via delta mode.

### 8.6 Delta Mode

A delta .swarm contains incremental operations since the last exchange with a specific peer. This is the steady-state sync format used for all ongoing synchronisation.

**Additional header fields:** `since_operation_id` (last op known to the target peer), `target_peer` (device ID of intended recipient, optional for broadcast), `recipients[]` (per-recipient encrypted symmetric key), `has_attachments` (boolean).

**Encrypted payload:** `operations[]` (signed Operation objects in chronological order), `manifest` (list of included attachments with UUIDs, SHA-256 hashes, sizes), `patch_signature` (signature over the entire payload by the source identity).

**Attachments:** Only attachments referenced by `AddAttachment` operations in this delta, encrypted per-recipient.

On application, each operation is individually verified (signature, RBAC), applied using the appropriate conflict resolution strategy, and recorded in the local log with `synced = 1`.

### 8.7 HLC Timestamps in Bundles

Operations within .swarm bundles carry HLC timestamps in compact array format: `[wall_ms, counter, node_id]`. For the commercial product's compact binary encoding (MessagePack/CBOR), delta-encoding against a bundle-level epoch reduces per-operation overhead to 6 bytes for single-author bundles.

---

## 8b. The .cloud Broadcast Format

The .cloud format fills the gap between unsigned public distribution (.krillnotes) and encrypted mutual collaboration (.swarm). It is designed for one-to-many broadcasting where the sender has a cryptographic identity but the audience is anonymous and open.

### 8b.1 Motivation — The Missing Middle

Many operational scenarios require a trusted sender broadcasting to an unknown audience. The sender has identity and the content is authentic, but the audience is open. Use cases include:

- **Emergency services situation broadcasts.** An incident management team publishes operational updates that any authorised agency can consume without individual onboarding.
- **Operational intelligence feeds.** A Swarm Server publishes derived analysis (weather overlays, hazard maps, fleet status) that field teams subscribe to without the server needing to enumerate every possible receiver.
- **Knowledge base distribution.** An organisation publishes SOPs, field guides, or reference data with cryptographic proof of authorship and tamper-evidence — properties that a .krillnotes archive lacks.
- **Community schema sharing.** Schema authors publish Rhai script packages as signed broadcasts, allowing consumers to verify they are running authentic, unmodified schemas.

The naming follows the Krill ecosystem metaphor: a krill cloud is a visible aggregation near the surface, observable by anyone without joining the swarm. You can see it and verify it, but you are not part of it.

### 8b.2 Modes

The .cloud format supports only two modes — there is no invitation or acceptance flow because there is no bilateral peer relationship:

| Mode | Purpose | Content |
|---|---|---|
| `snapshot` | Initial broadcast of full workspace state | Complete workspace as JSON, scripts, attachments. All operations signed by sender. |
| `delta` | Incremental broadcast update | Signed operations since last publication. Sequential feed model. |

### 8b.3 Header (Unencrypted)

Every .cloud file contains an unencrypted `header.json`:

| Field | Description |
|---|---|
| `format_version` | Cloud format version for forward compatibility |
| `mode` | One of: snapshot, delta |
| `workspace_id` | Identifies which workspace this broadcast belongs to |
| `workspace_name` | Human-readable workspace name |
| `source_identity` | Public key of the broadcasting identity (root owner) |
| `source_display_name` | Human-readable name of the sender |
| `source_device_id` | Device ID that generated this bundle |
| `created_at` | ISO 8601 timestamp of bundle creation |
| `sequence_number` | Monotonically increasing broadcast sequence. Enables gap detection by receivers. |
| `key_fingerprint` | BIP-39 word rendering of the sender's public key, for quick visual verification |

### 8b.4 Payload (Cleartext, Signed)

The payload contains operations in the same format as a .swarm bundle, but stored as cleartext JSON (or compact binary in the commercial product) rather than encrypted. Every operation retains its Ed25519 signature from the originating identity. Authenticity and integrity are preserved; confidentiality is intentionally absent.

Attachments, if present, are included as unencrypted files within the zip archive. Like .krillnotes archives, attachment content is accessible to anyone with the file. Unlike .krillnotes archives, every attachment is referenced by a signed `AddAttachment` operation, providing tamper-evidence.

### 8b.5 Bundle Signature

In addition to per-operation signatures, each .cloud bundle includes a bundle-level signature: the sender signs a manifest containing the BLAKE3 hash of every file in the zip archive. This makes the bundle atomic — files cannot be added, removed, or reordered without invalidating the bundle signature. This extends the replay protection mechanism described in Section 20.

### 8b.6 Receiver Verification Flow

Because the sender does not know the receiver, the standard Swarm invitation and fingerprint verification flow does not apply. Instead, the receiver obtains the sender's public key fingerprint through an independent trusted channel:

- **Website publication.** The sender publishes their public key fingerprint (BIP-39 word rendering and/or hex) on their website, alongside the .cloud download or feed URL.
- **QR code at events.** A conference poster or printed handout includes both the .cloud feed location and a QR code encoding the sender's public key fingerprint.
- **Trusted introduction.** A known contact vouches for the sender's key fingerprint, analogous to the existing vouching mechanism but without establishing a bilateral sync relationship.
- **DNS or well-known URI.** For organisations, the public key fingerprint could be published as a DNS TXT record or at a `.well-known` URI, enabling automated verification.

On first import, the client prompts the user to confirm the sender's key fingerprint against their trusted source. Once confirmed, subsequent .cloud bundles from the same workspace and identity are accepted automatically, with the client verifying every operation's signature against the stored public key.

### 8b.7 RBAC Model for Broadcast Workspaces

**Default permissions.** When a .cloud workspace is imported, the receiver's client creates a local workspace where: the workspace root default role is **reader** (or **none**, if the sender has configured selective visibility); the sender's identity holds the **owner** role on the workspace root; the receiver holds no explicit permission entry — they inherit the workspace default.

The sender can use the standard RBAC permission inheritance to control which subtrees are visible. For example, a workspace might expose a public briefing subtree (default: reader) while keeping operational planning notes restricted (explicit: none for default). Receivers see only the subtrees their effective role permits.

**Two-tier workspaces.** A workspace can simultaneously serve anonymous broadcast readers and named collaborative peers. The sender publishes .cloud bundles for the anonymous audience and exchanges .swarm bundles with trusted collaborators in the same workspace. The RBAC tree handles both:

- *Anonymous receivers* inherit the workspace default (reader on public subtrees, none on restricted subtrees). They receive .cloud bundles only.
- *Named collaborators* have explicit permission entries (writer, owner on their assigned subtrees). They exchange .swarm bundles with per-recipient encryption and can contribute content.

This enables scenarios like an incident management team collaborating via .swarm while publishing situation reports to a broader audience via .cloud — all from the same workspace, with a single RBAC tree governing visibility and modification rights.

**Local annotations.** Receivers may add local notes to a .cloud-derived workspace for personal annotation. These notes exist only in the receiver's local database — they are not signed by the broadcast sender and do not propagate. The client distinguishes local annotations from broadcast content visually, and local annotations do not affect the workspace's signed state.

### 8b.8 Broadcast Sync Model

The .cloud sync model is fundamentally one-way: the sender publishes, receivers consume. There is no peer registry on the sender's side for anonymous receivers — the sender does not know they exist.

**Publication.** The sender generates sequential .cloud bundles, each carrying a monotonically increasing sequence number. The first publication is a snapshot mode bundle. Subsequent publications are delta mode bundles. Publication is transport-agnostic, using the same mechanisms as .swarm distribution: a shared folder, a web server, an email list, or any file distribution channel.

**Consumption.** The receiver maintains a local peer marker for the broadcast sender's identity, tracking the last consumed sequence number. On encountering a new .cloud bundle, the receiver:

1. Verifies the bundle-level signature against the sender's stored public key.
2. Checks the sequence number for gaps (missing bundles trigger a warning; the receiver may request a fresh snapshot).
3. Verifies every operation's individual Ed25519 signature.
4. Applies operations to the local workspace, enforcing RBAC (all operations must come from an identity with appropriate permissions in the signed permission tree).
5. Updates the local peer marker to the new sequence number.

**Interaction with the Swarm Server.** The Swarm Server is a natural publication point for .cloud broadcasts. In its relay hub role, the server already holds root owner identity and generates .swarm bundles for known peers. Adding .cloud generation is a straightforward extension: the server produces broadcast bundles alongside per-recipient encrypted bundles, using the same operation log as the source. See Section 22.7.

### 8b.9 Export Restrictions

A workspace containing cryptographically signed operations can only be exported by the identity that holds the signed root ownership. This restriction applies uniformly to all export paths (.krillnotes archive, .cloud broadcast, or .swarm bundle) and is enforced through signature verification, not metadata flags.

**Enforcement mechanism.** When the client attempts any export operation:

1. Walk the permission tree to find the root ownership grant (the `SetPermission` operation on the workspace root with `role: owner`).
2. Verify that this operation is signed by the current active identity's keypair.
3. If the signature matches, export proceeds. If not, export is denied.

This check is unforgeable without the owner's private key. Tampering with the root ownership operation invalidates its signature.

**Scope of the rule:**

- *Unsigned workspace (.krillnotes import):* No prior cryptographic claim exists. The importer becomes the owner. Export is unrestricted.
- *Signed workspace (.cloud or .swarm import):* Root ownership is cryptographically established. Only the identity holding the root owner's private key can export.

**Subtree delegation.** Subtree owners cannot export the workspace. Workspace-level export requires root ownership specifically. This prevents a delegated contributor from repackaging the entire workspace.

**Root-owner-only privileges (complete list):** Export (any format), script governance (create/modify/delete Rhai scripts, per Section 9), root RBAC changes (modify permission grants on the workspace root node).

### 8b.10 Redistribution Threat Model

The .cloud format's cleartext nature means content is visible to any receiver. The primary threat is unauthorised redistribution.

**Wholesale redistribution.** If a receiver redistributes the original .cloud bundle or its operations without modification, the original Ed25519 signatures remain intact. Any third party who knows the original sender's public key can verify that the content came from the original author. The redistributor cannot strip or re-sign operations without the original author's private key. Provenance is cryptographically preserved.

**Content laundering.** A technically sophisticated attacker could import .cloud content into their own workspace and republish under their own identity. The following measures make this detectable and discourageable:

- *Client-enforced export restriction.* The honest client blocks all export paths for workspaces containing signed operations where the current identity is not the root owner. This prevents the vast majority of non-technical users from redistributing content. Circumventing this requires deliberate client modification (Threat B from the Security Assessment).
- *Origin provenance metadata.* Operations imported from a .cloud source carry an optional `origin` field recording the original operation UUID and author public key. An honest client populates this automatically during import.
- *Temporal priority.* The original .cloud bundles carry HLC timestamps signed by the original author, establishing a verifiable temporal record of first publication. A re-publisher's operations will necessarily carry later timestamps.
- *Content hash attestation.* The sender can publish a content manifest (BLAKE3 hashes of note content at each publication point) alongside their public key fingerprint. Third parties can compare any suspected copy against the original manifest to prove derivation.

**Threat assessment.** The export restriction raises the bar from trivial to requiring deliberate client modification. Cryptographic signatures prove original authorship. Timestamps prove priority. Content hashing proves derivation. If the content requires protection against any form of redistribution, it should not be broadcast to unknown receivers — use .swarm with known, vetted peers instead.

### 8b.11 Client Import Flow

When a user opens a .cloud file for the first time:

1. Parse `header.json` and display the workspace name, sender display name, and key fingerprint.
2. Prompt the user to verify the sender's key fingerprint against a trusted source. Display the BIP-39 word rendering for easy comparison.
3. On confirmation, create a local workspace with `source_type: cloud` in the workspace metadata.
4. Store the sender's public key and verified fingerprint in the local peer registry.
5. Verify the bundle-level signature.
6. Verify and apply all operations, enforcing the signed RBAC tree.
7. Display the workspace in a visually distinct broadcast mode, showing the sender's identity and the receiver's effective role (reader).

Subsequent .cloud bundles from the same workspace are applied automatically without re-prompting for fingerprint verification, provided the sender identity matches.

The UI for a broadcast workspace clearly indicates that content is externally authored. Every note displays its original author attribution. The workspace toolbar shows that export is unavailable and explains why: *"This workspace was received as a signed broadcast. Export requires root ownership."*

---

## 9. Schema Governance & Script Sync

Note types are defined by Rhai scripts that register schemas with the Schema Registry. Each schema defines the fields, types, flags, and hooks (`on_save`, `on_view`) for a note type. A single script change can alter the structure and behaviour of every note of that type across the entire workspace. For this reason, schema governance requires special treatment in the sync design.

### 9.1 The Root Owner Rule

Only the root owner of a workspace may create, modify, enable, disable, reorder, or delete user scripts. This is enforced at three levels:

- **Application level:** The Script Manager UI is fully interactive for the root owner and read-only for all other users. Non-root-owners can view scripts and their source code but cannot make changes.
- **Operation level:** `CreateUserScript`, `UpdateUserScript`, and `DeleteUserScript` operations are only valid when signed by the root owner's identity key.
- **Sync level:** During .swarm application, script operations signed by any identity other than the root owner are rejected.

The rationale: schema changes have workspace-wide consequences, cannot be meaningfully merged if two people make conflicting changes, and require careful consideration of data migration. Centralising this authority with the root owner eliminates schema conflicts entirely and ensures that all peers converge on identical type definitions.

> **Security Finding SA-004 (Critical): Root Owner Single Point of Failure**
>
> The root owner identity is bound to a single Ed25519 keypair. If the root owner's device is destroyed or the person rotates off shift, workspace schemas are frozen permanently.
>
> **Resolution:** The Swarm Server (Section 22) holds the root owner keypair in an HSM. For open-source KrillNotes, multi-signature ownership or a documented break-glass procedure for transferring root authority should be considered as a future enhancement.

### 9.2 How Script Changes Propagate

**Snapshot mode:** The full current state of all user scripts (source code, enabled/disabled status, load order) is included in the `workspace.json` payload. The recipient receives the current schema definitions directly.

**Delta mode:** Script changes arrive as individual operations (`CreateUserScript`, `UpdateUserScript`, `DeleteUserScript`) and are applied in chronological order. After applying script operations, the receiving app performs a full registry reload: the Schema Registry and Hook Registry are cleared, system scripts are re-evaluated, and all enabled user scripts are re-evaluated in load order.

### 9.3 Script Visibility for Non-Owners

Although non-owners cannot modify scripts, they benefit from full read access. The Script Manager shows all scripts with their names, descriptions, and source code in a read-only view. This transparency lets team members understand why their notes behave the way they do.

### 9.4 The System Script Exception

The built-in TextNote system script is embedded in the application binary. It is always present, always loaded first, and cannot be modified or deleted by any user. This ensures every workspace has at least one functional note type regardless of the state of user scripts.

---

## 10. File Attachments in Sync

Notes can have file attachments at two levels:

- **Note-level attachments** — files associated with the note as a whole, managed through the attachment panel.
- **Field-level attachments** — files associated with a specific `"file"` field, where the field value stores the `attachment_id` (UUID reference to the sidecar).

Both use identical sidecar storage (individually encrypted `.enc` files alongside the database) and the same `AddAttachment`/`RemoveAttachment` operations.

### 10.1 Attachment Encryption in Transit

Attachments undergo a decrypt/re-encrypt cycle at each end of a sync exchange:

1. Sender decrypts the attachment from local at-rest storage (ChaCha20-Poly1305, keyed from workspace password).
2. Sender re-encrypts for transit using the per-recipient X25519 + AES-256-GCM scheme used for the payload.
3. Recipient decrypts from transit encryption using their private identity key.
4. Recipient re-encrypts for local at-rest storage using their own workspace password via ChaCha20-Poly1305.

At no point is key material from one peer's local encryption transmitted to another peer.

### 10.2 Attachment Conflict Resolution

Attachments are atomic — there is no meaningful way to merge two file versions:

- Two users attach different files to the same note: no conflict (each has a unique UUID).
- Two users delete the same attachment: no conflict (idempotent).
- Two users attach different files to the same `"file"` field: the `UpdateField` operations resolve via LWW — one `attachment_id` wins. Both sidecar files exist; the losing reference becomes an orphan for cleanup.
- A note is deleted while it has attachments: cascade removes attachment metadata; orphan cleanup removes the `.enc` files.
- An `AddAttachment` operation targets a note that has been deleted: the operation is discarded during .swarm application.

### 10.3 Considerations for Large Workspaces

Workspaces with many or large attachments present practical challenges for sync bundle size:

- **RBAC-aware bundling:** Only include attachments for notes within the recipient peer's permitted subtrees.
- **Selective attachment sync:** Include `AddAttachment` metadata in every .swarm bundle, but include actual file blobs only for notes the recipient has recently accessed or explicitly requested. Missing attachments can be fetched in a subsequent sync exchange.
- **Chunked encryption:** For files over a configurable threshold (e.g., 10MB), use streaming ChaCha20-Poly1305 encryption/decryption to keep memory usage bounded.
- **Thumbnail generation:** For image attachments, generate and sync an encrypted thumbnail for quick preview, with the full file fetched on demand.

---

## 11. Sync Peer Registry & Contacts

Krill Notes maintains two related but distinct data stores for managing relationships with other users: a per-workspace peer registry for sync state, and a cross-workspace contacts address book for known identities.

### 11.1 Contacts (Cross-Workspace Address Book)

The contacts store lives outside any workspace, in the app's local configuration directory. It is a personal address book of every identity the user has ever interacted with across all workspaces.

```
~/.config/krillnotes/
    identities/          # YOUR identities (keys you own)
    ├─ <identity-uuid-1>.json    # e.g., work identity
    └─ <identity-uuid-2>.json    # e.g., personal identity
    contacts/            # OTHER people's identities
    ├─ <contact-uuid-1>.json     # e.g., Bob's work identity
    ├─ <contact-uuid-2>.json     # e.g., Carol's identity
    └─ <contact-uuid-3>.json     # e.g., Bob's personal identity
    settings.json                # workspace registry (see Section 14.4)
```

Each contact file contains:

| Field | Description |
|---|---|
| `contact_id` | UUID of this contact record |
| `display_name` | Human-readable name (e.g., "Bob", "Carol (Freelance)") |
| `public_key` | The contact's Ed25519 public key |
| `fingerprint` | Human-readable key fingerprint (e.g., ocean-maple-thunder-seven) |
| `trust_level` | `verified_in_person` \| `code_verified` \| `vouched` \| `tofu` |
| `vouched_by` | Contact ID of the vouching peer (if trust_level is `vouched`) |
| `first_seen` | ISO 8601 timestamp of first interaction |
| `notes` | Optional free-text notes (e.g., "Bob from accounting") |

The contacts list is populated automatically when invite/accept handshakes complete. A single real-world person may appear as multiple contacts if they use different identities in different contexts — "Bob (Work)" and "Bob (Personal)" as separate entries, preserving the privacy boundary between workspace contexts.

### 11.2 Per-Workspace Peer Registry

Each workspace maintains a local peer registry that tracks sync state with other devices. This registry is stored in the workspace database and is device-local state (not synced).

```sql
sync_peers (
    peer_device_id   TEXT PRIMARY KEY,
    peer_name        TEXT,   -- human-readable label
    peer_identity_id TEXT,   -- references a contact's public key
    last_sent_op     TEXT,   -- last operation_id sent to peer
    last_received_op TEXT,   -- last operation_id received from peer
    last_sync        TEXT    -- ISO timestamp of last exchange
)
```

### 11.3 Peer Lifecycle

- **Known peer invited to new workspace:** The user selects a contact from their address book. No invite/accept handshake is needed — the app already has the contact's public key. A snapshot .swarm is generated immediately, encrypted for the contact's key.
- **New peer via invite/accept:** After the handshake completes, the new peer's identity is added to the contacts address book and the workspace's peer registry simultaneously.
- **Ongoing sync:** After each .swarm exchange, the `last_sent_op` and `last_received_op` markers are updated, along with the `last_sync` timestamp.
- **Stale peers:** Peers that haven't synced for a configurable period can be flagged or removed from the workspace peer registry. Their contact record in the address book is unaffected.

---

## 12. Invitation & Onboarding Flow

There are two scenarios for adding a peer to a workspace, depending on whether the inviter already knows the peer's identity.

> **Security Finding SA-005 (Medium): Snapshot Onboarding Leaks Full Workspace**
>
> A snapshot-mode .swarm bundle contains the full workspace state. A new sector commander who should only see Sector 3 data receives everything.
>
> **Resolution:** Implement subtree-scoped snapshots for the commercial product. The snapshot generator evaluates RBAC permissions before including content, producing a snapshot containing only the notes within the recipient's permitted subtrees.

### 12.1 Scenario 1: Inviting a Known Contact

The inviter already has the peer's public key in their contacts address book (from a previous workspace interaction).

1. The inviter opens the workspace, navigates to Sync → Invite Peer, and is presented with their contacts list.
2. They select the contact (e.g., "Bob (Work)") and assign a role and scope.
3. The app creates a `SetPermission` operation (signed by the inviter), generates a snapshot .swarm encrypted for Bob's known public key, and saves or sends it.
4. Bob opens the .swarm file. His app recognises his own key in the recipients list, decrypts the snapshot, creates a local database, and imports everything.

One exchange. No handshake required.

### 12.2 Scenario 2: Inviting a New (Unknown) Peer

The inviter does not yet have the peer's public key. A two-step handshake is required.

**Step 1: Create and Send Invitation**

The inviter opens Sync → Invite New Peer and fills in a minimal form (display name, role, scope). The app generates an invite-mode .swarm file containing the workspace name, the inviter's public key and fingerprint, the offered role and scope, a signed `SetPermission` operation (with a placeholder target), and a random pairing token. This file is signed but not encrypted.

The inviter sends the .swarm file to the new peer via any channel.

**Step 2: Recipient Accepts**

The new peer opens the invite .swarm in Krill Notes. Their app displays:

```
Workspace Invitation
─────────────────────────
From: Carsten (ocean-maple-thunder-seven)
Workspace: Project Alpha Notes
Your role: Writer on /Project Alpha
Trust: ⚠ Unverified — verify fingerprint if possible

Use identity:
○ Carol (Work)
○ Carol (Personal)
○ Create new identity...

[ Accept ]  [ Decline ]
```

The new peer selects (or creates) an identity and clicks Accept. Their app generates an accept-mode .swarm file containing their public key, the pairing token, and a signed `JoinWorkspace` operation. They send this back to the inviter via any channel.

**Step 3: Inviter Completes Onboarding**

The inviter opens the accept .swarm. The app:

1. Verifies the pairing token matches the original invitation.
2. Registers the new peer's public key in the contacts address book.
3. Resolves the placeholder in the `SetPermission` operation with the new peer's actual identity.
4. Adds the new peer to the workspace's peer registry.
5. Generates a snapshot .swarm encrypted for the new peer's public key.

The inviter sends this snapshot .swarm to the new peer. They open it, their app decrypts and imports the workspace, and the sync relationship is fully established. All future exchanges use delta-mode .swarm files.

### 12.3 Flow Summary

| Scenario | Steps | Exchanges |
|---|---|---|
| Known contact (in address book) | Select contact → send snapshot .swarm | 1 file (inviter → recipient) |
| New peer (unknown identity) | Send invite .swarm → receive accept .swarm → send snapshot .swarm | 3 files (inviter → recipient → inviter → recipient) |

### 12.4 The Colleague Edge Case

A common real-world scenario: you share work workspaces with a colleague using their work identity, and also share a personal workspace where they prefer to use a different identity. From your perspective, these appear as two separate contacts — "Bob (Work)" and "Bob (Personal)" with different public keys. You invite whichever identity is appropriate for each workspace. Cryptographically the identities are distinct and unlinkable. This is the per-workspace identity model working as intended.

---

## 13. Transport Mechanisms

The .swarm file is the universal sync primitive. Any mechanism that can move a file from one device to another is a valid sync transport. Krill Notes does not privilege any transport over another.

> **Security Finding SA-008 (Low): LoRa Transport Throughput Claims**
>
> At realistic LoRa data rates (SF12, 300 bps), a 10KB bundle takes over 4 minutes before overhead.
>
> **Resolution:** Reframe LoRa as critical-priority-only: team status updates, hazard alerts, evacuation orders (~500 bytes each). Full workspace deltas require higher-bandwidth channels. The priority queuing system is the primary LoRa operating mode, not an optimisation.

### 13.1 Manual Transport

| Transport | How It Works |
|---|---|
| USB drive / external media | User exports a .swarm file, copies it to removable media, and hands it to the recipient. |
| Email attachment | Swarm bundle is attached to an email. Recipient opens it with Krill Notes. |
| File sharing (AirDrop, etc.) | Swarm bundle sent via OS-level file sharing. Recipient's Krill Notes opens it. |
| QR code (small deltas) | For very small delta bundles with no attachments, a QR code could encode the bundle data directly. |

### 13.2 Automated Transport

| Transport | How It Works |
|---|---|
| Shared folder (Dropbox, GDrive, S3, NAS) | Krill Notes watches a designated folder. Swarm bundles are written to and read from the folder automatically. Multiple peers can use the same folder as a hub. |
| SFTP / SCP | Scheduled or on-demand push/pull of .swarm files to a remote directory. |
| LAN auto-discovery | Devices on the same network discover each other via mDNS and exchange .swarm bundles directly over a local socket. |
| LoRa radio | Priority-filtered critical-only payloads (~500 bytes) over radio mesh. Compact binary encoding (Section 4.7). |
| Satellite modem | Same .swarm format; transport adapter handles the link-layer specifics. |
| Swarm Server relay | Server accepts bundles from all peers and fans out to others (Section 22). |

The shared folder model is particularly powerful because it turns any cloud storage service into a sync backend without any Krill Notes-specific infrastructure. A Dropbox folder, a Google Drive directory, an S3 bucket, or a NAS share all work identically.

### 13.3 Broadcast-Specific Transport (.cloud)

The transport catalogue above applies equally to .cloud bundles. In addition, the broadcast model naturally supports publication patterns not applicable to bilateral .swarm sync:

| Transport | How It Works |
|---|---|
| Web server (HTTP/HTTPS) | The sender publishes .cloud bundles at a stable URL. Receivers download or subscribe via HTTP. The sequence number in the header enables receivers to poll for new bundles efficiently. |
| Feed manifest | A standard feed manifest (analogous to RSS/Atom) lists available bundles, their sequence numbers, and BLAKE3 checksums, enabling automated consumption without relying on filesystem watching. |
| Email list / mailing list | Sequential .cloud delta bundles distributed as email attachments to a subscriber list. Recipients apply bundles in sequence number order. |

For broadcast distribution, the Swarm Server is the natural publication point: it generates .cloud bundles from the same operation log it uses for .swarm, serving them via HTTPS alongside its relay function (see Section 22.7).

---

## 14. Identity Model

Krill Notes uses per-workspace cryptographic identities. There is no global account, no central identity provider, and no linkability between a user's identities across different workspaces unless the user explicitly chooses to reveal the connection.

### 14.1 Core Principle: Multiple Independent Identities

A single user may maintain multiple completely independent identities — for example, a work persona, a personal persona, and a club or community persona. Each identity is a self-contained Ed25519 keypair stored in its own encrypted identity file. There is no technical mechanism to correlate identities, preserving privacy across contexts.

Examples:
- `"Carsten @ 2pi"` — company workspace identity
- `"Carsten K"` — personal workspace identity
- `"Treasurer, Canberra RC"` — club workspace identity

Each identity has its own keypair and its own passphrase. Identities are selected at app launch; the user works within one active identity at a time.

### 14.2 Identity File Structure

Each identity is stored as an encrypted JSON file in the app's local configuration directory:

```
~/.config/krillnotes/
    identities/
        <uuid-work>.json          # "Carsten @ 2pi"
        <uuid-personal>.json      # "Carsten K"
        <uuid-club>.json          # "Treasurer, Canberra RC"
    settings.json                 # workspace registry
```

Each identity file:

```json
{
  "identity_uuid": "...",
  "display_name": "Carsten @ 2pi",
  "public_key": "<base64 Ed25519 pubkey>",
  "private_key_enc": {
    "ciphertext": "<base64 AES-256-GCM ciphertext>",
    "nonce": "<base64>",
    "kdf": "argon2id",
    "kdf_params": {
      "salt": "<base64>",
      "m_cost": 65536,
      "t_cost": 3,
      "p_cost": 1
    }
  }
}
```

The private key seed is encrypted with a key derived from the user's passphrase via Argon2id. The public key is stored in plaintext so the display name and fingerprint can be shown before unlocking.

### 14.3 Passphrase-Protected Identity Unlock

The identity passphrase is the single credential that unlocks everything for that identity. No separate database password is ever presented to the user.

| Passphrase use | Description |
|---|---|
| Identity unlock | `Argon2id(passphrase, salt)` → 32-byte key → decrypt Ed25519 seed |
| Signing operations | Ed25519 private key derived from decrypted seed |
| DB password decrypt | X25519 key (converted from Ed25519 seed) decrypts the local DB password blob |
| Session lock | Seed wiped from memory on idle timeout or explicit lock |

The SQLite database password is a randomly-generated 32-byte value created at workspace initialisation. It is encrypted to the identity's public key and stored in `settings.json`. It is never shown to the user and never leaves the device. When the identity is unlocked, the DB password is silently decrypted and used to open SQLCipher — the user experiences a single passphrase prompt that opens everything.

### 14.4 Settings File — Workspace Registry

`settings.json` maps workspaces to identities and stores the encrypted DB password for each workspace:

```json
{
  "identities": [
    {
      "uuid": "<uuid-work>",
      "display_name": "Carsten @ 2pi",
      "file": "identities/<uuid-work>.json",
      "last_used": "2026-03-04T09:00:00Z"
    },
    {
      "uuid": "<uuid-personal>",
      "display_name": "Carsten K",
      "file": "identities/<uuid-personal>.json",
      "last_used": "2026-03-01T18:30:00Z"
    }
  ],
  "workspaces": {
    "<workspace-uuid-A>": {
      "db_path": "/path/to/workspace-a.db",
      "identity_uuid": "<uuid-work>",
      "db_password_enc": "<base64 AES-GCM blob>"
    },
    "<workspace-uuid-D>": {
      "db_path": "/path/to/workspace-d.db",
      "identity_uuid": "<uuid-personal>",
      "db_password_enc": "<base64 AES-GCM blob>"
    }
  }
}
```

Each workspace is bound to exactly one identity. The workspace list shown to the user is filtered by the active identity — workspaces belonging to other identities are not visible until the user switches identity.

### 14.5 Application Session Flow

**First launch — new identity**
1. User chooses a display name for the identity.
2. App generates a fresh Ed25519 keypair.
3. User sets a passphrase; Argon2id derives the encryption key; private key seed is encrypted and written to the identity file.
4. On first workspace creation, a random 32-byte DB password is generated, SQLCipher is opened, and the DB password is encrypted to the identity public key and stored in `settings.json`.

**Subsequent launches**
1. Identity picker displays all known identities by display name (last-used identity pre-selected).
2. User selects an identity and enters their passphrase.
3. Argon2id derives the key; private key seed is decrypted and held in a protected memory allocation.
4. For each workspace bound to this identity, the DB password blob is decrypted and SQLCipher is opened silently.
5. The workspace list for this identity is displayed. No further prompts.

**Switching identity within the app**
1. User selects "Switch Identity" from the app menu.
2. Active identity seed is wiped from memory; DB connections are closed.
3. Identity picker is displayed. Workspaces from the previous identity disappear.
4. User selects a different identity, enters its passphrase, and the new workspace list appears.
5. No application restart required.

**Passphrase change**
1. User unlocks with the current passphrase.
2. A new Argon2id salt is generated; the same Ed25519 seed is re-encrypted under the new derived key.
3. DB password blobs in `settings.json` are unaffected — they are encrypted to the keypair, not to the passphrase.

### 14.6 Multi-Device — Same Identity on Multiple Devices

A user may install the same identity on multiple devices (e.g., a desktop and a mobile). The identity keypair is identical on both devices, making the user cryptographically the same person. However, each device remains an independent sync peer because the `device_id` — derived from hardware — is unique per device.

| Property | Desktop | Mobile |
|---|---|---|
| Identity keypair | Same Ed25519 seed | Same Ed25519 seed |
| Identity passphrase | Same passphrase | Same passphrase (same KDF salt) |
| `device_id` | `MAC-hash-A` | `MAC-hash-B` — independent peer |
| SQLite DB | Independent local DB | Independent local DB |
| DB password | Own random password | Own random password — never shared |

The DB password is entirely local to each device. Mobile generates its own random DB password on workspace initialisation; it is never derived from, transmitted from, or coordinated with the desktop. Workspace data reaches mobile exclusively via `.swarm` sync bundles, exactly as it would from any other peer.

### 14.7 Identity Export and Import (`.krillid`)

To install an identity on a second device, the user exports a `.krillid` file from the source device and imports it on the target device. The format is minimal — it contains only the identity, not any workspace data or DB passwords:

```json
{
  "identity_uuid": "...",
  "display_name": "Carsten @ 2pi",
  "public_key": "<base64>",
  "private_key_enc": {
    "ciphertext": "...",
    "nonce": "...",
    "kdf": "argon2id",
    "kdf_params": { "salt": "...", "m_cost": 65536, "t_cost": 3, "p_cost": 1 }
  }
}
```

The same passphrase that protects the identity on the source device decrypts it on the target device — because Argon2id uses the same salt embedded in the file. No password coordination is required.

After import, the new device has no workspaces. Workspace data arrives via normal `.swarm` snapshot sync from the existing peer. The new device creates its own local DB with its own random DB password, entirely independent of any other device.

**Multi-device setup flow:**
1. Source device: Settings → Export Identity → saves `<display-name>.krillid`.
2. Transfer `.krillid` to target device (AirDrop, cable, email — user's choice).
3. Target device: Import Identity → enter same passphrase → identity installed.
4. Target device joins workspace: source device sends a `.swarm` snapshot encrypted to the shared public key.
5. Target device decrypts snapshot with shared private key, creates local DB with its own random password, imports workspace state.
6. Ongoing sync proceeds via delta `.swarm` bundles — target device is now a standard peer.

### 14.8 Device ID vs. Identity

Krill Notes maintains two independent axes of identification on every operation:

| Axis | Purpose |
|---|---|
| `device_id` | Identifies the physical machine. Used for sync logistics — tracking which operations each device has seen. Derived from hardware (MAC address hash). Stable across workspaces and identities. |
| identity (keypair) | Identifies the author. Used for RBAC, audit trails, and cryptographic verification. Scoped to a single workspace. One device may hold multiple identities; one identity may operate from multiple devices. |

### 14.9 Identity Recovery

If a device is lost, the private key is lost on that device. Recovery options:

- **Re-invitation:** The workspace owner issues a new invitation. The user imports their `.krillid` backup on a new device and rejoins. The old device's peer entry can be revoked.
- **Backup:** The `.krillid` file can be backed up to any secure location. Restoring it on a new device reinstates the identity with the same passphrase.
- **Recovery phrase:** The private key seed can be encoded as a mnemonic word list (similar to a cryptocurrency seed phrase) for offline paper backup.

### 14.10 Future: OS Keychain Integration (Commercial)

The open-source KrillNotes core uses passphrase-only identity unlock, keeping the implementation self-contained with no platform dependencies. The commercial OPswarm product will add OS keychain integration as a second unlock path:

```rust
fn unlock_identity(identity: &IdentityFile) -> Result<SigningKey> {
    match unlock_method {
        UnlockMethod::Passphrase(pp) => derive_and_decrypt(identity, pp),
        UnlockMethod::OsKeychain    => keychain::retrieve(identity.uuid), // commercial only
    }
}
```

The identity file format, DB password scheme, `settings.json` structure, and all downstream cryptography are identical in both cases. The OS keychain replaces only the Argon2id passphrase derivation step, providing a seamless zero-prompt experience on trusted devices while preserving full compatibility with the open-source core.

---

## 15. Trust & Verification

In a system without a central authority, trust must be established through external channels. Krill Notes supports a layered trust model that lets users choose the appropriate level of verification for each context.

### 15.1 Trust Levels

| Level | Description |
|---|---|
| Verified in person | Highest assurance. Public keys compared via QR code scan or side-by-side display during a physical meeting. Unimpeachable. |
| Code verified | Strong assurance. A short verification code (derived from the public key) is compared over a trusted out-of-band channel such as a phone call or video chat. |
| Vouched | Transitive trust. A verified peer vouches for a new participant by co-signing their invitation. Displayed as "Carol, vouched for by Bob". |
| TOFU (Trust On First Use) | Minimum assurance. The identity is accepted at first contact without independent verification. Suitable for low-stakes or trusted-channel scenarios. |

### 15.2 Key Fingerprints

For verification methods that require human comparison, public keys are displayed as short fingerprints. A fingerprint is a BLAKE3 hash of the public key rendered as a human-friendly format:

- **Word format (recommended):** four words from a fixed 2048-word BIP-39 dictionary, e.g., `ocean-maple-thunder-seven`
- **Hex format (fallback):** short hex blocks, e.g., `A4 3B 7F 12 D8 91`
- **QR code:** encodes the full public key plus display name for scanning

Word-based fingerprints are easy to read aloud over a phone call and easy to compare visually during in-person verification.

### 15.3 Trust Level Assignment

The trust level of a new participant depends on how the invitation was verified:

- In-person QR scan of fingerprints during the handshake yields "Verified in person."
- Verbal confirmation of fingerprints over a phone call yields "Code verified."
- A vouched introduction from an existing peer yields "Vouched by [peer]."
- An email invitation without separate verification yields "TOFU."

The owner can upgrade a peer's trust level at any time via a separate verification step. Known contacts invited from the address book retain their existing trust level from previous workspace interactions.

---

## 16. RBAC — Role-Based Access Control

Krill Notes implements role-based access control at the note level, with permission inheritance through the tree hierarchy. This serves two purposes: it controls who can do what, and it reduces sync conflicts by limiting the number of writers on any given subtree.

### 16.1 Roles

| Role | Capabilities |
|---|---|
| Owner | Full control: read, write, create, delete, move notes within the subtree. Can grant and revoke permissions on the subtree. Can delegate ownership. |
| Writer | Read, write, create, and move notes within the subtree. Cannot change permissions or delete the subtree root. |
| Reader | View notes within the subtree. Cannot modify anything. |
| None | Explicit denial of access, overriding any inherited permission. Enables private subtrees within an otherwise shared workspace. |

### 16.2 Permission Inheritance

Permissions are set on individual notes and inherited by all descendants. When checking access for a note, the system walks up the tree from the note to the root, stopping at the first explicit permission entry for the requesting user. If no explicit permission is found, the workspace default role applies.

Example workspace structure:

```
Workspace Root (default: reader)
├─ Company Wiki (editors group: writer)
│  ├─ Onboarding Guide
│  └─ API Docs
├─ Project Alpha (team-alpha: writer)
│  ├─ Sprint Notes
│  └─ Architecture Decisions
└─ Sarah's Drafts (sarah: owner, default: none)
```

In this example, everyone can read the Company Wiki, but only the editors group can edit it. Sarah's Drafts are invisible to other users because the default for that subtree is "none" (no access).

### 16.3 Permission Storage

```sql
note_permissions (
    note_id  TEXT NOT NULL REFERENCES notes(id),
    user_id  TEXT NOT NULL,
    role     TEXT NOT NULL CHECK(role IN ('owner','writer','reader','none')),
    PRIMARY KEY (note_id, user_id)
)
```

### 16.4 Sub-Delegation

Owners of a subtree can grant permissions within their subtree. These grants are signed by the granting user's key, and the chain of trust is verifiable:

```
Alice (root owner) → signed: "Bob is owner of /Project Alpha"
Bob (subtree owner) → signed: "Carol is writer on /Project Alpha/Sprint Notes"
```

Any device can verify this chain locally. If Alice later revokes Bob's ownership, Carol's permissions (granted by Bob) also become invalid — the chain is broken and cascades automatically.

---

## 17. Permission Enforcement Without a Server

Every operation in Krill Notes is cryptographically signed by the author's private key. Verification is performed locally on every device when applying incoming operations from a .swarm bundle. There is no server to enforce permissions — every device is its own enforcer.

### 17.1 Verification Steps

When an operation arrives in a .swarm or .cloud bundle, the receiving device:

1. *(For .swarm only)* Decrypts the payload using the recipient's private key and the per-recipient AES key wrapper.
2. Verifies the bundle-level signature against the sender's known public key. *(For .cloud, also verifies against the stored trusted fingerprint.)*
3. Verifies each operation's individual Ed25519 signature against the author's known public key.
4. Resolves the author's effective role on the target note by walking the permission tree.
5. Checks that the role permits the operation type (e.g., writer can `UpdateField` but not `SetPermission`).
6. If all checks pass, the operation is applied. Otherwise, it is rejected and logged.

For .cloud bundles, step 1 (decryption) is skipped — the payload is cleartext. All other verification steps are identical. The security boundary is the same: every receiving peer independently validates every operation's signature and RBAC permissions. The absence of encryption means confidentiality is not provided, but authenticity and integrity guarantees are unchanged.

### 17.2 Permission Operation Types

`SetPermission` and `RevokePermission` are standard operations that travel in .swarm bundles like any other. They are subject to the same signing and verification requirements. Only users with the Owner role on a note (or the workspace root) can emit `SetPermission` and `RevokePermission` operations for that note.

### 17.3 Modified Client Threat Model

The security assessment analysed four sub-threats:

| Threat | Risk | Mitigation |
|---|---|---|
| **A:** Modified client ignores RBAC locally | Contained to their device | Cannot be prevented; physical access reality |
| **B:** Modified client generates unauthorised operations | Primary threat | Every receiving peer validates signature + RBAC on ingest; unauthorised ops rejected |
| **C:** Two colluding modified clients | Contained between them | Cannot infect honest peers; honest peer perimeter is the security boundary |
| **D:** Authorised liar (valid access, false data) | Human process problem | Non-repudiation: every entry permanently signed with author's identity key |

### 17.4 Permission + Tree Move Interaction

Moving a note between subtrees requires write permission on both the source parent (to remove the child) and the destination parent (to add the child). If the user lacks permission on either, the move is rejected.

When a note is moved to a new subtree, it inherits the permissions of its new parent. This is an immediate consequence of the permission inheritance model and requires no special handling.

---

## 18. Revocation & Edge Cases

### 18.1 Revocation Is Eventually Consistent

In a decentralised system, revocation cannot be instantaneous. When Alice revokes Bob's write access, the `RevokePermission` operation must propagate to all peers via .swarm bundles. Until a peer receives the revocation, it may accept operations from Bob that were generated after the revocation.

> **Security Finding SA-002 (High): Revocation Propagation Gap**
>
> During the propagation window (hours over LoRa/sneakernet), peers continue accepting operations from revoked users. The original design specified retroactive rollback, which creates accountability problems — data that informed decisions disappears.
>
> **Resolution (proposed):** Implement a quarantine model for the commercial product. See Section 21.1 for the data preservation design.

### 18.2 Transport Encryption and Revocation

Transport encryption strengthens the revocation model. When a user's access is revoked:

1. The `RevokePermission` operation propagates to all peers via bundles.
2. Future .swarm bundles simply omit the revoked user from the recipients list.
3. The revoked user cannot decrypt any new .swarm bundles, even if they intercept them from a shared folder.
4. The revoked user's local database remains accessible (their SQLCipher password still works), but they are cryptographically cut off from all future updates.

This provides defence in depth: permission enforcement rejects unauthorised operations at the application level, and transport encryption prevents unauthorised access at the file level.

### 18.3 Delete vs. Edit Conflict

If device A deletes a note while device B edits it, the delete wins by default. Edits to a deleted note are discarded during bundle application. The deleted note's data is retained in the operation log (the original `CreateNote` and subsequent edits are still there), so it can be audited or potentially restored by an owner.

See Section 21.2 for the configurable conflict policy that allows schemas to override the default delete-wins behaviour.

### 18.4 Tree Move Conflicts

If two devices move the same note to different parents, the system applies LWW as the working state but flags the conflict for user review. Cycle detection is performed before applying any tree move — a move that would create a cycle is rejected.

---

## 19. Encryption Model

Krill Notes has three independent encryption layers, each protecting data in a different context.

### 19.1 Three Layers of Encryption

| Layer | Protects | Key |
|---|---|---|
| SQLCipher (AES-256-CBC) | Database at rest on each device | Per-device random password, derived via passphrase unlock (see Section 14.3). Never leaves the device. Never shared with peers. |
| Transport encryption (.swarm) | Data in transit between peers | Recipient's Ed25519 public key via hybrid encryption (asymmetric key wraps a random symmetric key). No shared passwords required. |
| Operation signatures (Ed25519) | Authenticity and authorisation of each operation | Author's Ed25519 private key, scoped to the workspace identity. Verified by all peers using the author's public key. |

The critical insight is that the SQLCipher database password is entirely device-local. Because workspaces are never exchanged as raw database files — they are always exchanged as .swarm bundles — the database encryption is completely decoupled from the transport encryption.

> **Security Finding SA-007 (Medium): Per-Recipient Encryption Scaling**
>
> In a 40-device incident with watched-folder sync, generating N-1 separate encrypted bundles per device is impractical.
>
> **Resolution:** The server-as-relay-hub architecture (Section 22) reduces the topology from N² to 2N bundles. For P2P without a server, multi-recipient key wrapping (single encrypted payload, per-recipient AES key wrappers) is the default for shared folder transports.

### 19.2 Transport File Encryption

The .swarm bundle uses hybrid encryption within its zip structure. The `header.json` is readable without decryption; the `payload.enc` and all attachment `.enc` files are encrypted:

1. Generate a random AES-256-GCM symmetric key for the payload.
2. Encrypt the entire payload (operations, signatures, metadata) with this symmetric key.
3. For each intended recipient, encrypt the symmetric key with that recipient's Ed25519 public key (converted to X25519 for Diffie-Hellman key agreement).
4. Include all encrypted key copies in the unencrypted file header.

```
.swarm bundle internal structure:
┌─ Header (unencrypted)
│  ├─ format_version
│  ├─ workspace_id
│  ├─ source_device_id
│  ├─ encryption_method: "x25519-aes256gcm"
│  └─ recipients[]
│     ├─ { peer_id: "bob",   encrypted_key: "..." }
│     └─ { peer_id: "carol", encrypted_key: "..." }
│
└─ Payload (AES-256-GCM encrypted)
   ├─ operations[]
   ├─ operation signatures
   └─ patch metadata
```

When Bob opens this file, his app finds his entry in the recipients list, decrypts the AES key using his private identity key, and decrypts the payload. A user not listed in the recipients cannot decrypt the payload even if they possess the file.

### 19.3 Single-Recipient vs. Multi-Recipient

For direct peer-to-peer bundles (USB drive, email), the recipients list contains a single entry. For shared-folder broadcast bundles, the recipients list contains an entry for every active peer in the workspace.

The payload is encrypted only once regardless of the number of recipients — only the small symmetric key is encrypted multiple times (once per recipient). This makes multi-recipient encryption efficient even for large bundles.

### 19.4 Cryptographic Foundations

| Component | Recommendation |
|---|---|
| Identity keypairs | Ed25519 (fast, compact, widely supported). Crate: `ed25519-dalek` or `ring`. |
| Operation signatures | Ed25519 signature over canonical JSON serialisation of the operation (excluding the signature field itself). |
| Transport encryption | Hybrid scheme: X25519 key agreement (Ed25519 keys converted to X25519) to establish a shared secret, then AES-256-GCM for payload encryption. Crates: `x25519-dalek` + `aes-gcm`. |
| Multi-recipient wrapping | Random AES-256 key encrypted per-recipient via X25519. Same pattern as PGP/GPG and NaCl `crypto_box`. |
| Database encryption | SQLCipher (AES-256-CBC, PBKDF2-HMAC-SHA512). Fully independent of transport encryption. |
| Key fingerprints | BLAKE3 hash of the public key, rendered as 4 words from a 2048-word BIP-39 dictionary. |
| Identity file key derivation | Argon2id with `m_cost: 65536`, `t_cost: 3`, `p_cost: 1`. |
| Invitation tokens | Random 256-bit token, HMAC-bound to the inviter's identity and the intended role. |

All cryptographic operations use well-established, audited Rust crates. No custom cryptography is implemented.

---

## 20. Replay Protection & Hash Chaining

> **Security Finding SA-010 (High): No Replay Attack or Bundle Freshness Protection**
>
> The design relies on operation UUID idempotency but does not address deliberate replay attacks. A captured .swarm containing a `RevokePermission` could be replayed after re-grant.

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

Three operation states:
- **valid** — signature and RBAC verified; applied to working state.
- **rejected** — failed signature or RBAC check; never applied.
- **contested** — applied at time of receipt, then retroactively invalidated by a subsequent revocation; preserved but flagged in the UI.

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

The server monitors: sync freshness per peer (alert when silent), clock drift detection (compare incoming HLC `wall_ms` against NTP-synced server clock), rejected operation patterns (signal compromised or modified client), operation volume anomalies, and transport channel health.

### 22.5 Multi-Incident Architecture

Each incident runs in an isolated workspace with its own root identity, RBAC rules, schemas, and peer registry. The server manages multiple workspaces concurrently. Cross-incident analysis is enabled through the integration layer feeding multiple workspaces into a shared knowledge graph.

### 22.7 .cloud Broadcast Generation

The Swarm Server adds .cloud generation as a publication mode alongside its existing .swarm relay function. The server produces broadcast bundles from the same operation log it uses for .swarm sync, signed by the same root owner identity stored in the HSM.

The publication pipeline runs as a separate scheduled task: after each merge cycle, the server evaluates whether any new operations affect public subtrees (those with a default reader permission in the RBAC tree). If so, it generates a .cloud delta bundle containing those operations and publishes it to the configured broadcast channels (HTTPS endpoint, S3 bucket, watched folder).

This arrangement means a single workspace simultaneously serves:
- Named collaborators via encrypted .swarm bundles (per-recipient, bidirectional)
- Anonymous readers via cleartext-signed .cloud bundles (broadcast, one-way)

The RBAC tree is the single source of truth governing what each audience sees. No separate workspace or data duplication is required.

### 22.9 Graceful Degradation

| Failure | Impact | Recovery |
|---|---|---|
| Server goes offline | P2P sync continues. Intelligence loop stops. .cloud publication pauses. | Server syncs with any peer on return; resumes .cloud generation. |
| Server destroyed | Root key at risk if not backed up. | Provision replacement with HSM-backed key. |
| Internet link lost | LoRa and local sync unaffected. .cloud HTTPS endpoint unavailable. | Server queues bundles for reconnection; .cloud receivers see gap until reconnect. |
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

### 25.1 Sync Menu

The Krill Notes UI exposes sync through a dedicated menu and status indicators:

- **File → Export Sync Bundle...** Select a peer (or "all peers"), generate a .swarm file, and save it to a chosen location.
- **File → Apply Sync Bundle...** Open a .swarm file, preview the changes (operations count, author, time range, attachment count), and apply.
- **File → Invite Peer...** Create an invitation for a new collaborator, specifying their role and scope.
- **Sync status indicator** A subtle icon in the status bar showing: number of unsynced local operations, time since last sync per peer, any unresolved conflicts.

### 25.2 Auto-Sync via Watched Folder

For hands-free sync, users can designate a shared folder (local NAS, Dropbox, GDrive, etc.) as a sync directory. Krill Notes watches this folder and automatically writes outgoing .swarm bundles and applies incoming ones. The user configures this once and sync happens transparently.

### 25.3 Identity Selector

When opening a workspace for the first time, the app prompts the user to select or create an identity. A small identity indicator in the title bar or status bar shows which identity is active for the current workspace. An identity manager in settings allows creating, renaming, exporting, and managing identities across workspaces.

### 25.4 Peer Management

The peer list (accessible from the Sync menu or a sidebar panel) shows all known peers with their sync status, trust level, and last sync time. Users can rename peers, verify trust levels, and generate targeted .swarm bundles.

### 25.5 Conflict Resolution UI

When conflicts are detected during bundle application, a notification appears. The user can review conflicts in a dedicated panel that shows the competing versions side by side (for field conflicts) or the competing tree positions (for move conflicts), and choose which version to keep or manually resolve.

---

## 26. Implementation Roadmap

### Core Protocol Phases

| Phase | Delivers | Dependencies | Security Findings Addressed |
|---|---|---|---|
| **1 — Swarm Infrastructure** | Manual sync (delta + snapshot), HLC timestamps, gated function API, revised Operation enum | None (builds on existing operation log) | SA-001 (HLC), SA-006 (delta fallback), SA-009 (loud textarea LWW) |
| **2 — Cryptographic Identity** | Signed operations, identities, invite/accept handshake, `.krillid` export/import | Phase 1 | — |
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

- **Patch size limits:** For workspaces with large operation histories, delta bundles could become very large. Should bundles support chunking or streaming? Should there be a maximum bundle size?

### .cloud-Specific Open Questions

- **Feed format standardisation.** Should the .cloud publication model define a standard feed manifest (analogous to RSS/Atom) listing available bundles, their sequence numbers, and checksums? This would enable automated consumption without relying on filesystem watching.

- **Revocation of broadcast workspaces.** If the sender's key is compromised, how should receivers be notified? The sender cannot push a revocation to unknown receivers. Options include publishing a revocation notice at the same trusted channel used for key distribution, or defining a standard revocation bundle type.

- **Partial broadcasts.** Should the sender be able to publish subtree-scoped .cloud bundles (broadcasting only specific branches of the workspace)? This would enable different broadcast feeds from the same workspace for different audiences, without requiring separate workspaces.

- **Broadcast licence metadata.** Should the .cloud header carry an explicit usage declaration from the author (e.g., "read-only, no redistribution" or "redistribution with attribution")? This has no cryptographic enforcement but establishes intent and may carry legal weight, following the Creative Commons model.

- **Upgrade path from .cloud to .swarm.** Should a receiver be able to "upgrade" a broadcast subscription to a collaborative .swarm relationship by contacting the sender and completing a standard invitation flow?

- **Anonymous reader analytics.** The sender has no visibility into how many receivers consume their broadcast. Should the protocol define an optional, privacy-preserving mechanism for receivers to signal their presence (e.g., an anonymous heartbeat to the publication endpoint)?

---

## Appendix A: Security Finding Traceability

| ID | Finding | Severity | Resolution | Section | Status |
|---|---|---|---|---|---|
| **SA-001** | Wall-clock timestamp dependency (LWW) | **Critical** | HLC timestamps in core protocol from Phase 1 | §4 | **Resolved** |
| **SA-002** | Revocation propagation gap | **High** | Quarantine model (contested state) proposed | §18.1, §21.1 | Design in progress |
| **SA-003** | Delete-wins conflict policy | **High** | Configurable per-schema conflict policy proposed | §21.2 | Design in progress |
| **SA-004** | Root owner single point of failure | **Critical** | Server-side root authority with HSM | §22.2 | **Resolved** (server design complete) |
| **SA-005** | Snapshot onboarding leaks full workspace | **Medium** | Subtree-scoped snapshots planned | §12 | Planned |
| **SA-006** | Operation log purge / delta fallback gap | **Medium** | Explicit delta-not-possible detection with snapshot fallback | §5.6 | Specified |
| **SA-007** | Per-recipient encryption scaling | **Medium** | Server relay hub (2N topology) + multi-recipient key wrapping | §19.1, §22.3 | **Resolved** |
| **SA-008** | LoRa transport throughput claims | **Low** | Reframed as critical-priority-only | §13 | **Resolved** |
| **SA-009** | Deferred text CRDT creates migration risk | **Medium** | Loud conflict detection for textarea LWW (interim); CRDT Phase 7 | §7.4 | Interim mitigation specified |
| **SA-010** | No replay attack or bundle freshness protection | **High** | Sequence counters + hash chaining + bundle manifests proposed | §20 | Design in progress |

---

*End of Unified Design Specification*

*Swarm Protocol — Unified Design v0.7*
