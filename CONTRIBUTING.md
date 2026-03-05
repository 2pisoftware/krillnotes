# Contributing to Krill Notes

Thank you for considering a contribution to Krill Notes! This document
explains how to contribute and what to expect.

## Contributor License Agreement (CLA)

Before we can accept your contribution, you must agree to the
[Contributor License Agreement](CLA.md).

**Why?** Krill Notes is dual-licensed: the open source version is
available under the [Mozilla Public License 2.0](LICENSE), and a
commercial version is available under a proprietary license. The CLA
grants TripleACS Pty Ltd t/a 2pi Software the rights needed to include your
contributions in both versions. Your contributions to the open source
project will always remain available under the MPL-2.0.

**How?** By submitting a pull request, you indicate your agreement to
the CLA. A bot will prompt you to confirm on your first PR.

If you are contributing on behalf of a company, please contact
licensing@2pisoftware.com to arrange a Corporate CLA before submitting.

## Getting Started

1. **Fork** the repository on GitHub.
2. **Clone** your fork locally.
3. **Create a branch** for your change (`git checkout -b my-feature`).
4. **Make your changes.** See the guidelines below.
5. **Test** your changes (`cargo test`).
6. **Commit** with a clear message describing the change.
7. **Push** to your fork and open a **Pull Request**.

## Development Guidelines

### Code Style

- Follow standard Rust conventions (`cargo fmt`, `cargo clippy`).
- All public types and functions must have doc comments.
- New modules should include unit tests.

### License Headers

Every new Rust source file must include the MPL-2.0 header at the top:

```rust
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2024-2026 TripleACS Pty Ltd t/a 2pi Software
```

For files in other languages, use the appropriate comment syntax with the
same text.

### Commit Messages

- Use the imperative mood ("Add feature" not "Added feature").
- First line: concise summary, 72 characters max.
- Optional body: separated by a blank line, explaining *why* not *what*.

### Tests

- All new functionality must include tests.
- Run the full test suite before submitting: `cargo test --workspace`
- If your change affects sync behaviour, include integration tests that
  exercise the operation log and .swarm bundle generation/application.

### Documentation

- Update relevant documentation if your change affects user-facing
  behaviour.
- Add or update doc comments for any changed public API.

## What We're Looking For

Contributions of all kinds are welcome:

- **Bug fixes** — always appreciated.
- **Tests** — improving coverage helps everyone.
- **Documentation** — clearer docs make the project more accessible.
- **Performance improvements** — with benchmarks showing the improvement.
- **New features** — please open an issue first to discuss the design.

## What Requires Discussion First

Please open an issue before submitting a PR for:

- New note types or schema changes
- Changes to the sync protocol or .swarm format
- Changes to the cryptographic implementation
- Significant architectural changes
- New dependencies (especially native/C dependencies)

These areas affect the core design and interoperability, so we want to
make sure any changes are aligned with the project's direction.

## Code of Conduct

Be respectful. Be constructive. Assume good intent. We're building
something together and everyone's time is valuable.

## Questions?

- **General questions:** Open a GitHub Discussion.
- **Bug reports:** Open a GitHub Issue with reproduction steps.
- **Security issues:** Email security@2pisoftware.com (do not open a
  public issue).
- **Licensing questions:** Email licensing@2pisoftware.com.
