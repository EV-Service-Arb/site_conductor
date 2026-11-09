import { type Infer, type Parser, arrayOf, bool, nullable, num, object, oneOf, str } from "@/shared/lib/parse";

import type { Candidates, Category, Country, Gmail, GmailRole, Gmails, Gmb, MapConfig, OnGoogle, Place, State, Trade, Trades } from "./generated";

export type { Category, Country, Gmail, GmailRole, Gmails, Gmb, MapConfig, OnGoogle, Place as Candidate, State, Trade, Trades } from "./generated";

export type LatLng = { lat: number; lng: number };

/** Each a key, so the compiler holds the lists to the generated unions. */
const STATES = Object.keys({ draft: true, verifying: true, verified: true, suspended: true } satisfies Record<State, true>) as State[];
export const ON_GOOGLE = Object.keys({ verifying: true, verified: true, suspended: true } satisfies Record<OnGoogle, true>) as OnGoogle[];
export const COUNTRIES = Object.keys({ FR: true, US: true, CA: true, GB: true } satisfies Record<Country, true>) as Country[];
export const GMAIL_ROLES = Object.keys({ owner: true, manager: true } satisfies Record<GmailRole, true>) as GmailRole[];

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
  trade_id: nullable(num),
  areas: arrayOf(str),
  hidden_address: nullable(str),
  customers_visit: nullable(bool),
  film: nullable(str),
  profile: nullable(str),
  created_at: str,
  enabled: bool,
  location: nullable(object({ lat: num, lng: num, placed: bool })),
});

export const mapConfigParser: Parser<MapConfig> = object({ key: str });

const gmailParser: Parser<Gmail> = object({ id: num, gmail: str, role: nullable(oneOf(GMAIL_ROLES)), cap: nullable(num), created_at: str });
export const gmailsParser: Parser<Gmails> = object({ gmails: arrayOf(gmailParser), gmbs: arrayOf(gmbParser) });

const categoryParser: Parser<Category> = object({ country: oneOf(COUNTRIES), lang: str, keywords: arrayOf(str), google_category: nullable(str), study: nullable(str) });
export const tradeParser: Parser<Trade> = object({ id: num, name: str, created_at: str, categories: arrayOf(categoryParser) });
export const tradesParser: Parser<Trades> = object({ trades: arrayOf(tradeParser) });

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
