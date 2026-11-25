"use client";

import { ConfirmDialog, Skeleton, Switch, cn } from "@evinvest/uikit";
import { Mail } from "lucide-react";
import { useState } from "react";

import { STATE_NAME, pageOf, under, useGmails, useMailCases, useScope } from "@/entities/gmb";
import { mail, over, subjectOf, switchAuto, useAutos } from "@/entities/mail";
import type { Gmail, Gmb, MailAutos, MailTopic } from "@/shared/api/schema";
import { type T, useT } from "@/shared/i18n";
import { ViewLink } from "@/shared/lib/location";
import { EmptyState } from "@/shared/ui/empty-state";
import { ErrorState } from "@/shared/ui/error-state";
import { notifyFailure } from "@/shared/ui/notify";

/** The topics that can be automatic, and what their replies ask for. */
const TOPICS: Partial<Record<MailTopic, { name: (t: T) => string; goes: (t: T) => string }>> = {
  verification: {
    name: (t) => t(mail.auto.verification, "Live verification"),
    goes: (t) => t(mail.auto.verification.goes, "They ask Google support for the live call while it is verifying."),
  },
  reviews: {
    name: (t) => t(mail.auto.reviews, "Review reinstatement"),
    goes: (t) => t(mail.auto.reviews.goes, "They ask Google support for its missing reviews."),
  },
};

function told(topic: MailTopic) {
  const of = TOPICS[topic];
  if (of === undefined) throw new Error(`replies on ${topic} can be automatic, and have no name here`);
  return of;
}

const ROW = "flex flex-col gap-1 px-4 py-2 @2xl:flex-row @2xl:items-center @2xl:gap-3";
const CELL = "@2xl:w-44 @2xl:justify-end";

/** Which GBPs' mail is answered by itself, and on what, under their gmails: off until switched on. */
export function AutoReplies() {
  const t = useT();
  const auto = useAutos();
  const gmails = useGmails();
  const scope = useScope();
  if (auto.status === "error") return <ErrorState failure={auto.failure} onRetry={auto.reload} />;
  if (auto.status === "loading" || gmails.status !== "ok") return <Skeleton className="h-60" />;
  const listed = gmails.data.gmails.filter((g) => scope.gmails === null || scope.gmails.has(g.id));
  if (listed.length === 0) return <EmptyState title={t(mail.auto.none, "No gmails in scope")} />;
  return (
    <div className="flex flex-col gap-3">
      {!auto.data.by_itself && <p className="rounded-lg border border-border bg-hover/60 px-4 py-3 text-sm text-ink">{t(mail.auto.here, "This conductor sends nothing by itself: production's does.")}</p>}
      <div aria-hidden className={cn(ROW, "py-0 text-xs text-ink-soft @max-2xl:hidden")}>
        <span className="flex-1" />
        {auto.data.topics.map((topic) => (
          <span key={topic} className="w-44 text-right">
            {told(topic).name(t)}
          </span>
        ))}
      </div>
      <ul className="flex flex-col divide-y divide-border rounded-lg border border-border">
        {listed.map((g) => {
          const gmbs = under(gmails.data, g.id).filter((b) => b.state !== "draft");
          return (
            <li key={g.id} className="flex flex-col py-1">
              <div className={ROW}>
                <span className="flex min-w-0 flex-1 flex-wrap items-center gap-x-2 text-sm font-medium text-ink">
                  <Mail aria-hidden className="size-4 shrink-0 text-ink-soft" />
                  <span className="min-w-0 truncate">{g.gmail}</span>
                  <Inbox g={g} />
                </span>
                {gmbs.length > 0 && <Switches g={g} gmbs={gmbs} of={t(mail.auto.all, "every GBP of {gmail}", { gmail: g.gmail })} auto={auto.data} reload={auto.reload} className={CELL} hidden="@2xl:sr-only" />}
              </div>
              {gmbs.length === 0 && <p className="px-4 pb-2 text-xs text-ink-soft">{t(mail.auto.noGbps, "No GBP on Google under it")}</p>}
              <ul className="flex flex-col">
                {gmbs.map((b) => (
                  <li key={b.id} className={cn(ROW, "py-0.5")}>
                    <span className="flex min-w-0 flex-1 items-baseline gap-2 pl-6 text-sm text-ink">
                      <span className="min-w-0 truncate">{b.name}</span>
                      <span className="shrink-0 text-xs text-ink-soft">{STATE_NAME[b.state](t)}</span>
                    </span>
                    <Switches g={g} gmbs={[b]} of={b.name} auto={auto.data} reload={auto.reload} className={cn(CELL, "@max-2xl:pl-6")} hidden="@2xl:sr-only" />
                  </li>
                ))}
              </ul>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

/** A GBP's own switches, in its window. */
export function AutoOf({ gmb, gmail, auto }: { gmb: Gmb; gmail: Gmail; auto: ReturnType<typeof useAutos> }) {
  const t = useT();
  if (auto.status === "error") return <ErrorState failure={auto.failure} onRetry={auto.reload} />;
  if (auto.status === "loading") return <Skeleton className="h-9" />;
  return (
    <div role="group" aria-label={t(mail.tab.automatic, "Automatic replies")} className="flex flex-wrap items-center gap-x-5 gap-y-1">
      <p className="flex w-full min-w-0 flex-wrap items-center gap-2 text-xs text-ink-soft">
        {t(mail.tab.automatic, "Automatic replies")}
        <Inbox g={gmail} />
        {!auto.data.by_itself && <span>{t(mail.automation.here, "nothing goes from this conductor")}</span>}
      </p>
      <Switches g={gmail} gmbs={[gmb]} of={gmb.name} auto={auto.data} reload={auto.reload} className="" hidden="" />
    </div>
  );
}

/** What stops its whole inbox, and where that is mended. */
function Inbox({ g }: { g: Gmail }) {
  const t = useT();
  if (g.imap !== null && g.imap.error === null) return null;
  return (
    <ViewLink
      to={pageOf({ kind: "account", gmail: g.id, tab: "connections" })}
      className={cn("shrink-0 rounded-sm text-xs font-normal underline outline-none focus-visible:ring-2 focus-visible:ring-ring", g.imap === null ? "text-ink-soft" : "text-accent-error")}
    >
      {g.imap === null ? t(mail.inbox.set, "Set its IMAP") : t(mail.inbox.reconnect, "Reconnect")}
    </ViewLink>
  );
}

/** `of`: `gmbs` in words. */
function Switches({ g, gmbs, of, auto, reload, className, hidden }: { g: Gmail; gmbs: readonly Gmb[]; of: string; auto: MailAutos; reload: () => void; className: string; hidden: string }) {
  const t = useT();
  const [asked, setAsked] = useState<MailTopic | null>(null);
  const set = async (topic: MailTopic, to: boolean) => {
    try {
      await switchAuto(
        gmbs.map((b) => b.id),
        topic,
        to,
      );
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      reload();
    }
  };
  return (
    <>
      {auto.topics.map((topic) => {
        const on = gmbs.filter((b) => auto.on.some((x) => x.gmb_id === b.id && x.topic === topic)).length;
        const checked = on === gmbs.length;
        return (
          <label key={topic} className={cn("flex min-h-9 items-center justify-between gap-3 text-sm text-ink", className)}>
            <span className={hidden}>{told(topic).name(t)}</span>
            <span className="flex items-center gap-2">
              {on > 0 && !checked && <span className="text-xs text-ink-soft tabular-nums">{t(mail.auto.some, "{on} of {n}", { on, n: gmbs.length })}</span>}
              <Switch
                checked={checked}
                disabled={g.imap === null && !checked}
                aria-label={t(mail.auto.switch, "{topic}: replies about {of} go by themselves", { topic: told(topic).name(t), of })}
                onCheckedChange={(to) => (to ? setAsked(topic) : void set(topic, false))}
              />
            </span>
          </label>
        );
      })}
      {asked !== null && (
        <ConfirmDialog
          open
          onOpenChange={(open) => !open && setAsked(null)}
          title={t(mail.auto.confirm, "{topic}: let replies about {of} go by themselves?", { topic: told(asked).name(t), of })}
          description={
            <>
              {told(asked).goes(t)}{" "}
              {t(mail.auto.confirm.how, "Each goes {minutes} minutes after it is written unless you stop it, again after {hours} h of silence, at most {limit} unanswered. Any that needs something of yours stays by hand.", { minutes: auto.hold_minutes, hours: auto.re_ask_hours, limit: auto.limit })}
              <Drafts gmbs={gmbs} topic={asked} />
            </>
          }
          confirmLabel={t(mail.auto.confirm.on, "Switch on")}
          labels={{ cancel: t(mail.cancel, "Cancel") }}
          onConfirm={() => {
            setAsked(null);
            return set(asked, true);
          }}
        />
      )}
    </>
  );
}

/** What is written now: what such a reply says. */
function Drafts({ gmbs, topic }: { gmbs: readonly Gmb[]; topic: MailTopic }) {
  const t = useT();
  const cases = useMailCases();
  if (cases.status !== "ok") return null;
  const written = cases.data.cases.flatMap((c) => {
    const gmb = gmbs.find((b) => b.id === c.gmb_id);
    const body = c.draft !== null && (c.draft.state === "proposed" || c.draft.state === "held") ? c.draft.body : null;
    return gmb === undefined || c.topic !== topic || over(c) || body === null ? [] : [{ c, gmb, body }];
  });
  return (
    <span className="mt-3 flex max-h-72 flex-col gap-2 overflow-y-auto">
      <span className="font-medium text-ink">{written.length === 0 ? t(mail.auto.confirm.none, "None is written now.") : t(mail.auto.confirm.written, "Written now, yours to send:")}</span>
      {written.map(({ c, gmb, body }) => (
        <span key={c.id} className="flex flex-col gap-1">
          <span className="text-xs">
            {gmbs.length > 1 && `${gmb.name} · `}
            {subjectOf(c, t)}
          </span>
          <span className="rounded-md bg-hover/60 px-3 py-2 text-left break-words whitespace-pre-wrap text-ink">{body}</span>
        </span>
      ))}
    </span>
  );
}
