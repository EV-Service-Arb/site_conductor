"use client";

import { useSyncExternalStore } from "react";

import type { Gmb, LatLng } from "@/shared/api/schema";
import { viewed } from "@/shared/lib/location";

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
  /** Where a draft is being started on the map. */
  drafting: LatLng | null;
  /** What an open form has picked, drawn numbered on the map. */
  marks: readonly LatLng[];
  /** The GMB the map is asked to go to, until it has. */
  focus: number | null;
}

const NONE: OnMap = { inView: null, hovered: null, placing: null, picking: null, drafting: null, marks: [], focus: null };
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

/** A draft started at a point marked on the map, unless a mode has the map: placing, picking, a kit's editor, an area drawn. */
export function draftAt(at: LatLng): void {
  const { at: here } = viewed();
  if (here.view !== "home") throw new Error(`the map marked on ${here.view}`);
  if (state.placing !== null || state.picking !== null || here.params.kit_edit === true || here.params.area === "") return;
  setOnMap({ drafting: at });
}

/** A GBP clicked on the map or in the outline beside it: the first click shows its card, the next goes to it on the map, its card alone. */
export function openOrFocus(gmb: Gmb, peeked: boolean): void {
  if (!peeked || gmb.location === null) return peek(gmb.id);
  cardOnly(gmb.id);
  setOnMap({ focus: gmb.id });
}
