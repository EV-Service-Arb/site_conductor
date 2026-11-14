"use client";

import { useSyncExternalStore } from "react";

const KEY = "sa:sidebar";
const WIDTH_KEY = "sa:sidebar-width";
const EVENT = "sa:sidebar-chosen";

export const WIDTH = { min: 240, max: 640, initial: 352 } as const;

const folded = () => localStorage.getItem(KEY) === "folded";
const width = () => {
  const kept = localStorage.getItem(WIDTH_KEY);
  if (kept === null) return WIDTH.initial;
  const px = Number(kept);
  if (!Number.isInteger(px)) throw new Error(`localStorage ${WIDTH_KEY} is ${JSON.stringify(kept)}, not a width in px`);
  return px;
};

const subscribe = (onChange: () => void) => {
  for (const e of ["storage", EVENT]) window.addEventListener(e, onChange);
  return () => {
    for (const e of ["storage", EVENT]) window.removeEventListener(e, onChange);
  };
};

/** Whether the outline is folded to its icons on a wide screen; kept in this browser. */
export function useSidebarFolded(): boolean {
  return useSyncExternalStore(subscribe, folded, () => false);
}

export function toggleSidebar(): void {
  if (folded()) localStorage.removeItem(KEY);
  else localStorage.setItem(KEY, "folded");
  window.dispatchEvent(new Event(EVENT));
}

/** The outline's width on a wide screen unfolded, in px; kept in this browser. */
export function useSidebarWidth(): number {
  return useSyncExternalStore(subscribe, width, () => WIDTH.initial);
}

export function setSidebarWidth(px: number): void {
  localStorage.setItem(WIDTH_KEY, String(Math.round(Math.min(WIDTH.max, Math.max(WIDTH.min, px)))));
  window.dispatchEvent(new Event(EVENT));
}
