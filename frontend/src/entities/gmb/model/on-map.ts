"use client";

import { useSyncExternalStore } from "react";

import type { Country, Gmb, LatLng, Tab } from "@/shared/api/schema";
import { type Home, setHome, useHome } from "@/shared/lib/location";

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

/** One window at a time over the map: opening one closes the rest. */
const NO_WINDOW = { gmb: undefined, trade: undefined, country: undefined, account: undefined, tab: undefined, categories: undefined, trash: undefined, add: undefined, kit: undefined, kit_gmb: undefined, kit_pool: undefined, kit_edit: undefined } as const;

/** The GMB whose card is open on the map. */
export function usePeeked(): number | null {
  return useHome()?.gmb ?? null;
}

export function peek(gmb: number | null): void {
  setHome({ gmb: gmb ?? undefined });
}

/** The tab the GBP's window is open on; `null`: no window, the card only, or a gmail's window holds `?tab`. */
export function useTab(): Tab | null {
  const home = useHome();
  if (home === null || home.account !== undefined) return null;
  return home.tab ?? null;
}

/** The GBP's window, on the open tab while it has it, else on its first; its card alone where it has none (`Gmb.tabs`). */
export function open(gmb: Pick<Gmb, "id" | "tabs">): void {
  setHome((now) => ({ ...NO_WINDOW, gmb: gmb.id, tab: now.account === undefined && gmb.tabs.find((t) => t === now.tab) !== undefined ? now.tab : gmb.tabs[0] }));
}

/** A GBP clicked on the map or in the outline beside it: the first click opens it, the next goes to it on the map, out of its window. */
export function openOrFocus(gmb: Gmb, peeked: boolean): void {
  if (!peeked || gmb.location === null) return open(gmb);
  setHome({ tab: undefined });
  setOnMap({ focus: gmb.id });
}

/** The trade whose window is open, on a country's tab; `null` country: the trade has none set up yet, or none picked. */
export function useTradeWindow(): { trade: number; country: Country | null } | null {
  const home = useHome();
  if (home?.trade === undefined) return null;
  return { trade: home.trade, country: home.country ?? null };
}

export function openTrade(trade: number | null, country?: Country): void {
  setHome({ ...NO_WINDOW, trade: trade ?? undefined, country });
}

/** Every trade by country, the level a trade's window sits under (`?categories`). */
export function useCategoriesWindow(): boolean {
  return useHome()?.categories === true;
}

export function openCategories(on: boolean): void {
  setHome({ ...NO_WINDOW, categories: on });
}

/** The gmail whose own window is open (`?account`, apart from the outline's `?gmail` scope), on its tab (one of `Gmail.tabs`). */
export function useAccountWindow(): { gmail: number; tab: Tab } | null {
  const home = useHome();
  if (home?.account === undefined) return null;
  if (home.tab === undefined) throw new Error(`?account=${home.account}: its window has no ?tab`);
  return { gmail: home.account, tab: home.tab };
}

export function openAccount(at: { gmail: number; tab: Tab } | null): void {
  setHome({ ...NO_WINDOW, account: at?.gmail, tab: at?.tab });
}

/** What the caller removed (`?trash`), restorable until purged. */
export function useTrashWindow(): boolean {
  return useHome()?.trash === true;
}

export function openTrash(on: boolean): void {
  setHome({ ...NO_WINDOW, trash: on });
}

/** Where in a review kit: `kit` its slug (`""`: the list, or with `edit` a new one); a GBP of it, a pool (`<trade>.<country>`) or its editor. */
export type KitAt = { kit: string; gmb?: number | undefined; pool?: string | undefined; edit?: true | undefined };

/** The kit floating over the map (`?kit`). */
export function useKitFloat(): KitAt | null {
  const home = useHome();
  if (home?.kit === undefined) return null;
  return { kit: home.kit, gmb: home.kit_gmb, pool: home.kit_pool, edit: home.kit_edit ? true : undefined };
}

/** Over the map: one window at a time. */
export function floatKit(at: KitAt | null): void {
  setHome({ ...NO_WINDOW, kit: at?.kit, kit_gmb: at?.gmb, kit_pool: at?.pool, kit_edit: at?.edit });
}

/** What the Add a GBP panel opens with: the outline's gmail and trade, when it is scoped to one. */
export type Adding = NonNullable<Home["add"]>;

/** The Add a GBP panel over the map (`?add`). */
export function useAddWindow(): Adding | null {
  return useHome()?.add ?? null;
}

export function openAdd(at: Adding | null): void {
  setHome({ ...NO_WINDOW, add: at ?? undefined });
}
