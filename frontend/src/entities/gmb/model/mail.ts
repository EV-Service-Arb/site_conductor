import type { MailCase } from "@/shared/api/schema";

export type MailIssue = "mail_deadline" | "mail_asks" | "mail_reply";

/** What its mail with Google says of a GBP. */
export interface MailTag {
  issues: ReadonlySet<MailIssue>;
  /** When the first reply held goes by itself. */
  held: string | null;
}

/** Whether a reply goes by itself unless stopped. */
export const goes = (c: MailCase) => c.state.is === "sending" && c.draft?.state === "held";

export function deadlineOf(c: MailCase): string | null {
  if (c.state.is === "needs_you" && c.state.why.code === "deadline") return c.state.why.at;
  return c.state.is === "done" ? null : c.deadline;
}

/** What a case wants of its operator; `null`: it waits on Google or the panel, or is over. */
export function needsOf(c: MailCase): MailIssue | null {
  if (c.state.is !== "needs_you" && c.state.is !== "reply_ready") return null;
  if (deadlineOf(c) !== null) return "mail_deadline";
  return c.state.is === "needs_you" ? "mail_asks" : "mail_reply";
}

/** The GBPs a case is about. */
export const gmbsOf = (c: MailCase): number[] => (c.gmb_id === null ? c.also : [c.gmb_id, ...c.also]);

export function mailTags(cases: readonly MailCase[]): ReadonlyMap<number, MailTag> {
  const tags = new Map<number, { issues: Set<MailIssue>; held: string | null }>();
  for (const c of cases) {
    const need = needsOf(c);
    const held = c.state.is === "sending" && goes(c) ? c.state.at : null;
    if (need === null && held === null) continue;
    for (const id of gmbsOf(c)) {
      const tag = tags.get(id) ?? { issues: new Set(), held: null };
      if (need !== null) tag.issues.add(need);
      if (held !== null && (tag.held === null || Date.parse(held) < Date.parse(tag.held))) tag.held = held;
      tags.set(id, tag);
    }
  }
  return tags;
}
