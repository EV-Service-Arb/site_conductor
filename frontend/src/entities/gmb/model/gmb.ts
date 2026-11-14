import type { T } from "@/shared/i18n";
import { type Infer, type Parser, arrayOf, bool, nullable, num, object, oneOf, recordOf, str } from "@/shared/lib/parse";

import type { Candidates, Category, Country, Gmail, GmailRole, Gmails, Gmb, GoogleCategories, GoogleCategory, MapConfig, OnGoogle, Place, State, Suggestion, Suggestions, Trade, Trades } from "./generated";
import { gmbs } from "./i18n";

export type { Category, Country, Gmail, GmailRole, Gmails, Gmb, GoogleCategory, MapConfig, OnGoogle, Place as Candidate, State, Suggestion, Trade, Trades } from "./generated";

export type LatLng = { lat: number; lng: number };
/** One of a draft's candidate spots. */
export type Spot = Gmb["spots"][number];

/** Each a key, so the compiler holds the lists to the generated unions. */
export const STATES = Object.keys({ draft: true, verifying: true, verified: true, suspended: true } satisfies Record<State, true>) as State[];
export const ON_GOOGLE = Object.keys({ verifying: true, verified: true, suspended: true } satisfies Record<OnGoogle, true>) as OnGoogle[];
export const COUNTRIES = Object.keys({ FR: true, US: true, CA: true, GB: true } satisfies Record<Country, true>) as Country[];
export const GMAIL_ROLES = Object.keys({ owner: true, manager: true } satisfies Record<GmailRole, true>) as GmailRole[];

export const GMAIL_ROLE_NAME = {
  owner: (t) => t(gmbs.gmail.role.owner, "Owner"),
  manager: (t) => t(gmbs.gmail.role.manager, "Manager"),
} satisfies Record<GmailRole, (t: T) => string>;

export const STATE_NAME = {
  draft: (t) => t(gmbs.state.draft, "Draft"),
  verifying: (t) => t(gmbs.state.verifying, "Verifying"),
  verified: (t) => t(gmbs.state.verified, "Verified"),
  suspended: (t) => t(gmbs.state.suspended, "Suspended"),
} satisfies Record<State, (t: T) => string>;

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
  created_at: str,
  enabled: bool,
  location: nullable(object({ lat: num, lng: num, placed: bool })),
  spots: arrayOf(object({ lat: num, lng: num, label: nullable(str), note: nullable(str) })),
});

export const mapConfigParser: Parser<MapConfig> = object({ key: str });

export const gmailParser: Parser<Gmail> = object({ id: num, gmail: str, role: nullable(oneOf(GMAIL_ROLES)), created_at: str, browser: bool });
export const gmailsParser: Parser<Gmails> = object({ gmails: arrayOf(gmailParser), gmbs: arrayOf(gmbParser) });

const googleCategoryParser: Parser<GoogleCategory> = object({ id: str, names: recordOf(str) });

/** `g`'s name in `lang`: the reader's, or its country's, which Google's form there reads. */
export function googleName(g: GoogleCategory, lang: string): string {
  const name = g.names[lang];
  if (name === undefined) throw new Error(`${g.id} has no name in ${lang}`);
  return name;
}
export const googleCategoriesParser: Parser<GoogleCategories> = object({ categories: arrayOf(googleCategoryParser) });
const categoryParser: Parser<Category> = object({
  country: oneOf(COUNTRIES),
  lang: str,
  keywords: arrayOf(str),
  google_primary: nullable(googleCategoryParser),
  google_additional: arrayOf(googleCategoryParser),
  study: nullable(str),
});
export const tradeParser: Parser<Trade> = object({ id: num, name: str, created_at: str, categories: arrayOf(categoryParser) });
export const tradesParser: Parser<Trades> = object({ trades: arrayOf(tradeParser) });

const candidateParser: Parser<Place> = object({
  place_id: str,
  name: nullable(str),
  address: nullable(str),
  location: nullable(object({ lat: num, lng: num })),
  country: nullable(oneOf(COUNTRIES)),
});
export const candidatesParser: Parser<Candidates> = object({ candidates: arrayOf(candidateParser) });
const suggestionParser: Parser<Suggestion> = object({ place_id: str, main: str, secondary: nullable(str) });
export const suggestionsParser: Parser<Suggestions> = object({ suggestions: arrayOf(suggestionParser) });

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

const daysParser = arrayOf(nullable(object({ live: num, lost: num })));
const live30dParser: Parser<Infer<typeof daysParser>> = (v, path) => {
  const days = daysParser(v, path);
  if (days.length !== 30) throw new Error(`${path}: expected 30 days, got ${days.length}`);
  return days;
};
const archiveTagParser = object({ tracked: bool, rating: nullable(num), reviews: nullable(num), suspended: num, reinstating: num, failing: bool, live_30d: live30dParser });
export type ArchiveTag = Infer<typeof archiveTagParser>;
export const archiveParser = object({ gmbs: byGmb(archiveTagParser), places_tracked: num });
export type Archive = Infer<typeof archiveParser>;

const siteTagParser = object({ brand: str, location: str, leads_7d: num, overdue: num });
export type SiteTag = Infer<typeof siteTagParser>;
export const sitesParser = byGmb(siteTagParser);

const sitesPlaceParser = object({ brand: str, location: str, withdrawn: bool });
export type SitesPlace = Infer<typeof sitesPlaceParser>;
export const sitesPlacesParser = object({ places: arrayOf(sitesPlaceParser) });
