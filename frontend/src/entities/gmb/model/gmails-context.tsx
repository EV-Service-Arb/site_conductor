"use client";

import { type UndoEntry, useUndo } from "@evinvest/uikit";
import { type ReactNode, createContext, useContext } from "react";

import { api, ok } from "@/shared/api";
import type { Gmails, Trade, Trash } from "@/shared/api/schema";
import { type Resource, useResource } from "@/shared/lib/use-resource";

import { fetchArchive, fetchSiteTags } from "../api/tags";
import type { Archive, SiteTag } from "./gmb";
import type { Tags } from "./issues";

type Read<T> = Resource<T> & { reload: () => void };

interface Gmbs {
  gmails: Read<Gmails>;
  trades: Read<Trade[]>;
  archive: Read<Archive | null>;
  sites: Read<Map<number, SiteTag> | null>;
  trash: Read<Trash>;
}

const GmbsContext = createContext<Gmbs | null>(null);

/** One read each of the caller's gmails, their trades and the services' tags on their GMBs, for the outline and the page beside it: a change made in one is seen by the other. */
export function GmailsProvider({ seesSites, children }: { seesSites: boolean; children: ReactNode }) {
  // on focus too: Google's pulls change GBPs, and are no one's write
  const gmails = useResource("gmbs", () => ok(api.GET("/gmails")), "gmbs", { focus: true, live: ["gmails", "gmbs", "trash"] });
  const trades = useResource("trades", async () => (await ok(api.GET("/trades"))).trades, "trades", { live: ["trades"] });
  const archive = useResource("gmbs.archive", fetchArchive, "gmbs.archive", { focus: true });
  const sites = useResource(`gmbs.sites.${seesSites}`, () => (seesSites ? fetchSiteTags() : Promise.resolve(null)), "gmbs.sites", { focus: true });
  // on focus too: the hourly purge is no one's write
  const trash = useResource("trash", () => ok(api.GET("/trash")), "trash", { focus: true, live: ["trash", "gmails", "gmbs"] });
  // whatever changes the gmails may put something in the trash or take it out
  const both = { ...gmails, reload: () => (gmails.reload(), trash.reload()) };
  return <GmbsContext value={{ gmails: both, trades, archive, sites, trash: { ...trash, reload: both.reload } }}>{children}</GmbsContext>;
}

function useGmbs(): Gmbs {
  const gmbs = useContext(GmbsContext);
  if (gmbs === null) throw new Error("used outside GmailsProvider");
  return gmbs;
}

export function useGmails(): Read<Gmails> {
  return useGmbs().gmails;
}

/** Pushes a GMB write's entry to the page's history: the gmails are re-read after its undo and its redo, as after the write. */
export function useUndoable(): (entry: UndoEntry) => void {
  const push = useUndo();
  const { reload } = useGmails();
  return (e) => push({ label: e.label, undo: () => e.undo().finally(reload), redo: () => e.redo().finally(reload) });
}

export function useTrash(): Read<Trash> {
  return useGmbs().trash;
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
