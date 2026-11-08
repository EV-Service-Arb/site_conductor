"use client";

import { useSyncExternalStore } from "react";

import { ApiError, http, readCsrf } from "@/shared/api";
import { type Infer, arrayOf, bool, nullable, num, object, oneOf, parse, str } from "@/shared/lib/parse";
import { setQuery, useQuery } from "@/shared/lib/query";

// gmaps_optimal_placement behind the forward (docs/ARCHITECTURE.md, Forward): its overlay module
// draws the heat, the pressure field and the competitors on the map it is handed.
const API = "/api/placement";
const MODULE = "/placement/mfe/overlay.js";
/** Below it a 200 m grid is noise, and a country's worth of payloads would load. */
export const GRID_ZOOM = 10;

const studiesParser = object({
  trades: arrayOf(str),
  locations: arrayOf(object({ name: str, bbox: arrayOf(num), country: str })),
});
type Location = Infer<typeof studiesParser>["locations"][number];

const layerParser = object({ id: str, name: str, note: str, trade: bool });
const layersParser = object({ layers: arrayOf(layerParser), tiers: arrayOf(object({ name: str, weight: num })), lambda_m: nullable(num) });
const legendParser = object({
  heat: nullable(object({ id: str, name: str, note: str, scale: oneOf(["percentile", "linear"] as const), stops: arrayOf(str), ticks: arrayOf(str) })),
  pressure: nullable(object({ stops: arrayOf(str), note: str })),
});
const noticeParser = object({ location: str, trade: nullable(str), text: nullable(str), missing: nullable(num) });

export type Layer = Infer<typeof layerParser>;
export type Legend = Infer<typeof legendParser>;
export type Notice = Infer<typeof noticeParser> & { text: string };

interface Handle extends EventTarget {
  show(trade: string | null, locations: string[]): void;
  heat(layer: string | null): void;
  pressure(opts: { lambda?: number } | null): void;
  competitors(on: boolean): void;
  pull(): Promise<void>;
}
interface OverlayModule {
  overlay(map: google.maps.Map, opts: { api: string; csrf: () => string }): Promise<Handle>;
}

/** What is in view: too far out for the grid, or the locations whose frame meets it (none: no grid here). */
export type InView = "zoom" | readonly string[];

export type Placement =
  | { status: "loading" }
  | { status: "unconnected" }
  | { status: "error"; message: string }
  | {
      status: "ok";
      trades: readonly string[];
      inView: InView;
      layers: readonly Layer[];
      lambda: number | null;
      legend: Legend | null;
      notices: readonly Notice[];
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
let attached: Promise<{ handle: Handle; locations: readonly Location[] } | null> | null = null;

export function attach(map: google.maps.Map): Promise<{ handle: Handle; locations: readonly Location[] } | null> {
  attached ??= (async () => {
    let studies: Infer<typeof studiesParser>;
    try {
      studies = await http.get(`${API}/studies.json`, studiesParser);
    } catch (e) {
      if (e instanceof ApiError && e.failure.kind === "unavailable") {
        set({ status: "unconnected" });
        return null;
      }
      throw e;
    }
    for (const l of studies.locations) if (l.bbox.length !== 4) throw new Error(`placement: ${l.name}'s bbox is not [west, south, east, north]`);
    const { overlay } = (await import(/* webpackIgnore: true */ /* turbopackIgnore: true */ MODULE)) as OverlayModule;
    const handle = await overlay(map, {
      api: API,
      csrf: () => {
        const token = readCsrf(document.cookie);
        if (token === null) throw new Error("no CSRF cookie beside a signed-in session");
        return token;
      },
    });
    set({ status: "ok", trades: studies.trades, inView: "zoom", layers: [], lambda: null, legend: null, notices: [], pulling: false, pullFailed: null });
    const on = <T>(name: string, parser: (v: unknown, path: string) => T, then: (v: T) => void) =>
      handle.addEventListener(name, (e) => then(parse(parser, (e as CustomEvent<unknown>).detail)));
    on("layers", layersParser, ({ layers, lambda_m }) => update({ layers, lambda: lambda_m }));
    on("legend", legendParser, (legend) => update({ legend }));
    on("notice", noticeParser, (n) => {
      const rest = (state.status === "ok" ? state.notices : []).filter((o) => o.location !== n.location || o.trade !== n.trade);
      update({ notices: n.text === null ? rest : [...rest, { ...n, text: n.text }] });
    });
    return { handle, locations: studies.locations };
  })().catch((e: unknown) => {
    set({ status: "error", message: e instanceof Error ? e.message : String(e) });
    return null;
  });
  return attached;
}

export function locationsIn(locations: readonly Location[], bounds: google.maps.LatLngBounds, zoom: number): InView {
  if (zoom < GRID_ZOOM) return "zoom";
  return locations
    .filter(({ bbox: [w, s, e, n] }) => bounds.intersects({ west: w!, south: s!, east: e!, north: n! }))
    .map((l) => l.name);
}

export function setInView(inView: InView) {
  if (state.status === "ok" && state.inView !== inView) update({ inView });
}

export async function pull(): Promise<void> {
  const live = await attached;
  if (live === null) throw new Error("placement: a pull with no overlay attached");
  update({ pulling: true, pullFailed: null });
  try {
    await live.handle.pull();
    update({ pulling: false });
  } catch (e) {
    update({ pulling: false, pullFailed: e instanceof Error ? e.message : String(e) });
  }
}

export interface MapView {
  heat: string | null;
  trade: string | null;
  /** On, at its radius in metres or at the trade's own. */
  pressure: { lambda?: number } | null;
  competitors: boolean;
}

/** The panel's state, in the URL of `/` as `?at` is. */
export function useMapView(): MapView {
  const heat = useQuery("heat");
  const trade = useQuery("trade");
  const pressure = useQuery("pressure");
  const competitors = useQuery("competitors");
  const lambda = pressure === null || pressure === "" ? undefined : Number(pressure);
  if (lambda !== undefined && !(Number.isFinite(lambda) && lambda > 0)) throw new Error(`?pressure=${pressure}: not a radius in metres`);
  return { heat, trade, pressure: pressure === null ? null : lambda === undefined ? {} : { lambda }, competitors: competitors !== null };
}

export function setMapView(change: Partial<MapView>): void {
  setQuery({
    ...(change.heat !== undefined && { heat: change.heat }),
    ...(change.trade !== undefined && { trade: change.trade }),
    ...(change.pressure !== undefined && { pressure: change.pressure === null ? null : change.pressure.lambda === undefined ? "" : String(Math.round(change.pressure.lambda)) }),
    ...(change.competitors !== undefined && { competitors: change.competitors ? "" : null }),
  });
}
