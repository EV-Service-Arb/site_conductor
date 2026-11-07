"use client";

import { useSyncExternalStore } from "react";

import { setQuery, useQuery } from "@/shared/lib/query";

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
