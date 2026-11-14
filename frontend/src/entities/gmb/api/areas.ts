import { ApiError, http } from "@/shared/api";

import { type Area, type Locality, type PutArea, areaParser, areasParser, localityParser } from "../model/areas";
import type { LatLng, Suggestion } from "../model/gmb";
import { suggestionsParser } from "../model/gmb";

export async function fetchAreas(): Promise<Area[]> {
  return (await http.get("/api/v1/areas", areasParser)).areas;
}

/** Refused: 400 a path of the conductor's own, 409 someone else's. */
export function putArea(slug: string, area: PutArea): Promise<Area> {
  return http.send("PUT", `/api/v1/areas/${encodeURIComponent(slug)}`, area, areaParser);
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
