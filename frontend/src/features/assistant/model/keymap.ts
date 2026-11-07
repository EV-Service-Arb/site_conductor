"use client";

import { useEffect, useRef } from "react";

export interface Keys {
  /** Ctrl+J: the sidebar or the floating window shown or hidden; none on the full screen page. */
  toggle: (() => void) | null;
  /** Ctrl+Shift+; */
  newChat: () => void;
  /** Ctrl+Alt+L: the open chat's link; none for a chat not started. */
  copyLink: (() => void) | null;
}

/** The assistant's shortcuts, on the window: one surface holds them at a time. */
export function useKeymap(keys: Keys): void {
  const latest = useRef(keys);
  useEffect(() => {
    latest.current = keys;
  });
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey)) return;
      const { toggle, newChat, copyLink } = latest.current;
      // By code: the shifted `;` is `:` on one layout and something else on the next.
      const run =
        e.code === "KeyJ" && !e.shiftKey && !e.altKey ? toggle : e.code === "Semicolon" && e.shiftKey && !e.altKey ? newChat : e.code === "KeyL" && e.altKey && !e.shiftKey ? copyLink : null;
      if (run === null) return;
      e.preventDefault();
      run();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
}

/** Escape inside the sidebar or the floating window hides it, unless a menu, the picker or an edit box took it first. */
export function hideOnEscape(e: React.KeyboardEvent<HTMLElement>, hide: () => void): void {
  if (e.key !== "Escape" || e.defaultPrevented || !e.currentTarget.contains(e.target as Node)) return;
  // An open menu, popover or dialog takes Escape itself, from the document, after this has heard it.
  if (document.querySelector('[data-state="open"][data-slot$="content"]:not([data-slot="tooltip-content"])') !== null) return;
  hide();
}
