import type { Tone } from "@evinvest/uikit";

import type { GbpRole, GoogleCategory, State } from "@/shared/api/schema";
import type { GmbReviews } from "@/shared/api/services/review_archive";
import type { GmbPlace } from "@/shared/api/services/sites";
import type { T } from "@/shared/i18n";

import { gmbs } from "./i18n";

export const ROLE_NAME = {
  owner: (t) => t(gmbs.role.owner, "Owner"),
  manager: (t) => t(gmbs.role.manager, "Manager"),
} satisfies Record<GbpRole, (t: T) => string>;

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

/** sites' tag on a GMB: its place and how its leads stand. */
export type SiteTag = GmbPlace;
