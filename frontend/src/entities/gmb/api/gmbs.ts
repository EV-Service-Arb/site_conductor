import { ApiError, http, ignoreBody } from "@/shared/api";

import {
  type Candidate,
  type Country,
  type Gmails,
  type Gmb,
  type LatLng,
  type MapConfig,
  type SitesPlace,
  type State,
  candidatesParser,
  gmailsParser,
  gmbParser,
  mapConfigParser,
  sitesPlacesParser,
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

export async function removeGmail(id: number): Promise<void> {
  await http.send("DELETE", `/api/v1/gmails/${id}`, undefined, ignoreBody);
}

export async function resolvePlace(place: string): Promise<Candidate[]> {
  return (await http.get("/api/v1/gmbs/resolve", candidatesParser, { place })).candidates;
}

/** A GBP on Google already; `name` and `country` fill what Google lacks. */
export function addGmb(gmailId: number, add: { place_id: string; state: State; name?: string; country?: Country }): Promise<Gmb> {
  return http.send("POST", `/api/v1/gmails/${gmailId}/gmbs`, add, gmbParser);
}

/** A GBP not on Google yet; refused when a live one has the name. */
export function createDraft(name: string): Promise<Gmb> {
  return http.send("POST", "/api/v1/gmbs", { name }, gmbParser);
}

/** `name` a draft's only; `phone: null` clears it. */
export function editGmb(id: number, edit: { name?: string; gmail_id?: number; phone?: string | null }): Promise<Gmb> {
  return http.send("PATCH", `/api/v1/gmbs/${id}`, edit, gmbParser);
}

/** `place` (pasted) takes a draft onto Google; refused for a move `NEXT` does not allow. */
export function moveGmb(id: number, state: State, place?: string): Promise<Gmb> {
  return http.send("PUT", `/api/v1/gmbs/${id}/state`, place === undefined ? { state } : { state, place }, gmbParser);
}

/** Refused when Google gives the GBP its country. */
export async function setCountry(id: number, country: Country): Promise<void> {
  await http.send("PUT", `/api/v1/gmbs/${id}/country`, { country }, ignoreBody);
}

/** Refused when Google has the place's point: only one it has none of is placed by hand. */
export async function placeGmb(id: number, at: LatLng): Promise<void> {
  await http.send("PUT", `/api/v1/gmbs/${id}/location`, { lat: at.lat, lng: at.lng }, ignoreBody);
}

export async function removeGmb(id: number): Promise<void> {
  await http.send("DELETE", `/api/v1/gmbs/${id}`, undefined, ignoreBody);
}

/** All of `ids` or none: one not the caller's refuses the lot. */
export async function switchGmbs(ids: number[], enabled: boolean): Promise<void> {
  await http.send("PUT", "/api/v1/gmbs/enabled", { gmbs: ids, enabled }, ignoreBody);
}

export async function fetchSitesPlaces(): Promise<SitesPlace[]> {
  return (await http.get("/api/sites/places", sitesPlacesParser)).places;
}

/** sites' place answer is not read: the page re-reads its tags. */
export async function linkPlace(brand: string, location: string, gmbId: number | null): Promise<void> {
  await http.send("PUT", `/api/sites/places/${encodeURIComponent(brand)}/${encodeURIComponent(location)}/gmb`, { gmb_id: gmbId }, ignoreBody);
}
