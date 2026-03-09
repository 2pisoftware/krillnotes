# SWARM PROTOCOL — Trust, Permissions & Access Control

**Version 1.0 — March 2026**

*Companion to Swarm Protocol Unified Design v0.7*

This document is the authoritative specification for trust, identity verification, role-based access control, permission enforcement, and revocation in the Swarm protocol. It consolidates and supersedes §15–18 of the Unified Design Specification v0.7 and incorporates the Design Addendum amendments.

The Swarm sync design (§1–14, §19–27) references this document for all permission-related behaviour.

---

## 1. Trust & Verification

In a system without a central authority, trust must be established through external channels. Krill Notes supports a layered trust model that lets users choose the appropriate level of verification for each context.

### 1.1 Trust Levels

| Level | Description | Assurance |
|---|---|---|
| **Verified in person** | Public keys compared via QR code scan or side-by-side display during a physical meeting. | Highest |
| **Code verified** | A short verification code (derived from the public key) is compared over a trusted out-of-band channel such as a phone call or video chat. | Strong |
| **Vouched** | A verified peer vouches for a new participant by co-signing their invitation. Displayed as "Carol, vouched for by Bob". | Transitive |
| **TOFU (Trust On First Use)** | The identity is accepted at first contact without independent verification. Also assigned to transitive identities encountered via delta sync. | Minimum |

### 1.2 Key Fingerprints

For verification methods that require human comparison, public keys are displayed as short fingerprints. A fingerprint is a BLAKE3 hash of the public key rendered as a human-friendly format:

- **Word format (recommended):** four words from a fixed 2048-word BIP-39 dictionary, e.g., `ocean-maple-thunder-seven`
- **Hex format (fallback):** short hex blocks, e.g., `A4 3B 7F 12 D8 91`
- **QR code:** encodes the full public key plus display name for scanning

Word-based fingerprints are easy to read aloud over a phone call and easy to compare visually during in-person verification.

### 1.3 Trust Level Assignment

The trust level of a new participant depends on how the invitation was verified:

- In-person QR scan of fingerprints during the handshake yields "Verified in person."
- Verbal confirmation of fingerprints over a phone call yields "Code verified."
- A vouched introduction from an existing peer yields "Vouched by [peer]."
- An email invitation without separate verification yields "TOFU."
- A transitive identity encountered via delta sync (not direct exchange) yields "TOFU."

Any peer can upgrade a contact's trust level at any time via a separate verification step. Known contacts invited from the address book retain their existing trust level from previous workspace interactions.

---

## 2. Roles & Capabilities

Krill Notes implements role-based access control at the note level, with permission inheritance through the tree hierarchy. This serves two purposes: it controls who can do what, and it reduces sync conflicts by limiting the number of writers on any given subtree.

### 2.1 Role Definitions

| Role | Data Capabilities | Can Invite | Max Grantable Role | Can Revoke |
|---|---|---|---|---|
| **Owner** | Read, write, create, delete, move within subtree | Yes | Owner, Writer, Reader | Any grant on their subtree |
| **Writer** | Read, write, create, move within subtree | Yes | Writer, Reader | Only grants they personally issued |
| **Reader** | View notes within subtree | Yes | Reader | Only grants they personally issued |
| **None** | No access (explicit denial, overrides inheritance) | No | — | — |

**Role ordering for comparison:** Owner > Writer > Reader > None. This ordering is used for invitation capability checks (Section 4) and SetPermission verification (Section 5).

### 2.2 Key Constraints

- Writers cannot change permissions or delete the subtree root.
- The None role is an explicit denial that overrides any inherited permission, enabling private subtrees within an otherwise shared workspace.
- **Script governance is not delegatable:** CreateUserScript, UpdateUserScript, and DeleteUserScript require the root owner's signature specifically, regardless of other permissions. This is the only capability reserved exclusively to the root owner.

---

## 3. Permission Inheritance

### 3.1 Tree-Walk Resolution

Permissions are set on individual notes and inherited by all descendants. When checking access for a note, the system walks up the tree from the note to the workspace root, stopping at the first explicit permission entry for the requesting user. If no explicit permission is found, the workspace default role applies.

Example workspace structure:

```
Workspace Root (default: reader)
├─ Company Wiki (bob: writer, carol: writer)
│  ├─ Onboarding Guide
│  └─ API Docs
├─ Project Alpha (bob: writer, dave: writer)
│  ├─ Sprint Notes
│  └─ Architecture Decisions
└─ Sarah's Drafts (sarah: owner, default: none)
```

In this example, everyone can read the Company Wiki, but only Bob and Carol can edit it (they have explicit Writer entries). Sarah's Drafts are invisible to other users because the default for that subtree is "none" (no access).

### 3.2 Permission Storage

```sql
note_permissions (
    note_id  TEXT NOT NULL REFERENCES notes(id),
    user_id  TEXT NOT NULL,
    role     TEXT NOT NULL CHECK(role IN ('owner','writer','reader','none')),
    PRIMARY KEY (note_id, user_id)
)
```

### 3.3 Tree Move Interaction

Moving a note between subtrees requires write permission on both the source parent (to remove the child) and the destination parent (to add the child). If the user lacks permission on either, the move is rejected.

When a note is moved to a new subtree, it inherits the permissions of its new parent. This is an immediate consequence of the permission inheritance model and requires no special handling.

---

## 4. Invitation & Sub-Delegation

### 4.1 Role-Capped Invitation

**Principle:** You can grant permissions up to but not exceeding your own effective role, within your own permitted scope.

This means a Writer in the field can bring in a colleague as a Writer or Reader without routing the request back to the Owner. A Reader can share visibility but not grant modification rights. No identity can escalate permissions beyond what they hold.

**Security invariant:** The overall permission surface area is bounded by the Owner's original grants. A Writer inviting another Writer creates no new capability that didn't already exist in the subtree.

### 4.2 Sub-Delegation Chains

Permission grants form verifiable chains. Each SetPermission operation is signed by the granting identity, and any receiving peer can walk the chain backwards to verify that every link was valid at the time of signing:

```
Alice (root owner) → "Bob is writer on /Project Alpha"
Bob (writer)       → "Carol is writer on /Project Alpha"
Carol (writer)     → "Dave is reader on /Project Alpha"
```

Every link is valid because the granter held at least the role they granted. The chain is verifiable locally by any peer without contacting a central authority.

### 4.3 Chain Cascade on Revocation

If Alice revokes Bob's Writer role, Carol's permissions (granted by Bob) automatically become invalid, and Dave's permissions (granted by Carol) also cascade to invalid. The chain is broken at Bob and everything downstream collapses.

This cascade is evaluated locally by every peer during bundle application. When a RevokePermission operation is applied, the receiving peer re-evaluates all permission chains that pass through the revoked identity and marks downstream grants as invalid.

### 4.4 Invitation Flow Integration

The invitation mechanics (described in the Swarm sync design, §12) are structurally unchanged by the generalised model. Both the known-contact path (1 exchange) and unknown-peer path (3 exchanges) work identically regardless of the inviter's role. The differences are:

- The SetPermission payload carries the role the inviter is authorised to grant (not necessarily Owner).
- The UI for the invitation form restricts the role picker to roles at or below the inviter's effective role on the target scope.
- Receiving peers verify the SetPermission against the generalised rule (§5.2) instead of checking for Owner specifically.

---

## 5. Permission Enforcement

Every operation in Krill Notes is cryptographically signed by the author's private key. Verification is performed locally on every device when applying incoming operations from a .swarm or .cloud bundle. There is no server to enforce permissions — every device is its own enforcer.

### 5.1 Verification Pipeline

When an operation arrives in an inbound bundle, the receiving device:

1. *(For .swarm only)* Decrypts the payload using the recipient's private key and the per-recipient AES key wrapper.
2. Verifies the bundle-level signature against the sender's known public key. *(For .cloud, also verifies against the stored trusted fingerprint.)*
3. Verifies each operation's individual Ed25519 signature against the author's known public key.
4. Resolves the author's effective role on the target note by walking the permission tree (§3.1).
5. Checks that the role permits the operation type (see §5.2).
6. If all checks pass, the operation is applied. Otherwise, it is rejected and logged.

### 5.2 Operation-Type Permission Matrix

| Operation Type | Owner | Writer | Reader | None |
|---|---|---|---|---|
| CreateNote | ✓ | ✓ | ✗ | ✗ |
| UpdateNote (title, properties) | ✓ | ✓ | ✗ | ✗ |
| UpdateField | ✓ | ✓ | ✗ | ✗ |
| DeleteNote | ✓ | ✗ (not subtree root) | ✗ | ✗ |
| MoveNote | ✓ (both parents) | ✓ (both parents) | ✗ | ✗ |
| SetTags | ✓ | ✓ | ✗ | ✗ |
| AddAttachment / RemoveAttachment | ✓ | ✓ | ✗ | ✗ |
| **SetPermission** | **✓ (any role)** | **✓ (≤ writer)** | **✓ (≤ reader)** | ✗ |
| **RevokePermission** | **✓ (any grant)** | **✓ (own grants)** | **✓ (own grants)** | ✗ |
| CreateUserScript / UpdateUserScript / DeleteUserScript | ✓ (root owner only) | ✗ | ✗ | ✗ |

**SetPermission verification rule:** The signer's effective role on the target scope must equal or exceed the role being granted. This replaces the v0.7 spec's "Owner only" check.

**RevokePermission verification rule:** An Owner on a scope can revoke any grant within that scope. A non-Owner can revoke only grants they personally issued (the `granted_by` field on the SetPermission operation must match their identity).

### 5.3 Modified Client Threat Model

The security assessment analysed four sub-threats:

| Threat | Risk | Mitigation |
|---|---|---|
| **A:** Modified client ignores RBAC locally | Contained to their device | Cannot be prevented; physical access reality |
| **B:** Modified client generates unauthorised operations | Primary threat | Every receiving peer validates signature + RBAC on ingest; unauthorised ops rejected |
| **C:** Two colluding modified clients | Contained between them | Cannot infect honest peers; honest peer perimeter is the security boundary |
| **D:** Authorised liar (valid access, false data) | Human process problem | Non-repudiation: every entry permanently signed with author's identity key |

---

## 6. Revocation & Edge Cases

### 6.1 Revocation Is Eventually Consistent

In a decentralised system, revocation cannot be instantaneous. When Alice revokes Bob's write access, the RevokePermission operation must propagate to all peers via .swarm bundles. Until a peer receives the revocation, it may accept operations from Bob that were generated after the revocation.

> **Security Finding SA-002 (High): Revocation Propagation Gap**
>
> During the propagation window (hours over LoRa/sneakernet), peers continue accepting operations from revoked users. The original design specified retroactive rollback, which creates accountability problems — data that informed decisions disappears.
>
> **Resolution:** Implement a quarantine model for the commercial product. Operations from revoked users that post-date the revocation are flagged as "contested" rather than removed. Three operation states: valid, rejected, contested. See the Swarm sync design (§21) for the full data preservation specification.

### 6.2 Transport Encryption and Revocation

Transport encryption provides defence in depth. When a user's access is revoked:

1. The RevokePermission operation propagates to all peers via bundles.
2. Future .swarm bundles omit the revoked user from the recipients list.
3. The revoked user cannot decrypt any new .swarm bundles, even if they intercept them from a shared folder.
4. The revoked user's local database remains accessible (their SQLCipher password still works), but they are cryptographically cut off from all future updates.

Permission enforcement rejects unauthorised operations at the application level; transport encryption prevents unauthorised access at the file level.

### 6.3 Revocation Rights

Revocation capability is asymmetric to prevent lateral interference:

- **Owner on a scope:** Can revoke any grant within that scope, regardless of who issued it. This is the administrative override.
- **Non-Owner (Writer, Reader):** Can revoke only grants they personally issued. The `granted_by` field on the SetPermission operation must match their identity. This prevents a Writer from revoking permissions granted by someone at the same or higher level.

Example: Bob (Writer) can revoke Carol's access if Bob invited Carol, but Bob cannot revoke Dave's access if Alice invited Dave — even if Bob and Dave are in the same subtree at the same level.

### 6.4 Chain Cascade Mechanics

When a revocation breaks a sub-delegation chain (§4.3), the cascade is evaluated as follows:

1. The RevokePermission operation is applied, removing the target's explicit permission entry.
2. All SetPermission operations where `granted_by` matches the revoked identity are identified.
3. Each downstream grant is evaluated: does the granter still hold a sufficient role? If not, the grant is invalidated.
4. This evaluation recurses: invalidating Carol's grant triggers re-evaluation of grants Carol issued.
5. Invalidated grants are not deleted from the operation log (they are preserved for audit). Their effect on the working permission state is removed.

### 6.5 Delete vs. Edit Conflict

If device A deletes a note while device B edits it, the delete wins by default. Edits to a deleted note are discarded during bundle application. The deleted note's data is retained in the operation log for audit and potential restoration.

The delete-vs-edit behaviour is configurable per schema (see Swarm sync design, §21.2). An AIIMS Situation Report schema can declare `on_delete_conflict: preserve` while a personal recipe schema keeps `delete_wins`.

### 6.6 Tree Move Conflicts

If two devices move the same note to different parents, the system applies LWW as the working state but flags the conflict for user review. Cycle detection is performed before applying any tree move — a move that would create a cycle is rejected.

---

## 7. The Root Owner

The root owner is the identity that created the workspace. They hold the highest level of authority and are the only identity with certain exclusive capabilities.

### 7.1 Root-Owner-Only Privileges

- Export (any format: .krillnotes, .cloud, .swarm)
- Script governance (create, modify, enable, disable, reorder, delete Rhai scripts)
- Root RBAC changes (modify permission grants on the workspace root node)

All other capabilities — including inviting peers, granting permissions, and revoking grants — are delegatable according to the role-capped model in §4.

### 7.2 Root Owner and the Swarm Server

> **Security Finding SA-004 (Critical): Root Owner Single Point of Failure**
>
> The root owner identity is bound to a single Ed25519 keypair. If the root owner's device is destroyed or the person rotates off shift, workspace schemas are frozen permanently.
>
> **Resolution:** The Swarm Server holds the root owner keypair in an HSM. Shift rotations have no impact because the root identity is institutional, not personal. For open-source KrillNotes, multi-signature ownership or a documented break-glass procedure should be considered.

---

*End of Trust, Permissions & Access Control Specification*

*Swarm Protocol — RBAC Specification v1.0*
