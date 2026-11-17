import type { Tone } from "@evinvest/uikit";

import type { GmailRole, GoogleCategory, State } from "@/shared/api/schema";
import type { GmbReviews } from "@/shared/api/services/review_archive";
import type { T } from "@/shared/i18n";
import { type Infer, type Parser, arrayOf, bool, num, object, str } from "@/shared/lib/parse";

import { gmbs } from "./i18n";

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

export const STATE_TONE: Record<State, Tone> = { draft: "neutral", verifying: "warn", verified: "positive", suspended: "error" };

/** `g`'s name in `lang`: the reader's, or its country's, which Google's form there reads. */
export function googleName(g: GoogleCategory, lang: string): string {
  const name = g.names[lang];
  if (name === undefined) throw new Error(`${g.id} has no name in ${lang}`);
  return name;
}

/** review_archive's tag on a GMB. */
export type ArchiveTag = GmbReviews;

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

const siteTagParser = object({ brand: str, location: str, leads_7d: num, overdue: num });
export type SiteTag = Infer<typeof siteTagParser>;
export const sitesParser = byGmb(siteTagParser);

const sitesPlaceParser = object({ brand: str, location: str, withdrawn: bool });
export type SitesPlace = Infer<typeof sitesPlaceParser>;
export const sitesPlacesParser = object({ places: arrayOf(sitesPlaceParser) });
