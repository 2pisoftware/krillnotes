// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2024-2026 TripleACS Pty Ltd t/a 2pi Software

/**
 * Returns a deterministic HSL background color for a tag.
 * Hue is derived from the sum of the tag's char codes, giving the same
 * tag the same color across renders and sessions.
 */
export function tagColor(tag: string): string {
  const hue = [...tag].reduce((acc, c) => acc + c.charCodeAt(0), 0) % 360;
  return `hsl(${hue}, 40%, 88%)`;
}
