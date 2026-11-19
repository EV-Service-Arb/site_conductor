"use client";

import { useEffect, useState } from "react";

/** Wide enough to hold a window's fixed tile beside its other tabs (15c). */
const SPLIT_PX = 1024;

/** A page window's split: its box, and whether the box is wide enough to lay its tiles side by side; over the map, never. */
export function useSplit(page: boolean): [(box: HTMLElement | null) => void, boolean] {
  const [box, setBox] = useState<HTMLElement | null>(null);
  const [width, setWidth] = useState(0);
  useEffect(() => {
    if (box === null) return;
    const seen = new ResizeObserver(([e]) => setWidth(e!.contentRect.width));
    seen.observe(box);
    return () => seen.disconnect();
  }, [box]);
  return [setBox, page && width >= SPLIT_PX];
}
