// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2024-2026 TripleACS Pty Ltd t/a 2pi Software

import { useCallback, useEffect, useRef, useState } from 'react';

export function useResizablePanels(initialTreeWidth = 300, initialTagCloudHeight = 120) {
  const [treeWidth, setTreeWidth] = useState(initialTreeWidth);
  const isDragging = useRef(false);
  const dragStartX = useRef(0);
  const dragStartWidth = useRef(0);

  const [tagCloudHeight, setTagCloudHeight] = useState(initialTagCloudHeight);
  const isTagDragging = useRef(false);
  const tagDragStartY = useRef(0);
  const tagDragStartHeight = useRef(0);

  const handleDividerMouseDown = useCallback((e: React.MouseEvent) => {
    isDragging.current = true;
    dragStartX.current = e.clientX;
    dragStartWidth.current = treeWidth;
    e.preventDefault();
  }, [treeWidth]);

  useEffect(() => {
    const onMouseMove = (e: MouseEvent) => {
      if (!isDragging.current) return;
      const delta = e.clientX - dragStartX.current;
      setTreeWidth(Math.max(180, Math.min(600, dragStartWidth.current + delta)));
    };
    const onMouseUp = () => { isDragging.current = false; };
    document.addEventListener('mousemove', onMouseMove);
    document.addEventListener('mouseup', onMouseUp);
    return () => {
      document.removeEventListener('mousemove', onMouseMove);
      document.removeEventListener('mouseup', onMouseUp);
    };
  }, []);

  const handleTagDividerMouseDown = useCallback((e: React.MouseEvent) => {
    isTagDragging.current = true;
    tagDragStartY.current = e.clientY;
    tagDragStartHeight.current = tagCloudHeight;
    e.preventDefault();
  }, [tagCloudHeight]);

  useEffect(() => {
    const onMouseMove = (e: MouseEvent) => {
      if (!isTagDragging.current) return;
      const delta = tagDragStartY.current - e.clientY;
      setTagCloudHeight(Math.max(0, Math.min(400, tagDragStartHeight.current + delta)));
    };
    const onMouseUp = () => { isTagDragging.current = false; };
    document.addEventListener('mousemove', onMouseMove);
    document.addEventListener('mouseup', onMouseUp);
    return () => {
      document.removeEventListener('mousemove', onMouseMove);
      document.removeEventListener('mouseup', onMouseUp);
    };
  }, []);

  return {
    treeWidth,
    tagCloudHeight,
    handleDividerMouseDown,
    handleTagDividerMouseDown,
  };
}
