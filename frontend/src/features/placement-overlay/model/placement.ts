"use client";

import { useState, useSyncExternalStore } from "react";

import { Refused, api, ok } from "@/shared/api";
import { covers, useScope, useTrades } from "@/entities/gmb";
import type { Area, Category, Trade } from "@/shared/api/schema";
import type { Event, Legend, Notice, Overlay, Summary, Tune } from "@/shared/placement_overlay/placement_overlay";
import { type Home, setHome, useHome } from "@/shared/lib/location";
import { useResource } from "@evinvest/api/react";

export type { Legend, Tune };
export type Data = Summary;
export type Layer = Summary["layers"][number];

export type Placement =
  | { status: "loading" }
  | { status: "unconnected" }
  | { status: "error"; message: string }
  | {
      status: "ok";
      /** The countries gop has a grid for. */
      countries: readonly string[];
      data: Data | null;
      legend: Legend | null;
      notice: (Notice & { text: string }) | null;
      pulling: boolean;
      pullFailed: string | null;
      /** Why the study drawn is not: the conductor's refusal, or a failure. */
      refused: { refused: Refusal; message: string } | { failed: string } | null;
    };

let state: Placement = { status: "loading" };
const listeners = new Set<() => void>();
const subscribe = (onChange: () => void) => {
  listeners.add(onChange);
  return () => listeners.delete(onChange);
};
function set(next: Placement) {
  state = next;
  for (const l of listeners) l();
}
function update(change: Partial<Extract<Placement, { status: "ok" }>>) {
  if (state.status !== "ok") throw new Error(`placement: an update while ${state.status}`);
  set({ ...state, ...change });
}

export function usePlacement(): Placement {
  return useSyncExternalStore(subscribe, () => state, () => state);
}

/** Once per page session, as the map is: the map outlives its page, and so does what is drawn on it. */
let attached: Promise<Overlay | null> | null = null;

export function attach(map: google.maps.Map): Promise<Overlay | null> {
  attached ??= (async () => {
    const answer = await api.GET("/placement/catalog");
    if (answer.error !== undefined) {
      if (answer.error.code === "unavailable") {
        set({ status: "unconnected" });
        return null;
      }
      throw new Error(answer.error.error);
    }
    const drawing = await import("@/shared/placement_overlay/placement_overlay");
    await drawing.default();
    set({ status: "ok", countries: answer.data.countries, data: null, legend: null, notice: null, pulling: false, pullFailed: null, refused: null });
    return new drawing.Overlay(map, (e: Event) => {
      switch (e.kind) {
        case "data":
          return update({ data: e.detail });
        case "legend":
          return update({ legend: e.detail });
        case "notice":
          return update({ notice: e.detail.text === null ? null : { ...e.detail, text: e.detail.text } });
      }
    });
  })().catch((e: unknown) => {
    set({ status: "error", message: e instanceof Error ? e.message : String(e) });
    return null;
  });
  return attached;
}

/** What a study is asked by: the area, and the category over it or none. */
export type Query = { area: string; category?: CategoryKey };

export async function pull(q: Query): Promise<void> {
  const overlay = await attached;
  if (overlay === null) throw new Error("placement: a pull with no overlay attached");
  update({ pulling: true, pullFailed: null });
  try {
    overlay.show(await ok(api.POST("/placement/study/pull", { params: { query: q }, parseAs: "text" })), true);
    update({ pulling: false });
  } catch (e) {
    update({ pulling: false, pullFailed: e instanceof Refused ? e.failure.error : e instanceof Error ? e.message : String(e) });
  }
}

/** A category by the URL's spelling of it, `<trade id>.<country>`. */
export type CategoryKey = `${number}.${string}`;
export const categoryKey = (trade: number, country: string): CategoryKey => `${trade}.${country}`;

export interface MapView {
  heat: string | null;
  /** The category whose study is drawn: the one picked, else the one studiable category in the outline's scope. */
  study: CategoryKey | null;
  /** Where `study` comes from. */
  from: "pick" | "scope" | null;
  /** The area it is drawn over, by slug; `""`: one being drawn. */
  area: string | null;
  /** On, at this radius in metres. */
  pressure: number | null;
  competitors: boolean;
  /** Tiers whose weight or visibility was moved off the study's own. */
  tiers: NonNullable<Home["tiers"]> | null;
  opacity: number | null;
  hideImputed: boolean;
}

/** The panel's state, in Home's URL as `?at` is; `?study=` empty: none picked, whatever the scope. */
export function useMapView(): MapView {
  const home = useHome();
  if (home === null) throw new Error("the map's panel off Home");
  const { study } = home;
  const scoped = useScopedStudy();
  return {
    heat: home.heat ?? null,
    study: study === undefined ? scoped : study === "" ? null : (study as CategoryKey),
    from: study !== undefined && study !== "" ? "pick" : study === undefined && scoped !== null ? "scope" : null,
    area: home.area ?? null,
    pressure: home.pressure ?? null,
    competitors: home.competitors === true,
    tiers: home.tiers ?? null,
    opacity: home.opacity ?? null,
    hideImputed: home.hide_imputed === true,
  };
}

/** A category placement can study: one with a primary Google category, in a country it has a grid for. */
export const studiable = (c: Category, countries: readonly string[]) => c.google_primary !== null && countries.includes(c.country);

/** The one studiable category in the outline's scope. */
function useScopedStudy(): CategoryKey | null {
  const { categories: scope } = useScope();
  const trades = useTrades();
  const placement = usePlacement();
  if (scope === null || trades.status !== "ok" || placement.status !== "ok") return null;
  const { countries } = placement;
  const studied = trades.data.flatMap((tr) => tr.categories.filter((c) => studiable(c, countries) && scope.some((p) => covers(p, tr.id, c.country))).map((c) => categoryKey(tr.id, c.country)));
  return studied.length === 1 ? studied[0]! : null;
}

export function setMapView(change: Partial<MapView>): void {
  setHome({
    ...(change.heat !== undefined && { heat: change.heat ?? undefined }),
    ...(change.study !== undefined && { study: change.study ?? "" }),
    ...(change.area !== undefined && { area: change.area ?? undefined }),
    ...(change.pressure !== undefined && { pressure: change.pressure === null ? undefined : Math.round(change.pressure) }),
    ...(change.competitors !== undefined && { competitors: change.competitors }),
    ...(change.tiers !== undefined && { tiers: change.tiers ?? undefined }),
    ...(change.opacity !== undefined && { opacity: change.opacity ?? undefined }),
    ...(change.hideImputed !== undefined && { hide_imputed: change.hideImputed }),
  });
}

/** Why the conductor would not build a study: its own words for it. */
export type Refusal = "no_google_category" | "another_country" | "unstudied";
const REFUSALS: readonly string[] = ["no_google_category", "another_country", "unstudied"] satisfies Refusal[];

export type Studied =
  | { status: "loading" }
  | { status: "error"; message: string }
  | {
      status: "ok";
      areas: readonly Area[];
      area: Area | null;
      picked: { trade: Trade; category: Category } | null;
      /** What is asked over `area`; `null` while no area is picked. */
      drawn: Query | null;
    };

/** The URL's study and area, resolved against the categories and the areas. */
export function useStudied(view: MapView): Studied {
  const trades = useTrades();
  const read = useResource(api, "/areas");
  const areas = read.status === "ok" ? { ...read, data: read.data.areas } : read;
  // an area drawn a moment ago may postdate the last read: read again once before calling it gone
  const missing = view.area !== null && view.area !== "" && areas.status === "ok" && !areas.data.some((a) => a.slug === view.area) ? view.area : null;
  const [asked, setAsked] = useState<string | null>(null);
  if (missing !== null && asked !== missing) {
    setAsked(missing);
    read.reload();
  }
  if (trades.status === "error") return { status: "error", message: trades.failure.code };
  if (areas.status === "error") return { status: "error", message: areas.failure.code };
  if (trades.status !== "ok" || areas.status !== "ok") return { status: "loading" };
  let picked: { trade: Trade; category: Category } | null = null;
  if (view.study !== null) {
    const [id, country] = view.study.split(".");
    const trade = trades.data.find((tr) => String(tr.id) === id);
    const category = trade?.categories.find((c) => c.country === country);
    if (trade === undefined || category === undefined) throw new Error(`?study=${view.study}: no such category`);
    picked = { trade, category };
  }
  const area = view.area === null || view.area === "" ? null : (areas.data.find((a) => a.slug === view.area) ?? null);
  if (missing !== null && asked === missing && areas.fresh) throw new Error(`?area=${view.area}: no such area`);
  const drawn = area === null ? null : { area: area.slug, ...(view.study !== null && { category: view.study }) };
  return { status: "ok", areas: areas.data, area, picked, drawn };
}

/** The payload of what is drawn, or why the conductor would not build it. */
export type Payload = { status: "loading" } | { status: "error"; message: string } | { status: "refused"; refused: Refusal; message: string } | { status: "ok"; payload: string; fresh: boolean };

export function usePayload(drawn: Query): Payload {
  const read = useResource(api, "/placement/study", { params: { query: drawn }, parseAs: "text" });
  if (read.status === "loading") return read;
  if (read.status === "ok") return { status: "ok", payload: read.data, fresh: read.fresh };
  const { code, error } = read.failure;
  return REFUSALS.includes(code) ? { status: "refused", refused: code as Refusal, message: error } : { status: "error", message: error };
}

/** What the overlay was handed, told to the panel. */
export function landed(p: Payload | null): void {
  if (state.status !== "ok") return; // the overlay is not attached yet: nothing is drawn to say why of
  const refused = p?.status === "refused" ? { refused: p.refused, message: p.message } : p?.status === "error" ? { failed: p.message } : null;
  if (JSON.stringify(refused) !== JSON.stringify(state.refused)) update({ refused });
}
