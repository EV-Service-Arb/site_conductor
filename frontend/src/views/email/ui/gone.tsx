"use client";

import { Archive, Camera, CameraOff, ChevronRight } from "lucide-react";

import { mail } from "@/entities/mail";
import type { MailGone } from "@/shared/api/schema";
import { useLocale, useT } from "@/shared/i18n";
import { formatDateTime } from "@/shared/lib/format";
import { ViewLink } from "@/shared/lib/location";
import { age } from "@/shared/lib/time";

/** The GBP's reviews missing now, as the archive saw them: what a reply's evidence is of. */
export function Gone({ gone, gmb }: { gone: MailGone; gmb: number | null }) {
  const t = useT();
  return (
    <section className="flex flex-col gap-2 rounded-lg border border-border px-4 py-3">
      <header className="flex flex-wrap items-center gap-x-3 gap-y-1">
        <h3 className="flex-1 text-sm font-semibold text-ink">
          {gone.state === "unknown" ? t(mail.gone, "Missing reviews") : gone.more ? t(mail.gone.atLeast, "At least {n} reviews missing", { n: gone.reviews.length }) : t(mail.gone.n, "{n} reviews missing", { n: gone.reviews.length })}
        </h3>
        {gmb !== null && (
          <ViewLink to={{ view: "review_archive.mine.gmb", params: { gmb } }} className="flex items-center gap-1 rounded-sm text-sm text-ink outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring">
            <Archive aria-hidden className="size-4 text-ink-soft" />
            {t(mail.gone.archive, "Its reviews in the archive")}
            <ChevronRight aria-hidden className="size-3.5" />
          </ViewLink>
        )}
      </header>
      {gone.state === "unknown" ? (
        <p className="text-sm break-words text-accent-warn">{t(mail.gone.unknown, "Not known: {why}.", { why: gone.why })}</p>
      ) : (
        <ul className="flex max-h-80 flex-col divide-y divide-border overflow-y-auto">
          {gone.reviews.map((r) => (
            <Review key={r.review_id} r={r} />
          ))}
        </ul>
      )}
    </section>
  );
}

function Review({ r }: { r: Extract<MailGone, { state: "read" }>["reviews"][number] }) {
  const t = useT();
  const locale = useLocale();
  const held = r.captured ? t(mail.gone.captured, "Screenshot held") : t(mail.gone.uncaptured, "No screenshot");
  const Held = r.captured ? Camera : CameraOff;
  return (
    <li className="flex flex-col gap-0.5 py-1.5 text-sm">
      <span className="flex flex-wrap items-baseline gap-x-2">
        {r.author_url === null ? (
          <span className="text-ink">{r.author}</span>
        ) : (
          <a href={r.author_url} target="_blank" rel="noreferrer" className="text-ink hover:underline">
            {r.author}
          </a>
        )}
        <span className="text-xs text-ink-soft tabular-nums">{r.rating === null ? t(mail.gone.unrated, "no stars") : `★${r.rating}`}</span>
        <span className="text-xs text-ink-soft" title={formatDateTime(r.gone_at, locale)}>
          {t(mail.gone.since, "gone {since}", { since: age(r.gone_at, locale) })}
        </span>
        {r.reinstatement_requested_at !== null && <span className="text-xs text-ink-soft">{t(mail.gone.asked, "asked back {since}", { since: age(r.reinstatement_requested_at, locale) })}</span>}
        {r.replied && <span className="text-xs text-ink-soft">{t(mail.gone.replied, "replied to")}</span>}
        <span className="flex-1" />
        <span className={r.captured ? "flex items-center gap-1 text-xs text-positive" : "flex items-center gap-1 text-xs text-ink-soft"}>
          <Held aria-hidden className="size-3.5" />
          {held}
        </span>
      </span>
      {r.text !== null && <span className="truncate text-xs text-ink-soft">{r.text}</span>}
    </li>
  );
}
