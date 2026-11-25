"use client";

import type { Tone } from "@evinvest/uikit";
import { useResource } from "@evinvest/api/react";

import { deadlineOf, goes, gmbsOf, needsOf, useMailCases, useScope, useSelection } from "@/entities/gmb";
import { api } from "@/shared/api";
import type { MailCase, MailQuietBecause, MailSettled, MailWhy } from "@/shared/api/schema";
import type { Locale, T } from "@/shared/i18n";
import { age, when } from "@/shared/lib/time";

import { mail } from "./i18n";
import { KIND_ASKS, KIND_NAME, KIND_TONE } from "./names";

export type Show = "needs_you" | "waiting" | "all";

/** Neither read nor answered until the model is back: one cause of them all. */
export const stuckOf = (c: MailCase): string | null => (c.state.is === "needs_you" && c.state.why.code === "stuck" ? c.state.why.error : null);

/** What is to be done first: a reply that goes unless stopped, Google's dates, what only a person can do, a reply to check, what waits for the model. */
function rank(c: MailCase): number {
  if (goes(c)) return 0;
  if (deadlineOf(c) !== null) return 1;
  if (c.state.is === "reply_ready") return 4;
  if (c.state.is !== "needs_you") return 6;
  const { why } = c.state;
  if (why.code === "stuck") return 5;
  if (why.code === "failed" || why.code === "login" || why.code === "may_have_gone_out") return 2;
  if (why.code === "kind" && why.kind === "asks") return 4;
  return why.code === "kind" && KIND_TONE[why.kind] !== undefined ? 2 : 3;
}

export function shownIn(cases: readonly MailCase[], show: Show): MailCase[] {
  if (show === "all") return [...cases];
  return pressing(cases.filter((c) => !over(c) && (needsOf(c) !== null) === (show === "needs_you")));
}

/** What presses most first. */
export function pressing(cases: readonly MailCase[]): MailCase[] {
  const until = (c: MailCase) => Date.parse(deadlineOf(c) ?? (c.state.is === "waiting" ? c.state.until : c.last.at));
  return [...cases].sort((a, b) => rank(a) - rank(b) || until(a) - until(b));
}

/** The caller's cases, and those of the outline's scope: of a selected GBP, or of none and a gmail in scope. */
export function useCases() {
  const read = useMailCases();
  const selection = useSelection();
  const { gmails } = useScope();
  if (read.status !== "ok" || selection === null) return { ...read, shown: null };
  const shown = read.data.cases.filter((c) => (gmbsOf(c).length === 0 ? gmails === null || gmails.has(c.gmail_id) : gmbsOf(c).some(selection.isSelected)));
  return { ...read, shown };
}

/** Which topics of which GBPs are answered by themselves. */
export function useAutos() {
  return useResource(api, "/mail/auto");
}

/** Google's mail about a GBP. */
export function useGmbMail(gmb: number) {
  return useResource(api, "/gmbs/{id}/mail", { params: { path: { id: gmb } } });
}

const DAY_MS = 24 * 3600 * 1000;

/** How a case stands, in a chip's words. */
export function standing(c: MailCase, t: T, locale: Locale, now: Date): { text: string; tone: Tone } {
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
      return { text: t(mail.state.sending, "sending"), tone: "info" };
    case "reply_due":
      return { text: t(mail.state.reply_due, "reply due · {since}", { since }), tone: "neutral" };
    case "waiting":
      return { text: c.said === "routed" ? t(mail.state.routed, "with a specialist until {at}", { at: when(c.state.until, locale, now) }) : t(mail.state.waiting, "with Google until {at}", { at: when(c.state.until, locale, now) }), tone: "neutral" };
    case "done":
      return { text: t(mail.state.done, "done · {since}", { since }), tone: "neutral" };
    case "quiet":
      return { text: quietChip(c.state.because, since, t), tone: "neutral" };
  }
}

function quietChip(because: MailQuietBecause, since: string, t: T): string {
  switch (because.is) {
    case "carried":
      return t(mail.state.carried, "carried · {since}", { since });
    case "removed":
      return t(mail.state.trashed, "in the trash · {since}", { since });
    case "nothing":
    case "old":
      return since;
  }
}

const TONE: Partial<Record<MailWhy["code"], Tone>> = { failed: "error", login: "error", may_have_gone_out: "error", stuck: "neutral" };

function chip(c: MailCase, why: MailWhy, t: T): string {
  switch (why.code) {
    case "kind":
      return KIND_NAME[why.kind](t);
    case "deadline":
      throw new Error(`case ${c.id}'s deadline is its chip`);
    case "needs":
      return t(mail.needs.needs, "needs {n} from you", { n: why.needs.length });
    case "asked_often":
      return t(mail.needs.asked_often, "asked {n}×, nothing new", { n: why.asked });
    case "unanswered":
      return t(mail.needs.unanswered, "no answer");
    case "reopened":
      return t(mail.needs.reopened, "reopened");
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
    case "login":
      return t(mail.needs.login, "login refused");
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
      return t(mail.why.asked_often, "Asked {n} times, and Google answers nothing new: the next is yours to send.", { n: why.asked });
    case "unanswered":
      return c.said === "routed" && c.last.who === "us" ? t(mail.why.silent, "Google's specialist stayed silent past the time given, and when asked again.") : t(mail.why.unanswered, "Google went silent, and nothing asks it again by itself.");
    case "reopened":
      return t(mail.why.reopened, "Reopened: until Google writes, nothing is written or sent here unasked.");
    case "no_reply":
      return t(mail.why.no_reply, "Google asks in a mail that takes no reply: answer it where it says.");
    case "unread":
      return why.error === null ? t(mail.why.unread, "Not read yet.") : t(mail.why.unread.failed, "It could not be read: {error}", { error: why.error });
    case "stuck":
      return why.error;
    case "nothing_to_say":
      return t(mail.why.nothing_to_say, "No reply was written: {why}", { why: why.why });
    case "may_have_gone_out":
      return t(mail.why.may_have_gone_out, "The reply may have gone out, and its copy is not in the inbox: check the thread, and answer there if it did not go.");
    case "failed":
      return t(mail.why.failed, "The reply did not go: {error}", { error: why.error });
    case "login":
      return t(mail.why.login, "The reply did not go, Gmail refused the login: {error}", { error: why.error });
  }
}

/** What settled a case. */
export function settledText(by: MailSettled, t: T, locale: Locale, now: Date): string {
  switch (by.is) {
    case "hand":
      return t(mail.settled.hand, "Marked done.");
    case "here":
      return t(mail.settled.here, "Settled by what Google said here.");
    case "elsewhere": {
      const what = { kind: KIND_NAME[by.kind](t), at: when(by.at, locale, now) };
      return by.case_id === null ? t(mail.settled.move, "Settled by the GBP's move here: {kind}, {at}.", what) : t(mail.settled.later, "Settled by other mail of the GBP: {kind}, {at}.", what);
    }
  }
}

/** Why nothing is to do in case `c`. */
export function quietText(c: MailCase, because: MailQuietBecause, t: T, locale: Locale, now: Date): string {
  switch (because.is) {
    case "nothing":
      return t(mail.quiet.nothing, "Nothing it says is to act on.");
    case "old":
      return t(mail.quiet.old, "Nothing was said since {at}.", { at: when(c.last.at, locale, now) });
    case "removed":
      return t(mail.quiet.removed, "Its GBP is in the trash.");
    case "carried":
      return t(mail.quiet.carried, "The GBP's latest conversation on this carries it.");
  }
}

/** A case's subject, without the numbers it is told by elsewhere. */
export function subjectOf(c: MailCase, t: T): string {
  const bare = c.numbers.reduce((s, n) => s.replace(`[${n}]`, ""), c.subject ?? "").trim();
  return bare === "" ? t(mail.noSubject, "(no subject)") : bare;
}

/** Whether it is about a GBP its mail does not name. */
export const unnamed = (c: MailCase) => c.gmb_id === null && c.topic !== "other" && c.said !== "account_restricted";

export const over = (c: MailCase) => c.state.is === "done" || c.state.is === "quiet";
