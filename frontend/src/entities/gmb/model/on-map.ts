"use client";

import { useSyncExternalStore } from "react";

import { setQuery, useQuery } from "@/shared/lib/query";

import type { Gmb, State } from "./gmb";

/** What the map and the outline beside it share about the GMBs. */
export interface OnMap {
  /** The located GMBs inside the map's viewport; `null` while no map is shown. */
  inView: ReadonlySet<number> | null;
  /** The GMB a pointer is on, in the outline or on the map. */
  hovered: number | null;
  /** The GMB whose point is being placed on the map by hand: until set or cancelled, the map left or not. */
  placing: number | null;
}

const NONE: OnMap = { inView: null, hovered: null, placing: null };
let state = NONE;
const listeners = new Set<() => void>();

const subscribe = (onChange: () => void) => {
  listeners.add(onChange);
  return () => listeners.delete(onChange);
};

export function useOnMap(): OnMap {
  return useSyncExternalStore(subscribe, () => state, () => NONE);
}

export function setOnMap(change: Partial<OnMap>): void {
  state = { ...state, ...change };
  for (const l of listeners) l();
}

/** The GMB whose card is open on the map. */
export function usePeeked(): number | null {
  const raw = useQuery("gmb");
  if (raw === null) return null;
  const id = Number(raw);
  if (!Number.isSafeInteger(id)) throw new Error(`?gmb=${raw}: not a GMB id`);
  return id;
}

export function peek(gmb: number | null): void {
  setQuery({ gmb: gmb === null ? null : String(gmb) });
}

export type Tab = "launch";

/** The location window's tabs by state, in order; a GBP whose state has none opens as its card only. */
export const TABS: Record<State, readonly Tab[]> = { draft: ["launch"], verifying: [], verified: [], suspended: [] };

/** The tab the window is open on; `null`: no window, the card only. */
export function useTab(): Tab | null {
  const raw = useQuery("tab");
  if (raw !== null && !Object.values(TABS).some((tabs) => tabs.includes(raw as Tab))) throw new Error(`?tab=${raw}: no state has it`);
  return raw as Tab | null;
}

/** The GBP's window, on the open tab while its state has it, else on its state's first; its card alone where it has none. */
export function open(gmb: Gmb): void {
  const tabs = TABS[gmb.state];
  setQuery((now) => ({ gmb: String(gmb.id), tab: tabs.includes(now.tab as Tab) ? now.tab : (tabs[0] ?? null) }));
}
