"use client";

import { useSyncExternalStore } from "react";

const KEY = "sa:sidebar";
const EVENT = "sa:sidebar-chosen";

const shown = () => localStorage.getItem(KEY) !== "hidden";

const subscribe = (onChange: () => void) => {
  for (const e of ["storage", EVENT]) window.addEventListener(e, onChange);
  return () => {
    for (const e of ["storage", EVENT]) window.removeEventListener(e, onChange);
  };
};

/** Whether the outline sits beside the page on a wide screen; shown unless hidden, kept in this browser. */
export function useSidebar(): boolean {
  return useSyncExternalStore(subscribe, shown, () => true);
}

export function toggleSidebar(): void {
  if (shown()) localStorage.setItem(KEY, "hidden");
  else localStorage.removeItem(KEY);
  window.dispatchEvent(new Event(EVENT));
}
