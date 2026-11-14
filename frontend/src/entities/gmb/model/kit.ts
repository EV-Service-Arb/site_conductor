import type { T } from "@/shared/i18n";
import { type Parser, arrayOf, bool, nullable, num, object, oneOf, str } from "@/shared/lib/parse";

import { areaParser, latLng } from "./areas";
import type { GmbKit, KitAreaGmbs, KitPool, KitTarget } from "./generated";
import { STATES } from "./gmb";
import { kit } from "./i18n";

export type { GmbKit, KitAreaGmbs, KitDraft, KitFunnel, KitNearby, KitPhoto, KitPool, KitRanked, KitTarget } from "./generated";

export const KIT_TARGETS = Object.keys({ google: true, drafts: true, one_tap: true } satisfies Record<KitTarget, true>) as KitTarget[];

export const TARGET_NAME = {
  google: (t) => t(kit.target.google, "Google's form"),
  drafts: (t) => t(kit.target.drafts, "Drafts page"),
  one_tap: (t) => t(kit.target.oneTap, "One tap"),
} satisfies Record<KitTarget, (t: T) => string>;


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
