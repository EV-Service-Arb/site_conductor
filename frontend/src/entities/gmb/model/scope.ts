"use client";

import { setQuery, useQuery } from "@/shared/lib/query";

/** The gmail every app's GMBs are narrowed to, `null` for all of them. */
export function useGmailScope(): number | null {
  const raw = useQuery("gmail");
  if (raw === null) return null;
  const id = Number(raw);
  if (!Number.isSafeInteger(id)) throw new Error(`?gmail=${raw}: not a gmail id`);
  return id;
}

export function chooseGmailScope(gmail: number | null): void {
  setQuery({ gmail: gmail === null ? null : String(gmail), gmbs: null });
}
