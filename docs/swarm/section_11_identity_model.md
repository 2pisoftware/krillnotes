# 11. Identity Model

KrillNotes uses per-workspace cryptographic identities. There is no global account, no central identity provider, and no linkability between a user's identities across different workspaces unless the user explicitly chooses to reveal the connection.

---

## 11.1 Core Principle: Multiple Independent Identities

A single user may maintain multiple completely independent identities — for example, a work persona, a personal persona, and a club or community persona. Each identity is a self-contained Ed25519 keypair stored in its own encrypted identity file. There is no technical mechanism to correlate identities, preserving privacy across contexts.

Examples:
- `"Carsten @ 2pi"` — company workspace identity
- `"Carsten K"` — personal workspace identity
- `"Treasurer, Canberra RC"` — club workspace identity

Each identity has its own keypair and its own passphrase. Identities are selected at app launch; the user works within one active identity at a time.

---

## 11.2 Identity File Structure

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

---

## 11.3 Passphrase-Protected Identity Unlock

The identity passphrase is the single credential that unlocks everything for that identity. No separate database password is ever presented to the user.

| Passphrase use | Description |
|---|---|
| Identity unlock | `Argon2id(passphrase, salt)` → 32-byte key → decrypt Ed25519 seed |
| Signing operations | Ed25519 private key derived from decrypted seed |
| DB password decrypt | X25519 key (converted from Ed25519 seed) decrypts the local DB password blob |
| Session lock | Seed wiped from memory on idle timeout or explicit lock |

The SQLite database password is a randomly-generated 32-byte value created at workspace initialisation. It is encrypted to the identity's public key and stored in `settings.json`. It is never shown to the user and never leaves the device. When the identity is unlocked, the DB password is silently decrypted and used to open SQLCipher — the user experiences a single passphrase prompt that opens everything.

---

## 11.4 Settings File — Workspace Registry

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

---

## 11.5 Application Session Flow

### First launch — new identity
1. User chooses a display name for the identity.
2. App generates a fresh Ed25519 keypair.
3. User sets a passphrase; Argon2id derives the encryption key; private key seed is encrypted and written to the identity file.
4. On first workspace creation, a random 32-byte DB password is generated, SQLCipher is opened, and the DB password is encrypted to the identity public key and stored in `settings.json`.

### Subsequent launches
1. Identity picker displays all known identities by display name (last-used identity pre-selected).
2. User selects an identity and enters their passphrase.
3. Argon2id derives the key; private key seed is decrypted and held in a protected memory allocation.
4. For each workspace bound to this identity, the DB password blob is decrypted and SQLCipher is opened silently.
5. The workspace list for this identity is displayed. No further prompts.

### Switching identity within the app
1. User selects "Switch Identity" from the app menu.
2. Active identity seed is wiped from memory; DB connections are closed.
3. Identity picker is displayed. Workspaces from the previous identity disappear.
4. User selects a different identity, enters its passphrase, and the new workspace list appears.
5. No application restart required.

### Passphrase change
1. User unlocks with the current passphrase.
2. A new Argon2id salt is generated; the same Ed25519 seed is re-encrypted under the new derived key.
3. DB password blobs in `settings.json` are unaffected — they are encrypted to the keypair, not to the passphrase.

---

## 11.6 Multi-Device — Same Identity on Multiple Devices

A user may install the same identity on multiple devices (e.g., a desktop and a mobile). The identity keypair is identical on both devices, making the user cryptographically the same person. However, each device remains an independent sync peer because the `device_id` — derived from hardware — is unique per device.

| Property | Desktop | Mobile |
|---|---|---|
| Identity keypair | Same Ed25519 seed | Same Ed25519 seed |
| Identity passphrase | Same passphrase | Same passphrase (same KDF salt) |
| `device_id` | `MAC-hash-A` | `MAC-hash-B` — independent peer |
| SQLite DB | Independent local DB | Independent local DB |
| DB password | Own random password | Own random password — never shared |

The DB password is entirely local to each device. Mobile generates its own random DB password on workspace initialisation; it is never derived from, transmitted from, or coordinated with the desktop. Workspace data reaches mobile exclusively via `.swarm` sync bundles, exactly as it would from any other peer.

### Identity export and import (`.krillid`)

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

### Multi-device setup flow
1. Source device: Settings → Export Identity → saves `<display-name>.krillid`.
2. Transfer `.krillid` to target device (AirDrop, cable, email — user's choice).
3. Target device: Import Identity → enter same passphrase → identity installed.
4. Target device joins workspace: source device sends a `.swarm` snapshot encrypted to the shared public key.
5. Target device decrypts snapshot with shared private key, creates local DB with its own random password, imports workspace state.
6. Ongoing sync proceeds via delta `.swarm` bundles — target device is now a standard peer.

---

## 11.7 Device ID vs. Identity

KrillNotes maintains two independent axes of identification on every operation:

| Axis | Purpose |
|---|---|
| `device_id` | Identifies the physical machine. Used for sync logistics — tracking which operations each device has seen. Derived from hardware (MAC address hash). Stable across workspaces and identities. |
| identity (keypair) | Identifies the author. Used for RBAC, audit trails, and cryptographic verification. Scoped to a single workspace. One device may hold multiple identities; one identity may operate from multiple devices. |

---

## 11.8 Identity Recovery

If a device is lost, the private key is lost on that device. Recovery options:

- **Re-invitation:** The workspace owner issues a new invitation. The user imports their `.krillid` backup on a new device and rejoins. The old device's peer entry can be revoked.
- **Backup:** The `.krillid` file can be backed up to any secure location. Restoring it on a new device reinstates the identity with the same passphrase.
- **Recovery phrase:** The private key seed can be encoded as a mnemonic word list (similar to a cryptocurrency seed phrase) for offline paper backup.

---

## 11.9 Future: OS Keychain Integration (Commercial)

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