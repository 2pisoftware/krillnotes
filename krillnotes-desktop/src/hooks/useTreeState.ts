// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2024-2026 TripleACS Pty Ltd t/a 2pi Software

import React, { useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { Note, TreeNode as TreeNodeType, SchemaInfo } from '../types';
import { getAncestorIds, flattenVisibleTree, findNoteInTree } from '../utils/tree';

export function useTreeState(
  notes: Note[],
  tree: TreeNodeType[],
  _schemas: Record<string, SchemaInfo>, // reserved for future schema-aware keyboard nav (e.g. expandable check)
  closePendingUndoGroupRef: React.MutableRefObject<(() => Promise<void>) | undefined>,
  loadNotes: () => Promise<unknown>,
  setRequestEditMode: React.Dispatch<React.SetStateAction<number>>,
) {
  const [selectedNoteId, setSelectedNoteId] = useState<string | null>(null);
  const [viewHistory, setViewHistory] = useState<string[]>([]);
  const selectedNoteIdRef = useRef<string | null>(selectedNoteId);
  const selectionInitialized = useRef(false);

  // Keep ref in sync with state
  selectedNoteIdRef.current = selectedNoteId;

  const handleSelectNote = async (noteId: string) => {
    // Close any pending note-creation undo group before switching notes.
    await closePendingUndoGroupRef.current?.();
    setViewHistory([]);
    setSelectedNoteId(noteId);
    try {
      await invoke('set_selected_note', { noteId });
    } catch (err) {
      console.error('Failed to save selection:', err);
    }
  };

  const handleToggleExpand = async (noteId: string) => {
    try {
      await invoke('toggle_note_expansion', { noteId });
      await loadNotes();
    } catch (err) {
      console.error('Failed to toggle expansion:', err);
    }
  };

  const handleLinkNavigate = async (noteId: string) => {
    if (selectedNoteId) {
      setViewHistory(h => [...h, selectedNoteId]);
    }

    // Expand any collapsed ancestors so the note becomes visible in the tree
    const ancestors = getAncestorIds(notes, noteId);
    const collapsedAncestors = ancestors.filter(
      id => notes.find(n => n.id === id)?.isExpanded === false
    );
    for (const ancestorId of collapsedAncestors) {
      await invoke('toggle_note_expansion', { noteId: ancestorId });
    }
    if (collapsedAncestors.length > 0) {
      await loadNotes();
    }

    setSelectedNoteId(noteId);
    invoke('set_selected_note', { noteId }).catch(err =>
      console.error('Failed to save selection:', err)
    );

    requestAnimationFrame(() => {
      document.querySelector(`[data-note-id="${noteId}"]`)?.scrollIntoView({ block: 'nearest' });
    });
  };

  const handleBack = () => {
    if (viewHistory.length === 0) return;
    const prev = viewHistory[viewHistory.length - 1];
    setViewHistory(h => h.slice(0, -1));
    setSelectedNoteId(prev);
    invoke('set_selected_note', { noteId: prev }).catch(err =>
      console.error('Failed to save selection:', err)
    );
  };

  const handleSearchSelect = async (noteId: string) => {
    // Expand any collapsed ancestors so the note becomes visible in the tree
    const ancestors = getAncestorIds(notes, noteId);
    const collapsedAncestors = ancestors.filter(
      id => notes.find(n => n.id === id)?.isExpanded === false
    );

    for (const ancestorId of collapsedAncestors) {
      await invoke('toggle_note_expansion', { noteId: ancestorId });
    }

    if (collapsedAncestors.length > 0) {
      await loadNotes();
    }

    await handleSelectNote(noteId);

    // Scroll the note into view in the tree
    requestAnimationFrame(() => {
      document.querySelector(`[data-note-id="${noteId}"]`)?.scrollIntoView({ block: 'nearest' });
    });
  };

  const handleTreeKeyDown = (e: React.KeyboardEvent) => {
    if ((e.target as HTMLElement).closest('button') !== null) return;
    if (!selectedNoteId) return;
    const flat = flattenVisibleTree(tree);
    const idx = flat.findIndex(n => n.note.id === selectedNoteId);
    if (idx === -1) return;
    const current = flat[idx];

    const selectAndScroll = (noteId: string) => {
      handleSelectNote(noteId);
      requestAnimationFrame(() => {
        document.querySelector(`[data-note-id="${noteId}"]`)?.scrollIntoView({ block: 'nearest' });
      });
    };

    switch (e.key) {
      case 'ArrowDown': {
        e.preventDefault();
        if (idx < flat.length - 1) selectAndScroll(flat[idx + 1].note.id);
        break;
      }
      case 'ArrowUp': {
        e.preventDefault();
        if (idx > 0) selectAndScroll(flat[idx - 1].note.id);
        break;
      }
      case 'ArrowRight': {
        e.preventDefault();
        if (current.children.length > 0) {
          if (!current.note.isExpanded) {
            handleToggleExpand(current.note.id);
          } else {
            selectAndScroll(current.children[0].note.id);
          }
        }
        break;
      }
      case 'ArrowLeft': {
        e.preventDefault();
        if (current.note.isExpanded) {
          handleToggleExpand(current.note.id);
        } else if (current.note.parentId) {
          const parent = findNoteInTree(tree, current.note.parentId);
          if (parent) selectAndScroll(parent.note.id);
        }
        break;
      }
      case 'Enter': {
        e.preventDefault();
        setRequestEditMode(prev => prev + 1);
        break;
      }
    }
  };

  return {
    selectedNoteId,
    setSelectedNoteId,
    selectedNoteIdRef,
    viewHistory,
    handleSelectNote,
    handleToggleExpand,
    handleLinkNavigate,
    handleBack,
    handleSearchSelect,
    handleTreeKeyDown,
    selectionInitialized,
  };
}
