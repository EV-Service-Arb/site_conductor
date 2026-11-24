"use client";

import { ConfirmDialog, Skeleton, Switch } from "@evinvest/uikit";
import { useResource } from "@evinvest/api/react";
import { Mail } from "lucide-react";
import { useState } from "react";

import { gmbs as gmbsKeys, under, useGmails, useScope } from "@/entities/gmb";
import { mail, switchAuto } from "@/entities/mail";
import { api } from "@/shared/api";
import type { Gmail, MailTopic } from "@/shared/api/schema";
import { type T, useT } from "@/shared/i18n";
import { EmptyState } from "@/shared/ui/empty-state";
import { ErrorState } from "@/shared/ui/error-state";
import { notifyFailure } from "@/shared/ui/notify";

/** The topics that can be automatic: what their replies ask for, and what goes out. */
const TOPICS: Partial<Record<MailTopic, { name: (t: T) => string; goes: (t: T, gbps: number) => string }>> = {
  verification: {
    name: (t) => t(mail.auto.verification, "Live verification"),
    goes: (t, gbps) => t(mail.auto.verification.goes, "Replies asking Google support for the live verification call, about {n} GBPs, go out by themselves.", { n: gbps }),
  },
  reviews: {
    name: (t) => t(mail.auto.reviews, "Review reinstatement"),
    goes: (t, gbps) => t(mail.auto.reviews.goes, "Replies asking Google support to bring missing reviews back, about {n} GBPs, go out by themselves.", { n: gbps }),
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
  const [asked, setAsked] = useState<{ gmail: Gmail; topic: MailTopic } | null>(null);
  const [busy, setBusy] = useState(false);
  if (auto.status === "error") return <ErrorState failure={auto.failure} onRetry={auto.reload} />;
  if (auto.status === "loading" || gmails.status !== "ok") return <Skeleton className="h-60" />;
  const listed = gmails.data.gmails.filter((g) => scope.gmails === null || scope.gmails.has(g.id));
  if (listed.length === 0) return <EmptyState title={t(mail.auto.none, "No gmails in scope")} />;
  const { on, topics } = auto.data;
  const set = async (gmail: Gmail, topic: MailTopic, to: boolean) => {
    setBusy(true);
    try {
      await switchAuto(gmail.id, topic, to);
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
      auto.reload();
    }
  };
  return (
    <div className="flex flex-col gap-3">
      <ul className="flex flex-col divide-y divide-border rounded-lg border border-border">
        {listed.map((g) => (
          <li key={g.id} className="flex flex-col gap-3 px-4 py-3 @2xl:flex-row @2xl:items-center">
            <span className="flex min-w-0 flex-1 items-center gap-2 text-sm text-ink">
              <Mail aria-hidden className="size-4 shrink-0 text-ink-soft" />
              <span className="truncate">{g.gmail}</span>
              <span className="shrink-0 text-xs text-ink-soft tabular-nums">{t(gmbsKeys.count, "{n} GBPs", { n: under(gmails.data, g.id).length })}</span>
            </span>
            {topics.map((topic) => (
              <label key={topic} className="flex min-h-9 items-center justify-between gap-3 text-sm text-ink @2xl:w-56 @2xl:justify-end">
                {told(topic).name(t)}
                <Switch
                  checked={on.some((x) => x.gmail_id === g.id && x.topic === topic)}
                  disabled={busy}
                  aria-label={t(mail.auto.switch, "{topic} replies from {gmail} go by themselves", { topic: told(topic).name(t), gmail: g.gmail })}
                  onCheckedChange={(to) => (to ? setAsked({ gmail: g, topic }) : void set(g, topic, false))}
                />
              </label>
            ))}
          </li>
        ))}
      </ul>
      {asked !== null && (
        <ConfirmDialog
          open
          onOpenChange={(open) => !open && setAsked(null)}
          title={t(mail.auto.confirm, "{topic}: let replies from {gmail} go by themselves?", { topic: told(asked.topic).name(t), gmail: asked.gmail.gmail })}
          description={`${told(asked.topic).goes(t, under(gmails.data, asked.gmail.id).length)} ${t(mail.auto.confirm.how, "Each goes 15 minutes after it is written, unless you stop it. One that needs something of yours never goes by itself.")}`}
          confirmLabel={t(mail.auto.confirm.on, "Switch on")}
          labels={{ cancel: t(mail.cancel, "Cancel") }}
          onConfirm={() => {
            setAsked(null);
            return set(asked.gmail, asked.topic, true);
          }}
        />
      )}
    </div>
  );
}
