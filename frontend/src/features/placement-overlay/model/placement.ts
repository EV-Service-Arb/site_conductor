"use client";

import { useSyncExternalStore } from "react";

import { ApiError, api, http, ok, readCsrf } from "@/shared/api";
import { type Infer, arrayOf, bool, nullable, num, object, oneOf, parse, recordOf, str } from "@/shared/lib/parse";
import { covers, useScope, useTrades } from "@/entities/gmb";
import type { Area, Category, Trade } from "@/shared/api/schema";
import { setQuery, useQuery } from "@/shared/lib/query";
import { useResource } from "@/shared/lib/use-resource";

// gmaps_optimal_placement behind the forward (docs/ARCHITECTURE.md, Forward): its overlay module
// builds a category's study over an area and draws it on the map it is handed.
const API = "/api/placement";
const MODULE = "/mfe/placement/overlay.js";

const servedParser = object({ trades: arrayOf(str), countries: arrayOf(str) });

const layerParser = object({ id: str, name: str, note: str, trade: bool });
const dataParser = nullable(
  object({
    name: str,
    cells: num,
    imputed: num,
    layers: arrayOf(layerParser),
    trade: nullable(
      object({
        lambda_m: num,
        tiers: arrayOf(object({ name: str, weight: num, fetched_s: arrayOf(num) })),
        categories: arrayOf(object({ kind: str, label: str, color: str, counts: arrayOf(num) })),
        queries: arrayOf(object({ text: str, hits: num })),
        terms: arrayOf(object({ text: str, weight: num })),
        unweighted: arrayOf(str),
        searches: num,
      }),
    ),
  }),
);
const legendParser = object({
  heat: nullable(object({ id: str, name: str, note: str, scale: oneOf(["percentile", "linear"] as const), stops: arrayOf(str), cells: num, ticks: nullable(arrayOf(str)), focus: nullable(str) })),
  pressure: nullable(object({ stops: arrayOf(str), note: str, levels: nullable(arrayOf(object({ q: num, value: str }))) })),
  competitors: nullable(object({ in_view: num })),
});
const noticeParser = object({ text: nullable(str), missing: nullable(num) });

export type Layer = Infer<typeof layerParser>;
export type Data = NonNullable<Infer<typeof dataParser>>;
export type Legend = Infer<typeof legendParser>;
export type Notice = Infer<typeof noticeParser> & { text: string };

/** What gop builds and caches: the conductor's category applied to the conductor's area; no category, the area's grid alone. */
type Definition = {
  area: { slug: string; country: string; margin_m: number } & ({ kind: "city"; viewport: { south: number; west: number; north: number; east: number } } | { kind: "circle"; center: { lat: number; lng: number }; radius_m: number });
  category: { trade: number; country: string; study: string; keywords: string[]; google_primary: { id: string; name: string }; google_additional: { id: string; name: string }[] } | null;
};

export type Tuning = { lambda_m: number | null; tiers: Record<string, { weight: number; show: boolean }> | null };

interface Handle extends EventTarget {
  show(def: Definition | null): Promise<void>;
  heat(layer: string | null): void;
  tune(t: Tuning): void;
  pressure(on: boolean): void;
  competitors(on: boolean): void;
  opacity(x: number): void;
  hideImputed(on: boolean): void;
  pull(): Promise<void>;
}
interface OverlayModule {
  overlay(map: google.maps.Map, opts: { api: string; csrf: () => string }): Promise<Handle>;
}

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
      notice: Notice | null;
      pulling: boolean;
      pullFailed: string | null;
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
let attached: Promise<Handle | null> | null = null;

export function attach(map: google.maps.Map): Promise<Handle | null> {
  attached ??= (async () => {
    let served: Infer<typeof servedParser>;
    try {
      served = await http.get(`${API}/trades.json`, servedParser);
    } catch (e) {
      if (e instanceof ApiError && e.failure.kind === "unavailable") {
        set({ status: "unconnected" });
        return null;
      }
      throw e;
    }
    const { overlay } = (await import(/* webpackIgnore: true */ /* turbopackIgnore: true */ MODULE)) as OverlayModule;
    const handle = await overlay(map, {
      api: API,
      csrf: () => {
        const token = readCsrf(document.cookie);
        if (token === null) throw new Error("no CSRF cookie beside a signed-in session");
        return token;
      },
    });
    set({ status: "ok", trades: served.trades, countries: served.countries, data: null, legend: null, notice: null, pulling: false, pullFailed: null });
    const on = <T>(name: string, parser: (v: unknown, path: string) => T, then: (v: T) => void) =>
      handle.addEventListener(name, (e) => then(parse(parser, (e as CustomEvent<unknown>).detail)));
    on("data", dataParser, (data) => update({ data }));
    on("legend", legendParser, (legend) => update({ legend }));
    on("notice", noticeParser, (n) => update({ notice: n.text === null ? null : { ...n, text: n.text } }));
    return handle;
  })().catch((e: unknown) => {
    set({ status: "error", message: e instanceof Error ? e.message : String(e) });
    return null;
  });
  return attached;
}

export async function pull(): Promise<void> {
  const handle = await attached;
  if (handle === null) throw new Error("placement: a pull with no overlay attached");
  update({ pulling: true, pullFailed: null });
  try {
    await handle.pull();
    update({ pulling: false });
  } catch (e) {
    update({ pulling: false, pullFailed: e instanceof Error ? e.message : String(e) });
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

export type Refusal = "no study" | "no google category" | "no keywords" | "another country";

export type Studied =
  | { status: "loading" }
  | { status: "error"; message: string }
  | {
      status: "ok";
      areas: readonly Area[];
      area: Area | null;
      picked: { trade: Trade; category: Category } | null;
      /** What is shown over `area`; `null` while no area is picked. */
      drawn: { definition: Definition } | { refused: Refusal } | null;
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
  const drawn = area === null ? null : picked === null ? { definition: gridOnly(area) } : define(picked.trade, picked.category, area);
  return { status: "ok", areas: areas.data, area, picked, drawn };
}

/** Why a category cannot be studied, or its definition over `area`. */
function define(trade: Trade, category: Category, area: Area): { refused: Refusal } | { definition: Definition } {
  if (category.study === null) return { refused: "no study" };
  if (category.google_primary === null) return { refused: "no google category" };
  if (category.keywords.length === 0) return { refused: "no keywords" };
  if (area.country !== category.country) return { refused: "another country" };
  const named = (g: { id: string; names: Record<string, string> }) => {
    const name = g.names[category.lang];
    if (name === undefined) throw new Error(`${g.id} has no name in ${category.lang}, ${category.country}'s language`);
    return { id: g.id, name };
  };
  return {
    definition: {
      area: framed(area),
      category: { trade: trade.id, country: category.country, study: category.study, keywords: [...category.keywords], google_primary: named(category.google_primary), google_additional: category.google_additional.map(named) },
    },
  };
}

/** The area's grid alone: no category, nothing billed. */
function gridOnly(area: Area): Definition {
  return { area: framed(area), category: null };
}

function framed(a: Area): Definition["area"] {
  const base = { slug: a.slug, country: a.country, margin_m: a.margin_m };
  return a.shape.kind === "city" ? { ...base, kind: "city", viewport: a.shape.viewport } : { ...base, kind: "circle", center: a.center, radius_m: a.shape.radius_m };
}
