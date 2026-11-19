"use client";

import { useSyncExternalStore } from "react";

import { type Country, type Tab, countryValues } from "@/shared/api/schema";
import { type At, type Home, go, setHome, useAt, viewed } from "@/shared/lib/location";

/** Where in a review kit: `kit` its slug (`""`: the list); a GBP of it, or a pool (`<trade>.<country>`). */
export type KitAt = { kit: string; gmb?: number | undefined; pool?: string | undefined };

/** What the Add a GBP panel opens with: the outline's gmail and trade, when it is scoped to one. */
export type Adding = NonNullable<Home["add"]>;

/** A window over the map, or as its own page: what it shows, its tab and where in it. */
export type WinKey =
  | { kind: "gbp"; gmb: number; tab: Tab }
  | { kind: "account"; gmail: number; tab: Tab }
  | { kind: "trade"; trade: number; country: Country | null; brand: number | null }
  | { kind: "categories" }
  | { kind: "trash" }
  | { kind: "add"; at: Adding }
  | { kind: "kit"; at: KitAt };
export type Kind = WinKey["kind"];
export type KeyOf<K extends Kind> = Extract<WinKey, { kind: K }>;

/** Every Home key a window reads: one floats at a time, so opening one writes its own and clears the rest. */
const NO_WINDOW = { gmb: undefined, trade: undefined, country: undefined, brand: undefined, account: undefined, tab: undefined, categories: undefined, trash: undefined, add: undefined, kit: undefined, kit_gmb: undefined, kit_pool: undefined, kit_edit: undefined } as const;

interface Route<K extends Kind> {
  fromHome(h: Home): KeyOf<K> | null;
  toHome(k: KeyOf<K>): { [P in keyof Home]?: Home[P] | undefined };
  page(k: KeyOf<K>): At;
  fromPage(at: At): KeyOf<K> | null;
  /** Its entry in the open set: one per GBP, gmail, trade, kit. */
  subject(k: KeyOf<K>): string;
}

/** `<trade>.<country>`. */
export function poolOf(pool: string): { trade: number; country: Country } {
  const [trade, country] = pool.split(".");
  const c = countryValues.find((v) => v === country);
  if (trade === undefined || !Number.isSafeInteger(Number(trade)) || c === undefined) throw new Error(`kit_pool=${pool}: not <trade>.<country>`);
  return { trade: Number(trade), country: c };
}

export const WINDOWS = {
  gbp: {
    // with no `tab` a GBP has its card on the map, no window
    fromHome: (h) => (h.gmb !== undefined && h.tab !== undefined && h.account === undefined ? { kind: "gbp", gmb: h.gmb, tab: h.tab } : null),
    toHome: (k) => ({ gmb: k.gmb, tab: k.tab }),
    page: (k) => ({ view: "gbp", params: { gmb: k.gmb, tab: k.tab } }),
    // every GBP's first tab
    fromPage: (at) => (at.view === "gbp" ? { kind: "gbp", gmb: at.params.gmb, tab: at.params.tab ?? "profile" } : null),
    subject: (k) => `gbp:${k.gmb}`,
  },
  account: {
    fromHome: (h) => {
      if (h.account === undefined) return null;
      if (h.tab === undefined) throw new Error(`?account=${h.account}: its window has no ?tab`);
      return { kind: "account", gmail: h.account, tab: h.tab };
    },
    toHome: (k) => ({ account: k.gmail, tab: k.tab }),
    page: (k) => ({ view: "gmail", params: { account: k.gmail, tab: k.tab } }),
    // every gmail's first tab
    fromPage: (at) => (at.view === "gmail" ? { kind: "account", gmail: at.params.account, tab: at.params.tab ?? "connections" } : null),
    subject: (k) => `account:${k.gmail}`,
  },
  trade: {
    fromHome: (h) => {
      if (h.trade === undefined) return null;
      if (h.brand !== undefined && h.country === undefined) throw new Error(`?brand=${h.brand}: its page sits under a country's tab`);
      return { kind: "trade", trade: h.trade, country: h.country ?? null, brand: h.brand ?? null };
    },
    toHome: (k) => ({ trade: k.trade, country: k.country ?? undefined, brand: k.brand ?? undefined }),
    page: (k) => ({ view: "category", params: { trade: k.trade, ...(k.country !== null && { country: k.country }), ...(k.brand !== null && { brand: k.brand }) } }),
    fromPage: (at) => {
      if (at.view !== "category") return null;
      if (at.params.brand !== undefined && at.params.country === undefined) throw new Error(`?brand=${at.params.brand}: its page sits under a country's tab`);
      return { kind: "trade", trade: at.params.trade, country: at.params.country ?? null, brand: at.params.brand ?? null };
    },
    subject: (k) => `trade:${k.trade}`,
  },
  categories: {
    fromHome: (h) => (h.categories === true ? { kind: "categories" } : null),
    toHome: () => ({ categories: true }),
    page: () => ({ view: "categories", params: {} }),
    fromPage: (at) => (at.view === "categories" ? { kind: "categories" } : null),
    subject: () => "categories",
  },
  trash: {
    fromHome: (h) => (h.trash === true ? { kind: "trash" } : null),
    toHome: () => ({ trash: true }),
    page: () => ({ view: "trash", params: {} }),
    fromPage: (at) => (at.view === "trash" ? { kind: "trash" } : null),
    subject: () => "trash",
  },
  add: {
    fromHome: (h) => (h.add !== undefined ? { kind: "add", at: h.add } : null),
    toHome: (k) => ({ add: k.at }),
    page: (k) => ({ view: "adding", params: { ...(k.at.gmail != null && { account: k.at.gmail }), ...(k.at.trade != null && { trade: k.at.trade }) } }),
    fromPage: (at) => (at.view === "adding" ? { kind: "add", at: { ...(at.params.account !== undefined && { gmail: at.params.account }), ...(at.params.trade !== undefined && { trade: at.params.trade }) } } : null),
    subject: () => "add",
  },
  kit: {
    // its editor draws on the map: a map mode, no window
    fromHome: (h) => (h.kit !== undefined && h.kit_edit !== true ? { kind: "kit", at: { kit: h.kit, gmb: h.kit_gmb, pool: h.kit_pool } } : null),
    toHome: (k) => ({ kit: k.at.kit, kit_gmb: k.at.gmb, kit_pool: k.at.pool }),
    page: ({ at }) => {
      if (at.kit === "") return { view: "kits", params: {} };
      if (at.gmb !== undefined) return { view: "kit_gmb", params: { slug: at.kit, gmb: at.gmb } };
      if (at.pool !== undefined) {
        const { trade, country } = poolOf(at.pool);
        return { view: "kit_pool", params: { slug: at.kit, trade, country } };
      }
      return { view: "kit", params: { slug: at.kit } };
    },
    fromPage: (at) => {
      switch (at.view) {
        case "kits":
          return { kind: "kit", at: { kit: "" } };
        case "kit":
          return { kind: "kit", at: { kit: at.params.slug } };
        case "kit_gmb":
          return { kind: "kit", at: { kit: at.params.slug, gmb: at.params.gmb } };
        case "kit_pool":
          return { kind: "kit", at: { kit: at.params.slug, pool: `${at.params.trade}.${at.params.country}` } };
        default:
          return null;
      }
    },
    subject: (k) => `kit:${k.at.kit}`,
  },
} satisfies { [K in Kind]: Route<K> };

const KINDS = Object.keys(WINDOWS) as Kind[];
const route = (k: WinKey) => WINDOWS[k.kind] as Route<Kind>;

export const pageOf = (k: WinKey): At => route(k).page(k);
export const subjectOf = (k: WinKey): string => route(k).subject(k);

/** The window the location shows: over the map, or as its page. */
export type Shown = { key: WinKey; page: boolean };

export function shownAt(at: At): Shown | null {
  for (const kind of KINDS) {
    const r = WINDOWS[kind] as Route<Kind>;
    const key = at.view === "home" ? r.fromHome(at.params) : r.fromPage(at);
    if (key !== null) return { key, page: at.view !== "home" };
  }
  return null;
}

export function useShown(): Shown | null {
  return shownAt(useAt());
}

/** Over the map, one at a time. */
export function floatWindow(k: WinKey): void {
  setHome({ ...NO_WINDOW, ...route(k).toHome(k) });
}

export function pageWindow(k: WinKey, how: "push" | "replace" = "push"): void {
  go(pageOf(k), how);
}

/** As its page where a window's page is shown, else over the map. */
export function openWindow(k: WinKey): void {
  if (shownAt(viewed().at)?.page === true) return pageWindow(k);
  floatWindow(k);
}

/** Out of sight and still open: its keys gone, a GBP's card on its pin with them. */
export function hideWindow(): void {
  setHome(NO_WINDOW);
}

/** Out of the open set; out of sight first when it is the one shown. */
export function closeWindow(k: WinKey): void {
  const subject = subjectOf(k);
  writeOpen(readOpen().filter((o) => subjectOf(o) !== subject));
  const shown = shownAt(viewed().at);
  if (shown !== null && subjectOf(shown.key) === subject) hideWindow();
}

/** A review kit's editor: a map mode over the map, no window. */
export function editKit(slug: string | null): void {
  setHome({ ...NO_WINDOW, ...(slug !== null && { kit: slug, kit_edit: true }) });
}

export function useKitEdit(): string | null {
  const at = useAt();
  return at.view === "home" && at.params.kit_edit === true ? (at.params.kit ?? "") : null;
}

/** The GMB whose card is open on the map. */
export function usePeeked(): number | null {
  const at = useAt();
  return at.view === "home" ? (at.params.gmb ?? null) : null;
}

export function peek(gmb: number | null): void {
  setHome({ gmb: gmb ?? undefined });
}

/** GBP `gmb`'s card on its pin, and no window. */
export function cardOnly(gmb: number): void {
  setHome({ ...NO_WINDOW, gmb });
}

/** The open set: what this tab has open, shown or not, in the order each was first shown (sessionStorage). */
const STORE = "sa-windows";
let cached: { raw: string | null; keys: readonly WinKey[] } = { raw: null, keys: [] };
const listeners = new Set<() => void>();

function readOpen(): readonly WinKey[] {
  const raw = sessionStorage.getItem(STORE);
  if (raw === cached.raw) return cached.keys;
  const keys: unknown = raw === null ? [] : JSON.parse(raw);
  if (!Array.isArray(keys) || keys.some((k) => typeof k !== "object" || k === null || !KINDS.includes((k as WinKey).kind))) throw new Error(`sessionStorage["${STORE}"] is not a list of windows: ${raw}`);
  cached = { raw, keys: keys as WinKey[] };
  return cached.keys;
}

function writeOpen(keys: readonly WinKey[]) {
  sessionStorage.setItem(STORE, JSON.stringify(keys));
  for (const l of listeners) l();
}

const NONE: readonly WinKey[] = [];

export function useOpenWindows(): readonly WinKey[] {
  return useSyncExternalStore(
    (on) => {
      listeners.add(on);
      return () => listeners.delete(on);
    },
    readOpen,
    () => NONE,
  );
}

/** `k` shown: in the open set, or its entry brought to where it now is (its tab, its country). */
export function keepOpen(k: WinKey): void {
  const subject = subjectOf(k);
  const keys = readOpen();
  const at = keys.findIndex((o) => subjectOf(o) === subject);
  if (at === -1) return writeOpen([...keys, k]);
  if (JSON.stringify(keys[at]) !== JSON.stringify(k)) writeOpen(keys.map((o, i) => (i === at ? k : o)));
}
