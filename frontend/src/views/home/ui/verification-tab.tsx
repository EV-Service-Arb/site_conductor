"use client";

import { Badge, Button, Planned, Skeleton } from "@evinvest/uikit";
import { ChevronRight, Phone, RefreshCw, ShieldCheck, Video } from "lucide-react";

import { useNow } from "@/entities/alerts";
import { type Listed, STATE_NAME, openWindow, windowKeys } from "@/entities/gmb";
import { METHOD_NAME, MailLines, OUTCOME_NAME, OUTCOME_TONE, over, unnamed, useCases, useGmbMail } from "@/entities/mail";
import type { Gmail, GmbMail, MailKind, MailLine, VerificationAttempt, VerificationMethod, VerificationOutcome } from "@/shared/api/schema";
import { useLocale, useT } from "@/shared/i18n";
import { formatDateTime } from "@/shared/lib/format";
import { ViewLink } from "@/shared/lib/location";
import { ago } from "@/shared/lib/time";
import { ErrorState } from "@/shared/ui/error-state";
import { useButtonSize } from "@/shared/ui/touch";

import { useLastWord } from "../model/last-word";

/** A verifying GBP's road to Verified (05e). */
export function VerificationTab({ listed: { gmb, gmail } }: { listed: Listed }) {
  const t = useT();
  const locale = useLocale();
  const button = useButtonSize();
  const mail = useGmbMail(gmb.id);
  const word = useLastWord(gmb.id);
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
        <p className="min-w-0 flex-1 basis-60 font-medium break-words text-ink">
          {word === null && t(windowKeys.verification.banner.none, "Nothing from Google yet")}
          {word != null &&
            ("who" in word ? (
              <ViewLink to={{ view: "email", params: { case: word.case_id } }} className="rounded-sm outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring">
                {t(windowKeys.verification.banner.email, "Google wrote: {said} · {ago}", { said: word.summary ?? word.subject ?? t(windowKeys.log.noSubject, "(no subject)"), ago: ago(word.at, locale) })}
              </ViewLink>
            ) : (
              t(windowKeys.verification.banner.state, "{from} → {to} · {ago}", { from: STATE_NAME[word.from](t), to: STATE_NAME[word.to](t), ago: ago(word.at, locale) })
            ))}
        </p>
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

/** A try at verifying a GBP, as its mail's reader says it. */
const TRIES: readonly (MailKind | null)[] = ["video_rejected", "video_in_review", "call_offered", "call_failed"];

/** How far it is, each try at verifying it, support's thread and the rest of Google's mail, as it was read. */
function FromGoogle({ mail: { attempts, cases, lines }, gmail }: { mail: GmbMail; gmail: Gmail | null }) {
  const t = useT();
  const card = "flex flex-col gap-2 rounded-lg border border-border px-4 py-3";
  // a lone notice carries no number: support's conversations do
  const support = new Set(cases.filter((c) => c.topic === "verification" && c.numbers.length > 0).map((c) => c.id));
  const thread = lines.filter((l) => support.has(l.case_id));
  const others = lines.filter((l) => !support.has(l.case_id) && !attempts.some((a) => a.message_id === l.id));
  // newest first: a video's review is over with what came of it
  const tries = attempts.filter((a, i) => a.outcome !== "in_review" || !attempts.slice(0, i).some((b) => b.method === "video" || b.outcome === "verified"));
  const { shown } = useCases();
  const nameless = (shown ?? []).filter((c) => c.gmail_id === gmail?.id && unnamed(c) && !over(c) && TRIES.includes(c.said));
  return (
    <>
      <Progress attempts={attempts} thread={thread} />
      <section className={card}>
        <h3 className="text-sm font-semibold text-ink">{t(windowKeys.verification.attempts, "Attempts")}</h3>
        {attempts.length === 0 && nameless.length === 0 && <p className="text-sm text-ink-soft">{t(windowKeys.verification.attempts.none, "None in Google's mail.")}</p>}
        <ol className="flex flex-col divide-y divide-border">
          {tries.map((a) => (
            <Attempt key={a.message_id} a={a} />
          ))}
          {nameless[0] !== undefined && (
            <li>
              <ViewLink to={{ view: "email", params: nameless.length === 1 ? { case: nameless[0].id } : {} }} className="flex items-center gap-1 rounded-sm py-2.5 text-sm text-ink outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring">
                {t(windowKeys.verification.attempts.nameless, "{n} of {gmail} name no GBP", { n: nameless.length, gmail: gmail!.gmail })}
                <ChevronRight aria-hidden className="size-3.5 text-ink-soft" />
              </ViewLink>
            </li>
          )}
        </ol>
      </section>
      <section className={card}>
        <h3 className="text-sm font-semibold text-ink">{t(windowKeys.verification.support, "With Google support")}</h3>
        {thread.length === 0 ? <p className="text-sm text-ink-soft">{t(windowKeys.verification.support.none, "No conversation with support.")}</p> : <MailLines lines={thread} />}
      </section>
      {(others.length > 0 || lines.length === 0) && (
        <section className={card}>
          <h3 className="text-sm font-semibold text-ink">{t(windowKeys.verification.emails, "Other emails with Google")}</h3>
          {lines.length === 0 && <p className="text-sm text-ink-soft">{t(windowKeys.log.noEmails, "No emails with Google: they are read from the gmail's inbox once its IMAP is set.")}</p>}
          <MailLines lines={others} />
        </section>
      )}
    </>
  );
}

const DAY_MS = 24 * 3600 * 1000;

/** The road from the first try to a verified GBP, how far it came and for how long. `thread`: the lines with support. */
function Progress({ attempts, thread }: { attempts: readonly VerificationAttempt[]; thread: readonly MailLine[] }) {
  const t = useT();
  const now = useNow(60_000);
  const tried = (method: VerificationMethod, outcome: VerificationOutcome) => attempts.filter((a) => a.method === method && a.outcome === outcome).length;
  const steps = [
    [t(windowKeys.verification.step.rejected, "Video rejected"), tried("video", "rejected")],
    [t(windowKeys.verification.step.asked, "Live call asked"), thread.filter((l) => l.who === "us").length],
    [t(windowKeys.verification.step.deflected, "Deflected"), thread.filter((l) => l.kind === "self_serve").length],
    [t(windowKeys.verification.step.offered, "Call offered"), tried("call", "offered")],
    [t(windowKeys.verification.step.held, "Call held"), tried("call", "failed")],
    [t(windowKeys.verification.step.verified, "Verified"), attempts.filter((a) => a.outcome === "verified").length],
  ] as const;
  const first = [...attempts, ...thread.filter((l) => l.who === "us")].map((x) => Date.parse(x.at));
  if (first.length === 0) return null;
  const days = Math.floor((now.getTime() - Math.min(...first)) / DAY_MS);
  return (
    <section className="flex flex-col gap-1.5">
      <ol className="flex flex-wrap items-center gap-x-1.5 gap-y-1 text-sm">
        {steps.map(([name, n], i) => (
          <li key={name} className={n > 0 ? "flex items-center gap-1.5 text-ink" : "flex items-center gap-1.5 text-ink-faint"}>
            {i > 0 && <ChevronRight aria-hidden className="size-3.5 text-ink-faint" />}
            {name}
            {n > 1 && <span className="text-xs text-ink-soft tabular-nums">×{n}</span>}
          </li>
        ))}
      </ol>
      <p className="text-xs text-ink-soft tabular-nums">{t(windowKeys.verification.days, "{n, plural, =0 {First tried today} one {# day since the first try} other {# days since the first try}}", { n: days })}</p>
    </section>
  );
}

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
        {a.reasons.length === 0 && <p className="text-xs text-ink-soft">{a.summary}</p>}
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
