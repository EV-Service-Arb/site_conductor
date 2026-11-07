"use client";

import { setQuery, useQuery } from "@/shared/lib/query";

/** The gmails whose GMBs the filters select, `null` for every gmail. */
export function useGmailScope(): ReadonlySet<number> | null {
  const raw = useQuery("gmail");
  if (raw === null) return null;
  return new Set(
    raw.split(",").map((s) => {
      const id = Number(s);
      if (!Number.isSafeInteger(id)) throw new Error(`?gmail=${raw}: not gmail ids`);
      return id;
    }),
  );
}

export function chooseGmailScope(gmails: Iterable<number> | null): void {
  const ids = gmails === null ? [] : [...gmails].sort((a, b) => a - b);
  setQuery({ gmail: ids.length === 0 ? null : ids.join(","), gmbs: null });
}
