"use client";

import { useSyncExternalStore } from "react";

import type { KitAt } from "@/entities/gmb";
import { type Country, countryValues } from "@/shared/api/schema";
import type { At } from "@/shared/lib/location";

/** A kit's page for `k`; its editor has none: it draws on the map. */
export function pageOf(k: KitAt): At {
  if (k.edit) throw new Error("a kit's editor is over the map only");
  if (k.kit === "") return { view: "kits", params: {} };
  if (k.gmb !== undefined) return { view: "kit_gmb", params: { slug: k.kit, gmb: k.gmb } };
  if (k.pool !== undefined) {
    const { trade, country } = poolOf(k.pool);
    return { view: "kit_pool", params: { slug: k.kit, trade, country } };
  }
  return { view: "kit", params: { slug: k.kit } };
}

/** Where in a kit a page is; `null`: no kit's page. */
export function kitOfPage(at: At): KitAt | null {
  switch (at.view) {
    case "kits":
      return { kit: "" };
    case "kit":
      return { kit: at.params.slug };
    case "kit_gmb":
      return { kit: at.params.slug, gmb: at.params.gmb };
    case "kit_pool":
      return { kit: at.params.slug, pool: `${at.params.trade}.${at.params.country}` };
    default:
      return null;
  }
}

/** `<trade>.<country>`. */
export function poolOf(pool: string): { trade: number; country: Country } {
  const [trade, country] = pool.split(".");
  const c = countryValues.find((v) => v === country);
  if (trade === undefined || !Number.isSafeInteger(Number(trade)) || c === undefined) throw new Error(`kit_pool=${pool}: not <trade>.<country>`);
  return { trade: Number(trade), country: c };
}

export type Period = "today" | "7d" | "30d" | "all";
export const PERIODS: readonly Period[] = ["today", "7d", "30d", "all"];
const DAYS: Record<Period, number | null> = { today: 0, "7d": 7, "30d": 30, all: null };

let period: Period = "today";
const listeners = new Set<() => void>();

/** The period a kit's tallies are over, the page's while it lives: the same in each of its views. */
export function usePeriod(): Period {
  return useSyncExternalStore(
    (on) => {
      listeners.add(on);
      return () => listeners.delete(on);
    },
    () => period,
    () => period,
  );
}

export function setPeriod(p: Period): void {
  period = p;
  for (const l of listeners) l();
}

/** Unix seconds the period starts at: a local midnight, so it holds still through the day. */
export function sinceOf(p: Period): number {
  const days = DAYS[p];
  if (days === null) return 0;
  const d = new Date();
  d.setHours(0, 0, 0, 0);
  d.setDate(d.getDate() - days);
  return Math.floor(d.getTime() / 1000);
}
