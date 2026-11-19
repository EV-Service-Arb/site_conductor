"use client";

import { useResource } from "@evinvest/api/react";
import { Planned, Skeleton, cn } from "@evinvest/uikit";
import { ExternalLink, TrendingDown, TrendingUp } from "lucide-react";
import { useEffect, useState } from "react";

import { type Listed, STATE_NAME, useGmbTags, windowKeys } from "@/entities/gmb";
import { Refused, api, ok, reviewArchive, sites } from "@/shared/api";
import type { LogDto } from "@/shared/api/schema";
import type { GmbEvent } from "@/shared/api/services/review_archive";
import { useLocale, useT } from "@/shared/i18n";
import { ago } from "@/shared/lib/time";
import { ErrorState } from "@/shared/ui/error-state";

const DAY = 24 * 3600 * 1000;
const PERIODS = [7, 30, 90] as const;
type Period = (typeof PERIODS)[number];

/** Review counts are kept 30 days: `live_30d`. */
const REVIEWS_DAYS = 30;

type Fetched = { days: Period; at: string[] } | { days: Period; failure: Refused["failure"] };

/** Its output (02a): leads and reviews a day, what changed on it. */
export function StatsTab({ listed }: { listed: Listed }) {
  const t = useT();
  const gmb = listed.gmb.id;
  const [days, setDays] = useState<Period>(30);
  const { tags } = useGmbTags();
  const archive = tags.archive?.get(gmb);
  const [now] = useState(() => Date.now());
  const [fetched, setFetched] = useState<Fetched | null>(null);
  // this period and the one before it, to say what it is against
  useEffect(() => {
    let stale = false;
    void (async () => {
      const from = new Date(now - (2 * days - 1) * DAY).toISOString().slice(0, 10);
      const at: string[] = [];
      let cursor: string | null = null;
      try {
        do {
          const page: Awaited<ReturnType<typeof listLeads>> = await listLeads(gmb, from, cursor);
          // a lead seen before its creation has no day to count under
          at.push(...page.leads.flatMap((l) => (l.created_at === null ? [] : [l.created_at])));
          cursor = page.next_cursor;
        } while (cursor !== null);
        if (!stale) setFetched({ days, at });
      } catch (e) {
        if (!(e instanceof Refused)) throw e;
        if (!stale) setFetched({ days, failure: e.failure });
      }
    })();
    return () => {
      stale = true;
    };
  }, [gmb, days, now]);
  const leads = fetched?.days === days ? fetched : null;
  const today = Math.floor(now / DAY) * DAY;
  const perDay = Array.from({ length: days }, (_, i) => new Date(today - (days - 1 - i) * DAY).toISOString().slice(0, 10));
  const counted = leads !== null && "at" in leads ? leads.at.map((a) => new Date(a).toISOString().slice(0, 10)) : null;
  const series = counted === null ? null : perDay.map((d) => counted.filter((c) => c === d).length);
  const total = series?.reduce((a, b) => a + b, 0);
  const before = counted === null ? null : counted.length - (total ?? 0);
  const live = archive?.live_30d.slice(-Math.min(days, REVIEWS_DAYS)) ?? null;
  const collected = live?.flatMap((d) => (d === null ? [] : [d.live])) ?? [];
  const board = `/grafana/a/evmain-tilers-app/d/ra-location?var-gmb=${gmb}`;
  const chip = (on: boolean) => cn("rounded-md px-2.5 py-0.5 text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring", on ? "bg-hover text-ink" : "text-ink-soft hover:text-ink");
  return (
    <div className="flex flex-col gap-4 p-5">
      <div className="flex items-center gap-3">
        <div role="group" className="flex rounded-lg border border-border p-0.5">
          {PERIODS.map((p) => (
            <button key={p} type="button" aria-pressed={days === p} className={chip(days === p)} onClick={() => setDays(p)}>
              {t(windowKeys.stats.days, "{n} d", { n: p })}
            </button>
          ))}
        </div>
        <span className="text-xs text-ink-soft">{t(windowKeys.stats.versus, "vs previous {n} d", { n: days })}</span>
        <span className="flex-1" />
        <a href={board} target="_blank" rel="noreferrer" className="flex items-center gap-1 text-xs text-ink-soft hover:text-ink">
          {t(windowKeys.stats.grafana, "Open in Grafana")}
          <ExternalLink aria-hidden className="size-3" />
        </a>
      </div>
      <div className="grid grid-cols-[minmax(0,1fr)_18rem] items-start gap-4">
        <div className="flex min-w-0 flex-col gap-4">
          <Planned what={t(windowKeys.stats.rankingWhat, "Ranking: ARP, ATRP and SoLV over the period, with the profile edits and suspensions on it")}>
            <div className="grid grid-cols-3 gap-3">
              {[
                ["ARP", "3.2", "−1.9", t(windowKeys.stats.arp, "avg rank, my quadrant")],
                ["ATRP", "5.8", "−2.3", t(windowKeys.stats.atrp, "avg rank, all points")],
                ["SoLV", "41%", "+12 pts", t(windowKeys.stats.solv, "grid points in top 3")],
              ].map(([name, value, delta, note]) => (
                <div key={name} className="flex flex-col gap-1 rounded-lg border border-border p-3">
                  <p className="text-xs text-ink-soft">{name}</p>
                  <p className="flex items-baseline gap-2 text-2xl text-ink tabular-nums">
                    {value}
                    <span className="flex items-center gap-1 text-xs text-positive">
                      <TrendingUp aria-hidden className="size-3" />
                      {delta}
                    </span>
                  </p>
                  <p className="text-xs text-ink-soft">{note}</p>
                </div>
              ))}
            </div>
            <Bars values={[3, 4, 4, 5, 6, 5, 4, 3, 3, 4, 3, 2, 3, 3]} />
          </Planned>
          <section className="flex flex-col gap-3 rounded-panel border border-border p-3">
            <h3 className="text-sm font-semibold text-ink">{t(windowKeys.stats.output, "Output")}</h3>
            <div className="grid grid-cols-3 gap-3">
              <Kpi name={t(windowKeys.stats.leads, "Leads")} value={total} delta={total === undefined || before === null ? null : total - before} />
              <Kpi
                name={t(windowKeys.stats.liveReviews, "Live reviews")}
                value={collected.at(-1)}
                delta={collected.length < 2 ? null : collected.at(-1)! - collected[0]!}
                note={days > REVIEWS_DAYS ? t(windowKeys.stats.reviewsCapped, "kept 30 d: the 90 d range shows 30") : undefined}
              />
              <Planned what={t(windowKeys.stats.owner, "Calls · site clicks · directions: they join once a Multilogin session owns the profile")} className="p-2">
                <p className="text-xs text-ink-soft">{t(windowKeys.stats.ownerTitle, "Calls · site clicks · directions")}</p>
                <p className="text-lg text-ink">{t(windowKeys.stats.ownership, "with ownership")}</p>
              </Planned>
            </div>
            {leads === null && <Skeleton className="h-28" />}
            {leads !== null && "failure" in leads && <ErrorState failure={leads.failure} />}
            {series !== null && <Bars values={series} label={t(windowKeys.stats.leads, "Leads")} />}
            {live !== null && live.some((d) => d !== null) && <Bars values={live.map((d) => (d === null ? null : d.live))} label={t(windowKeys.stats.liveReviews, "Live reviews")} />}
          </section>
        </div>
        <Changes gmb={gmb} since={now - days * DAY} />
      </div>
    </div>
  );
}

function listLeads(gmb: number, from: string, cursor: string | null) {
  return ok(sites.GET("/leads", { params: { query: { gmb: String(gmb), created_from: from, limit: 200, cursor } } }));
}

function Kpi({ name, value, delta, note }: { name: string; value: number | undefined; delta: number | null; note?: string | undefined }) {
  return (
    <div className="flex flex-col gap-1 rounded-lg border border-border p-3">
      <p className="text-xs text-ink-soft">{name}</p>
      <p className="flex items-baseline gap-2 text-2xl text-ink tabular-nums">
        {value ?? "–"}
        {delta !== null && (
          <span className={cn("flex items-center gap-1 text-xs", delta > 0 ? "text-positive" : delta < 0 ? "text-accent-error" : "text-ink-soft")}>
            {delta < 0 ? <TrendingDown aria-hidden className="size-3" /> : <TrendingUp aria-hidden className="size-3" />}
            {delta > 0 ? `+${delta}` : delta < 0 ? `−${-delta}` : "0"}
          </span>
        )}
      </p>
      {note && <p className="text-xs text-ink-soft">{note}</p>}
    </div>
  );
}

/** A bar a day; a day nothing was collected on is an empty slot. */
function Bars({ values, label }: { values: (number | null)[]; label?: string }) {
  const max = Math.max(1, ...values.map((v) => v ?? 0));
  return (
    <figure className="flex flex-col gap-1">
      <div aria-hidden className="flex h-24 items-end gap-0.5">
        {values.map((v, i) => (
          <span key={i} className="flex h-full flex-1 flex-col justify-end">
            {v === null ? <span className="h-px bg-border-subtle" /> : <span className="rounded-sm bg-accent-info" style={{ height: `${(v / max) * 100}%`, minHeight: 2 }} title={String(v)} />}
          </span>
        ))}
      </div>
      {label && <figcaption className="text-xs text-ink-soft">{label}</figcaption>}
    </figure>
  );
}

type Change = { at: string; key: string; title: string };

/** What changed on it: the conductor's edits, syncs and state moves with the archive's events, newest first. */
function Changes({ gmb, since }: { gmb: number; since: number }) {
  const t = useT();
  const locale = useLocale();
  const ours = useResource(api, "/gmbs/{id}/log", { params: { path: { id: gmb } } });
  const theirs = useResource(reviewArchive, "/me/gmbs/{gmb}/events", { params: { path: { gmb } } });
  // a GBP the archive was never told of (a draft, one with no place) has no events there
  const archiveKnows = !(theirs.status === "error" && theirs.failure.code === "not_found");
  if (ours.status === "loading" || (archiveKnows && theirs.status === "loading")) return <Skeleton className="h-64" />;
  if (ours.status === "error") return <ErrorState failure={ours.failure} onRetry={ours.reload} />;
  if (archiveKnows && theirs.status === "error") return <ErrorState failure={theirs.failure} onRetry={theirs.reload} />;
  const theirTitle = (e: Exclude<GmbEvent, { kind: "scan_failed" }>) =>
    ({
      appeared: t(windowKeys.log.appeared, "A review on Google"),
      suspended: t(windowKeys.log.suspended, "A 5-star review suspended"),
      removed: t(windowKeys.log.reviewRemoved, "A review removed"),
      returned: t(windowKeys.log.returned, "A review back on Google"),
      reinstatement_set: t(windowKeys.log.reinstatementSet, "Reinstatement asked of Google"),
      reinstatement_cleared: t(windowKeys.log.reinstatementCleared, "Reinstatement withdrawn"),
    })[e.kind];
  const lines: Change[] = [
    ...(ours.data as LogDto).events.flatMap((e): Change[] => {
      switch (e.kind) {
        case "state":
          return [{ at: e.at, key: `c${e.id}`, title: `${STATE_NAME[e.from](t)} → ${STATE_NAME[e.to](t)}` }];
        case "edited":
          return [{ at: e.at, key: `c${e.id}`, title: t(windowKeys.stats.edited, "Edited: {n} fields", { n: e.changes.length }) }];
        case "synced":
          return [{ at: e.at, key: `c${e.id}`, title: e.changes.length === 0 ? t(windowKeys.log.syncedSame, "Sync: nothing changed on Google") : t(windowKeys.log.synced, "Sync: {n} fields changed on Google", { n: e.changes.length }) }];
        default:
          return [];
      }
    }),
    ...(theirs.status === "ok" ? theirs.data.events : []).map((e, i): Change => ({ at: e.at, key: `a${e.at}${i}${e.kind}`, title: e.kind === "scan_failed" ? t(windowKeys.log.scanFailed, "A scan of its reviews failed") : theirTitle(e) })),
  ]
    .filter((l) => Date.parse(l.at) >= since)
    .sort((a, b) => Date.parse(b.at) - Date.parse(a.at));
  return (
    <section className="flex flex-col gap-2 rounded-panel border border-border p-3">
      <h3 className="text-sm font-semibold text-ink">{t(windowKeys.stats.changes, "Changes")}</h3>
      {lines.length === 0 && <p className="text-sm text-ink-soft">{t(windowKeys.stats.none, "Nothing changed in this period.")}</p>}
      <ol className="flex flex-col gap-2">
        {lines.map((l) => (
          <li key={l.key} className="flex items-baseline gap-2 text-sm">
            <span className="min-w-0 flex-1 text-ink">{l.title}</span>
            <time dateTime={l.at} className="shrink-0 text-xs text-ink-soft">
              {ago(l.at, locale)}
            </time>
          </li>
        ))}
      </ol>
    </section>
  );
}
