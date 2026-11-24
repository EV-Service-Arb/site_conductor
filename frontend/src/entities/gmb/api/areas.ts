import { Refused, api, ok } from "@/shared/api";
import type { LatLng, Locality, Suggestion } from "@/shared/api/schema";

import type { Drawn } from "../ui/area-draw";

export const citiesLike = (input: string): Promise<Suggestion[]> => ok(api.GET("/kit/cities", { params: { query: { input } } })).then((r) => r.suggestions);

export const placesLike = (input: string): Promise<Suggestion[]> => ok(api.GET("/gmbs/suggest", { params: { query: { input, gmail: null } } })).then((r) => r.suggestions);

export const frameOf = (place_id: string): Promise<Locality> => ok(api.GET("/kit/cities/{place_id}", { params: { path: { place_id } } }));

/** `null`: the point is in no locality. */
export async function localityAt(at: LatLng): Promise<Locality | null> {
  const answer = await api.GET("/kit/locality", { params: { query: { lat: at.lat, lng: at.lng } } });
  if (answer.error === undefined) return answer.data;
  if (answer.error.code === "not_found") return null;
  throw new Refused(answer.error);
}

/** The area `drawn` names, saved unless an equal one holds its link; its slug. */
export async function saveArea({ center, radius_m, city, area }: Drawn): Promise<string> {
  if (area === null) throw new Error("an area saved before it is named");
  if (!area.saved) await ok(api.PUT("/areas/{slug}", { params: { path: { slug: area.slug } }, body: { name: area.name, center, radius_m, city: city?.place_id ?? null } }));
  return area.slug;
}
