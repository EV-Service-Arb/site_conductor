"use client";

import { useBinding, useKeymap } from "@evinvest/uikit";
import { type RefObject, useEffect, useRef, useSyncExternalStore } from "react";

import { keys, useT } from "@/shared/i18n";

interface Floatie {
  el: RefObject<HTMLElement | null>;
  flip: () => void;
}

const ID = "floatie-flip";

// the one last focused or pressed in first, else the last opened
let floaties: readonly Floatie[] = [];
const listeners = new Set<() => void>();
const set = (next: readonly Floatie[]) => {
  floaties = next;
  for (const l of listeners) l();
};
const subscribe = (l: () => void) => {
  listeners.add(l);
  return () => listeners.delete(l);
};

/** A window over the page or as its own page, `flip` moving it to the other: `leader z` runs the one in focus. The keys, for its button's hint. */
export function useFloatie(el: RefObject<HTMLElement | null>, flip: (() => void) | null): string | undefined {
  const latest = useRef(flip);
  useEffect(() => {
    latest.current = flip;
  });
  const on = flip !== null;
  useEffect(() => {
    if (!on) return;
    const me: Floatie = {
      el,
      flip: () => {
        if (latest.current === null) throw new Error("a floatie's flip went null without leaving");
        latest.current();
      },
    };
    set([me, ...floaties]);
    const front = (e: Event) => {
      if (floaties[0] !== me && el.current?.contains(e.target as Node)) set([me, ...floaties.filter((f) => f !== me)]);
    };
    document.addEventListener("focusin", front, true);
    document.addEventListener("pointerdown", front, true);
    return () => {
      document.removeEventListener("focusin", front, true);
      document.removeEventListener("pointerdown", front, true);
      set(floaties.filter((f) => f !== me));
    };
  }, [el, on]);
  return useKeymap().bound.get(ID)?.sequence;
}

/** The shell's one `leader z`. */
export function FloatieKey() {
  const t = useT();
  const front = useSyncExternalStore(subscribe, () => floaties[0] ?? null, () => null);
  useBinding({ on: { key: "z" }, id: ID, run: { label: t(keys.floatie, "Window: over the page or its own page"), run: front && front.flip } });
  return null;
}
