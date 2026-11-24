"use client";

import { Badge, cn } from "@evinvest/uikit";
import { ChevronRight } from "lucide-react";

import { useNow } from "@/entities/alerts";
import type { MailCase, MailLine, MailTopic } from "@/shared/api/schema";
import { useLocale, useT } from "@/shared/i18n";
import { formatDateTime } from "@/shared/lib/format";
import { type At, ViewLink } from "@/shared/lib/location";

import { over, standing, subjectOf, whyText } from "../model/cases";
import { mail } from "../model/i18n";
import { KIND_NAME, KIND_TONE, TOPIC_NAME, TOPIC_TONE, WHO_NAME } from "../model/names";

export function TopicBadge({ topic }: { topic: MailTopic }) {
  const t = useT();
  return <Badge tone={TOPIC_TONE[topic]}>{TOPIC_NAME[topic](t)}</Badge>;
}

const INK = { neutral: "text-ink-soft", primary: "text-primary-ink", positive: "text-positive", info: "text-accent-info", warn: "text-accent-warn", error: "text-accent-error" };

/** Whose turn a case is, and since or until when. */
export function Standing({ c, className }: { c: MailCase; className?: string }) {
  const t = useT();
  const locale = useLocale();
  const { text, tone } = standing(c, t, locale, useNow(30_000));
  return <span className={cn("shrink-0 text-xs whitespace-nowrap tabular-nums", INK[tone], tone === "error" && "font-medium", className)}>{text}</span>;
}

/** Why a case is the operator's, else what Google last said in it, unless we wrote since. */
function useGist(c: MailCase): string | null {
  const t = useT();
  const locale = useLocale();
  const now = useNow(60_000);
  if (c.state.is === "needs_you" && c.state.why.code !== "kind") return whyText(c, c.state.why, t, locale, now);
  return c.last.summary;
}

/** A case in a list: what it is about, who speaks in it, whose turn it is and why. */
export function CaseRow({ c, gbp, current = false, to }: { c: MailCase; gbp: string | null; current?: boolean; to: At }) {
  const t = useT();
  const gist = useGist(c);
  const ink = over(c) ? "text-ink-mid" : "text-ink";
  return (
    <ViewLink
      to={to}
      data-case={c.id}
      aria-current={current || undefined}
      className={cn("flex flex-col gap-0.5 rounded-md px-2.5 py-2 outline-none hover:bg-hover/50 focus-visible:ring-2 focus-visible:ring-ring", current && "bg-hover")}
    >
      <span className="flex min-w-0 items-center gap-2">
        <TopicBadge topic={c.topic} />
        <span className={cn("min-w-0 flex-1 truncate text-sm", ink, !over(c) && "font-medium")}>{WHO_NAME[c.last.who](t)}</span>
        <Standing c={c} />
      </span>
      <span className={cn("truncate text-sm", ink)}>{subjectOf(c, t)}</span>
      <span className="truncate text-xs text-ink-soft">{[gbp ?? t(mail.noGbp, "no GBP known"), gist].filter(Boolean).join(" · ")}</span>
    </ViewLink>
  );
}

/** Cases in a GBP's window: what was last said in each, how it stands, and a link to it. */
export function CaseLinks({ cases }: { cases: readonly MailCase[] }) {
  const t = useT();
  return (
    <ul className="flex flex-col divide-y divide-border">
      {cases.map((c) => (
        <li key={c.id}>
          <ViewLink to={{ view: "email", params: { case: c.id } }} className="flex min-w-0 items-center gap-2 rounded-md py-2 outline-none hover:bg-hover/40 focus-visible:ring-2 focus-visible:ring-ring">
            <span className="flex min-w-0 flex-1 flex-col gap-1">
              <span className="flex min-w-0 items-center justify-between gap-2">
                {c.said === null ? <TopicBadge topic={c.topic} /> : <Badge tone={TOPIC_TONE[c.topic]}>{KIND_NAME[c.said](t)}</Badge>}
                <Standing c={c} />
              </span>
              <span className="truncate text-sm text-ink">{subjectOf(c, t)}</span>
            </span>
            <ChevronRight aria-hidden className="size-4 shrink-0 text-ink-soft" />
          </ViewLink>
        </li>
      ))}
    </ul>
  );
}

/** Messages in a GBP's window, each with its kind and a link to its case; the latest of one of `cases` with how it stands. */
export function MailLines({ lines, cases }: { lines: readonly MailLine[]; cases: readonly MailCase[] }) {
  const t = useT();
  const locale = useLocale();
  return (
    <ol className="flex flex-col divide-y divide-border">
      {lines.map((l) => {
        const c = cases.find((x) => x.last.id === l.id);
        return (
          <li key={l.id}>
            <ViewLink to={{ view: "email", params: { case: l.case_id } }} className="flex min-w-0 items-center gap-2 rounded-md py-2 outline-none hover:bg-hover/40 focus-visible:ring-2 focus-visible:ring-ring">
              <span className="flex min-w-0 flex-1 flex-col gap-1">
                <span className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
                  {l.kind !== null ? <Badge tone={KIND_TONE[l.kind] ?? "neutral"}>{KIND_NAME[l.kind](t)}</Badge> : <Badge tone="neutral">{l.who === "us" ? WHO_NAME.us(t) : t(mail.message.unread, "Not read")}</Badge>}
                  <time dateTime={l.at} className="flex-1 font-mono text-xs whitespace-nowrap text-ink-soft tabular-nums">
                    {formatDateTime(l.at, locale)}
                  </time>
                  {c !== undefined && !over(c) && <Standing c={c} />}
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
        );
      })}
    </ol>
  );
}
