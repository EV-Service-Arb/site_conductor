"use client";

import { useBinding } from "@evinvest/uikit";

import { useT } from "@/shared/i18n";

/** Its leaves under the shell's leader; `null` leaves one unbound. One surface holds them at a time. */
export function useKeymap(keys: { toggle: (() => void) | null; newChat: () => void; copyLink: (() => void) | null }): void {
  const t = useT();
  useBinding({ on: { key: "a" }, run: { id: "assistant", label: t("Assistant: show or hide"), run: keys.toggle } });
  useBinding({ on: { key: "n" }, run: { label: t("New chat"), run: keys.newChat } });
  useBinding({ on: { key: "l" }, run: { label: t("Copy link"), run: keys.copyLink } });
}

/** Escape inside the sidebar or the floating window hides it, unless a menu, the picker or an edit box took it first. */
export function hideOnEscape(e: React.KeyboardEvent<HTMLElement>, hide: () => void): void {
  if (e.key !== "Escape" || e.defaultPrevented || !e.currentTarget.contains(e.target as Node)) return;
  // An open menu, popover or dialog takes Escape itself, from the document, after this has heard it.
  if (document.querySelector('[data-state="open"][data-slot$="content"]:not([data-slot="tooltip-content"])') !== null) return;
  hide();
}
