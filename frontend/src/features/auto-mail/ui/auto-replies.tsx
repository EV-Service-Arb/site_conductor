"use client";

import { ConfirmDialog, Skeleton, Switch, cn } from "@evinvest/uikit";
import { useResource } from "@evinvest/api/react";
import { Mail } from "lucide-react";
import { useState } from "react";

import { pageOf, under, useGmails, useScope } from "@/entities/gmb";
import { mail, switchAuto } from "@/entities/mail";
import { api } from "@/shared/api";
import type { Gmail, Gmb, MailAutos, MailTopic } from "@/shared/api/schema";
import { type T, useT } from "@/shared/i18n";
import { ViewLink } from "@/shared/lib/location";
import { EmptyState } from "@/shared/ui/empty-state";
import { ErrorState } from "@/shared/ui/error-state";
import { notifyFailure } from "@/shared/ui/notify";

/** The topics that can be automatic: what their replies ask for, of which GBPs, and what goes out. */
const TOPICS: Partial<Record<MailTopic, { name: (t: T) => string; of: Gmb["state"]; goes: (t: T, n: number) => string }>> = {
  verification: {
    name: (t) => t(mail.auto.verification, "Live verification"),
    of: "verifying",
    goes: (t, n) => t(mail.auto.verification.goes, "They ask Google support for the live call of its {n, plural, one {# verifying GBP} other {# verifying GBPs}}.", { n }),
  },
  reviews: {
    name: (t) => t(mail.auto.reviews, "Review reinstatement"),
    of: "verified",
    goes: (t, n) => t(mail.auto.reviews.goes, "They ask Google support for the missing reviews of its {n, plural, one {# verified GBP} other {# verified GBPs}}.", { n }),
  },
};

function told(topic: MailTopic) {
  const of = TOPICS[topic];
  if (of === undefined) throw new Error(`replies on ${topic} can be automatic, and have no name here`);
  return of;
}

/** Which gmails answer Google by themselves, and on what: off until switched on. */
export function AutoReplies() {
  const t = useT();
  const auto = useResource(api, "/mail/auto");
  const gmails = useGmails();
  const scope = useScope();
  if (auto.status === "error") return <ErrorState failure={auto.failure} onRetry={auto.reload} />;
  if (auto.status === "loading" || gmails.status !== "ok") return <Skeleton className="h-60" />;
  const listed = gmails.data.gmails.filter((g) => scope.gmails === null || scope.gmails.has(g.id));
  if (listed.length === 0) return <EmptyState title={t(mail.auto.none, "No gmails in scope")} />;
  return (
    <div className="flex flex-col gap-3">
      {!auto.data.by_itself && <p className="rounded-lg border border-border bg-hover/60 px-4 py-3 text-sm text-ink">{t(mail.auto.here, "This conductor sends nothing by itself: production's does.")}</p>}
      <ul className="flex flex-col divide-y divide-border rounded-lg border border-border">
        {listed.map((g) => (
          <li key={g.id} className="flex flex-col gap-3 px-4 py-3 @2xl:flex-row @2xl:items-center">
            <span className="flex min-w-0 flex-1 items-center gap-2 text-sm text-ink">
              <Mail aria-hidden className="size-4 shrink-0 text-ink-soft" />
              <span className="truncate">{g.gmail}</span>
              <Inbox g={g} />
            </span>
            <Switches g={g} auto={auto.data} reload={auto.reload} className="@2xl:w-56 @2xl:justify-end" />
          </li>
        ))}
      </ul>
    </div>
  );
}

/** Gmail `g`'s own switches in its GBPs' windows, or its one of `only`. */
export function AutoOf({ g, only }: { g: Gmail; only?: MailTopic }) {
  const t = useT();
  const auto = useResource(api, "/mail/auto");
  if (auto.status === "error") return <ErrorState failure={auto.failure} onRetry={auto.reload} />;
  if (auto.status === "loading") return <Skeleton className="h-16" />;
  return (
    <div role="group" aria-label={t(mail.tab.automatic, "Automatic replies")} className="flex flex-col">
      <p className="flex items-center gap-2 text-xs text-ink-soft">
        {t(mail.tab.automatic, "Automatic replies")}
        <Inbox g={g} />
      </p>
      <Switches g={g} auto={only === undefined ? auto.data : { ...auto.data, topics: auto.data.topics.filter((x) => x === only) }} reload={auto.reload} className="" />
    </div>
  );
}

/** Its inbox while nothing comes out of it, and where that is mended. */
function Inbox({ g }: { g: Gmail }) {
  const t = useT();
  if (g.imap !== null && g.imap.error === null) return null;
  return (
    <ViewLink
      to={pageOf({ kind: "account", gmail: g.id, tab: "connections" })}
      className={cn("shrink-0 rounded-sm text-xs underline outline-none focus-visible:ring-2 focus-visible:ring-ring", g.imap === null ? "text-ink-soft" : "text-accent-error")}
    >
      {g.imap === null ? t(mail.inbox.set, "Set its IMAP") : t(mail.inbox.reconnect, "Reconnect")}
    </ViewLink>
  );
}

function Switches({ g, auto: { on, topics }, reload, className }: { g: Gmail; auto: MailAutos; reload: () => void; className: string }) {
  const t = useT();
  const gmails = useGmails();
  const [asked, setAsked] = useState<MailTopic | null>(null);
  if (gmails.status !== "ok") return null;
  const set = async (topic: MailTopic, to: boolean) => {
    try {
      await switchAuto(g.id, topic, to);
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      reload();
    }
  };
  const gbps = (topic: MailTopic) => under(gmails.data, g.id).filter((x) => x.state === told(topic).of).length;
  return (
    <>
      {topics.map((topic) => {
        const checked = on.some((x) => x.gmail_id === g.id && x.topic === topic);
        return (
          <label key={topic} className={cn("flex min-h-9 items-center justify-between gap-3 text-sm text-ink", className)}>
            {told(topic).name(t)}
            <Switch
              checked={checked}
              disabled={g.imap === null && !checked}
              aria-label={t(mail.auto.switch, "{topic} replies from {gmail} go by themselves", { topic: told(topic).name(t), gmail: g.gmail })}
              onCheckedChange={(to) => (to ? setAsked(topic) : void set(topic, false))}
            />
          </label>
        );
      })}
      {asked !== null && (
        <ConfirmDialog
          open
          onOpenChange={(open) => !open && setAsked(null)}
          title={t(mail.auto.confirm, "{topic}: let replies from {gmail} go by themselves?", { topic: told(asked).name(t), gmail: g.gmail })}
          description={`${told(asked).goes(t, gbps(asked))} ${t(mail.auto.confirm.how, "Each goes 15 minutes after it is written unless you stop it, again after 48 h of silence, at most 6 unanswered. Those already written, and any that needs something of yours, stay by hand.")}`}
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
