import { categoryOf } from "./category";
import type { Tags } from "./issues";
import type { Listed } from "./narrow";

/** What a draft needs before Google's form: the form's order, then what stands behind it. */
export const MUST_HAVES = ["name", "category", "areas", "phone", "site", "hidden_address", "film", "gmail", "profile"] as const;
export type MustHave = (typeof MUST_HAVES)[number];

/** Each must-have with whether it is ready, in [`MUST_HAVES`]' order. Advice: moving onto Google never checks it. */
export function readiness({ gmb, gmail }: Listed, tags: Tags): { must: MustHave; ready: boolean }[] {
  const placed = tags.trades === null ? null : categoryOf(gmb, tags.trades);
  const ready: Record<MustHave, boolean> = {
    name: true,
    category: placed?.status === "set_up" && placed.category.google_primary !== null,
    areas: gmb.areas.length > 0,
    phone: gmb.phone !== null,
    site: tags.sites?.has(gmb.id) ?? false,
    hidden_address: gmb.hidden_address !== null && gmb.customers_visit !== null,
    film: gmb.film !== null,
    gmail: gmail !== null,
    profile: gmail?.browser === true,
  };
  return MUST_HAVES.map((must) => ({ must, ready: ready[must] }));
}
