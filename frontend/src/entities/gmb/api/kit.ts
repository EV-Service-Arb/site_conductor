import { ApiError, http, ignoreBody } from "@/shared/api";

import type { Country, LatLng, Suggestion } from "../model/gmb";
import { suggestionsParser } from "../model/gmb";
import { type GmbKit, type KitArea, type KitAreaGmbs, type KitPool, type KitTarget, type Locality, type PutKitArea, areaGmbsParser, areaParser, areasParser, gmbKitParser, localityParser, poolParser } from "../model/kit";

/** A photo's largest size, as the server takes it. */
export const PHOTO_MAX = 10 << 20;

const pool = (trade: number, country: Country) => `/api/v1/trades/${trade}/${country}/kit`;

export function fetchPool(trade: number, country: Country): Promise<KitPool> {
  return http.get(pool(trade, country), poolParser);
}

export function addDraft(trade: number, country: Country, text: string, photos: number[]): Promise<KitPool> {
  return http.send("POST", `${pool(trade, country)}/drafts`, { text, photos }, poolParser);
}

/** Refused (409) for the text of a draft copied somewhere already. */
export function editDraft(trade: number, country: Country, draft: number, edit: { text?: string; photos?: number[] }): Promise<KitPool> {
  return http.send("PATCH", `${pool(trade, country)}/drafts/${draft}`, edit, poolParser);
}

/** Out of every offer; its uses are kept. */
export async function removeKitItem(trade: number, country: Country, kind: "drafts" | "photos", id: number): Promise<void> {
  await http.send("DELETE", `${pool(trade, country)}/${kind}/${id}`, undefined, ignoreBody);
}

/** Re-encoded by the server: a JPEG, ≤ 2048 px, no EXIF. */
export function uploadPhoto(trade: number, country: Country, photo: Blob, onProgress: (sent: number) => void): Promise<void> {
  return http.upload("POST", `${pool(trade, country)}/photos`, photo, photo.type || "application/octet-stream", onProgress);
}

export const photoSrc = (trade: number, country: Country, photo: number) => `${pool(trade, country)}/photos/${photo}.jpg`;

export function fetchGmbKit(gmb: number): Promise<GmbKit> {
  return http.get(`/api/v1/gmbs/${gmb}/kit`, gmbKitParser);
}

/** `null` turns it off; a page target is refused (409) without a landing page. */
export function setKitTarget(gmb: number, target: KitTarget | null): Promise<GmbKit> {
  return http.send("PUT", `/api/v1/gmbs/${gmb}/kit`, { target }, gmbKitParser);
}

export async function kitShown(gmb: number): Promise<void> {
  await http.send("POST", `/api/v1/gmbs/${gmb}/kit/shown`, undefined, ignoreBody);
}

export async function fetchAreas(): Promise<KitArea[]> {
  return (await http.get("/api/v1/kit/areas", areasParser)).areas;
}

/** Refused: 400 a path of the conductor's own, 409 someone else's. */
export function putArea(slug: string, area: PutKitArea): Promise<KitArea> {
  return http.send("PUT", `/api/v1/kit/areas/${encodeURIComponent(slug)}`, area, areaParser);
}

/** The caller's GBPs in it or next to it, nearest first. */
export function fetchAreaGmbs(slug: string): Promise<KitAreaGmbs> {
  return http.get(`/api/v1/kit/areas/${encodeURIComponent(slug)}/gmbs`, areaGmbsParser);
}

/** Cities and towns, paid as `suggestPlaces`; the caller debounces. */
export async function suggestCities(input: string): Promise<Suggestion[]> {
  return (await http.get("/api/v1/kit/cities", suggestionsParser, { input })).suggestions;
}

export function fetchCity(placeId: string): Promise<Locality> {
  return http.get(`/api/v1/kit/cities/${encodeURIComponent(placeId)}`, localityParser);
}

/** `null`: the point is in no locality. */
export async function localityAt(at: LatLng): Promise<Locality | null> {
  try {
    return await http.get("/api/v1/kit/locality", localityParser, { lat: at.lat, lng: at.lng });
  } catch (e) {
    if (e instanceof ApiError && e.failure.kind === "not_found") return null;
    throw e;
  }
}
