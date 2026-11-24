"use client";

import { flushSync } from "react-dom";

import { openWindow, setOnMap, useGmails } from "@/entities/gmb";
import type { Gmb } from "@/shared/api/schema";

/** Onto a GBP just added: its window, or its point placed first when it has none. */
export function useLanded(): (added: Gmb) => void {
  const gmails = useGmails();
  return (added) => {
    // committed first: the window opening takes the list without the GBP for one it was removed from, unless that is being read again
    flushSync(gmails.reload);
    if (added.location === null) setOnMap({ placing: added.id });
    else openWindow({ kind: "gbp", gmb: added.id, tab: added.tabs[0]!.tab });
  };
}
