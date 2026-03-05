// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2024-2026 TripleACS Pty Ltd t/a 2pi Software

// @name: Photo Note
// @description: A note type with a photo field to test display_image and get_attachments.

schema("PhotoNote", #{
    fields: [
        #{ name: "photo",   type: "file",     required: false },
        #{ name: "caption", type: "text",     required: false },
        #{ name: "body",    type: "textarea", required: false },
    ],

    // on_view: full detail view in the InfoPanel.
    on_view: |note| {
        let caption = note.fields["caption"] ?? "";
        let body    = note.fields["body"]    ?? "";

        // note.fields["photo"] is the UUID string (or () when not set).
        let img = display_image(note.fields["photo"], 480, caption);

        let parts = [img];
        if caption != "" { parts += [text(caption)]; }
        if body != "" { parts += [section("Notes", markdown(body))]; }
        stack(parts)
    },

    // on_hover: compact thumbnail shown in tree hover tooltips.
    on_hover: |note| {
        display_image(note.fields["photo"], 200, note.title)
    },
});
