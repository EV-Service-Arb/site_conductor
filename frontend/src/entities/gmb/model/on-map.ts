"use client";

import { useSyncExternalStore } from "react";

import type { Country, Gmb, LatLng, State } from "@/shared/api/schema";
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
const NO_WINDOW = { gmb: undefined, trade: undefined, country: undefined, account: undefined, tab: undefined, categories: undefined, trash: undefined, add: undefined, kit: undefined } as const;

/** The GMB whose card is open on the map. */
export function usePeeked(): number | null {
  return useHome()?.gmb ?? null;
}

export function peek(gmb: number | null): void {
  setHome({ gmb: gmb ?? undefined });
}

/** A GBP window's tab; a gmail's window has its own (`AccountTab`), both in `?tab`. */
export type Tab = Exclude<NonNullable<Home["tab"]>, "connections">;

/** The location window's tabs by state, in order; a GBP whose state has none opens as its card only. */
export const TABS: Record<State, readonly Tab[]> = { draft: ["launch", "browser"], verifying: ["browser"], verified: ["kit"], suspended: [] };

/** The tab the GBP's window is open on; `null`: no window, the card only, or a gmail's window holds `?tab`. */
export function useTab(): Tab | null {
  const home = useHome();
  if (home === null || home.account !== undefined || home.tab === undefined) return null;
  if (home.tab === "connections") throw new Error("?tab=connections: a gmail's tab, with no ?account");
  return home.tab;
}

/** The GBP's window, on the open tab while its state has it, else on its state's first; its card alone where it has none. */
export function open(gmb: Pick<Gmb, "id" | "state">): void {
  const tabs = TABS[gmb.state];
  setHome((now) => ({ ...NO_WINDOW, gmb: gmb.id, tab: now.account === undefined && tabs.find((t) => t === now.tab) !== undefined ? now.tab : tabs[0] }));
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

export const ACCOUNT_TABS = ["connections", "browser"] as const satisfies readonly NonNullable<Home["tab"]>[];
export type AccountTab = (typeof ACCOUNT_TABS)[number];

/** The gmail whose own window is open (`?account`, apart from the outline's `?gmail` scope), on its tab. */
export function useAccountWindow(): { gmail: number; tab: AccountTab } | null {
  const home = useHome();
  if (home?.account === undefined) return null;
  const tab = ACCOUNT_TABS.find((t) => t === home.tab);
  if (tab === undefined) throw new Error(`?tab=${home.tab}: not a tab of gmail ${home.account}'s window`);
  return { gmail: home.account, tab };
}

export function openAccount(at: { gmail: number; tab: AccountTab } | null): void {
  setHome({ ...NO_WINDOW, account: at?.gmail, tab: at?.tab });
}

/** What the caller removed (`?trash`), restorable until purged. */
export function useTrashWindow(): boolean {
  return useHome()?.trash === true;
}

export function openTrash(on: boolean): void {
  setHome({ ...NO_WINDOW, trash: on });
}

/** The review kit's area window (`?kit`): `""` draws a new area, a slug shows that one. */
export function useKitWindow(): string | null {
  return useHome()?.kit ?? null;
}

export function openKit(area: string | null): void {
  setHome({ ...NO_WINDOW, kit: area ?? undefined });
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
