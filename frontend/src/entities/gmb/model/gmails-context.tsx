"use client";

import { type ReactNode, createContext, useContext } from "react";

import { type Resource, useResource } from "@/shared/lib/use-resource";

import { fetchGmails, fetchTrades } from "../api/gmbs";
import { fetchArchive, fetchSiteTags } from "../api/tags";
import type { Archive, Gmails, SiteTag, Trade } from "./gmb";
import type { Tags } from "./issues";

type Read<T> = Resource<T> & { reload: () => void };

interface Gmbs {
  gmails: Read<Gmails>;
  trades: Read<Trade[]>;
  archive: Read<Archive | null>;
  sites: Read<Map<number, SiteTag> | null>;
}

const GmbsContext = createContext<Gmbs | null>(null);

/** One read each of the caller's gmails, their trades and the services' tags on their GMBs, for the outline and the page beside it: a change made in one is seen by the other. */
export function GmailsProvider({ seesSites, children }: { seesSites: boolean; children: ReactNode }) {
  // a gmail's `browser` follows its browser's changes
  const gmails = useResource("gmbs", fetchGmails, "gmbs", { focus: true, live: "browsers" });
  const trades = useResource("trades", fetchTrades, "trades", { focus: true });
  const archive = useResource("gmbs.archive", fetchArchive, "gmbs.archive", { focus: true });
  const sites = useResource(`gmbs.sites.${seesSites}`, () => (seesSites ? fetchSiteTags() : Promise.resolve(null)), "gmbs.sites", { focus: true });
  return <GmbsContext value={{ gmails, trades, archive, sites }}>{children}</GmbsContext>;
}

function useGmbs(): Gmbs {
  const gmbs = useContext(GmbsContext);
  if (gmbs === null) throw new Error("used outside GmailsProvider");
  return gmbs;
}

export function useGmails(): Read<Gmails> {
  return useGmbs().gmails;
}

export function useTrades(): Read<Trade[]> {
  return useGmbs().trades;
}

/** The services' reads, and their tags and the trades as an issue is judged by: one not answered yet has none. */
export function useGmbTags(): { archive: Gmbs["archive"]; sites: Gmbs["sites"]; tags: Tags } {
  const { archive, sites, trades } = useGmbs();
  return {
    archive,
    sites,
    tags: {
      archive: archive.status === "ok" ? (archive.data?.gmbs ?? null) : null,
      sites: sites.status === "ok" ? sites.data : null,
      trades: trades.status === "ok" ? trades.data : null,
    },
  };
}
