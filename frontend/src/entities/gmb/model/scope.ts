"use client";

import { setQuery, useQuery } from "@/shared/lib/query";

import { COUNTRIES, type Country, type Gmb } from "./gmb";

/** A category pattern, `<trade>.<country>`, either side `*`: `1.FR` a category, `2.*` a trade in every country, `*.US` a country across trades. */
export interface Pattern {
  trade: number | "*";
  country: Country | "*";
}

/** What the outline is scoped to: the gmails and the category patterns, each `null` for all; a GBP is in when both facets take it. */
export interface Scope {
  gmails: ReadonlySet<number> | null;
  categories: readonly Pattern[] | null;
}

function parsePattern(raw: string): Pattern {
  const [trade, country, ...rest] = raw.split(".");
  const id = Number(trade);
  if (rest.length > 0 || trade === undefined || country === undefined || (trade !== "*" && !Number.isSafeInteger(id))) throw new Error(`?category=${raw}: not <trade>.<country>`);
  if (country !== "*" && !(COUNTRIES as string[]).includes(country)) throw new Error(`?category=${raw}: ${country} is not one of ${COUNTRIES.join(", ")}`);
  if (trade === "*" && country === "*") throw new Error(`?category=${raw}: a pattern names a trade or a country`);
  return { trade: trade === "*" ? "*" : id, country: country as Country | "*" };
}

export const patternText = (p: Pattern) => `${p.trade}.${p.country}`;

export function useScope(): Scope {
  const gmail = useQuery("gmail");
  const category = useQuery("category");
  return {
    gmails:
      gmail === null
        ? null
        : new Set(
            gmail.split(",").map((s) => {
              const id = Number(s);
              if (!Number.isSafeInteger(id)) throw new Error(`?gmail=${gmail}: not gmail ids`);
              return id;
            }),
          ),
    categories: category === null ? null : category.split(",").map(parsePattern),
  };
}

/** Whether `p` takes `trade` in `country` (`null`: none yet, which only a `*` takes). */
export function covers(p: Pattern, trade: number, country: Country | null): boolean {
  return (p.trade === "*" || p.trade === trade) && (p.country === "*" || p.country === country);
}

/** Whether the GBP is under any of `patterns`: one with no trade is under none. */
export function inCategories(gmb: Gmb, patterns: readonly Pattern[]): boolean {
  const trade = gmb.trade_id;
  return trade !== null && patterns.some((p) => covers(p, trade, gmb.country?.code ?? null));
}

export function inScope(gmb: Gmb, scope: Scope): boolean {
  return (scope.gmails === null || (gmb.gmail_id !== null && scope.gmails.has(gmb.gmail_id))) && (scope.categories === null || inCategories(gmb, scope.categories));
}

export function chooseGmailScope(gmails: Iterable<number> | null): void {
  const ids = gmails === null ? [] : [...gmails].sort((a, b) => a - b);
  setQuery({ gmail: ids.length === 0 ? null : ids.join(","), gmbs: null });
}

export function chooseCategoryScope(patterns: readonly Pattern[] | null): void {
  const texts = [...new Set((patterns ?? []).map(patternText))].sort();
  setQuery({ category: texts.length === 0 ? null : texts.join(","), gmbs: null });
}
