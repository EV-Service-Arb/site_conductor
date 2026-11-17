"use client";

import { useSyncExternalStore } from "react";

import { ApiError, api, ok } from "@/shared/api";
import { bool, num, object, parse, recordOf } from "@/shared/lib/parse";
import { covers, useScope, useTrades } from "@/entities/gmb";
import type { Area, Category, ErrorCode, Trade } from "@/shared/api/schema";
import type { Event, Legend, Notice, Overlay, Summary, Tune } from "@/shared/placement_overlay/placement_overlay";
import { setQuery, useQuery } from "@/shared/lib/query";
import { useResource } from "@/shared/lib/use-resource";

export type { Legend, Tune };
export type Data = Summary;
export type Layer = Summary["layers"][number];

export type Placement =
  | { status: "loading" }
  | { status: "unconnected" }
  | { status: "error"; message: string }
  | {
      status: "ok";
      /** The trade files gop serves, a category's `study` one of them. */
      trades: readonly string[];
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
    let catalog;
    try {
      catalog = await ok(api.GET("/placement/catalog"));
    } catch (e) {
      if (e instanceof ApiError && e.failure.kind === "unavailable") {
        set({ status: "unconnected" });
        return null;
      }
      throw e;
    }
    const drawing = await import("@/shared/placement_overlay/placement_overlay");
    await drawing.default();
    set({ status: "ok", trades: catalog.trades, countries: catalog.countries, data: null, legend: null, notice: null, pulling: false, pullFailed: null, refused: null });
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

/** The payload of `q`, as placement wrote it: the overlay parses it. */
export async function payloadOf(q: Query): Promise<string> {
  return ok(api.GET("/placement/study", { params: { query: q }, parseAs: "text" }));
}

export async function pull(q: Query): Promise<void> {
  const overlay = await attached;
  if (overlay === null) throw new Error("placement: a pull with no overlay attached");
  update({ pulling: true, pullFailed: null });
  try {
    const payload = await ok(api.POST("/placement/study/pull", { params: { query: q }, parseAs: "text" }));
    overlay.show(payload, true);
    update({ pulling: false });
  } catch (e) {
    update({ pulling: false, pullFailed: e instanceof ApiError && "message" in e.failure ? e.failure.message : e instanceof Error ? e.message : String(e) });
  }
}

/** A category by the URL's spelling of it, `<trade id>.<country>`. */
export type CategoryKey = `${number}.${string}`;
export const categoryKey = (trade: number, country: string): CategoryKey => `${trade}.${country}`;

export interface MapView {
  heat: string | null;
  /** The category whose study is drawn: the one picked, else the one category in the outline's scope that has a study. */
  study: CategoryKey | null;
  /** Where `study` comes from. */
  from: "pick" | "scope" | null;
  /** The area it is drawn over, by slug; `""`: one being drawn. */
  area: string | null;
  /** On, at its radius in metres or at the trade's own. */
  pressure: { lambda?: number } | null;
  competitors: boolean;
  /** Tiers whose weight or visibility was moved off the study's own. */
  tiers: Record<string, { weight: number; show: boolean }> | null;
  opacity: number | null;
  hideImputed: boolean;
}

const tiersParser = recordOf(object({ weight: num, show: bool }));

/** The panel's state, in the URL of `/` as `?at` is; `?study=` empty: none picked, whatever the scope. */
export function useMapView(): MapView {
  const heat = useQuery("heat");
  const study = useQuery("study");
  const area = useQuery("area");
  const pressure = useQuery("pressure");
  const competitors = useQuery("competitors");
  const tiers = useQuery("tiers");
  const opacity = useQuery("opacity");
  const imputed = useQuery("imputed");
  const scoped = useScopedStudy();
  const lambda = pressure === null || pressure === "" ? undefined : Number(pressure);
  if (lambda !== undefined && !(Number.isFinite(lambda) && lambda > 0)) throw new Error(`?pressure=${pressure}: not a radius in metres`);
  if (study !== null && study !== "" && !/^\d+\.[A-Z]{2}$/.test(study)) throw new Error(`?study=${study}: not <trade id>.<country>`);
  const alpha = opacity === null ? null : Number(opacity);
  if (alpha !== null && !(alpha >= 0 && alpha <= 1)) throw new Error(`?opacity=${opacity}: not within 0..1`);
  return {
    heat,
    study: study === null ? scoped : study === "" ? null : (study as CategoryKey),
    from: study !== null && study !== "" ? "pick" : study === null && scoped !== null ? "scope" : null,
    area,
    pressure: pressure === null ? null : lambda === undefined ? {} : { lambda },
    competitors: competitors !== null,
    tiers: tiers === null ? null : parse(tiersParser, JSON.parse(tiers)),
    opacity: alpha,
    hideImputed: imputed !== null,
  };
}

/** The one category in the outline's scope that has a study. */
function useScopedStudy(): CategoryKey | null {
  const { categories: scope } = useScope();
  const trades = useTrades();
  if (scope === null || trades.status !== "ok") return null;
  const studied = trades.data.flatMap((tr) => tr.categories.filter((c) => c.study !== null && scope.some((p) => covers(p, tr.id, c.country))).map((c) => categoryKey(tr.id, c.country)));
  return studied.length === 1 ? studied[0]! : null;
}

export function setMapView(change: Partial<MapView>): void {
  setQuery({
    ...(change.heat !== undefined && { heat: change.heat }),
    ...(change.study !== undefined && { study: change.study ?? "" }),
    ...(change.area !== undefined && { area: change.area }),
    ...(change.pressure !== undefined && { pressure: change.pressure === null ? null : change.pressure.lambda === undefined ? "" : String(Math.round(change.pressure.lambda)) }),
    ...(change.competitors !== undefined && { competitors: change.competitors ? "" : null }),
    ...(change.tiers !== undefined && { tiers: change.tiers === null ? null : JSON.stringify(change.tiers) }),
    ...(change.opacity !== undefined && { opacity: change.opacity === null ? null : String(change.opacity) }),
    ...(change.hideImputed !== undefined && { imputed: change.hideImputed ? "" : null }),
  });
}

/** Why the conductor would not build a study: its own words for it. */
export type Refusal = Extract<ErrorCode, "no_study" | "no_google_category" | "no_keywords" | "another_country" | "unstudied">;
const REFUSALS: readonly string[] = ["no_study", "no_google_category", "no_keywords", "another_country", "unstudied"] satisfies Refusal[];

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
  // keyed by the pick: an area drawn a moment ago is read with it
  const areas = useResource(`areas.${view.area}`, async () => (await ok(api.GET("/areas"))).areas, "areas");
  if (trades.status === "error") return { status: "error", message: trades.failure.kind };
  if (areas.status === "error") return { status: "error", message: areas.failure.kind };
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
  if (view.area !== null && view.area !== "" && area === null && areas.fresh) throw new Error(`?area=${view.area}: no such area`);
  const drawn = area === null ? null : { area: area.slug, ...(view.study !== null && { category: view.study }) };
  return { status: "ok", areas: areas.data, area, picked, drawn };
}

/** The payload of what `studied` draws, or why the conductor would not build it. */
export type Payload = { status: "loading" } | { status: "error"; message: string } | { status: "refused"; refused: Refusal; message: string } | { status: "ok"; payload: string | null; fresh: boolean };

export function usePayload(drawn: Query | null): Payload {
  const key = drawn === null ? "none" : `${drawn.area}.${drawn.category ?? ""}`;
  const read = useResource(`placement.${key}`, () => (drawn === null ? Promise.resolve(null) : payloadOf(drawn)), "placement");
  if (read.status === "loading") return read;
  if (read.status === "ok") return { status: "ok", payload: read.data, fresh: read.fresh };
  const f = read.failure;
  if (f.kind === "refused" && REFUSALS.includes(f.code)) return { status: "refused", refused: f.code as Refusal, message: f.message };
  return { status: "error", message: "message" in f ? f.message : f.kind };
}

/** What the overlay was handed, told to the panel. */
export function landed(p: Payload): void {
  if (state.status !== "ok") return; // the overlay is not attached yet: nothing is drawn to say why of
  const refused = p.status === "refused" ? { refused: p.refused, message: p.message } : p.status === "error" ? { failed: p.message } : null;
  if (JSON.stringify(refused) !== JSON.stringify(state.refused)) update({ refused });
}
