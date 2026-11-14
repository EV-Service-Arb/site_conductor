import { type Parser, arrayOf, num, object, oneOf, str } from "@/shared/lib/parse";

import type { Area, AreaShape, Areas, Locality } from "./generated";

export type { Area, AreaShape, Locality, PutArea } from "./generated";

export const latLng = object({ lat: num, lng: num });
const viewport = object({ south: num, west: num, north: num, east: num });

const shapeParser: Parser<AreaShape> = (v, path) => {
  const { kind } = object({ kind: oneOf(["city", "circle"]) })(v, path);
  return kind === "city" ? { kind, ...object({ place_id: str, viewport })(v, path) } : { kind, ...object({ radius_m: num })(v, path) };
};

export const areaParser: Parser<Area> = object({ slug: str, name: str, country: str, center: latLng, shape: shapeParser, margin_m: num });
export const areasParser: Parser<Areas> = object({ areas: arrayOf(areaParser) });
export const localityParser: Parser<Locality> = object({ place_id: str, name: str, location: latLng, viewport, country: str });

/** As `sa_core::ids::is_slug`, which the server holds an area's link to. */
export const isSlug = (s: string) => /^[a-z0-9][a-z0-9_-]{0,63}$/.test(s);

/** An area's link from its name: `Saint-Étienne` → `saint-etienne`. */
export const slugOf = (name: string) =>
  name
    .normalize("NFD")
    .replace(/\p{M}/gu, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 64);
