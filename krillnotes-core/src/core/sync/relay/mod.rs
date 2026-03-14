// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2024-2026 TripleACS Pty Ltd t/a 2pi Software

pub mod auth;

#[cfg(feature = "relay")]
pub mod client;

pub use auth::{
    delete_relay_credentials, load_relay_credentials, save_relay_credentials, RelayCredentials,
};

#[cfg(feature = "relay")]
pub use client::RelayClient;
