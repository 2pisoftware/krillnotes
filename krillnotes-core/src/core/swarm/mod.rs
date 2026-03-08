// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2024-2026 TripleACS Pty Ltd t/a 2pi Software

//! `.swarm` bundle format — codec, crypto, and state machine.

pub mod crypto;
pub mod delta;
pub mod header;
pub mod invite;
pub mod signature;
pub mod snapshot;
