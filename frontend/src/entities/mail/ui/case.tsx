"use client";

import { Badge, cn } from "@evinvest/uikit";
import { ChevronRight } from "lucide-react";

import { useNow } from "@/entities/alerts";
import type { MailAutos, MailCase, MailLine, MailTopic } from "@/shared/api/schema";
import { useLocale, useT } from "@/shared/i18n";
import { formatDateTime } from "@/shared/lib/format";
import { type At, ViewLink } from "@/shared/lib/location";

import { automation } from "../model/automation";
import { over, standing, subjectOf, whyText } from "../model/cases";
import { mail } from "../model/i18n";
import { KIND_NAME, KIND_TONE, TOPIC_NAME, TOPIC_TONE, WHO_NAME, sentBy } from "../model/names";

export function TopicBadge({ topic }: { topic: MailTopic }) {
  const t = useT();
  return <Badge tone={TOPIC_TONE[topic]}>{TOPIC_NAME[topic](t)}</Badge>;
}

const INK = { neutral: "text-ink-soft", primary: "text-primary-ink", positive: "text-positive", info: "text-accent-info", warn: "text-accent-warn", error: "text-accent-error" };

/** Whose turn a case is, and since or until when. */
export function Standing({ c }: { c: MailCase }) {
  const t = useT();
  const locale = useLocale();
  const { text, tone } = standing(c, t, locale, useNow(30_000));
  return <span className={cn("shrink-0 text-xs whitespace-nowrap tabular-nums", INK[tone], tone === "error" && "font-medium")}>{text}</span>;
}

/** How answering a case by itself stands. */
export function Automation({ c, autos }: { c: MailCase; autos: MailAutos }) {
  const t = useT();
  const locale = useLocale();
  return <span className="text-xs break-words text-ink-soft">{automation(c, autos, t, locale, useNow(30_000))}</span>;
}

/** Why a case is the operator's, else what Google last said in it, unless we wrote since. */
export function useGist(c: MailCase): string | null {
  const t = useT();
  const locale = useLocale();
  const now = useNow(60_000);
  if (c.state.is === "needs_you" && c.state.why.code !== "kind" && c.state.why.code !== "unwritten" && c.state.why.code !== "deadline" && c.state.why.code !== "stuck") return whyText(c, c.state.why, t, locale, now);
  return c.last.summary;
}

/** What two lines say the same of. */
const told = (s: string) => s.toLowerCase().replace(/[^\p{L}\p{N}]+/gu, " ").trim();

/** A case in a list: whose it is (its GBP, else its gmail), whose turn it is, what it is about and why. */
export function CaseRow({ c, of, current, to }: { c: MailCase; of: string; current: boolean; to: At }) {
  const t = useT();
  const gist = useGist(c);
  const subject = subjectOf(c, t);
  const who = c.last.who === "us" || c.last.who === "unproven" ? WHO_NAME[c.last.who](t) : null;
  const line = [c.last.spam && t(mail.spam, "in Spam"), gist === c.last.summary && who, gist !== null && told(gist) !== told(subject) && gist].filter(Boolean).join(" · ");
  const ink = over(c) ? "text-ink-mid" : "text-ink";
  return (
    <ViewLink
      to={to}
      data-case={c.id}
      aria-current={current || undefined}
      className={cn("flex flex-col gap-0.5 rounded-md px-2.5 py-2 outline-none hover:bg-hover/50 focus-visible:ring-2 focus-visible:ring-ring", current && "bg-hover")}
    >
      <span className="flex min-w-0 items-center gap-2">
        <span className={cn("min-w-0 flex-1 truncate text-sm", ink, !over(c) && "font-medium")}>{of}</span>
        <Standing c={c} />
      </span>
      <span className="flex min-w-0 items-center gap-2">
        <TopicBadge topic={c.topic} />
        <span className={cn("min-w-0 truncate text-sm", ink)}>{subject}</span>
      </span>
      {line !== "" && <span className="truncate text-xs text-ink-soft">{line}</span>}
    </ViewLink>
  );
}

/** Messages in a GBP's window, each with its kind, or who sent it, and a link to its case. */
export function MailLines({ lines }: { lines: readonly MailLine[] }) {
  const t = useT();
  const locale = useLocale();
  return (
    <ol className="flex flex-col divide-y divide-border">
      {lines.map((l) => (
        <li key={l.id}>
          <ViewLink to={{ view: "email", params: { case: l.case_id } }} className="flex min-w-0 items-center gap-2 rounded-md py-2 outline-none hover:bg-hover/40 focus-visible:ring-2 focus-visible:ring-ring">
            <span className="flex min-w-0 flex-1 flex-col gap-1">
              <span className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
                {l.kind !== null ? <Badge tone={KIND_TONE[l.kind] ?? "neutral"}>{KIND_NAME[l.kind](t)}</Badge> : <Badge tone="neutral">{l.who === "us" ? WHO_NAME.us(t) : t(mail.message.unread, "Not read")}</Badge>}
                <time dateTime={l.at} className="flex-1 font-mono text-xs whitespace-nowrap text-ink-soft tabular-nums">
                  {formatDateTime(l.at, locale)}
                </time>
                {l.sent_by !== null && <span className="text-xs text-ink-soft">{sentBy(l.sent_by, t)}</span>}
              </span>
              <span className="truncate text-sm text-ink">{l.summary ?? l.subject ?? t(mail.noSubject, "(no subject)")}</span>
              {l.reasons.map((r) => (
                <span key={r} className="text-xs break-words text-accent-error">
                  “{r}”
                </span>
              ))}
            </span>
            <ChevronRight aria-hidden className="size-4 shrink-0 text-ink-soft" />
          </ViewLink>
        </li>
      ))}
    </ol>
  );
}
