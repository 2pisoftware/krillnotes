// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2024-2026 TripleACS Pty Ltd t/a 2pi Software

/**
 * Converts a workspace display name into a filesystem/window-label-safe slug.
 * Lowercases, replaces runs of non-alphanumeric characters with a single
 * hyphen, and strips leading/trailing hyphens.
 *
 * Used by both NewWorkspaceDialog and WorkspaceManagerDialog to ensure
 * the folder name is always a valid Tauri window label.
 */
export function slugify(name: string): string {
  return name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '');
}
