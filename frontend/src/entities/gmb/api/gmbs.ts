import type { UndoEntry } from "@evinvest/uikit";

import { ApiError, http, ignoreBody } from "@/shared/api";
import type { T } from "@/shared/i18n";

import {
  type Candidate,
  type Country,
  type GmailRole,
  type Gmails,
  type Gmb,
  type LatLng,
  type MapConfig,
  type SitesPlace,
  type State,
  type Trade,
  candidatesParser,
  gmailsParser,
  gmbParser,
  mapConfigParser,
  sitesPlacesParser,
  tradeParser,
  tradesParser,
} from "../model/gmb";

export function fetchGmails(): Promise<Gmails> {
  return http.get("/api/v1/gmails", gmailsParser);
}

/** `null`: the conductor has no browser key, so there is no map. */
export async function fetchMapConfig(): Promise<MapConfig | null> {
  try {
    return await http.get("/api/v1/map", mapConfigParser);
  } catch (e) {
    if (e instanceof ApiError && e.failure.kind === "not_found") return null;
    throw e;
  }
}

export async function addGmail(gmail: string): Promise<void> {
  await http.send("POST", "/api/v1/gmails", { gmail }, ignoreBody);
}

export async function setGmailRole(id: number, role: GmailRole | null): Promise<void> {
  await http.send("PATCH", `/api/v1/gmails/${id}`, { role }, ignoreBody);
}

export async function removeGmail(id: number): Promise<void> {
  await http.send("DELETE", `/api/v1/gmails/${id}`, undefined, ignoreBody);
}

/** Asked with `gmail`'s Maps key first, if given, then the caller's others', then the shared keys their tokens pay for. */
export async function resolvePlace(place: string, gmail: number | null): Promise<Candidate[]> {
  return (await http.get("/api/v1/gmbs/resolve", candidatesParser, gmail === null ? { place } : { place, gmail: String(gmail) })).candidates;
}

/** A GBP on Google already; `name` and `country` fill what Google lacks. */
export function addGmb(gmailId: number, add: { place_id: string; state: State; name?: string; country?: Country; trade: number | null }): Promise<Gmb> {
  return http.send("POST", `/api/v1/gmails/${gmailId}/gmbs`, add, gmbParser);
}

/** A GBP not on Google yet; refused when a live one has the name. */
export function createDraft(name: string, trade: number | null): Promise<Gmb> {
  return http.send("POST", "/api/v1/gmbs", { name, trade }, gmbParser);
}

/** What a GBP's PATCH takes: `name` a draft's only, `null` clears a field. */
export type GmbEdit = Partial<Pick<Gmb, "phone" | "hidden_address" | "customers_visit" | "film">> & {
  name?: string;
  gmail_id?: number;
  trade?: number | null;
  areas?: string[] | null;
};

/** Refused when another live GBP has the name or the phone. */
export function editGmb(id: number, edit: GmbEdit): Promise<Gmb> {
  return http.send("PATCH", `/api/v1/gmbs/${id}`, edit, gmbParser);
}

/** `place` (pasted) takes a draft onto Google; refused for a move not in its `next`. */
export function moveGmb(id: number, state: State, place?: string): Promise<Gmb> {
  return http.send("PUT", `/api/v1/gmbs/${id}/state`, place === undefined ? { state } : { state, place }, gmbParser);
}

/** Refused when Google gives the GBP its country. */
export async function setCountry(id: number, country: Country): Promise<void> {
  await http.send("PUT", `/api/v1/gmbs/${id}/country`, { country }, ignoreBody);
}

/** What a write sets on each GBP, and how it writes a value. */
interface Field<V> {
  of: (gmb: Gmb) => V;
  write: (ids: number[], value: V) => Promise<void>;
}

const LOCATION: Field<Gmb["location"]> = {
  of: (b) => b.location,
  write: async (ids, at) => {
    for (const id of ids) {
      if (at === null) await http.send("DELETE", `/api/v1/gmbs/${id}/location`, undefined, ignoreBody);
      else await http.send("PUT", `/api/v1/gmbs/${id}/location`, { lat: at.lat, lng: at.lng }, ignoreBody);
    }
  },
};
const ENABLED: Field<boolean> = {
  of: (b) => b.enabled,
  write: async (gmbs, enabled) => {
    await http.send("PUT", "/api/v1/gmbs/enabled", { gmbs, enabled }, ignoreBody);
  },
};
const TRADE: Field<number | null> = {
  of: (b) => b.trade_id,
  write: async (gmbs, trade) => {
    await http.send("PUT", "/api/v1/gmbs/trade", { gmbs, trade }, ignoreBody);
  },
};

/**
 * `gmbs`, as the client held them, given `to`. The entry writes either side back only while every
 * one still holds the other, as the conductor answers at the press (another operator, another tab).
 */
async function set<V>(field: Field<V>, gmbs: Gmb[], to: V, label: string, t: T): Promise<UndoEntry> {
  const ids = gmbs.map((b) => b.id);
  await field.write(ids, to);
  const was = new Map(gmbs.map((b) => [b.id, JSON.stringify(field.of(b))]));
  const after = JSON.stringify(to);
  const holds = async (value: (id: number) => string) => {
    const now = new Map((await fetchGmails()).gmbs.map((b) => [b.id, JSON.stringify(field.of(b))]));
    if (!ids.every((id) => now.get(id) === value(id))) throw new Error(t("undo.changed", { label }));
  };
  return {
    label,
    undo: async () => {
      await holds(() => after);
      const back = new Map<string, number[]>();
      for (const [id, v] of was) back.set(v, [...(back.get(v) ?? []), id]);
      for (const [v, group] of back) await field.write(group, JSON.parse(v) as V);
    },
    redo: async () => {
      await holds((id) => was.get(id)!);
      await field.write(ids, to);
    },
  };
}

/** Refused when Google has the place's point: only one it has none of is placed by hand. */
export function placeGmb(gmb: Gmb, at: LatLng, t: T): Promise<UndoEntry> {
  return set(LOCATION, [gmb], { lat: at.lat, lng: at.lng, placed: true }, t("undo.placed", { name: gmb.name ?? gmb.place_id ?? String(gmb.id) }), t);
}

export async function removeGmb(id: number): Promise<void> {
  await http.send("DELETE", `/api/v1/gmbs/${id}`, undefined, ignoreBody);
}

/** All of `gmbs` or none: one not the caller's refuses the lot. */
export function switchGmbs(gmbs: Gmb[], enabled: boolean, t: T): Promise<UndoEntry> {
  return set(ENABLED, gmbs, enabled, t(enabled ? "undo.on" : "undo.off", { n: gmbs.length }), t);
}

/** All of `gmbs` or none: one not the caller's refuses the lot. `null` takes their trade away. */
export function setTrades(gmbs: Gmb[], trade: number | null, t: T): Promise<UndoEntry> {
  return set(TRADE, gmbs, trade, t("undo.trade", { n: gmbs.length }), t);
}

export async function fetchTrades(): Promise<Trade[]> {
  return (await http.get("/api/v1/trades", tradesParser)).trades;
}

/** Refused when the caller has a trade of that name, whatever its case. */
export function addTrade(name: string): Promise<Trade> {
  return http.send("POST", "/api/v1/trades", { name }, tradeParser);
}

export function renameTrade(id: number, name: string): Promise<Trade> {
  return http.send("PATCH", `/api/v1/trades/${id}`, { name }, tradeParser);
}

/** Refused while a live GBP holds it. */
export async function deleteTrade(id: number): Promise<void> {
  await http.send("DELETE", `/api/v1/trades/${id}`, undefined, ignoreBody);
}

/** Sets the trade up in `country`, or edits it there; refused for a keyword twice once lowercased. */
export function setUpCategory(id: number, country: Country, c: { keywords: string[]; google_category: string | null; study: string | null }): Promise<Trade> {
  return http.send("PUT", `/api/v1/trades/${id}/${country}`, c, tradeParser);
}

export async function removeCategory(id: number, country: Country): Promise<void> {
  await http.send("DELETE", `/api/v1/trades/${id}/${country}`, undefined, ignoreBody);
}

export async function fetchSitesPlaces(): Promise<SitesPlace[]> {
  return (await http.get("/api/sites/places", sitesPlacesParser)).places;
}

/** sites' place answer is not read: the page re-reads its tags. */
export async function linkPlace(brand: string, location: string, gmbId: number | null): Promise<void> {
  await http.send("PUT", `/api/sites/places/${encodeURIComponent(brand)}/${encodeURIComponent(location)}/gmb`, { gmb_id: gmbId }, ignoreBody);
}
