import { type Infer, type Parser, arrayOf, bool, nullable, num, object, oneOf, str } from "@/shared/lib/parse";

import type { Candidates, Country, Gmails, Gmb, MapConfig, OnGoogle, Place, State } from "./generated";

export type { Country, Gmail, Gmails, Gmb, MapConfig, OnGoogle, Place as Candidate, State } from "./generated";

export type LatLng = { lat: number; lng: number };

/** Each a key, so the compiler holds the lists to the generated unions. */
const STATES = Object.keys({ draft: true, verifying: true, verified: true, suspended: true } satisfies Record<State, true>) as State[];
export const ON_GOOGLE = Object.keys({ verifying: true, verified: true, suspended: true } satisfies Record<OnGoogle, true>) as OnGoogle[];
export const COUNTRIES = Object.keys({ FR: true, US: true, CA: true, GB: true } satisfies Record<Country, true>) as Country[];

export const gmbParser: Parser<Gmb> = object({
  id: num,
  gmail_id: nullable(num),
  place_id: nullable(str),
  name: str,
  address: nullable(str),
  state: oneOf(STATES),
  next: arrayOf(oneOf(STATES)),
  country: nullable(object({ code: oneOf(COUNTRIES), by_hand: bool })),
  phone: nullable(str),
  created_at: str,
  enabled: bool,
  location: nullable(object({ lat: num, lng: num, placed: bool })),
});

export const mapConfigParser: Parser<MapConfig> = object({ key: str });

export const gmailsParser: Parser<Gmails> = object({ gmails: arrayOf(object({ id: num, gmail: str, created_at: str })), gmbs: arrayOf(gmbParser) });

const candidateParser: Parser<Place> = object({ place_id: str, name: nullable(str), address: nullable(str) });
export const candidatesParser: Parser<Candidates> = object({ candidates: arrayOf(candidateParser) });

/** A JSON object keyed by GMB id, each value checked. */
function byGmb<T>(p: Parser<T>): Parser<Map<number, T>> {
  return (v, path) => {
    if (typeof v !== "object" || v === null || Array.isArray(v)) throw new Error(`${path}: expected an object of GMB ids`);
    return new Map(
      Object.entries(v).map(([id, value]) => {
        const n = Number(id);
        if (!Number.isSafeInteger(n)) throw new Error(`${path}: ${id} is not a GMB id`);
        return [n, p(value, `${path}.${id}`)];
      }),
    );
  };
}

const archiveTagParser = object({ tracked: bool, rating: nullable(num), reviews: nullable(num), suspended: num, reinstating: num, failing: bool });
export type ArchiveTag = Infer<typeof archiveTagParser>;
export const archiveParser = object({ gmbs: byGmb(archiveTagParser), places_tracked: num });
export type Archive = Infer<typeof archiveParser>;

const siteTagParser = object({ brand: str, location: str, leads_7d: num, overdue: num });
export type SiteTag = Infer<typeof siteTagParser>;
export const sitesParser = byGmb(siteTagParser);

const sitesPlaceParser = object({ brand: str, location: str, withdrawn: bool });
export type SitesPlace = Infer<typeof sitesPlaceParser>;
export const sitesPlacesParser = object({ places: arrayOf(sitesPlaceParser) });
