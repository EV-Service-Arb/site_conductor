"use client";

import { useResource } from "@evinvest/api/react";
import { Badge, Button, Planned, Skeleton, toast } from "@evinvest/uikit";
import { ExternalLink, Mail, RefreshCw, ShieldCheck } from "lucide-react";
import { useState } from "react";

import { type Listed, STATE_NAME, STATE_TONE, openWindow, windowKeys } from "@/entities/gmb";
import { api, ok } from "@/shared/api";
import { useLocale, useT } from "@/shared/i18n";
import { formatDateTime } from "@/shared/lib/format";
import { ago } from "@/shared/lib/time";
import { ErrorState } from "@/shared/ui/error-state";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize } from "@/shared/ui/touch";

/** A verifying GBP's road to Verified (05e). */
export function VerificationTab({ listed: { gmb, gmail } }: { listed: Listed }) {
  const t = useT();
  const locale = useLocale();
  const button = useButtonSize();
  const log = useResource(api, "/gmbs/{id}/log", { params: { path: { id: gmb.id } } });
  const [opening, setOpening] = useState<string | null>(null);
  if (log.status === "loading") return <Skeleton className="m-5 h-64" />;
  if (log.status === "error")
    return (
      <div className="p-5">
        <ErrorState failure={log.failure} onRetry={log.reload} />
      </div>
    );
  const latest = log.data.events.find((e) => e.kind === "state" || e.kind === "email");
  const emails = log.data.events.flatMap((e) => (e.kind === "email" ? [e] : []));
  const open = async (thread: string, gmailId: number | null) => {
    setOpening(thread);
    try {
      if (gmailId === null) throw new Error("an email is logged under its gmail");
      await ok(api.POST("/gmails/{id}/browser/thread", { params: { path: { id: gmailId } }, body: { thread } }));
      toast(t(windowKeys.log.threadOpened, "Opened in the gmail's browser"));
    } catch (err) {
      notifyFailure(err, t);
    } finally {
      setOpening(null);
    }
  };
  const kit = [
    [t(windowKeys.verification.kit.phone, "Phone"), gmb.phone],
    [t(windowKeys.verification.kit.film, "Film"), gmb.film],
    [t(windowKeys.verification.kit.address, "Address"), gmb.hidden_address && `${gmb.customers_visit ? t(windowKeys.profile.visit, "customers visit") : t(windowKeys.profile.serviceArea, "service area")} · ${gmb.hidden_address}`],
    [t(windowKeys.verification.kit.areas, "Areas"), gmb.areas.length === 0 ? null : gmb.areas.join(" · ")],
  ] as const;
  return (
    <div className="@container flex flex-col gap-4 p-5">
      <section className="flex items-center gap-4 rounded-lg border border-accent-info/40 bg-accent-info/5 px-4 py-3">
        <ShieldCheck aria-hidden className="size-5 shrink-0 text-accent-info" />
        <div className="flex min-w-0 flex-1 flex-col gap-0.5">
          <p className="font-medium text-ink">
            {latest === undefined
              ? t(windowKeys.verification.banner.none, "Nothing from Google yet")
              : latest.kind === "state"
                ? t(windowKeys.verification.banner.state, "{from} → {to} · {ago}", { from: STATE_NAME[latest.from](t), to: STATE_NAME[latest.to](t), ago: ago(latest.at, locale) })
                : t(windowKeys.verification.banner.email, "Google wrote: {subject} · {ago}", { subject: latest.kind === "email" ? (latest.subject ?? t(windowKeys.log.noSubject, "(no subject)")) : "", ago: ago(latest.at, locale) })}
          </p>
          <p className="text-sm text-ink-soft">{t(windowKeys.verification.banner.hint, "Google's own word: its state moves and its emails to the gmail, read from the inbox.")}</p>
        </div>
        <Badge tone={STATE_TONE[gmb.state]}>{STATE_NAME[gmb.state](t)}</Badge>
        <Button variant="outline" size={button("sm")} planned={t(windowKeys.verification.read.planned, "Read Google's state now: it is read every 6 h")}>
          <RefreshCw aria-hidden />
          {t(windowKeys.verification.read, "Read Google now")}
        </Button>
      </section>

      <div className="grid grid-cols-1 gap-4 @3xl:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]">
        <div className="flex flex-col gap-4">
          <Planned what={t(windowKeys.verification.attempts.what, "Each try at verifying it, one row per try: its kind, when, the film or the call, and Google's answer")}>
            <h3 className="text-sm font-semibold text-ink">{t(windowKeys.verification.attempts, "Attempts")}</h3>
            <p className="text-sm text-ink">1 · Video · Nov 9, 14:31 <span className="text-accent-error">rejected</span></p>
            <p className="text-xs text-ink-soft">film v2 · 38 s, from the kit, fed as camera · one cut, nothing generated</p>
            <p className="text-sm text-ink">2 · Live call through support · Nov 12 <span className="text-accent-info">waiting</span></p>
          </Planned>

          <section className="flex flex-col gap-2 rounded-lg border border-border px-4 py-3">
            <h3 className="text-sm font-semibold text-ink">{t(windowKeys.verification.emails, "Google's emails")}</h3>
            {emails.length === 0 && (
              <p className="text-sm text-ink-soft">{t(windowKeys.log.noEmails, "No emails with Google: they are read from the gmail's inbox once its IMAP is set.")}</p>
            )}
            <ul className="flex flex-col divide-y divide-border">
              {emails.map((e) => (
                <li key={e.id} className="flex items-start gap-3 py-2.5">
                  <Mail aria-hidden className="mt-0.5 size-4 shrink-0 text-ink-soft" />
                  <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                    <p className="text-sm text-ink">{e.subject ?? t(windowKeys.log.noSubject, "(no subject)")}</p>
                    <p className="text-xs text-ink-soft">
                      {[e.sent ? t(windowKeys.log.emailTo, "to {who}", { who: e.counterparts.join(", ") }) : t(windowKeys.log.emailFrom, "from {who}", { who: e.counterparts.join(", ") }), formatDateTime(e.date, locale), e.of_gmail ? t(windowKeys.log.ofGmail, "the gmail's, {gmail}", { gmail: gmail?.gmail ?? "" }) : null].filter(Boolean).join(" · ")}
                    </p>
                    {e.snippet && <p className="line-clamp-2 text-xs text-ink-soft">{e.snippet}</p>}
                    <button type="button" disabled={opening === e.thread} onClick={() => void open(e.thread, e.gmail_id)} className="flex items-center gap-1 self-start text-xs text-ink-soft hover:text-ink disabled:opacity-50">
                      <ExternalLink aria-hidden className="size-3" />
                      {t(windowKeys.log.openThread, "Open thread")}
                    </button>
                  </div>
                </li>
              ))}
            </ul>
          </section>

          <Planned what={t(windowKeys.verification.after.what, "The playbook's verification guide for after a rejected video, step by step, with what is done and what is next")}>
            <h3 className="text-sm font-semibold text-ink">{t(windowKeys.verification.after, "After a rejected video")}</h3>
            <p className="text-sm text-ink">Live is next, not a second video</p>
            <p className="text-sm text-ink">Tickets opened, three at once</p>
            <p className="text-sm text-ink">The IMAP rule re-asks for the call until one is booked</p>
            <p className="text-sm text-ink-soft">On the call, from the verification kit</p>
          </Planned>
        </div>

        <div className="flex flex-col gap-4">
          <Planned what={t(windowKeys.verification.shows.what, "What Google shows of it now, read every 6 h, and how that changed")}>
            <h3 className="text-sm font-semibold text-ink">{t(windowKeys.verification.shows, "What Google shows")}</h3>
            <p className="font-mono text-xs text-ink">now · Get verified · a specialist will call</p>
            <p className="font-mono text-xs text-ink-soft">Nov 12 18:02 · Couldn’t verify · try another way</p>
            <p className="font-mono text-xs text-ink-soft">Nov 9 14:40 · Video in review</p>
          </Planned>

          <section className="flex flex-col gap-2 rounded-lg border border-border px-4 py-3">
            <h3 className="flex items-baseline gap-2 text-sm font-semibold text-ink">
              {t(windowKeys.verification.kit, "Verification kit")}
              <button type="button" onClick={() => openWindow({ kind: "gbp", gmb: gmb.id, tab: "connections" })} className="text-xs font-normal text-ink-soft hover:text-ink">
                {t(windowKeys.verification.kit.in, "in Connections")}
              </button>
            </h3>
            <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
              {kit.map(([label, value]) => (
                <div key={label} className="contents">
                  <dt className="text-ink-soft">{label}</dt>
                  <dd className="min-w-0 break-words text-right text-ink">{value ?? <span className="text-accent-warn">{t(windowKeys.browser.unset, "not set")}</span>}</dd>
                </div>
              ))}
            </dl>
          </section>

          <Planned what={t(windowKeys.verification.waits.what, "The fields prepared on the draft that the profile applies the hour Google verifies it")}>
            <h3 className="text-sm font-semibold text-ink">{t(windowKeys.verification.waits, "Waits for Verified")}</h3>
            <ul className="grid grid-cols-2 gap-2 text-xs">
              {[
                ["More categories", "Débouchage +3"],
                ["Hours", "24/7 · urgences"],
                ["Description", "agent draft · 712 ch"],
                ["Services", "12 from template"],
              ].map(([label, value]) => (
                <li key={label} className="rounded-md bg-hover/60 px-2 py-1.5">
                  <span className="block text-ink-soft">{label}</span>
                  <span className="text-ink">{value}</span>
                </li>
              ))}
            </ul>
          </Planned>
        </div>
      </div>
    </div>
  );
}
