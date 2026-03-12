# Peer Contact Manager — Design Spec
*Issue #90 | Date: 2026-03-10*

---

## Overview

Krillnotes needs a contact management system that spans three layers:

1. **App-wide (per-identity):** An encrypted address book of known peers, owned by a specific local identity
2. **Workspace-level:** The list of sync peers for a given workspace, resolved against the contact book
3. **Invite flow:** The mechanism for bringing new peers into a workspace and discovering new contacts

This spec covers all three layers, split into three independent implementation phases.

---

## Existing Foundation

The following is already implemented and must not be replaced, only extended:

| Component | Location | Purpose |
|---|---|---|
| `ContactManager` | `krillnotes-core/src/core/contact.rs` | CRUD for contact JSON files |
| `Contact` | `contact.rs` | Struct with trust level, fingerprint, declared/local name |
| `TrustLevel` | `contact.rs` | `Tofu`, `CodeVerified`, `Vouched`, `VerifiedInPerson` |
| `PeerRegistry` | `krillnotes-core/src/core/peer_registry.rs` | Per-workspace `sync_peers` table |
| `IdentityManager` | `krillnotes-core/src/core/identity.rs` | Identity CRUD, unlock, passphrase |
| `UnlockedIdentity` | `identity.rs` | In-memory decrypted identity |
| `generate_fingerprint` | `contact.rs` | BLAKE3 → 4 BIP-39 words |
| `resolve_identity_name` | `src-tauri/src/lib.rs` | Name resolution: local → contact → fingerprint |

---

## Design Principles

- **No contact exists outside an identity or workspace.** Contacts are owned by one identity. The same real-world peer can appear as independent contacts under multiple local identities — each copy is fully managed by its owner identity.
- **Contacts are encrypted at rest.** Readable only when the owning identity is unlocked.
- **Trust is explicit.** `VerifiedInPerson` (highest trust) can only be set after fingerprint verification in the UI.
- **camelCase at the Tauri boundary.** All structs crossing Rust → TypeScript use `#[serde(rename_all = "camelCase")]`. Enum `rename_all` renames variants only — flat structs are preferred at IPC boundaries.

---

## Phase A — Contact Book UI

### A1. Storage Restructuring

**New location:**
```
~/.config/krillnotes/
└── identities/
    └── <identity_uuid>/
        └── contacts/
            └── <contact_uuid>.json    ← encrypted blob
```

**On-disk format** (same pattern as `EncryptedKey` in `identity.rs`):
```json
{ "ciphertext": "base64...", "nonce": "base64..." }
```

**Migration:** Existing contacts in `~/.config/krillnotes/contacts/` are orphaned with no recoverable identity binding. They are left in place and ignored (not deleted, not migrated). This is acceptable — contacts are not yet surfaced in any UI.

### A2. Encryption Model

Contacts are encrypted with a key derived deterministically from the identity seed:

```
contacts_key = HKDF-SHA256(ikm: identity_seed_bytes, info: b"krillnotes-contacts-v1")
```

**Unlock flow:**
1. User unlocks identity → `UnlockedIdentity` with `signing_key` in memory
2. `identity.contacts_key()` derives `[u8; 32]` via HKDF from the signing key seed
3. `ContactManager::for_identity(contacts_dir, contacts_key)` created → decrypts and caches all contacts into `HashMap<Uuid, Contact>` in memory
4. Manager stored in `AppState.contact_managers: HashMap<Uuid, ContactManager>`
5. On lock → `UnlockedIdentity` dropped, `ContactManager` dropped from map (memory cleared)

**`UnlockedIdentity`** gains:
```rust
pub fn contacts_key(&self) -> [u8; 32] {
    // HKDF-SHA256 from signing_key.as_bytes() with info = b"krillnotes-contacts-v1"
}
```

**`ContactManager`** changes:
- Constructor changes to `for_identity(contacts_dir: PathBuf, key: [u8; 32])`
- Holds `encryption_key: [u8; 32]` and `cache: HashMap<Uuid, Contact>`
- All reads serve from cache; all writes encrypt to disk and update cache

### A3. AppState Change

```rust
// Before:
contact_manager: Arc<Mutex<ContactManager>>,

// After:
contact_managers: Arc<Mutex<HashMap<Uuid, ContactManager>>>,
```

### A4. Tauri Commands

All commands require the owning identity to be unlocked (otherwise return an error).

| Command | Parameters | Returns |
|---|---|---|
| `list_contacts` | `identity_uuid: String` | `Vec<ContactInfo>` |
| `get_contact` | `identity_uuid: String`, `contact_id: String` | `ContactInfo` |
| `create_contact` | `identity_uuid: String`, `declared_name: String`, `public_key: String`, `trust_level: String` | `ContactInfo` |
| `update_contact` | `identity_uuid: String`, `contact_id: String`, `local_name: Option<String>`, `notes: Option<String>`, `trust_level: String` | `ContactInfo` |
| `delete_contact` | `identity_uuid: String`, `contact_id: String` | `()` |
| `get_fingerprint` | `public_key: String` | `String` |

**`ContactInfo` struct** (Rust → TS, camelCase):
```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactInfo {
    pub contact_id: String,
    pub declared_name: String,
    pub local_name: Option<String>,
    pub public_key: String,
    pub fingerprint: String,
    pub trust_level: String,   // serialized as camelCase variant name
    pub first_seen: String,    // ISO 8601
    pub notes: Option<String>,
}
```

`get_fingerprint` is stateless — pure derivation, no identity required.

### A5. UI Components

**Entry point:** Identity manager dialog → each identity row gets a **"Contacts (n)"** button (n = contact count, only visible when identity is unlocked).

**`ContactBookDialog`**
- Header: identity display name + "Contacts"
- Search bar (filters by name or public key prefix)
- Contact list rows: display name (local override if set, else declared name) · fingerprint (4 words, muted) · trust level badge (colour-coded)
- **"Add Contact"** button → `AddContactDialog`
- Click a row → `EditContactDialog`

**`AddContactDialog`**
1. `Name` text field
2. `Public Key` text field (paste) → live fingerprint preview below as key is entered
3. `Trust Level` selector: TOFU / Code Verified / Vouched / Verified In Person
4. If `Verified In Person` selected: fingerprint verification step slides in:
   - Display the 4 BIP-39 words prominently
   - Label: *"Ask your contact to read their fingerprint aloud. Does it match?"*
   - Checkbox: *"Yes, the fingerprint matches"* — required to enable Save at this trust level
5. Save / Cancel

**`EditContactDialog`**
- Pre-populated with existing contact data
- Additional `Local Name` field (override shown in UI, never propagated)
- `Notes` textarea
- Trust level selector (same fingerprint gate for `VerifiedInPerson`)
- Delete button with confirmation prompt

---

## Phase B — Workspace Peers UI

*(Planned, not yet designed in detail)*

- New "Peers" section in workspace settings or info panel
- Lists `sync_peers` for the current workspace
- Resolves peer names via `ContactManager` → local identity → fingerprint fallback (existing `resolve_identity_name`)
- Shows: display name, fingerprint, trust level (if in contacts), last sync timestamp
- **"Add to contacts"** action on peers not yet in the contact book (pre-fills name from declared name in their signed operations)

---

## Phase C — Invite Flow

### File Format Clarification

| File | Purpose |
|------|---------|
| `.swarmid` | Export **your own identity** to another device (private key transfer, e.g. desktop → mobile). Contains secret material — never shared with peers. |
| `.swarm` | **Peer-to-peer** public information exchange. Contains only public keys, signatures, and metadata. Used for invites and responses. |

### Flow Overview

Invites are **multi-use**: the same invite `.swarm` file can be posted publicly (e.g. a forum) and responded to by many people. The inviter reviews and accepts each responder individually, case-by-case.

```
Inviter                              Invitee
   |                                    |
   |-- invite.swarm ------------------> |  (delivered out-of-band: email,
   |   • invite_id (UUID)               |   chat, forum post, USB, etc.)
   |   • workspace_id + display name    |
   |   • inviter public key             |
   |   • inviter declared name          |
   |   • inviter signature over above   |  invitee verifies fingerprint
   |   • expires_at (optional)          |  out-of-band (call, in person)
   |                                    |
   |<-- response.swarm ---------------- |
   |   • invite_id (references invite)  |
   |   • invitee public key             |
   |   • invitee declared name          |
   |   • invitee signature over above   |
   |                                    |
   inviter verifies fingerprint         |
   → sets trust level                   |
   → adds to contact book               |
   → adds as workspace peer             |
```

### C1. Invite Storage

Invites are stored per-identity alongside contacts:

```
~/.config/krillnotes/
└── identities/
    └── <identity_uuid>/
        ├── contacts/
        │   └── <contact_uuid>.json
        └── invites/
            └── <invite_uuid>.json    ← plaintext (public data only)
```

Each invite record:
```json
{
  "invite_id": "uuid",
  "workspace_id": "uuid",
  "workspace_name": "string",
  "created_at": "ISO 8601",
  "expires_at": "ISO 8601 | null",
  "revoked": false,
  "use_count": 0
}
```

### C2. .swarm File Format

**Invite file** (`invite_<short_id>.swarm`):
```json
{
  "type": "krillnotes-invite-v1",
  "invite_id": "uuid",
  "workspace_id": "uuid",
  "workspace_name": "string",
  "workspace_description": "string | null",
  "workspace_author_name": "string | null",
  "workspace_author_org": "string | null",
  "workspace_homepage_url": "string | null",
  "workspace_license": "string | null",
  "workspace_language": "string | null",
  "workspace_tags": ["string"],
  "inviter_public_key": "base64",
  "inviter_declared_name": "string",
  "expires_at": "ISO 8601 | null",
  "signature": "base64"
}
```
Workspace metadata fields are optional (omitted if not set). They are read from `WorkspaceMetadata` via `get_workspace_metadata()` at invite creation time and embedded as-is — they are informational only and not verified by the invitee.

Signature covers all fields except `signature` itself (canonical JSON, sorted keys).

**Response file** (`response_<short_id>.swarm`):
```json
{
  "type": "krillnotes-invite-response-v1",
  "invite_id": "uuid",
  "invitee_public_key": "base64",
  "invitee_declared_name": "string",
  "signature": "base64"
}
```
Signature covers all fields except `signature` itself. Inviter verifies it against `invitee_public_key`.

### C3. Tauri Commands

| Command | Parameters | Returns |
|---------|-----------|---------|
| `create_invite` | `identity_uuid`, `workspace_id`, `expires_in_days: Option<u32>` | `InviteInfo` + writes `.swarm` file to chosen path |
| `list_invites` | `identity_uuid` | `Vec<InviteInfo>` |
| `revoke_invite` | `identity_uuid`, `invite_id` | `()` |
| `import_invite_response` | `identity_uuid`, `path: String` | `PendingPeer` (parsed response, not yet accepted) |
| `accept_peer` | `identity_uuid`, `workspace_id`, `invitee_public_key`, `declared_name`, `trust_level`, `local_name: Option<String>` | `ContactInfo` |

**`InviteInfo`** (Rust → TS, camelCase):
```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InviteInfo {
    pub invite_id: String,
    pub workspace_id: String,
    pub workspace_name: String,
    pub created_at: String,
    pub expires_at: Option<String>,
    pub revoked: bool,
    pub use_count: u32,
}
```

**`PendingPeer`** (Rust → TS, camelCase):
```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingPeer {
    pub invite_id: String,
    pub invitee_public_key: String,
    pub invitee_declared_name: String,
    pub fingerprint: String,   // 4 BIP-39 words, derived server-side
}
```

### C4. UI Components

**Entry point:** Workspace peers panel (Phase B) → **"Create Invite"** button.

**`InviteManagerDialog`**
- Header: workspace name + "Invites"
- List of open invites, each row showing:
  - Creation date · expiry (or "No expiry") · use count · Revoke button
- **"Create Invite"** button → `CreateInviteDialog`
- **"Import Response"** button → file picker → opens `AcceptPeerDialog`

**`CreateInviteDialog`**
1. Expiry selector: "No expiry" / "7 days" / "30 days" / "Custom (days)"
2. Preview of what the invite file will contain (workspace name, your display name)
3. **"Create & Save"** → file save dialog → writes `invite_<short_id>.swarm`

**`AcceptPeerDialog`** (opened after importing a response `.swarm`)
- Shows parsed peer info: declared name, fingerprint (4 BIP-39 words, prominent)
- Label: *"Ask this peer to read their fingerprint aloud. Does it match?"*
- Checkbox: *"Yes, the fingerprint matches"* — required to enable Accept
- Trust level selector (same options as Phase A contact manager)
- Optional **Local Name** override
- **Accept** / **Reject** buttons
- Accept → creates contact + adds as workspace peer in one operation

**`ImportInviteDialog`** (opened after receiving an invite `.swarm` as an invitee)
- Displays workspace info from the invite file:
  - Workspace name (always shown)
  - Description, author name/org, homepage URL, license, language, tags (each shown only if present)
- Displays inviter info: declared name, fingerprint (4 BIP-39 words, prominent)
- Label: *"Verify the fingerprint with the inviter before accepting."*
- Checkbox: *"Yes, the fingerprint matches"* — required to enable Respond
- **Respond** → generates `response_<short_id>.swarm` and opens a file save dialog
- **Reject** → discards, no file written

### C5. Validation Rules

- Expired invites: `import_invite_response` rejects response files that reference a revoked or expired invite (checked by `expires_at` and `revoked` flag)
- Signature verification: both invite and response signatures are verified before any UI is shown
- Duplicate detection: if `invitee_public_key` already exists in contacts, `AcceptPeerDialog` shows a notice and pre-fills from existing contact data

---

## Out of Scope (All Phases)

- In-band invite delivery (direct network transport) — `.swarm` files are transferred manually for now
- `.swarmid` contact import — `.swarmid` is for identity migration between devices, not peer discovery
- Contact sync across devices
- Vouched trust chain UI (vouch chains are stored but not surfaced in UI yet)
- Anonymous read-only workspace peers (`.cloud` broadcasts)
