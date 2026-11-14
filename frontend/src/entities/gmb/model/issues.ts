import type { T } from "@/shared/i18n";

import { categoryOf } from "./category";
import type { ArchiveTag, Gmb, SiteTag, Trade } from "./gmb";
import type { Listed } from "./narrow";
import { MUST_HAVES, readiness } from "./readiness";

/** What needs the operator on a GMB, most pressing first. */
export const ISSUES = ["overdue", "suspended", "failing", "draft", "unlocated", "no_site", "no_category", "off"] as const;
export type Issue = (typeof ISSUES)[number];

export const ISSUE_LABEL = {
  overdue: (t) => t("Overdue"),
  suspended: (t) => t("Reviews suspended"),
  failing: (t) => t.ctx("issue", "Failing"),
  draft: (t) => t("Drafts"),
  unlocated: (t) => t("No location"),
  no_site: (t) => t("No site"),
  no_category: (t) => t("No category"),
  off: (t) => t("Off"),
} satisfies Record<Issue, (t: T) => string>;

/** The services' tags by GMB id, and the caller's trades; `null`: not read, the service did not answer, or the caller does not see it. */
export interface Tags {
  archive: ReadonlyMap<number, ArchiveTag> | null;
  sites: ReadonlyMap<number, SiteTag> | null;
  trades: readonly Trade[] | null;
}

/** The GMB's most pressing issue; `null` when nothing needs the operator. An off GMB is out of every service, so being off is all it has. */
export function issueOf(gmb: Gmb, tags: Tags): Issue | null {
  if (!gmb.enabled) return "off";
  const archive = tags.archive?.get(gmb.id);
  const site = tags.sites?.get(gmb.id);
  if (site !== undefined && site.overdue > 0) return "overdue";
  if (archive !== undefined && archive.suspended > 0) return "suspended";
  if (archive?.failing) return "failing";
  if (gmb.state === "draft") return "draft";
  if (gmb.location === null) return "unlocated";
  if (tags.sites !== null && site === undefined) return "no_site";
  if (tags.trades !== null && categoryOf(gmb, tags.trades).status !== "set_up") return "no_category";
  return null;
}

/** How many of `gmbs` have each issue as their most pressing one. */
export function issueCounts(gmbs: readonly Gmb[], tags: Tags): Map<Issue, number> {
  const counts = new Map<Issue, number>();
  for (const gmb of gmbs) {
    const issue = issueOf(gmb, tags);
    if (issue !== null) counts.set(issue, (counts.get(issue) ?? 0) + 1);
  }
  return counts;
}

/** The issue as a row and a card name it: with its count where it has one. `issue` is `issueOf` the GMB under these tags. */
export function issueText(issue: Issue, listed: Listed, tags: Tags, t: T): string {
  const { gmb } = listed;
  if (issue === "overdue") return t("{n} overdue", { n: tags.sites!.get(gmb.id)!.overdue });
  if (issue === "suspended") return t("{n} reviews suspended", { n: tags.archive!.get(gmb.id)!.suspended });
  if (issue === "draft") return `${readiness(listed, tags).filter((r) => r.ready).length}/${MUST_HAVES.length}`;
  return ISSUE_LABEL[issue](t);
}
