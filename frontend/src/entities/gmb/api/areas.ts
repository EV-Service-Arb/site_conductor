import { ApiError, api, ok } from "@/shared/api";
import type { LatLng, Locality } from "@/shared/api/schema";

/** `null`: the point is in no locality. */
export async function localityAt(at: LatLng): Promise<Locality | null> {
  try {
    return await ok(api.GET("/kit/locality", { params: { query: { lat: at.lat, lng: at.lng } } }));
  } catch (e) {
    if (e instanceof ApiError && e.failure.kind === "not_found") return null;
    throw e;
  }
}
