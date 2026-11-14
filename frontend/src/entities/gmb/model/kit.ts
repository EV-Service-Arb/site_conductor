import type { T } from "@/shared/i18n";
import { type Parser, arrayOf, bool, nullable, num, object, oneOf, str } from "@/shared/lib/parse";

import type { GmbKit, KitArea, KitAreaGmbs, KitAreas, KitPool, KitShape, KitTarget, Locality } from "./generated";
import { STATES } from "./gmb";
import { kit } from "./i18n";

export type { GmbKit, KitArea, KitAreaGmbs, KitDraft, KitFunnel, KitNearby, KitPhoto, KitPool, KitRanked, KitTarget, Locality, PutKitArea } from "./generated";

export const KIT_TARGETS = Object.keys({ google: true, drafts: true, one_tap: true } satisfies Record<KitTarget, true>) as KitTarget[];

export const TARGET_NAME = {
  google: (t) => t(kit.target.google, "Google's form"),
  drafts: (t) => t(kit.target.drafts, "Drafts page"),
  one_tap: (t) => t(kit.target.oneTap, "One tap"),
} satisfies Record<KitTarget, (t: T) => string>;

const latLng = object({ lat: num, lng: num });
const viewport = object({ south: num, west: num, north: num, east: num });

export const poolParser: Parser<KitPool> = object({
  drafts: arrayOf(object({ id: num, text: str, photos: arrayOf(num), uses: num, created_at: str })),
  photos: arrayOf(object({ id: num, width: num, height: num, uses: num, created_at: str })),
});

export const gmbKitParser: Parser<GmbKit> = object({
  target: nullable(oneOf(KIT_TARGETS)),
  url: nullable(str),
  page: nullable(str),
  drafts: arrayOf(
    object({
      id: num,
      text: str,
      uses: num,
      went_here: bool,
      held: bool,
      offered: bool,
      photos: arrayOf(object({ id: num, width: num, height: num, uses: num, went_here: bool })),
    }),
  ),
  funnel: object({ shown: num, scanned: num, opened: num, copied: num, photos_saved: num }),
});

const shapeParser: Parser<KitShape> = (v, path) => {
  const { kind } = object({ kind: oneOf(["city", "circle"]) })(v, path);
  return kind === "city" ? { kind, ...object({ place_id: str, viewport })(v, path) } : { kind, ...object({ radius_m: num })(v, path) };
};

export const areaParser: Parser<KitArea> = object({ slug: str, name: str, center: latLng, shape: shapeParser, margin_m: num });
export const areasParser: Parser<KitAreas> = object({ areas: arrayOf(areaParser) });
export const areaGmbsParser: Parser<KitAreaGmbs> = object({
  area: areaParser,
  gmbs: arrayOf(
    object({
      id: num,
      name: str,
      state: oneOf(STATES),
      address: nullable(str),
      location: latLng,
      distance_m: num,
      inside: bool,
      target: nullable(oneOf(KIT_TARGETS)),
      last_shown: nullable(str),
    }),
  ),
});
export const localityParser: Parser<Locality> = object({ place_id: str, name: str, location: latLng, viewport });

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
