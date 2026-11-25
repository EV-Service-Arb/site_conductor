import { useResource } from "@evinvest/api/react";

import { api } from "@/shared/api";
import type { MailGoal, MailKind, MailPursuit } from "@/shared/api/schema";
import type { T } from "@/shared/i18n";

import { mail } from "./i18n";

const DAY = 24 * 3600 * 1000;

export const GOAL_NAME: Record<MailGoal, (t: T) => string> = {
  call: (t) => t(mail.goal.call, "Live verification call"),
  reviews: (t) => t(mail.goal.reviews, "Missing reviews"),
};

/** What was asked of Google; about one GBP, or every one. */
export const useStats = (gmb?: number) => useResource(api, "/mail/stats", { params: { query: gmb === undefined ? {} : { gmb } } });

const askedAt = (p: MailPursuit) => Date.parse(p.asks[0]!.at);

/** From its first ask to its end, or until now. */
export const daysOf = (p: MailPursuit, now: number) => Math.max(0, Math.round(((p.over_at === null ? now : Date.parse(p.over_at)) - askedAt(p)) / DAY));

function median(of: number[]): number | null {
  const sorted = [...of].sort((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  return sorted.length === 0 ? null : sorted.length % 2 === 1 ? sorted[mid]! : (sorted[mid - 1]! + sorted[mid]!) / 2;
}

export type Figures = { given: number; of: number; asks: number | null; days: number | null; open: MailPursuit[] };

/** Asks and days are of those Google gave. */
export function figures(pursuits: readonly MailPursuit[], now: number): Figures {
  const given = pursuits.filter((p) => p.given);
  return {
    given: given.length,
    of: pursuits.length,
    asks: median(given.map((p) => p.asks.length)),
    days: median(given.map((p) => daysOf(p, now))),
    open: pursuits.filter((p) => p.over_at === null),
  };
}

export type Wording = { text: string; used: number; worked: number; answers: [MailKind | null, number][] };

/** Each wording as it was written last, alike but for its case, spacing and which of `names` it names; those that worked most first. */
export function wordings(pursuits: readonly MailPursuit[], names: readonly string[]): Wording[] {
  const norm = (s: string) => s.toLowerCase().split(/\s+/).join(" ").trim();
  const longest = names.map(norm).filter((n) => n !== "").sort((a, b) => b.length - a.length);
  const found = new Map<string, { text: string; at: number; used: number; worked: number; answers: Map<MailKind | null, number> }>();
  for (const a of pursuits.flatMap((p) => p.asks)) {
    if (a.text === null) continue;
    const key = longest.reduce((s, n) => s.replaceAll(n, "{gbp}"), norm(a.text));
    if (key === "") continue;
    const w = found.get(key) ?? { text: a.text, at: 0, used: 0, worked: 0, answers: new Map() };
    if (Date.parse(a.at) > w.at) Object.assign(w, { text: a.text, at: Date.parse(a.at) });
    w.used += 1;
    if (a.worked) w.worked += 1;
    w.answers.set(a.answer, (w.answers.get(a.answer) ?? 0) + 1);
    found.set(key, w);
  }
  return [...found.values()]
    .sort((a, b) => b.worked - a.worked || b.used - a.used || b.at - a.at)
    .map(({ text, used, worked, answers }) => ({ text, used, worked, answers: [...answers].sort((a, b) => b[1] - a[1]) }));
}
