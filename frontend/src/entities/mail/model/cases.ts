"use client";

import type { Tone } from "@evinvest/uikit";
import { useResource } from "@evinvest/api/react";

import { useScope, useSelection } from "@/entities/gmb";
import { api } from "@/shared/api";
import type { MailCase, MailStanding, MailWhy } from "@/shared/api/schema";
import type { Locale, T } from "@/shared/i18n";
import { age, when } from "@/shared/lib/time";

import { mail } from "./i18n";
import { KIND_ASKS, KIND_NAME } from "./names";

export type Show = "needs_you" | "waiting" | "all";

/** Whom a case waits on: the operator, or Google or the panel; neither, it is in All alone. */
const STANDS: Record<MailStanding, Exclude<Show, "all"> | null> = { needs_you: "needs_you", reply_ready: "needs_you", sending: "waiting", reply_due: "waiting", waiting: "waiting", done: null, quiet: null };

export function shownIn(cases: readonly MailCase[], show: Show): MailCase[] {
  const of = cases.filter((c) => show === "all" || STANDS[c.state.is] === show);
  if (show === "all") return of;
  // a deadline that runs first, the soonest of them; the rest as they came
  return of.sort((a, b) => due(a) - due(b));
}

const due = (c: MailCase) => (c.deadline === null ? Number.MAX_SAFE_INTEGER : Date.parse(c.deadline));

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
export function standing(c: MailCase, t: T, locale: Locale, now: Date): { text: string; tone: Tone } {
  const since = age(c.last.at, locale);
  if (c.deadline !== null && c.state.is !== "done") {
    const soon = Date.parse(c.deadline) - now.getTime() < DAY_MS;
    return { text: t(mail.state.deadline, "by {at}", { at: when(c.deadline, locale, now) }), tone: soon ? "error" : "warn" };
  }
  switch (c.state.is) {
    case "needs_you":
      return { text: `${NEEDS[c.state.why.code](t)} · ${since}`, tone: c.state.why.code === "failed" || c.state.why.code === "may_have_gone_out" ? "error" : "warn" };
    case "reply_ready":
      return { text: t(mail.state.reply_ready, "reply ready · {since}", { since }), tone: "warn" };
    case "sending":
      return c.draft?.state === "held" ? { text: t(mail.state.held, "goes at {at}", { at: when(c.state.at, locale, now) }), tone: "info" } : { text: t(mail.state.sending, "sending"), tone: "info" };
    case "reply_due":
      return { text: t(mail.state.reply_due, "reply due · {since}", { since }), tone: "neutral" };
    case "waiting":
      return { text: t(mail.state.waiting, "with Google until {at}", { at: when(c.state.until, locale, now) }), tone: "neutral" };
    case "done":
      return { text: t(mail.state.done, "done · {since}", { since }), tone: "neutral" };
    case "quiet":
      return { text: since, tone: "neutral" };
  }
}

const NEEDS: Record<MailWhy["code"], (t: T) => string> = {
  kind: (t) => t(mail.needs.kind, "needs you"),
  deadline: (t) => t(mail.needs.kind, "needs you"),
  needs: (t) => t(mail.needs.needs, "needs reply"),
  asked_often: (t) => t(mail.needs.asked_often, "asked often"),
  unanswered: (t) => t(mail.needs.unanswered, "no answer"),
  no_reply: (t) => t(mail.needs.no_reply, "takes no reply"),
  unread: (t) => t(mail.needs.unread, "not read"),
  stuck: (t) => t(mail.needs.stuck, "stuck"),
  nothing_to_say: (t) => t(mail.needs.kind, "needs you"),
  may_have_gone_out: (t) => t(mail.needs.may_have_gone_out, "check the thread"),
  failed: (t) => t(mail.needs.failed, "not sent"),
};

/** Why a case is the operator's. */
export function whyText(why: MailWhy, t: T, locale: Locale, now: Date): string {
  switch (why.code) {
    case "kind":
      return KIND_ASKS[why.kind]?.(t) ?? KIND_NAME[why.kind](t);
    case "deadline":
      return t(mail.why.deadline, "Google set a date: {at}.", { at: when(why.at, locale, now) });
    case "needs":
      return t(mail.why.needs, "Google wants what only you have: {what}.", { what: why.needs.join(", ") });
    case "asked_often":
      return t(mail.why.asked_often, "Google answers nothing new, however often it was asked.");
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

/** A case's subject, without the numbers it is told by elsewhere. */
export function subjectOf(c: MailCase, t: T): string {
  const bare = c.numbers.reduce((s, n) => s.replace(`[${n}]`, ""), c.subject ?? "").trim();
  return bare === "" ? t(mail.noSubject, "(no subject)") : bare;
}

/** Who speaks in a case: only support's conversations carry a number. */
export function whoIn(c: MailCase, t: T): string {
  if (c.last.kind === "impersonation") return KIND_NAME.impersonation(t);
  return c.numbers.length > 0 ? t(mail.who.support, "Google support") : t(mail.who.google, "Google");
}
