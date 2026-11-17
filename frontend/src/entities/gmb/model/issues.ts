import type { Gmb, Trade } from "@/shared/api/schema";
import type { T } from "@/shared/i18n";

import { categoryOf } from "./category";
import type { ArchiveTag, SiteTag } from "./gmb";
import type { Listed } from "./narrow";
import { MUST_HAVES, readiness } from "./readiness";
import { gmbs as gmbsKeys } from "./i18n";

const issueKeys = gmbsKeys.issue;

/** What needs the operator on a GMB, most pressing first. */
export const ISSUES = ["overdue", "suspended", "failing", "draft", "unlocated", "no_site", "no_category", "off"] as const;
export type Issue = (typeof ISSUES)[number];

export const ISSUE_NAME = {
  overdue: (t) => t(issueKeys.overdue, "Overdue"),
  suspended: (t) => t(issueKeys.suspended, "Reviews suspended"),
  failing: (t) => t(issueKeys.failing, "Failing"),
  draft: (t) => t(issueKeys.draft, "Drafts"),
  unlocated: (t) => t(issueKeys.unlocated, "No location"),
  no_site: (t) => t(issueKeys.no_site, "No site"),
  no_category: (t) => t(issueKeys.no_category, "No category"),
  off: (t) => t(issueKeys.off, "Off"),
} satisfies Record<Issue, (t: T) => string>;

/** The services' tags by GMB id, and the caller's trades; `null`: not read, the service did not answer, or the caller does not see it. */
export interface Tags {
  archive: ReadonlyMap<number, ArchiveTag> | null;
  sites: ReadonlyMap<number, SiteTag> | null;
  trades: readonly Trade[] | null;
}

/** Every issue of the GMB, most pressing first. An off GMB is out of every service, so being off is all it has; a draft has no location or site yet by being one. */
export function issuesOf(gmb: Gmb, tags: Tags): Issue[] {
  if (!gmb.enabled) return ["off"];
  const archive = tags.archive?.get(gmb.id);
  const site = tags.sites?.get(gmb.id);
  const draft = gmb.state === "draft";
  const has: Record<Exclude<Issue, "off">, boolean> = {
    overdue: site !== undefined && site.overdue > 0,
    suspended: archive !== undefined && archive.suspended > 0,
    failing: archive?.failing === true,
    draft,
    unlocated: !draft && gmb.location === null,
    no_site: !draft && tags.sites !== null && site === undefined,
    no_category: tags.trades !== null && categoryOf(gmb, tags.trades).status !== "set_up",
  };
  return ISSUES.filter((i) => i !== "off" && has[i]);
}

/** The GMB's most pressing issue; `null` when nothing needs the operator. */
export function issueOf(gmb: Gmb, tags: Tags): Issue | null {
  return issuesOf(gmb, tags)[0] ?? null;
}

/** How many of `gmbs` have each issue, a GMB counted under every one it has. */
export function issueCounts(gmbs: readonly Gmb[], tags: Tags): Map<Issue, number> {
  const counts = new Map<Issue, number>();
  for (const gmb of gmbs) for (const issue of issuesOf(gmb, tags)) counts.set(issue, (counts.get(issue) ?? 0) + 1);
  return counts;
}

/** The issue as a row and a card name it: with its count where it has one. `issue` is `issueOf` the GMB under these tags. */
export function issueText(issue: Issue, listed: Listed, tags: Tags, t: T): string {
  const { gmb } = listed;
  if (issue === "overdue") return t(gmbsKeys.tag.overdue, "{n} overdue", { n: tags.sites!.get(gmb.id)!.overdue });
  if (issue === "suspended") return t(gmbsKeys.tag.suspended, "{n} reviews suspended", { n: tags.archive!.get(gmb.id)!.suspended });
  if (issue === "draft") return `${readiness(listed, tags).filter((r) => r.ready).length}/${MUST_HAVES.length}`;
  return ISSUE_NAME[issue](t);
}
