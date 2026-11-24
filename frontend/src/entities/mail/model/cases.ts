"use client";

import type { Tone } from "@evinvest/uikit";
import { useResource } from "@evinvest/api/react";

import { useScope, useSelection } from "@/entities/gmb";
import { api } from "@/shared/api";
import type { MailCase, MailStanding, MailWhy } from "@/shared/api/schema";
import type { Locale, T } from "@/shared/i18n";
import { age, when } from "@/shared/lib/time";

import { mail } from "./i18n";
import { KIND_ASKS, KIND_NAME, KIND_TONE } from "./names";

export type Show = "needs_you" | "waiting" | "all";

/** Whom a case waits on: the operator, or Google or the panel; neither, it is in All alone. */
const STANDS: Record<MailStanding, Exclude<Show, "all"> | null> = { needs_you: "needs_you", reply_ready: "needs_you", sending: "waiting", reply_due: "waiting", waiting: "waiting", done: null, quiet: null };

const held = (c: MailCase) => c.state.is === "sending" && c.draft?.state === "held";

/** Whether a held reply goes by itself. */
export type Goes = (c: MailCase) => boolean;

export function useGoes(): Goes {
  const auto = useResource(api, "/mail/auto");
  return (c) => {
    if (!held(c)) return false;
    // not known yet: it may, and Stop is shown
    if (auto.status !== "ok") return true;
    return auto.data.by_itself && auto.data.on.some((x) => x.gmail_id === c.gmail_id && x.topic === c.topic);
  };
}

/** Neither read nor answered until the model is back: one cause of them all. */
export const stuckOf = (c: MailCase): string | null => (c.state.is === "needs_you" && c.state.why.code === "stuck" ? c.state.why.error : null);

/** What is to be done first: a reply that goes unless stopped, Google's dates, what only a person can do, a reply to check, what waits for the model. */
function rank(c: MailCase, goes: Goes): number {
  if (goes(c)) return 0;
  if (deadlineOf(c) !== null) return 1;
  if (c.state.is !== "needs_you") return 4;
  const { why } = c.state;
  if (why.code === "stuck") return 5;
  if (why.code === "failed" || why.code === "may_have_gone_out") return 2;
  if (why.code === "kind" && why.kind === "asks") return 4;
  return why.code === "kind" && KIND_TONE[why.kind] !== undefined ? 2 : 3;
}

export function shownIn(cases: readonly MailCase[], show: Show, goes: Goes): MailCase[] {
  if (show === "all") return [...cases];
  const of = cases.filter((c) => (held(c) ? "needs_you" : STANDS[c.state.is]) === show);
  const until = (c: MailCase) => Date.parse(deadlineOf(c) ?? (c.state.is === "waiting" ? c.state.until : c.last.at));
  return of.sort((a, b) => (show === "needs_you" ? rank(a, goes) - rank(b, goes) : 0) || until(a) - until(b));
}

export function deadlineOf(c: MailCase): string | null {
  if (c.state.is === "needs_you" && c.state.why.code === "deadline") return c.state.why.at;
  return c.state.is === "done" ? null : c.deadline;
}

/** The caller's cases, and those of the outline's scope: of a selected GBP, or of none and a gmail in scope. */
export function useCases() {
  const read = useResource(api, "/mail/cases");
  const selection = useSelection();
  const { gmails } = useScope();
  if (read.status !== "ok" || selection === null) return { ...read, shown: null };
  const shown = read.data.cases.filter((c) => (c.gmb_id === null ? gmails === null || gmails.has(c.gmail_id) : selection.isSelected(c.gmb_id)));
  return { ...read, shown };
}

/** Google's mail about a GBP. */
export function useGmbMail(gmb: number) {
  return useResource(api, "/gmbs/{id}/mail", { params: { path: { id: gmb } } });
}

const DAY_MS = 24 * 3600 * 1000;

/** How a case stands, in a chip's words. */
export function standing(c: MailCase, goes: Goes, t: T, locale: Locale, now: Date): { text: string; tone: Tone } {
  const since = age(c.last.at, locale);
  if (c.state.is === "sending" && goes(c)) return { text: Date.parse(c.state.at) > now.getTime() ? t(mail.state.held, "goes at {at}", { at: when(c.state.at, locale, now) }) : t(mail.held.due, "due"), tone: "info" };
  const deadline = deadlineOf(c);
  if (deadline !== null) {
    const left = Date.parse(deadline) - now.getTime();
    const at = when(deadline, locale, now);
    return { text: left > 0 ? t(mail.state.deadline, "by {at}", { at }) : t(mail.state.overdue, "overdue since {at}", { at }), tone: left < DAY_MS ? "error" : "warn" };
  }
  const ready = { text: t(mail.state.reply_ready, "reply ready · {since}", { since }), tone: "warn" } as const;
  switch (c.state.is) {
    case "needs_you": {
      const { why } = c.state;
      // what Google asks, its reply written: there to check
      if (why.code === "kind" && why.kind === "asks") return ready;
      return { text: `${chip(c, why, t)} · ${since}`, tone: TONE[why.code] ?? (why.code === "kind" ? KIND_TONE[why.kind] : undefined) ?? "warn" };
    }
    case "reply_ready":
      return ready;
    case "sending":
      return held(c) ? ready : { text: t(mail.state.sending, "sending"), tone: "info" };
    case "reply_due":
      return { text: t(mail.state.reply_due, "reply due · {since}", { since }), tone: "neutral" };
    case "waiting":
      return { text: c.said === "routed" ? t(mail.state.routed, "with a specialist until {at}", { at: when(c.state.until, locale, now) }) : t(mail.state.waiting, "with Google until {at}", { at: when(c.state.until, locale, now) }), tone: "neutral" };
    case "done":
      return { text: t(mail.state.done, "done · {since}", { since }), tone: "neutral" };
    case "quiet":
      return { text: since, tone: "neutral" };
  }
}

const TONE: Partial<Record<MailWhy["code"], Tone>> = { failed: "error", may_have_gone_out: "error", stuck: "neutral" };

function chip(c: MailCase, why: MailWhy, t: T): string {
  switch (why.code) {
    case "kind":
      return KIND_NAME[why.kind](t);
    case "deadline":
      throw new Error(`case ${c.id}'s deadline is its chip`);
    case "needs":
      return t(mail.needs.needs, "needs {n} from you", { n: why.needs.length });
    case "asked_often":
      return c.said === "routed" ? t(mail.needs.unanswered, "no answer") : t(mail.needs.asked_often, "nothing new");
    case "unanswered":
      return t(mail.needs.unanswered, "no answer");
    case "no_reply":
      return t(mail.needs.no_reply, "takes no reply");
    case "unread":
      return t(mail.needs.unread, "not read");
    case "stuck":
      return t(mail.needs.stuck, "waits for Claude");
    case "nothing_to_say":
      return t(mail.needs.nothing_to_say, "no reply written");
    case "may_have_gone_out":
      return t(mail.needs.may_have_gone_out, "check the thread");
    case "failed":
      return t(mail.needs.failed, "not sent");
  }
}

/** Why case `c` is the operator's. */
export function whyText(c: MailCase, why: MailWhy, t: T, locale: Locale, now: Date): string {
  switch (why.code) {
    case "kind":
      return KIND_ASKS[why.kind]?.(t) ?? KIND_NAME[why.kind](t);
    case "deadline":
      return t(mail.why.deadline, "Google set a date: {at}.", { at: when(why.at, locale, now) });
    case "needs":
      return t(mail.why.needs, "Needs from you: {what}", { what: why.needs.join(", ") });
    case "asked_often":
      return c.said === "routed" ? t(mail.why.silent, "Google's specialist stayed silent past the time given, and when asked again.") : t(mail.why.asked_often, "Google answers nothing new, however often it was asked.");
    case "unanswered":
      return t(mail.why.unanswered, "Google went silent, and nothing here takes a reply.");
    case "no_reply":
      return t(mail.why.no_reply, "Google asks in a mail that takes no reply: answer it where it says.");
    case "unread":
      return why.error === null ? t(mail.why.unread, "Not read yet.") : t(mail.why.unread.failed, "It could not be read: {error}", { error: why.error });
    case "stuck":
      return why.error;
    case "nothing_to_say":
      return t(mail.why.nothing_to_say, "No reply was written: {why}", { why: why.why });
    case "may_have_gone_out":
      return t(mail.why.may_have_gone_out, "The reply may have gone out: check the thread. It is never sent again by itself.");
    case "failed":
      return t(mail.why.failed, "The reply did not go: {error}", { error: why.error });
  }
}

/** What made a case done that nobody marked so. */
export function settledText(c: MailCase, gbp: string | null, t: T): string {
  const here = c.said === "closed" || (c.said !== null && KIND_TONE[c.said] === "positive");
  return here || gbp === null ? t(mail.settled.here, "Settled by what Google last said here.") : t(mail.settled.later, "Settled by later mail of {gbp}, or by its being verified.", { gbp });
}

/** A case's subject, without the numbers it is told by elsewhere. */
export function subjectOf(c: MailCase, t: T): string {
  const bare = c.numbers.reduce((s, n) => s.replace(`[${n}]`, ""), c.subject ?? "").trim();
  return bare === "" ? t(mail.noSubject, "(no subject)") : bare;
}

/** Whether it is about a GBP its mail does not name. */
export const unnamed = (c: MailCase) => c.gmb_id === null && c.topic !== "other" && c.said !== "account_restricted";

export const over = (c: MailCase) => c.state.is === "done" || c.state.is === "quiet";
