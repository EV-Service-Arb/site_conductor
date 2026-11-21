"use client";

import { useEffect, useSyncExternalStore } from "react";

import { useLocale } from "@/shared/i18n";

import { type Failure, Refused, api, ok } from "@/shared/api";
import { type Country, type Gmb, type GoogleCategory, countryValues } from "@/shared/api/schema";

import { useCategories } from "./gmails-context";
import { googleName } from "./gmb";

/** Google's closed list in a country: one read a page session, shared by every reader of it. */
export type GoogleList = { status: "loading" } | { status: "ok"; data: readonly GoogleCategory[] } | { status: "error"; failure: Failure };

const lists = new Map<Country, GoogleList>();
const listeners = new Set<() => void>();
let version = 0;

function load(country: Country) {
  if (lists.has(country)) return;
  lists.set(country, { status: "loading" });
  void ok(api.GET("/google-categories/{country}", { params: { path: { country } } }))
    .then((d): GoogleList => ({ status: "ok", data: d.categories }))
    .catch((e: unknown): GoogleList => {
      if (!(e instanceof Refused)) throw e;
      return { status: "error", failure: e.failure };
    })
    .then((l) => {
      lists.set(country, l);
      version += 1;
      for (const on of listeners) on();
    });
}

const subscribe = (on: () => void) => {
  listeners.add(on);
  return () => listeners.delete(on);
};

/** Google's list in `country`. */
export function useGoogleList(country: Country): GoogleList {
  useSyncExternalStore(subscribe, () => version, () => version);
  useEffect(() => load(country), [country]);
  return lists.get(country) ?? { status: "loading" };
}

/** Each gcid's names, off the lists of `countries` (a gcid is named alike in every list holding it); `null` until all are read. */
export function useGoogleNames(countries: readonly Country[]): Named {
  useSyncExternalStore(subscribe, () => version, () => version);
  const key = countries.join();
  useEffect(() => {
    for (const c of key.split(",")) if (c !== "") load(c as Country);
  }, [key]);
  const read = countries.map((c) => lists.get(c) ?? { status: "loading" as const });
  const failed = read.find((l) => l.status === "error");
  if (failed !== undefined) return failed;
  if (read.some((l) => l.status !== "ok")) return { status: "loading" };
  const was = merged.get(key);
  if (was?.version === version) return was.named;
  const named: Named = { status: "ok", data: new Map(read.flatMap((l) => (l.status === "ok" ? l.data.map((g) => [g.id, g] as const) : []))) };
  merged.set(key, { version, named });
  return named;
}

type Named = { status: "loading" } | { status: "ok"; data: ReadonlyMap<string, GoogleCategory> } | { status: "error"; failure: Failure };
const merged = new Map<string, { version: number; named: Named }>();

/** Trade `gcid`, named: off the caller's categories, else Google's list in `country` (every country's for a GBP with none); `null` until read. */
export function useTrade(gcid: string | null, country: Country | null): GoogleCategory | null {
  const categories = useCategories();
  const own = categories.status === "ok" && gcid !== null ? categories.data.find((c) => c.trade.id === gcid)?.trade : undefined;
  const names = useGoogleNames(gcid === null || categories.status !== "ok" || own !== undefined ? [] : country !== null ? [country] : countryValues);
  if (own !== undefined) return own;
  if (gcid === null) return null;
  if (categories.status !== "ok" || names.status !== "ok") return null;
  const g = names.data.get(gcid);
  if (g === undefined) throw new Error(`${gcid} is not among Google's categories${country === null ? "" : ` in ${country}`}`);
  return g;
}

const fold = (s: string) => s.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
const WORD = /[^\p{L}\p{N}]+/u;

/** Whether `text` holds every term of `search`, its case and accents aside. */
export const matches = (text: string, search: string) => fold(search).split(/\s+/).every((term) => fold(text).includes(term));

/** `list` sorted in the reader's language, then those every term of `search` matches (the reader's name, `lang`'s or the gcid's slug): prefix, then word-start, then the rest. */
export function rankGoogle(list: readonly GoogleCategory[], search: string, locale: string, lang: string): { g: GoogleCategory; shown: string }[] {
  const collator = new Intl.Collator(locale);
  const indexed = list
    .map((g) => {
      const shown = googleName(g, locale);
      const name = fold(shown);
      const slug = [g.id.replace(/^gcid:/, "").replaceAll("_", " "), fold(googleName(g, lang))].join(" ");
      return { g, shown, name, slug, nameWords: name.split(WORD), words: [...name.split(WORD), ...slug.split(WORD)] };
    })
    .sort((a, b) => collator.compare(a.shown, b.shown));
  const terms = fold(search).split(/\s+/).filter((x) => x !== "");
  const whole = terms.join(" ");
  return indexed
    .filter((x) => terms.every((term) => x.name.includes(term) || x.slug.includes(term)))
    .map((x) => {
      const starts = (words: string[]) => terms.every((term) => words.some((w) => w.startsWith(term)));
      // the name, shown and typed, before the slug
      return { x, rank: [x.name.startsWith(whole), x.slug.startsWith(whole), starts(x.nameWords), starts(x.words), true].indexOf(true) };
    })
    // stable: the collator's order within a rank
    .sort((a, b) => a.rank - b.rank)
    .map(({ x }) => ({ g: x.g, shown: x.shown }));
}

/** A trade the caller deals in: one they have a category of, or one a GBP of theirs holds; the countries it is set up in. */
export type Traded = { trade: GoogleCategory; setUp: readonly Country[] };

/** Every trade of the caller's categories and of `gmbs`, in the reader's order; `null` until named. */
export function useTrades(gmbs: readonly Gmb[]): readonly Traded[] | null {
  const locale = useLocale();
  const categories = useCategories();
  const own = categories.status === "ok" ? categories.data : [];
  const loose = gmbs.filter((b) => b.trade !== null && !own.some((c) => c.trade.id === b.trade));
  const needed = loose.some((b) => b.country === null) ? countryValues : countryValues.filter((c) => loose.some((b) => b.country?.code === c));
  const names = useGoogleNames(categories.status === "ok" ? needed : []);
  if (categories.status !== "ok" || names.status !== "ok") return null;
  const byId = new Map<string, Traded>();
  for (const c of own) byId.set(c.trade.id, { trade: c.trade, setUp: [...(byId.get(c.trade.id)?.setUp ?? []), c.country] });
  for (const b of loose) {
    const g = names.data.get(b.trade!);
    if (g === undefined) throw new Error(`GBP ${b.id}'s trade ${b.trade} is not among Google's categories`);
    if (!byId.has(g.id)) byId.set(g.id, { trade: g, setUp: [] });
  }
  const collator = new Intl.Collator(locale);
  return [...byId.values()].sort((a, b) => collator.compare(googleName(a.trade, locale), googleName(b.trade, locale)));
}
