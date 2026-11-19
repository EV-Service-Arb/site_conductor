"use client";

import { useSyncExternalStore } from "react";

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
