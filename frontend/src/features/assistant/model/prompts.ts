import type { Listed, Tags } from "@/entities/gmb";
import type { Mention } from "@/shared/api/schema";

/** Prompts worth one press for `subject`, specific to what it needs; the whole account's without one. */
export function suggestions(subject: Listed | null, tags: Tags): string[] {
  if (subject === null) return ["Which locations need me today?", "Which leads are overdue?", "Which locations have suspended reviews?"];
  const name = subject.gmb.name ?? subject.gmb.place_id;
  const archive = tags.archive?.get(subject.gmb.id);
  const site = tags.sites?.get(subject.gmb.id);
  return [
    archive !== undefined && archive.suspended > 0 ? `Appeal the ${archive.suspended} suspended reviews` : `How is ${name} doing?`,
    site !== undefined && site.overdue > 0 ? `Which of the ${site.overdue} overdue leads first?` : `Why did calls change at ${name} this week?`,
    `Draft replies to unanswered reviews`,
  ];
}

export function asMentions(listed: Listed[]): Mention[] {
  return listed.map((l) => ({ kind: "location", id: l.gmb.id, label: l.gmb.name ?? l.gmb.place_id }));
}
