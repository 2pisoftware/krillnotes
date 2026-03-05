# Security Policy

## Reporting a Vulnerability

If you discover a security vulnerability in Krill Notes, **please do not
open a public GitHub issue.**

Instead, report it by emailing:

    security@2pisoftware.com

Please include:

- A description of the vulnerability
- Steps to reproduce
- Potential impact
- Suggested fix (if you have one)

We will acknowledge your report within 48 hours and aim to provide a
fix or mitigation within 7 days for critical issues.

## Scope

The following areas are in scope for security reports:

- Cryptographic implementation (identity, signing, encryption)
- Sync protocol vulnerabilities (.swarm bundle handling)
- RBAC enforcement bypass
- Local database encryption (SQLCipher usage)
- Attachment encryption (ChaCha20-Poly1305)
- Script engine sandboxing (Rhai)
- Any vulnerability that could lead to data loss or unauthorised access

## Supported Versions

Security fixes are provided for the latest release only. We recommend
always running the most recent version.

## Recognition

We gratefully acknowledge security researchers who responsibly disclose
vulnerabilities. With your permission, we will credit you in the
release notes for the fix.
