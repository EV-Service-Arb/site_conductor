import type { GmailRole, GoogleCategory, State } from "@/shared/api/schema";
import type { T } from "@/shared/i18n";
import { type Infer, type Parser, arrayOf, bool, nullable, num, object, str } from "@/shared/lib/parse";

/** Each a key, so the compiler holds the lists to the generated unions. */
export const STATE_LABEL = { draft: (t) => t("Draft"), verifying: (t) => t("Verifying"), verified: (t) => t("Verified"), suspended: (t) => t("Suspended") } satisfies Record<State, (t: T) => string>;
export const GMAIL_ROLE_LABEL = { owner: (t) => t("Owner"), manager: (t) => t("Manager") } satisfies Record<GmailRole, (t: T) => string>;

/** `g`'s name in `lang`: the reader's, or its country's, which Google's form there reads. */
export function googleName(g: GoogleCategory, lang: string): string {
  const name = g.names[lang];
  if (name === undefined) throw new Error(`${g.id} has no name in ${lang}`);
  return name;
}

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
