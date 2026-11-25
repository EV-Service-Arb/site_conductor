"use client";

import { Button, cn } from "@evinvest/uikit";
import { TriangleAlert } from "lucide-react";
import type { ReactNode } from "react";

import { useNow } from "@/entities/alerts";
import { deadlineOf, pageOf } from "@/entities/gmb";
import { mail, whyText } from "@/entities/mail";
import type { MailCase } from "@/shared/api/schema";
import { useLocale, useT } from "@/shared/i18n";
import { formatDateTime } from "@/shared/lib/format";
import { ViewLink } from "@/shared/lib/location";
import { when } from "@/shared/lib/time";
import { useButtonSize } from "@/shared/ui/touch";

import { Mend } from "./mend";

const NOTE = { warn: "border-accent-warn/40 bg-accent-warn/5 [&>svg]:text-accent-warn", error: "border-accent-error/40 bg-accent-error/5 [&>svg]:text-accent-error" };

export function Note({ tone, icon, children }: { tone: keyof typeof NOTE; icon: ReactNode; children: ReactNode }) {
  return (
    <section className={cn("flex gap-3 rounded-lg border px-4 py-3 text-sm text-ink [&>svg]:mt-0.5 [&>svg]:size-4 [&>svg]:shrink-0", NOTE[tone])}>
      {icon}
      <div className="flex min-w-0 flex-1 flex-col gap-2">{children}</div>
    </section>
  );
}

/** Why a person decides, and the date Google set. */
export function Why({ c }: { c: MailCase }) {
  const t = useT();
  const locale = useLocale();
  const now = useNow(60_000);
  const button = useButtonSize();
  const why = c.state.is === "needs_you" ? c.state.why : null;
  const deadline = deadlineOf(c);
  if (why === null && deadline === null) {
    if (c.state.is !== "waiting" || c.said !== "routed") return null;
    const until = when(c.state.until, locale, now);
    return (
      <p className="text-sm text-ink-soft">
        {c.last.who === "us" || c.draft?.state === "sent"
          ? t(mail.routed.pressed, "Asked again. With no answer by {until}, it needs you.", { until })
          : t(mail.routed, "A specialist is to answer by {until}. Silent past it, one reply asks again.", { until })}
      </p>
    );
  }
  // what the draft lacks stands beside its text
  const said = why === null || why.code === "deadline" || why.code === "needs" ? null : whyText(c, why, t, locale, now);
  if (said === null && deadline === null) return null;
  const reasons = why !== null && c.last.who !== "us" ? c.last.reasons : [];
  return (
    <Note tone={deadline !== null || why?.code === "failed" || why?.code === "login" || why?.code === "may_have_gone_out" ? "error" : "warn"} icon={<TriangleAlert aria-hidden />}>
      {deadline !== null && (
        <p className="font-medium">
          {Date.parse(deadline) > now.getTime() ? t(mail.deadline, "Google's deadline: {at}", { at: formatDateTime(deadline, locale) }) : t(mail.deadline.passed, "Google's deadline passed: {at}", { at: formatDateTime(deadline, locale) })}
        </p>
      )}
      {said !== null && <p className="break-words">{said}</p>}
      {reasons.map((r) => (
        <p key={r} className="break-words text-accent-error">
          “{r}”
        </p>
      ))}
      {why?.code === "stuck" && <Mend />}
      {why?.code === "login" && (
        <Button asChild variant="outline" size={button("sm")} className="self-start">
          <ViewLink to={pageOf({ kind: "account", gmail: c.gmail_id, tab: "connections" })}>{t(mail.inbox.reconnect, "Reconnect")}</ViewLink>
        </Button>
      )}
      {why?.code === "unread" && why.error !== null && <p className="text-ink-soft">{t(mail.unread.mend, "Read it below and set its kind.")}</p>}
    </Note>
  );
}
