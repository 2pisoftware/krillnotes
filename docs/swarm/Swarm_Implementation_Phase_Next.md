# Swarm Implementation — Next Three Work Packages

**Status:** Planning  
**Prerequisite:** WP1 (HLC + Operation Model), WP2 (Gated Function API), and WP4 (Identity + Cryptography) are implemented.

---

## Overview

With the local engine (HLC timestamps, Operation enum, gated functions) and cryptographic identity subsystem complete, the remaining path to working two-device sync consists of three work packages:

| Package | Name | Depends On | Delivers |
|---------|------|------------|----------|
| **WP-A** | Peer Model + Bundle Format | WP1, WP4 | Contacts address book, per-workspace peer registry, .swarm codec (all four modes), invitation state machine (both known-contact and unknown-peer flows) |
| **WP-B** | RBAC Engine | WP1 (Operation enum only) | Permission tree, role definitions, inheritance walk, sub-delegation chain validation, enforcement function |
| **WP-C** | Sync Engine | WP-A, WP-B, WP2 | Inbound bundle application pipeline (decrypt → verify → RBAC check → conflict resolution → apply → update peer markers). This is where conflict resolution lives, because conflicts only arise when a remote operation meets a divergent local state. |

**WP-A and WP-B are independent and both unblocked now.** WP-C needs both.

After WP-C, working two-device sync exists and everything else (schema governance, attachments, transports, .cloud, server) layers on top.

---

## Why Conflict Resolution Is Not a Separate Package

Conflict resolution was originally planned as a standalone work package (WP3) on the assumption that it could be tested as pure functions against synthetic operation pairs. While the bare comparison logic (given two HLC timestamps, which wins?) is trivially testable, the meaningful conflict scenarios — concurrent field edits, delete-vs-edit, competing tree moves, textarea divergence — only arise during **bundle application**, when an inbound operation lands against a local database that already contains a competing edit from a different peer's operation stream.

On a single device editing sequentially, every operation observes all previous operations. There is no window for divergence. Conflict resolution therefore collapses into WP-C as the core logic inside the bundle application engine.

---

## Design Amendment 1: Sync Peers vs. Known Identities

The v0.7 spec conflates two distinct relationships that the implementation must separate.

**Sync peers** are devices you directly exchange .swarm bundles with. They are tracked in the per-workspace peer registry with sync state markers (`last_sent_op`, `last_received_op`). You generate bundles *for* them and apply bundles *from* them.

**Known identities** are people whose operations you encounter because they arrived transitively through a sync peer. In a hub-and-spoke topology, a field device might only sync directly with the Swarm Server, but the delta bundles from the server contain operations signed by Bob, Carol, and Dave — none of whom the field device has ever exchanged a bundle with directly. The field device needs their public keys to verify those signatures and sees their names in attribution, but has no sync relationship with them.

The contacts address book covers both cases — it stores public keys for everyone you interact with, whether directly or transitively. The key question is: how do transitive identities arrive in your contacts?

- **Snapshot import:** The `workspace.json` includes all known identities with public keys (the "known users" field). On initial onboarding, the new peer receives every identity in the workspace.
- **Delta propagation:** When a new peer joins the workspace (via `JoinWorkspace` operation), their identity propagates to all other peers through normal delta sync. Each receiving peer adds the new identity to their contacts automatically.
- **SetPermission operations** also carry the target identity's public key, providing a second propagation path.

### Two-Layer Naming Model

Identity naming follows a two-layer model, analogous to phone contacts:

**Declared name** — the display name the person chose when they created their identity (e.g., "Bob Chen"). This propagates through the protocol in `JoinWorkspace` operations and bundle headers. It appears by default in operation attribution and audit trails.

**Local name** — an optional override set by the local user in their contacts address book (e.g., "Robert — Field Team Lead"). This is purely local, never propagates, and does not affect what anyone else calls the person.

The display logic is: `local_name.unwrap_or(&declared_name)`.

The `peer_name` field in the `sync_peers` table is removed. The peer registry references the contact record by public key; the contact record holds both the declared name and the local override. This eliminates name duplication and ensures a single source of truth for display names.

---

## Design Amendment 2: Generalised Invitation Model

The v0.7 spec restricts invitation capability to Owners only (Section 16.4: "only users with the Owner role can emit SetPermission"). This creates a bottleneck in true peer-to-peer field deployments — the person who most needs to grow the team (a Writer in the field) cannot bring in colleagues without routing back to the Owner.

**New principle:** You can grant permissions up to but not exceeding your own effective role, within your own permitted scope.

| Role | Can Invite | Maximum Grantable Role | Can Revoke |
|------|-----------|----------------------|------------|
| **Owner** | Yes | Owner, Writer, Reader | Any grant on their subtree |
| **Writer** | Yes | Writer, Reader | Only grants they personally issued |
| **Reader** | Yes | Reader | Only grants they personally issued |
| **None** | No | — | — |

**Security invariant preserved:** No identity can escalate permissions beyond what they hold. A Writer inviting another Writer creates no new capability that didn't already exist in the subtree.

**Verification rule change:** The check on SetPermission operations shifts from "is this signed by an Owner?" to "does the signer's effective role on the target scope equal or exceed the role being granted?" Role ordering: Owner > Writer > Reader > None.

**Sub-delegation chains generalise:**

```
Alice (root owner) → "Bob is writer on /Project Alpha"
Bob (writer)       → "Carol is writer on /Project Alpha"
Carol (writer)     → "Dave is reader on /Project Alpha"
```

Every link is valid because the granter held at least the role they granted. Chain verification walks backwards through SetPermission operations. If Alice revokes Bob, the entire chain collapses — Carol's and Dave's access cascades away.

**Revocation rights:** An Owner on a scope can revoke any grant within that scope. A non-Owner can revoke only grants they personally signed (the `granted_by` field on SetPermission matches their identity). This prevents a Writer from revoking permissions issued by someone above them.

**Invitation flow impact:** The invite/accept/snapshot mechanics are unchanged — the same state machine works regardless of the inviter's role. The only difference is what role appears in the SetPermission payload and what scope it covers. Validation shifts from "is this an Owner?" to "is the offered role within the signer's capability?"

---

# WP-A: Peer Model + Bundle Format

## Scope

This package delivers everything needed to produce and consume .swarm bundles, and the peer relationship model that drives bundle generation.

### In Scope

1. **Contacts address book** (cross-workspace, app-level)
   - Storage format and location (`~/.config/krillnotes/contacts/`)
   - Contact record structure: `contact_id`, `declared_name`, `local_name` (optional override), `public_key`, `fingerprint`, `trust_level`, `vouched_by`, `first_seen`, `notes`
   - Two-layer naming: display logic is `local_name.unwrap_or(&declared_name)` (see Design Amendment 1)
   - CRUD operations on contacts
   - Automatic population on invitation completion
   - **Transitive identity propagation:** contacts created automatically when operations from unknown identities arrive in delta bundles, or when identities are included in snapshot workspace.json

2. **Per-workspace peer registry** (workspace-level, stored in SQLite)
   - `sync_peers` table: `peer_device_id`, `peer_identity_id` (references contact by public key), `last_sent_op`, `last_received_op`, `last_sync`
   - No name duplication — display name comes from the contact record (see Design Amendment 1)
   - Peer lifecycle: creation on invitation completion, marker updates after bundle exchange, stale peer detection
   - **Distinction from known identities:** the peer registry tracks only devices you directly exchange bundles with; identities encountered transitively are tracked in contacts only

3. **.swarm bundle codec** (all four modes)
   - **Common header** (`header.json`): `format_version`, `mode`, `workspace_id`, `workspace_name`, `source_device_id`, `source_identity`, `source_display_name`, `created_at`
   - **Invite mode**: additional fields (`pairing_token`, `offered_role`, `offered_scope`, `inviter_fingerprint`), signed SetPermission with placeholder target, not encrypted
   - **Accept mode**: additional fields (`pairing_token`, `accepted_identity`, `accepted_display_name`, `accepted_fingerprint`), signed JoinWorkspace operation
   - **Snapshot mode**: additional fields (`as_of_operation_id`, `recipients[]`, `has_attachments`), encrypted `workspace.json` containing full resolved workspace state
   - **Delta mode**: additional fields (`since_operation_id`, `target_peer`, `recipients[]`, `has_attachments`), encrypted operations array + manifest + patch signature
   - Zip archive structure for all modes
   - Bundle-level signature (BLAKE3 manifest hash, signed by source identity)

4. **Hybrid encryption for snapshot and delta modes**
   - Random AES-256-GCM symmetric key for payload
   - Per-recipient key wrapping via X25519 (Ed25519 keys converted)
   - Single-recipient (direct P2P) and multi-recipient (shared folder) variants
   - Recipient lookup in header, decrypt with own private key

5. **Invitation state machine**
   - **Any peer with a role can invite** — not restricted to Owners (see Design Amendment 2). The offered role must not exceed the inviter's own effective role on the target scope.
   - **Known contact flow** (1 exchange): select contact from address book → create SetPermission (with real target identity) → generate snapshot .swarm encrypted for contact's key → send
   - **Unknown peer flow** (3 exchanges): generate invite .swarm (signed, unencrypted, pairing token) → receive accept .swarm (verify pairing token, register contact, resolve SetPermission placeholder, add to peer registry) → generate snapshot .swarm (encrypted for new peer's key) → send
   - **Invitation validation:** verify inviter's effective role ≥ offered role on the target scope
   - Pairing token generation (random 256-bit) and verification
   - Error cases: duplicate pairing token, expired invitation, mismatched workspace_id, role exceeds inviter's capability

6. **Delta bundle generation**
   - Query operation log for operations since `last_sent_op` for target peer
   - Package operations in chronological order with HLC timestamps in compact array format
   - Generate manifest (list of included operation_ids)
   - Sign and encrypt
   - Update `last_sent_op` marker after successful generation

7. **Snapshot generation**
   - Resolve current workspace state (notes with all fields, tags, tree structure, user scripts, resolved permission state, known users with public keys)
   - Package as `workspace.json`
   - Record `as_of_operation_id` (the latest operation reflected in the snapshot)

### Out of Scope (deferred to later packages)

- RBAC permission checking during bundle application (WP-B provides this; stub with allow-all for testing)
- Conflict resolution during bundle application (WP-C)
- File attachments in bundles (WP-10, later)
- .cloud broadcast format (WP-12, later)
- Transport adapters — this package produces and consumes .swarm files; how they move between devices is irrelevant here (manual file copy is the test transport)
- Subtree-scoped snapshots (SA-005, commercial product)

---

## Key Design Decisions (from the spec)

**The .swarm file is a zip archive.** Not a custom binary format. This means standard zip tools can inspect headers (the unencrypted `header.json`) even without the app, which aids debugging and interoperability.

**The header is always unencrypted.** The app can identify a bundle's purpose and workspace without attempting decryption. This is a deliberate design choice — the header reveals the workspace name, sender identity, and mode, but no workspace content.

**Invite bundles are signed but not encrypted** because the recipient's public key is not yet known. They should not be treated as sensitive. Anyone who intercepts an invitation cannot use it without also completing the handshake.

**The payload is encrypted once, keys wrapped per-recipient.** The expensive encryption happens once (AES-256-GCM on the full payload); only the small symmetric key is encrypted per-recipient (X25519 key agreement). This makes multi-recipient bundles efficient.

**Snapshot mode carries resolved state, not operations.** The recipient imports current state directly — no conflict resolution needed on snapshot import. This is critical for initial onboarding: the new peer doesn't replay the workspace's entire operation history.

**Delta mode carries signed operations in chronological order.** Each operation retains its original Ed25519 signature from the authoring identity. The bundle also carries a bundle-level signature from the sender, making the bundle atomic.

---

## Data Structures

### Contact Record

```rust
pub struct Contact {
    pub contact_id: Uuid,
    pub declared_name: String,            // what they call themselves (from protocol)
    pub local_name: Option<String>,       // what I call them (my local override)
    pub public_key: Ed25519PublicKey,
    pub fingerprint: String,             // BIP-39 4-word rendering
    pub trust_level: TrustLevel,         // VerifiedInPerson | CodeVerified | Vouched | Tofu
    pub vouched_by: Option<Uuid>,        // contact_id of voucher
    pub first_seen: DateTime<Utc>,
    pub notes: Option<String>,
}
```

Display name logic: `local_name.unwrap_or(&declared_name)`.

Stored as individual JSON files in `~/.config/krillnotes/contacts/<contact_id>.json`.

### Peer Registry Entry

```sql
CREATE TABLE IF NOT EXISTS sync_peers (
    peer_device_id   TEXT PRIMARY KEY,
    peer_identity_id TEXT NOT NULL,       -- public key; references contact record for display name
    last_sent_op     TEXT,               -- operation_id of last op sent to this peer
    last_received_op TEXT,               -- operation_id of last op received from this peer
    last_sync        TEXT                -- ISO 8601 timestamp of last exchange
);
```

Note: `peer_name` has been removed. The display name for a sync peer is resolved via the contact record matching `peer_identity_id`. This ensures a single source of truth for naming (see Design Amendment 1).

### .swarm Header

```rust
pub struct SwarmHeader {
    pub format_version: u32,
    pub mode: SwarmMode,                 // Invite | Accept | Snapshot | Delta
    pub workspace_id: Uuid,
    pub workspace_name: String,
    pub source_device_id: Uuid,
    pub source_identity: Ed25519PublicKey,
    pub source_display_name: String,
    pub created_at: DateTime<Utc>,

    // Mode-specific fields
    pub pairing_token: Option<[u8; 32]>,          // Invite, Accept
    pub offered_role: Option<Role>,                // Invite
    pub offered_scope: Option<Uuid>,               // Invite (subtree root, or None = workspace)
    pub inviter_fingerprint: Option<String>,        // Invite
    pub accepted_identity: Option<Ed25519PublicKey>, // Accept
    pub accepted_display_name: Option<String>,      // Accept
    pub accepted_fingerprint: Option<String>,        // Accept
    pub as_of_operation_id: Option<Uuid>,           // Snapshot
    pub since_operation_id: Option<Uuid>,           // Delta
    pub target_peer: Option<Uuid>,                  // Delta (device_id, optional)
    pub recipients: Option<Vec<RecipientEntry>>,    // Snapshot, Delta
    pub has_attachments: bool,                      // Snapshot, Delta
}

pub struct RecipientEntry {
    pub peer_id: String,                 // device_id or display identifier
    pub encrypted_key: Vec<u8>,          // AES-256 key wrapped via X25519
}
```

### Zip Archive Layout

```
invite.swarm:
├── header.json              (unencrypted, signed)
├── payload.json             (unencrypted, signed — SetPermission op with placeholder)
└── signature.bin            (Ed25519 signature over BLAKE3 manifest)

accept.swarm:
├── header.json
├── payload.json             (JoinWorkspace operation)
└── signature.bin

snapshot.swarm:
├── header.json              (unencrypted, includes recipients[])
├── payload.enc              (AES-256-GCM encrypted workspace.json)
└── signature.bin

delta.swarm:
├── header.json              (unencrypted, includes recipients[], since_operation_id)
├── payload.enc              (AES-256-GCM encrypted operations + manifest)
└── signature.bin
```

Note: Attachment support (an `attachments/` directory within the zip) is deferred to WP-10.

---

## Implementation Tasks

### A1: Contacts Address Book

- [ ] Define `Contact` struct with `declared_name` + `local_name` (optional override) and serialisation (JSON)
- [ ] Implement contacts directory management (`~/.config/krillnotes/contacts/`)
- [ ] CRUD: create, read, list, update, delete contacts
- [ ] Display name resolution: `local_name.unwrap_or(&declared_name)`
- [ ] Fingerprint generation: BLAKE3 hash of public key → BIP-39 4-word rendering
- [ ] Trust level enum and assignment logic
- [ ] Contact deduplication by public key: same key across workspaces = same contact record
- [ ] Transitive identity registration: auto-create contact when an operation arrives signed by an unknown identity (with `declared_name` from the operation or JoinWorkspace payload, trust level: TOFU)

### A2: Peer Registry (SQLite)

- [ ] Create `sync_peers` table in workspace database (no `peer_name` — resolved via contact record)
- [ ] CRUD operations on peer entries
- [ ] Peer display name resolution: look up `peer_identity_id` in contacts, apply `local_name.unwrap_or(&declared_name)`
- [ ] Marker update functions: `update_last_sent(peer_device_id, operation_id)`, `update_last_received(peer_device_id, operation_id)`
- [ ] Query: operations since `last_sent_op` for a given peer (feeds delta generation)
- [ ] Stale peer detection (configurable threshold)

### A3: .swarm Header Codec

- [ ] Define `SwarmHeader` struct with mode-specific optional fields
- [ ] Serialise to / deserialise from JSON
- [ ] Validation: required fields per mode, format_version check

### A4: Bundle-Level Signature

- [ ] BLAKE3 hash over all files in the zip archive (canonical order)
- [ ] Sign manifest hash with source identity's Ed25519 key
- [ ] Verify on ingest: recompute manifest hash, verify against signature and sender's known public key

### A5: Hybrid Encryption (Payload)

- [ ] Generate random AES-256-GCM symmetric key
- [ ] Encrypt payload bytes with symmetric key
- [ ] For each recipient: X25519 key agreement (convert Ed25519 to X25519), wrap symmetric key
- [ ] Populate `recipients[]` in header
- [ ] Decrypt path: find own entry in recipients, unwrap symmetric key, decrypt payload
- [ ] Handle single-recipient and multi-recipient cases

### A6: Invite Mode — Generate and Parse

- [ ] Generate pairing token (random 256-bit, CSPRNG)
- [ ] **Validate inviter's capability:** verify inviter's effective role on the target scope ≥ offered role (Design Amendment 2)
- [ ] Create SetPermission operation with placeholder `target_user_id`
- [ ] Sign the operation with inviter's identity key
- [ ] Package as invite .swarm (header + payload.json + signature)
- [ ] Parse: read header, extract pairing token, extract SetPermission, verify signature

### A7: Accept Mode — Generate and Parse

- [ ] User selects or creates identity for this workspace
- [ ] Create JoinWorkspace operation with public key and pairing token reference
- [ ] Sign with accepting identity's key
- [ ] Package as accept .swarm
- [ ] Parse: read header, extract JoinWorkspace, verify signature, verify pairing token matches

### A8: Invitation State Machine (Inviter Side)

- [ ] On receiving accept .swarm:
  - Verify pairing token matches outstanding invitation
  - Register new peer's public key in contacts address book (trust level: TOFU or as verified)
  - Resolve SetPermission placeholder → new peer's real identity
  - Add peer to workspace peer registry
  - Generate snapshot .swarm for new peer (→ A10)

### A9: Known Contact Invitation (Short Path)

- [ ] Select contact from address book
- [ ] **Validate inviter's capability:** verify effective role ≥ offered role on target scope
- [ ] Create SetPermission with contact's real identity (no placeholder)
- [ ] Generate snapshot .swarm encrypted for contact's public key (→ A10)
- [ ] No handshake — one exchange

### A10: Snapshot Bundle Generation

- [ ] Resolve current workspace state:
  - All notes with fields, tags, tree structure
  - All user scripts (source, enabled, load_order)
  - Resolved permission state (all SetPermission/RevokePermission applied)
  - **All known identities** with public keys and declared names (enables transitive identity propagation — see Design Amendment 1)
- [ ] Serialise as `workspace.json`
- [ ] Record `as_of_operation_id` (latest operation_id in the workspace)
- [ ] Encrypt payload for recipient(s) (→ A5)
- [ ] Package as snapshot .swarm

### A11: Snapshot Bundle Import

- [ ] Decrypt payload
- [ ] Verify bundle-level signature
- [ ] Parse `workspace.json`
- [ ] Create fresh local database (new random DB password, encrypt to identity)
- [ ] Import workspace state directly (no conflict resolution — snapshot is resolved state)
- [ ] Set peer tracking: `last_received_op = as_of_operation_id`
- [ ] Add sender to peer registry

### A12: Delta Bundle Generation

- [ ] Query operations since `last_sent_op` for target peer
- [ ] Collect operations in chronological (HLC) order
- [ ] Build manifest: list of operation_ids with BLAKE3 hashes
- [ ] Sign bundle (→ A4)
- [ ] Encrypt payload for recipient(s) (→ A5)
- [ ] Package as delta .swarm
- [ ] Update `last_sent_op` marker for target peer

### A13: Delta Bundle Ingest (Partial — Without Full Conflict Resolution)

- [ ] Decrypt payload
- [ ] Verify bundle-level signature
- [ ] Parse operations array
- [ ] For each operation:
  - Verify individual Ed25519 signature against author's known public key
  - **RBAC check: STUB (allow all) — real enforcement deferred to WP-B**
  - **Conflict resolution: STUB (apply unconditionally) — real resolution deferred to WP-C**
  - Apply operation to local database
  - Record in operation log with `synced = 1`
- [ ] Observe each operation's HLC timestamp (update local HLC clock state)
- [ ] Update peer tracking: `last_received_op`, `last_sync`

This partial ingest is sufficient for testing the full invitation and sync cycle between cooperative peers that don't create conflicts. The stubs will be replaced in WP-B and WP-C.

---

## Test Scenarios

### T1: Known Contact — Full Cycle

1. Alice creates workspace, becomes root owner
2. Alice already has Bob's public key in contacts (from a prior interaction — seed it manually for testing)
3. Alice invites Bob via known-contact path (A9)
4. Verify: snapshot .swarm produced, encrypted for Bob's key, contains full workspace state
5. Bob opens snapshot .swarm (A11)
6. Verify: Bob has identical workspace state, peer registry tracks Alice
7. Bob makes local edits
8. Bob generates delta .swarm for Alice (A12)
9. Alice applies delta (A13)
10. Verify: Alice sees Bob's edits, peer markers updated
11. Alice generates return delta for Bob
12. Both workspaces converged

### T2: Unknown Peer — Full Handshake

1. Alice creates workspace
2. Alice invites Carol (unknown) — generates invite .swarm (A6)
3. Carol opens invite .swarm, sees workspace name, role, inviter fingerprint
4. Carol selects an identity, generates accept .swarm (A7)
5. Alice opens accept .swarm (A8):
   - Pairing token verified
   - Carol added to contacts
   - SetPermission resolved
   - Carol added to peer registry
   - Snapshot .swarm generated for Carol
6. Carol opens snapshot, imports workspace (A11)
7. Delta exchange proceeds as in T1

### T3: Multi-Recipient Delta (Shared Folder Scenario)

1. Alice has workspace shared with Bob and Carol
2. Alice generates a delta .swarm with both Bob and Carol in recipients[]
3. Verify: payload encrypted once, two key wrappers in header
4. Bob decrypts with his key — success
5. Carol decrypts with her key — success
6. Dave (not a recipient) attempts to decrypt — fails

### T4: Round-Trip Integrity

1. Generate operations with various types (CreateNote, UpdateField, SetTags, MoveNote)
2. Bundle as delta .swarm
3. Parse the .swarm back
4. Verify: every operation matches exactly (field values, HLC timestamps, signatures all preserved)

### T5: Bundle Signature Tampering

1. Generate a valid delta .swarm
2. Modify one byte in `payload.enc`
3. Attempt to verify bundle signature — must fail
4. Modify `header.json` (change workspace_name)
5. Attempt to verify — must fail

### T6: Invitation Replay / Mismatch

1. Alice generates invite .swarm for workspace A
2. Carol generates accept .swarm referencing a different pairing token
3. Alice processes accept — must reject (pairing token mismatch)
4. Carol generates accept .swarm with correct token but for a different workspace_id
5. Alice processes — must reject

### T7: Stale Peer Detection

1. Alice syncs with Bob — `last_sync` updated
2. Time passes beyond configurable threshold
3. Query stale peers — Bob appears in results

### T8: Non-Owner Invitation (Design Amendment 2)

1. Alice (root owner) gives Bob Writer role on /Project Alpha
2. Bob invites Carol as Writer on /Project Alpha — succeeds (Writer can grant Writer)
3. Bob invites Dave as Reader on /Project Alpha — succeeds (Writer can grant Reader)
4. Bob attempts to invite Eve as Owner on /Project Alpha — **rejected** (Writer cannot grant Owner)
5. Carol (Writer, invited by Bob) invites Frank as Reader on /Project Alpha — succeeds (Writer can grant Reader, sub-delegation chain: Alice → Bob → Carol → Frank)
6. Alice revokes Bob's Writer role
7. Verify: Carol's, Dave's, and Frank's permissions all cascade to invalid (chain broken at Bob)

### T9: Writer Revocation Scope (Design Amendment 2)

1. Alice grants Bob Writer on /Project Alpha
2. Bob invites Carol as Writer (Bob is `granted_by`)
3. Alice invites Dave as Reader on /Project Alpha (Alice is `granted_by`)
4. Bob attempts to revoke Dave's Reader — **rejected** (Bob didn't issue Dave's grant and Bob is not an Owner)
5. Bob revokes Carol's Writer — succeeds (Bob issued Carol's grant)
6. Alice revokes Dave's Reader — succeeds (Alice is Owner)

### T10: Transitive Identity Propagation (Design Amendment 1)

1. Alice creates workspace, invites Bob (via server relay)
2. Bob invites Carol directly (Bob ↔ Carol exchange bundles, Alice doesn't)
3. Bob syncs with server; Carol's JoinWorkspace operation propagates
4. Server syncs with Alice; Alice receives Carol's JoinWorkspace
5. Verify: Alice's contacts now contain Carol with `declared_name` and TOFU trust level
6. Alice receives a delta containing an UpdateField signed by Carol
7. Verify: Alice can verify Carol's signature against the public key now in her contacts

### T11: Two-Layer Contact Naming (Design Amendment 1)

1. Bob's declared name is "Bob Chen"
2. Alice renames Bob to "Robert — Field Lead" in her local contacts (`local_name` override)
3. Verify: Alice's UI shows "Robert — Field Lead" everywhere Bob appears
4. Carol (who has not set a local override) sees "Bob Chen"
5. Bob's declared name is unchanged in the protocol — audit trails show "Bob Chen"

---

## Open Questions for WP-A

1. **Pairing token expiry.** Should invite .swarm bundles have a TTL? The spec doesn't define one, but a week-old invitation floating around on a USB drive is a reasonable concern. Options: embed an `expires_at` in the header, or leave it to operational practice.

2. ~~**Contact deduplication.**~~ **Resolved:** Same public key = same contact record, regardless of which workspace the interaction originated from. The workspace peer registry is the per-workspace binding; the contact is the cross-workspace identity. Implemented in A1.

3. **Snapshot size limits.** For large workspaces (thousands of notes, deep trees), the snapshot `workspace.json` could be substantial. Should there be a streaming/chunked approach, or is this a "cross that bridge when we hit it" concern?

4. **Delta generation with purged operations.** The spec addresses this in Section 5.6 (SA-006) — if the earliest unpurged operation is newer than `last_sent_op`, fall back to snapshot. Should this fallback logic be in WP-A or deferred?

5. **Header format_version strategy.** What does a version bump look like? Should the codec reject unknown versions, or attempt best-effort parsing? The spec says "forward compatibility" but doesn't specify the mechanism.

6. **Transitive identity trust propagation.** When Carol's identity arrives via delta sync (not direct exchange), she gets TOFU trust level. Should there be a mechanism for the person who originally verified Carol (Bob) to propagate his trust assessment? This is related to the vouching model in Section 15 of the spec but the mechanics for automatic propagation are unspecified.

---

# WP-B: RBAC Engine (Next Steps)

## Scope Summary

The RBAC engine is a standalone policy module: given a permission tree and an operation, return allow or deny. It does not depend on bundles or peers — it's a pure function over the permission state.

### Key Deliverables

- **Role definitions**: Owner, Writer, Reader, None — with the capability matrix from Section 16.1
- **Invitation capability matrix** (Design Amendment 2): Owner can grant up to Owner; Writer can grant up to Writer; Reader can grant Reader; None cannot grant
- **Permission storage**: `note_permissions` table (`note_id`, `user_id`, `role`)
- **Permission inheritance**: tree-walk from target note to workspace root, stopping at first explicit entry; fall back to workspace default role
- **Sub-delegation validation**: verify the chain of SetPermission operations — generalised beyond Owner chains to include Writer→Writer and Writer→Reader grants (Design Amendment 2). Detect broken chains when an intermediate grant is revoked.
- **SetPermission validation**: verify that the signer's effective role on the target scope ≥ the role being granted. This replaces the v0.7 spec's "Owner only" check.
- **Revocation validation**: Owner on a scope can revoke any grant within it. Non-Owners can revoke only grants where `granted_by` matches their identity.
- **Enforcement function**: `check_permission(identity, note_id, operation_type) → Allow | Deny` — this is the function WP-C calls during bundle application
- **Root owner rule for script operations**: CreateUserScript, UpdateUserScript, DeleteUserScript require root owner identity specifically (unchanged — script governance is not delegatable)
- **Permission + tree move interaction**: MoveNote requires write on both source parent and destination parent

### Why It's Independent

RBAC is testable with synthetic permission trees and identity/operation pairs. You build a tree in memory, assign permissions, and assert that the enforcement function returns the correct result for various (identity, note, operation_type) combinations. No bundles, no sync, no crypto needed — just the permission data model and the tree-walk algorithm.

### Test Focus

- Inheritance: writer on parent → writer on child (no explicit entry on child)
- Override: reader on workspace root, none on private subtree → access denied on private subtree
- Sub-delegation cascade: revoke Bob's writer → Carol's grant (from Bob) becomes invalid, and Dave's grant (from Carol) also cascades
- **Writer inviting Writer**: Bob (writer) signs SetPermission granting Carol writer on same scope → valid
- **Writer inviting Owner**: Bob (writer) signs SetPermission granting Carol owner → **rejected** (role exceeds capability)
- **Reader inviting Reader**: Carol (reader) signs SetPermission granting Dave reader → valid
- **Revocation scope**: Bob (writer, granted Carol) revokes Carol → valid. Bob attempts to revoke Dave (granted by Alice) → **rejected** (not Bob's grant and Bob is not Owner)
- Root owner rule: non-root identity signing CreateUserScript → reject
- Move permission: write on source parent but reader on destination → reject

---

# WP-C: Sync Engine (Next Steps)

## Scope Summary

The sync engine is the integration layer that ties WP-A and WP-B together with the existing local engine (WP1 + WP2). It replaces the stubs from WP-A task A13 with real RBAC enforcement and real conflict resolution.

### Key Deliverables

- **Inbound bundle application pipeline**: decrypt (WP-A) → verify bundle signature (WP-A) → for each operation: verify individual signature (WP4) → check RBAC (WP-B) → detect conflicts → resolve conflicts → apply to local database (WP1/2) → observe HLC (WP1) → update peer markers (WP-A)
- **Conflict resolution** (the logic from Section 7):
  - Field-level LWW for atomic types
  - Property-level LWW for UpdateNote
  - Set-level LWW for SetTags
  - Tree conflict detection (MoveNote to different parents) with user notification
  - Cycle detection for tree moves
  - Loud LWW for textarea fields (interim, pre-CRDT)
  - Delete-vs-edit default behaviour (delete wins, data preserved in op log)
- **Operation rejection logging**: operations that fail signature or RBAC go into a rejection log with reason, not silently dropped
- **Conflict UI hooks**: when a conflict is detected and resolved via LWW, emit an event or record that the UI can surface (especially for tree moves and textarea loud-LWW)

### Why It Depends on Both WP-A and WP-B

- Conflict resolution only arises during bundle application (WP-A provides the bundles)
- RBAC checking is step 4 in the pipeline (WP-B provides the enforcement function)
- The test harness requires two actual workspaces exchanging .swarm bundles and producing conflicting edits

### Test Focus

- **Field-level LWW**: Alice and Bob both edit field X on the same note. Later HLC wins. Loser preserved in op log.
- **Independent field edits**: Alice edits field X, Bob edits field Y on the same note. Both preserved — no conflict.
- **Delete vs. edit**: Alice deletes note, Bob edits it. Delete wins. Bob's edit preserved in op log but not in working state.
- **Tree move conflict**: Alice moves note to parent A, Bob moves same note to parent B. LWW applied, conflict flagged for user.
- **Cycle rejection**: Alice moves parent under its own child. Rejected.
- **RBAC rejection**: Bob (reader) sends an UpdateField operation. Rejected during bundle application. Logged.
- **Unauthorised script change**: non-root identity sends UpdateUserScript. Rejected.
- **Convergence**: after all exchanges complete, both workspaces have identical state regardless of application order.

---

*End of implementation roadmap — WP-A, WP-B, WP-C*
