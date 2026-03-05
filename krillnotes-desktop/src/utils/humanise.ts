// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2024-2026 TripleACS Pty Ltd t/a 2pi Software

/**
 * Converts a snake_case field key to a Title Case display label.
 *
 * Examples:
 *   "first_name"        → "First Name"
 *   "note_title"        → "Note Title"
 *   "email"             → "Email"
 *   "first_name (legacy)" → "First Name (legacy)"
 */
export function humaniseKey(key: string): string {
  return key
    .split('_')
    .map(word => word.charAt(0).toUpperCase() + word.slice(1))
    .join(' ');
}
