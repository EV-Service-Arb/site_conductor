"use client";

import { useSyncExternalStore } from "react";

import { setQuery, useQuery } from "@/shared/lib/query";

import { COUNTRIES, type Country, type Gmb, type LatLng, type State } from "./gmb";

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

export type Tab = "launch" | "browser" | "kit";

/** The location window's tabs by state, in order; a GBP whose state has none opens as its card only. */
export const TABS: Record<State, readonly Tab[]> = { draft: ["launch", "browser"], verifying: ["browser"], verified: ["kit"], suspended: [] };

/** The tab the GBP's window is open on; `null`: no window, the card only, or the trade's or a gmail's window holds `?tab`. */
export function useTab(): Tab | null {
  const raw = useQuery("tab");
  const trade = useQuery("trade");
  const account = useQuery("account");
  if (trade !== null || account !== null) return null;
  if (raw !== null && !Object.values(TABS).some((tabs) => tabs.includes(raw as Tab))) throw new Error(`?tab=${raw}: no state has it`);
  return raw as Tab | null;
}

/** The GBP's window, on the open tab while its state has it, else on its state's first; its card alone where it has none. */
export function open(gmb: Pick<Gmb, "id" | "state">): void {
  const tabs = TABS[gmb.state];
  setQuery((now) => ({ gmb: String(gmb.id), trade: null, categories: null, account: null, trash: null, add: null, kit: null, tab: now.trade === null && tabs.includes(now.tab as Tab) ? now.tab : (tabs[0] ?? null) }));
}

/** A GBP clicked on the map or in the outline beside it: the first click opens it, the next goes to it on the map, out of its window. */
export function openOrFocus(gmb: Gmb, peeked: boolean): void {
  if (!peeked || gmb.location === null) return open(gmb);
  setQuery({ tab: null });
  setOnMap({ focus: gmb.id });
}

/** The trade whose window is open, on a country's tab; `null` tab: the trade has none set up yet, or none picked. One window at a time: it and a GBP's share `?tab`. */
export function useTradeWindow(): { trade: number; tab: Country | null } | null {
  const raw = useQuery("trade");
  const tab = useQuery("tab");
  if (raw === null) return null;
  const trade = Number(raw);
  if (!Number.isSafeInteger(trade)) throw new Error(`?trade=${raw}: not a trade id`);
  if (tab !== null && !(COUNTRIES as string[]).includes(tab)) throw new Error(`?tab=${tab}: not a country of trade ${trade}`);
  return { trade, tab: tab as Country | null };
}

export function openTrade(trade: number | null, tab?: Country): void {
  setQuery({ trade: trade === null ? null : String(trade), tab: tab ?? null, categories: null, gmb: null, account: null, trash: null, add: null, kit: null });
}

/** Every trade by country, the level a trade's window sits under (`?categories`). */
export function useCategoriesWindow(): boolean {
  return useQuery("categories") !== null;
}

export function openCategories(on: boolean): void {
  setQuery({ categories: on ? "" : null, trade: null, tab: null, gmb: null, account: null, trash: null, add: null, kit: null });
}

export const ACCOUNT_TABS = ["connections", "browser"] as const;
export type AccountTab = (typeof ACCOUNT_TABS)[number];

/** The gmail whose own window is open (`?account`, apart from the outline's `?gmail` scope), on its tab. */
export function useAccountWindow(): { gmail: number; tab: AccountTab } | null {
  const raw = useQuery("account");
  const tab = useQuery("tab");
  if (raw === null) return null;
  const gmail = Number(raw);
  if (!Number.isSafeInteger(gmail)) throw new Error(`?account=${raw}: not a gmail id`);
  if (!ACCOUNT_TABS.includes(tab as AccountTab)) throw new Error(`?tab=${tab}: not a tab of gmail ${gmail}'s window`);
  return { gmail, tab: tab as AccountTab };
}

export function openAccount(at: { gmail: number; tab: AccountTab } | null): void {
  setQuery({ account: at === null ? null : String(at.gmail), tab: at?.tab ?? null, gmb: null, trade: null, categories: null, trash: null, add: null, kit: null });
}

/** What the caller removed (`?trash`), restorable until purged. */
export function useTrashWindow(): boolean {
  return useQuery("trash") !== null;
}

export function openTrash(on: boolean): void {
  setQuery({ trash: on ? "" : null, add: null, kit: null, trade: null, tab: null, gmb: null, account: null, categories: null });
}

/** The review kit's area window (`?kit`): `""` draws a new area, a slug shows that one. */
export function useKitWindow(): string | null {
  return useQuery("kit");
}

export function openKit(area: string | null): void {
  setQuery({ kit: area, add: null, trade: null, tab: null, gmb: null, account: null, categories: null, trash: null });
}

/** What the Add a GBP panel opens with: the outline's gmail and trade, when it is scoped to one. */
export type Adding = { gmail: number | null; trade: number | null };

const addValue = ({ gmail, trade }: Adding) => `${gmail ?? ""},${trade ?? ""}`;

/** The Add a GBP panel over the map (`?add=<gmail>,<trade>`, each empty for none). */
export function useAddWindow(): { raw: string; adding: Adding } | null {
  const raw = useQuery("add");
  if (raw === null) return null;
  const parts = raw.split(",");
  if (parts.length !== 2) throw new Error(`?add=${raw}: not <gmail>,<trade>`);
  const id = (p: string) => {
    if (p === "") return null;
    const n = Number(p);
    if (!Number.isSafeInteger(n)) throw new Error(`?add=${raw}: ${p} is no id`);
    return n;
  };
  return { raw, adding: { gmail: id(parts[0]!), trade: id(parts[1]!) } };
}

export function openAdd(at: Adding | null): void {
  setQuery({ add: at === null ? null : addValue(at), kit: null, gmb: null, trade: null, tab: null, categories: null, account: null, trash: null });
}

/** The home page with the panel open, from another page. */
export const addHref = (at: Adding) => `/?add=${addValue(at)}`;
