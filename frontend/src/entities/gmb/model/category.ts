import type { Category, CategorySettings, Country, GoogleCategory, Gmb } from "@/shared/api/schema";

import { googleName } from "./gmb";

/** A GBP's category: its trade (a gcid) in its own country, set up there or not; never another country's. */
export type Placed =
  | { status: "no_trade" }
  | { status: "no_country"; trade: string }
  | { status: "not_set_up"; trade: string; country: Country }
  | { status: "set_up"; category: Category };

export function categoryOf(gmb: Gmb, categories: readonly Category[]): Placed {
  if (gmb.trade === null) return { status: "no_trade" };
  if (gmb.country === null) return { status: "no_country", trade: gmb.trade };
  const country = gmb.country.code;
  const category = categories.find((c) => c.trade.id === gmb.trade && c.country === country);
  return category === undefined ? { status: "not_set_up", trade: gmb.trade, country } : { status: "set_up", category };
}

/** "Plumber · FR", the trade in the reader's language. */
export function categoryName(trade: GoogleCategory, country: Country, locale: string): string {
  return `${googleName(trade, locale)} · ${country}`;
}

/** The language Google's form is typed in there (sa_core::gbp::Country::lang). */
export const LANG = { FR: "fr", US: "en", CA: "en", GB: "en" } as const satisfies Record<Country, string>;

/** As placement stores a keyword. */
export const normalized = (s: string) => s.trim().split(/\s+/).join(" ").toLowerCase();

/** What a trade picked off Google's list starts from: its name in the country's language, at weight 1. */
export const emptySettings = (trade: GoogleCategory, country: Country): CategorySettings => ({
  keywords: [{ text: normalized(googleName(trade, LANG[country])), weight: 1 }],
  additional: [],
  excluded: { words: [], types: [] },
  demand: null,
});

