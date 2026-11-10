"use client";

import { useSyncExternalStore } from "react";

const KEY = "sa:sidebar";
const EVENT = "sa:sidebar-chosen";

const folded = () => localStorage.getItem(KEY) === "folded";

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
