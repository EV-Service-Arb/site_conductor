"use client";

import { useState, useSyncExternalStore } from "react";

import { Refused, api, ok } from "@/shared/api";
import { covers, draftAt, useCategories, useScope } from "@/entities/gmb";
import { type Area, type Category, type Country, countryValues } from "@/shared/api/schema";
import type { Event, Legend, Notice, Overlay, Summary, Tune } from "@/shared/placement_overlay/placement_overlay";
import { setHome, useHome } from "@/shared/lib/location";
import { useResource } from "@evinvest/api/react";

export type { Legend, Tune };
export type Ages = NonNullable<Legend["ages"]>;
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
        case "mark":
          return draftAt(e.detail);
      }
    });
  })().catch((e: unknown) => {
    set({ status: "error", message: e instanceof Error ? e.message : String(e) });
    return null;
  });
  return attached;
}

/** What a study is asked by: the area, and the category version over it or none; or a published study, its own area and version. */
export type Query = { area: string; version?: number } | { sha: string };

export async function pull(q: Query): Promise<void> {
  const overlay = await attached;
  if (overlay === null) throw new Error("placement: a pull with no overlay attached");
  update({ pulling: true, pullFailed: null });
  try {
    const answer = "sha" in q ? api.POST("/placement/studies/{sha}/pull", { params: { path: { sha: q.sha } }, parseAs: "text" }) : api.POST("/placement/study/pull", { params: { query: q }, parseAs: "text" });
    overlay.show(await ok(answer), true);
    update({ pulling: false });
  } catch (e) {
    update({ pulling: false, pullFailed: e instanceof Refused ? e.failure.error : e instanceof Error ? e.message : String(e) });
  }
}

/** A study's category version, as `?study` spells it: `<gcid>.<country>` the caller's category's head, `v<id>` a version, `s<sha>` a published study. */
export type StudyKey = { kind: "head"; trade: string; country: Country } | { kind: "version"; id: number } | { kind: "published"; sha: string };

export function parseStudy(raw: string): StudyKey {
  if (raw.startsWith("v")) {
    const id = Number(raw.slice(1));
    if (!Number.isSafeInteger(id)) throw new Error(`?study=${raw}: not v<version id>`);
    return { kind: "version", id };
  }
  if (raw.startsWith("s")) return { kind: "published", sha: raw.slice(1) };
  const [trade, country, ...rest] = raw.split(".");
  const c = countryValues.find((v) => v === country);
  if (trade === undefined || !trade.startsWith("gcid:") || c === undefined || rest.length > 0) throw new Error(`?study=${raw}: not <gcid>.<country>, v<id> nor s<sha>`);
  return { kind: "head", trade, country: c };
}

export const studyText = (k: StudyKey): string => (k.kind === "head" ? `${k.trade}.${k.country}` : k.kind === "version" ? `v${k.id}` : `s${k.sha}`);

export interface MapView {
  heat: string | null;
  /** The category version whose study is drawn: the one picked, else the head of the one studiable category in the outline's scope. */
  study: StudyKey | null;
  /** Where `study` comes from. */
  from: "pick" | "scope" | null;
  /** The area it is drawn over, by slug; `""`: one being drawn. */
  area: string | null;
  pressure: boolean;
  competitors: boolean;
  /** The tiers of competitors shown; `null`: every one. */
  tiers: readonly string[] | null;
  opacity: number;
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
    study: study === undefined ? scoped : study === "" ? null : parseStudy(study),
    from: study !== undefined && study !== "" ? "pick" : study === undefined && scoped !== null ? "scope" : null,
    area: home.area ?? null,
    pressure: home.pressure === true,
    competitors: home.competitors === true,
    tiers: home.tiers_shown ?? null,
    opacity: home.opacity ?? 0.5,
    hideImputed: home.hide_imputed === true,
  };
}

/** The tiers of `data` shown, `null` every one: as `?heat`, `?tiers_shown` waits for the study that has them. */
export function tiersShown(view: MapView, data: Data | null): readonly string[] | null {
  const own = data?.trade?.tiers.map((x) => x.name);
  return view.tiers?.every((x) => own?.includes(x)) ? view.tiers : null;
}

/** A category placement can study: one in a country it has a grid for. */
export const studiable = (c: Category, countries: readonly string[]) => countries.includes(c.country);

/** The head of the one studiable category in the outline's scope. */
function useScopedStudy(): StudyKey | null {
  const { categories: scope } = useScope();
  const categories = useCategories();
  const placement = usePlacement();
  if (scope === null || categories.status !== "ok" || placement.status !== "ok") return null;
  const { countries } = placement;
  const studied = categories.data.filter((c) => studiable(c, countries) && scope.some((p) => covers(p, c.trade.id, c.country)));
  return studied.length === 1 ? { kind: "head", trade: studied[0]!.trade.id, country: studied[0]!.country } : null;
}

export function setMapView(change: Partial<MapView>): void {
  setHome({
    ...(change.heat !== undefined && { heat: change.heat ?? undefined }),
    ...(change.study !== undefined && { study: change.study === null ? "" : studyText(change.study) }),
    ...(change.area !== undefined && { area: change.area ?? undefined }),
    ...(change.pressure !== undefined && { pressure: change.pressure }),
    ...(change.competitors !== undefined && { competitors: change.competitors }),
    ...(change.tiers !== undefined && { tiers_shown: change.tiers === null ? undefined : [...change.tiers] }),
    ...(change.opacity !== undefined && { opacity: change.opacity }),
    ...(change.hideImputed !== undefined && { hide_imputed: change.hideImputed }),
  });
}

/** Why the conductor would not build a study: its own words for it. */
export type Refusal = "another_country" | "unstudied";
const REFUSALS: readonly string[] = ["another_country", "unstudied"] satisfies Refusal[];

export type Studied =
  | { status: "loading" }
  | { status: "error"; message: string }
  | {
      status: "ok";
      areas: readonly Area[];
      area: Area | null;
      /** The caller's category whose head is picked. */
      head: Category | null;
      /** What is asked over `area`; `null` while no area is picked. */
      drawn: Query | null;
    };

/** The URL's study and area, resolved against the categories and the areas. */
export function useStudied(view: MapView): Studied {
  const categories = useCategories();
  const read = useResource(api, "/areas");
  const areas = read.status === "ok" ? { ...read, data: read.data.areas } : read;
  // an area drawn a moment ago may postdate the last read: read again once before calling it gone
  const missing = view.area !== null && view.area !== "" && areas.status === "ok" && !areas.data.some((a) => a.slug === view.area) ? view.area : null;
  const [asked, setAsked] = useState<string | null>(null);
  if (missing !== null && asked !== missing) {
    setAsked(missing);
    read.reload();
  }
  if (categories.status === "error") return { status: "error", message: categories.failure.code };
  if (areas.status === "error") return { status: "error", message: areas.failure.code };
  if (categories.status !== "ok" || areas.status !== "ok") return { status: "loading" };
  const { study } = view;
  let head: Category | null = null;
  if (study?.kind === "head") {
    head = categories.data.find((c) => c.trade.id === study.trade && c.country === study.country) ?? null;
    if (head === null) throw new Error(`?study=${studyText(study)}: no such category of the caller's`);
  }
  const area = view.area === null || view.area === "" ? null : (areas.data.find((a) => a.slug === view.area) ?? null);
  if (missing !== null && asked === missing && areas.fresh) throw new Error(`?area=${view.area}: no such area`);
  const drawn: Query | null = area === null ? null : study?.kind === "published" ? { sha: study.sha } : { area: area.slug, ...(head !== null && { version: head.head.id }), ...(study?.kind === "version" && { version: study.id }) };
  return { status: "ok", areas: areas.data, area, head, drawn };
}

/** The payload of what is drawn, or why the conductor would not build it. */
export type Payload = { status: "loading" } | { status: "error"; message: string } | { status: "refused"; refused: Refusal; message: string } | { status: "ok"; payload: string; fresh: boolean };

export function useAreaPayload(drawn: Extract<Query, { area: string }>): Payload {
  return payloadOf(useResource(api, "/placement/study", { params: { query: drawn }, parseAs: "text" }));
}

export function usePublishedPayload(sha: string): Payload {
  return payloadOf(useResource(api, "/placement/studies/{sha}", { params: { path: { sha } }, parseAs: "text" }));
}

function payloadOf(read: { status: "loading" } | { status: "ok"; data: string; fresh: boolean } | { status: "error"; failure: { code: string; error: string } }): Payload {
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
