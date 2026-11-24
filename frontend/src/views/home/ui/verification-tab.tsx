"use client";

import { useResource } from "@evinvest/api/react";
import { Badge, Button, Planned, Skeleton } from "@evinvest/uikit";
import { Phone, RefreshCw, ShieldCheck, Video } from "lucide-react";

import { type Listed, STATE_NAME, STATE_TONE, openWindow, windowKeys } from "@/entities/gmb";
import { CaseLinks, METHOD_NAME, OUTCOME_NAME, OUTCOME_TONE, useGmbMail } from "@/entities/mail";
import { AutoLink } from "@/features/auto-mail";
import { api } from "@/shared/api";
import type { Gmail, GmbMail, VerificationAttempt } from "@/shared/api/schema";
import { useLocale, useT } from "@/shared/i18n";
import { formatDateTime } from "@/shared/lib/format";
import { ViewLink } from "@/shared/lib/location";
import { ago } from "@/shared/lib/time";
import { ErrorState } from "@/shared/ui/error-state";
import { useButtonSize } from "@/shared/ui/touch";

/** A verifying GBP's road to Verified (05e). */
export function VerificationTab({ listed: { gmb, gmail } }: { listed: Listed }) {
  const t = useT();
  const locale = useLocale();
  const button = useButtonSize();
  const log = useResource(api, "/gmbs/{id}/log", { params: { path: { id: gmb.id } } });
  const mail = useGmbMail(gmb.id);
  if (log.status === "loading") return <Skeleton className="m-5 h-64" />;
  if (log.status === "error")
    return (
      <div className="p-5">
        <ErrorState failure={log.failure} onRetry={log.reload} />
      </div>
    );
  const latest = log.data.events.find((e) => e.kind === "state" || e.kind === "email");
  const kit = [
    [t(windowKeys.verification.kit.phone, "Phone"), gmb.phone],
    [t(windowKeys.verification.kit.film, "Film"), gmb.film],
    [t(windowKeys.verification.kit.address, "Address"), gmb.hidden_address && `${gmb.customers_visit ? t(windowKeys.profile.visit, "customers visit") : t(windowKeys.profile.serviceArea, "service area")} · ${gmb.hidden_address}`],
    [t(windowKeys.verification.kit.areas, "Areas"), gmb.areas.length === 0 ? null : gmb.areas.join(" · ")],
  ] as const;
  return (
    <div className="@container flex flex-col gap-4 p-5">
      <section className="flex flex-wrap items-center gap-x-4 gap-y-2 rounded-lg border border-accent-info/40 bg-accent-info/5 px-4 py-3">
        <ShieldCheck aria-hidden className="size-5 shrink-0 text-accent-info" />
        <div className="flex min-w-0 flex-1 basis-60 flex-col gap-0.5">
          <p className="font-medium break-words text-ink">
            {latest === undefined
              ? t(windowKeys.verification.banner.none, "Nothing from Google yet")
              : latest.kind === "state"
                ? t(windowKeys.verification.banner.state, "{from} → {to} · {ago}", { from: STATE_NAME[latest.from](t), to: STATE_NAME[latest.to](t), ago: ago(latest.at, locale) })
                : t(windowKeys.verification.banner.email, "Google wrote: {subject} · {ago}", { subject: latest.kind === "email" ? (latest.subject ?? t(windowKeys.log.noSubject, "(no subject)")) : "", ago: ago(latest.date, locale) })}
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
          {mail.status === "loading" && <Skeleton className="h-40" />}
          {mail.status === "error" && <ErrorState failure={mail.failure} onRetry={mail.reload} />}
          {mail.status === "ok" && <FromGoogle mail={mail.data} gmail={gmail} />}
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

/** Each try at verifying it, what stands since a rejected video, and Google's mail, as it was read. */
function FromGoogle({ mail: { attempts, cases }, gmail }: { mail: GmbMail; gmail: Gmail | null }) {
  const t = useT();
  const card = "flex flex-col gap-2 rounded-lg border border-border px-4 py-3";
  return (
    <>
      <section className={card}>
        <h3 className="text-sm font-semibold text-ink">{t(windowKeys.verification.attempts, "Attempts")}</h3>
        {attempts.length === 0 && <p className="text-sm text-ink-soft">{t(windowKeys.verification.attempts.none, "None in Google's mail.")}</p>}
        <ol className="flex flex-col divide-y divide-border">
          {attempts.map((a) => (
            <Attempt key={a.message_id} a={a} />
          ))}
        </ol>
      </section>
      {rejected(attempts) && (
        <section className={card}>
          <h3 className="text-sm font-semibold text-ink">{t(windowKeys.verification.after, "After a rejected video")}</h3>
          <AfterRejected cases={cases} gmail={gmail} />
        </section>
      )}
      <section className={card}>
        <h3 className="text-sm font-semibold text-ink">{t(windowKeys.verification.emails, "Google's emails")}</h3>
        {cases.length === 0 && <p className="text-sm text-ink-soft">{t(windowKeys.log.noEmails, "No emails with Google: they are read from the gmail's inbox once its IMAP is set.")}</p>}
        <CaseLinks cases={cases} />
      </section>
    </>
  );
}

/** Whether the latest try of a recorded video was rejected and nothing verified it since. */
const rejected = (attempts: readonly VerificationAttempt[]) => attempts.find((a) => a.method === "video" || a.outcome === "verified")?.outcome === "rejected";

function Attempt({ a }: { a: VerificationAttempt }) {
  const t = useT();
  const locale = useLocale();
  const Icon = a.method === "video" ? Video : a.method === "call" ? Phone : ShieldCheck;
  return (
    <li className="flex items-start gap-3 py-2.5">
      <Icon aria-hidden className="mt-0.5 size-4 shrink-0 text-ink-soft" />
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <ViewLink to={{ view: "email", params: { case: a.case_id } }} className="rounded-sm text-sm text-ink outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring">
          {METHOD_NAME[a.method](t)} · {formatDateTime(a.at, locale)}
        </ViewLink>
        <p className="text-xs text-ink-soft">{a.summary}</p>
        {a.reasons.map((r) => (
          <p key={r} className="text-xs break-words text-accent-error">
            “{r}”
          </p>
        ))}
      </div>
      <Badge tone={OUTCOME_TONE[a.outcome]}>{OUTCOME_NAME[a.outcome](t)}</Badge>
    </li>
  );
}

function AfterRejected({ cases, gmail }: { cases: GmbMail["cases"]; gmail: Gmail | null }) {
  const t = useT();
  // a lone notice carries no number: support's conversations do
  const asking = cases.filter((c) => c.topic === "verification" && c.numbers.length > 0 && c.state.is !== "done" && c.state.is !== "quiet");
  return (
    <>
      {asking.length === 0 ? <p className="text-sm text-ink-soft">{t(windowKeys.verification.after.none, "No open conversation asks Google for the live call.")}</p> : <CaseLinks cases={asking} />}
      {gmail !== null && <AutoLink gmail={gmail.id} />}
    </>
  );
}
