"use client";

import { type Resource, useResource } from "@evinvest/api/react";
import { type UndoEntry, useUndo } from "@evinvest/uikit";
import { type ReactNode, createContext, useContext, useMemo } from "react";

import { type Failure, api, reviewArchive, sites as sitesApi } from "@/shared/api";
import type { Brand, Gmails, Trade, Trash } from "@/shared/api/schema";
import type { MyGmbs } from "@/shared/api/services/review_archive";
import type { GmbPlace } from "@/shared/api/services/sites";

import type { Tags } from "./issues";

type Read<T> = Resource<T, Failure> & { reload: () => void };

interface Gmbs {
  gmails: Read<Gmails>;
  trades: Read<{ trades: Trade[] }>;
  brands: Read<{ brands: Brand[] }>;
  archive: Read<MyGmbs>;
  sites: Read<{ [gmb: string]: GmbPlace }>;
  trash: Read<Trash>;
}

const GmbsContext = createContext<Gmbs | null>(null);

/** One read each of the caller's gmails, their trades and the services' tags on their GMBs, for the outline and the page beside it: a change made in one is seen by the other. */
export function GmailsProvider({ children }: { children: ReactNode }) {
  const gmails = useResource(api, "/gmails");
  const trades = useResource(api, "/trades");
  const brands = useResource(api, "/brands");
  const archive = useResource(reviewArchive, "/me/gmbs");
  // refused to whoever sees no sites: their GMBs have no sites tags
  const sites = useResource(sitesApi, "/gmbs");
  const trash = useResource(api, "/trash");
  return <GmbsContext value={{ gmails, trades, brands, archive, sites, trash }}>{children}</GmbsContext>;
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
  const trades = useGmbs().trades;
  return trades.status === "ok" ? { ...trades, data: trades.data.trades } : trades;
}

export function useBrands(): Read<Brand[]> {
  const brands = useGmbs().brands;
  return brands.status === "ok" ? { ...brands, data: brands.data.brands } : brands;
}

/** The services' reads, and their tags and the trades as an issue is judged by: one not answered yet has none. */
export function useGmbTags(): { archive: Gmbs["archive"]; sites: Gmbs["sites"]; tags: Tags } {
  const { archive, sites } = useGmbs();
  const trades = useTrades();
  const gmbs = archive.status === "ok" ? archive.data.gmbs : null;
  const places = sites.status === "ok" ? sites.data : null;
  // the keys are GMB ids, as the schemas' patterns have them
  const byGmb = useMemo(() => (gmbs === null ? null : new Map(Object.entries(gmbs).map(([id, tag]) => [Number(id), tag]))), [gmbs]);
  const placed = useMemo(() => (places === null ? null : new Map(Object.entries(places).map(([id, tag]) => [Number(id), tag]))), [places]);
  return {
    archive,
    sites,
    tags: {
      archive: byGmb,
      sites: placed,
      trades: trades.status === "ok" ? trades.data : null,
    },
  };
}
