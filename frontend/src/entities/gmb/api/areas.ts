import { Refused, api } from "@/shared/api";
import type { LatLng, Locality } from "@/shared/api/schema";

/** `null`: the point is in no locality. */
export async function localityAt(at: LatLng): Promise<Locality | null> {
  const answer = await api.GET("/kit/locality", { params: { query: { lat: at.lat, lng: at.lng } } });
  if (answer.error === undefined) return answer.data;
  if (answer.error.code === "not_found") return null;
  throw new Refused(answer.error);
}
