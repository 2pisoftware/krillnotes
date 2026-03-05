// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2024-2026 TripleACS Pty Ltd t/a 2pi Software

// @name: Text Note
// @description: A simple text note with a body field.

schema("TextNote", #{
    fields: [
        #{ name: "body", type: "textarea", required: false },
    ]
});

// Example tree action: sort a TextNote's children alphabetically by title.
// Right-click any TextNote in the tree and choose "Sort Children A→Z".
add_tree_action("Sort Children A→Z", ["TextNote"], |note| {
    let children = get_children(note.id);
    children.sort_by(|a, b| a.title <= b.title);
    children.map(|c| c.id)
});

// Example tree action: add a new child TextNote with a placeholder title.
add_tree_action("Add Child Note", ["TextNote"], |note| {
    let child = create_note(note.id, "TextNote");
    child.title = "New note";
    update_note(child);
});
