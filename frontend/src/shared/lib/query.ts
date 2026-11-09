"use client";

import { useSyncExternalStore } from "react";

/** The query keys the conductor keeps its own state in: a reload or a shared link opens the page as it was left. */
export type QueryKey = "gmbs" | "gmail" | "find" | "issue" | "assistant" | "gmb" | "at" | "chat" | "heat" | "trade" | "pressure" | "competitors" | "base";
type Query = Readonly<Record<QueryKey, string | null>>;

/** The outline's and the assistant's: they are beside every page, so they follow it from page to page. */
const CARRIED: readonly QueryKey[] = ["gmbs", "gmail", "find", "issue", "assistant"];
const KEYS: readonly QueryKey[] = [...CARRIED, "gmb", "at", "chat", "heat", "trade", "pressure", "competitors", "base"];

let state: Query | undefined;
const listeners = new Set<() => void>();

const subscribe = (onChange: () => void) => {
  listeners.add(onChange);
  return () => listeners.delete(onChange);
};

function read(): Query {
  const params = new URLSearchParams(window.location.search);
  return Object.fromEntries(KEYS.map((k) => [k, params.get(k)])) as Query;
}

function write(next: Query) {
  const here = read();
  if (KEYS.some((k) => here[k] !== next[k])) {
    const url = new URL(window.location.href);
    for (const k of KEYS) {
      const v = next[k];
      if (v === null) url.searchParams.delete(k);
      else url.searchParams.set(k, v);
    }
    window.history.replaceState(null, "", url);
  }
  const was = state;
  state = next;
  if (was === undefined || KEYS.some((k) => was[k] !== next[k])) for (const l of listeners) l();
}

export function useQuery(key: QueryKey): string | null {
  return useSyncExternalStore(subscribe, () => (state ??= read())[key], () => null);
}

export function setQuery(change: Partial<Query>): void {
  write({ ...(state ??= read()), ...change });
}

/** After every move of the location: what it says, and the outline's keys it dropped kept. */
export function syncQuery(): void {
  const here = read();
  const was = state ?? here;
  write({ ...here, ...Object.fromEntries(CARRIED.filter((k) => here[k] === null && was[k] !== null).map((k) => [k, was[k]])) });
}
