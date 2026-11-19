"use client";

import { useSyncExternalStore } from "react";

import type { Gmb, LatLng } from "@/shared/api/schema";

import { cardOnly, peek } from "./windows";

/** What the map and the outline beside it share about the GMBs. */
export interface OnMap {
  /** The located GMBs inside the map's viewport; `null` while no map is shown. */
  inView: ReadonlySet<number> | null;
  /** The GMB a pointer is on, in the outline or on the map. */
  hovered: number | null;
  /** The GMB whose point is being placed on the map by hand: until set or cancelled, the map left or not. */
  placing: number | null;
  /** A point being picked on the map: a click gives it to `onPick` and ends the picking, Escape ends it. */
  picking: { label: string; onPick: (at: LatLng) => void } | null;
  /** What an open form has picked, drawn numbered on the map. */
  marks: readonly LatLng[];
  /** The GMB the map is asked to go to, until it has. */
  focus: number | null;
}

const NONE: OnMap = { inView: null, hovered: null, placing: null, picking: null, marks: [], focus: null };
let state = NONE;
const listeners = new Set<() => void>();

const subscribe = (onChange: () => void) => {
  listeners.add(onChange);
  return () => listeners.delete(onChange);
};

/** `part` of it: a hover re-renders only what reads the hover. */
export function useOnMap<T>(part: (s: OnMap) => T): T {
  return useSyncExternalStore(subscribe, () => part(state), () => part(NONE));
}

export function setOnMap(change: Partial<OnMap>): void {
  state = { ...state, ...change };
  for (const l of listeners) l();
}

/** A GBP clicked on the map or in the outline beside it: the first click shows its card, the next goes to it on the map, its card alone. */
export function openOrFocus(gmb: Gmb, peeked: boolean): void {
  if (!peeked || gmb.location === null) return peek(gmb.id);
  cardOnly(gmb.id);
  setOnMap({ focus: gmb.id });
}
