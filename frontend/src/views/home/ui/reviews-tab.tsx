"use client";

import { useResource } from "@evinvest/api/react";
import { Button, Input, Planned, Skeleton, cn, toast } from "@evinvest/uikit";
import { ChevronRight, ExternalLink, RefreshCw, Search } from "lucide-react";
import { useState } from "react";

import { LiveReviews, type Listed, useGmbTags, windowKeys } from "@/entities/gmb";
import { CaseLinks, over, useGmbMail } from "@/entities/mail";
import { useSpendConfirm } from "@/entities/tokens";
import { ok, reviewArchive } from "@/shared/api";
import type { MailCase } from "@/shared/api/schema";
import type { Board, BoardCard, LocationSummary, RunDto } from "@/shared/api/services/review_archive";
import { useLocale, useT } from "@/shared/i18n";
import { ViewLink } from "@/shared/lib/location";
import { ago } from "@/shared/lib/time";
import { EmptyState } from "@/shared/ui/empty-state";
import { ErrorState } from "@/shared/ui/error-state";
import { notifyFailure } from "@/shared/ui/notify";

const SHOWN = 20;
const WEEK_MS = 7 * 24 * 3600 * 1000;

type Filter = "non_five" | "unresponded" | "new_7d";
const FILTERS: readonly Filter[] = ["non_five", "unresponded", "new_7d"];

/** Its reviews as review_archive tracks them (02c), and what Google says of those asked back. */
export function ReviewsTab({ listed }: { listed: Listed }) {
  const mail = useGmbMail(listed.gmb.id);
  const cases = mail.status === "ok" ? mail.data.cases.filter((c) => c.topic === "reviews") : [];
  return (
    <>
      {mail.status === "error" && (
        <div className="px-5 pt-5">
          <ErrorState failure={mail.failure} onRetry={mail.reload} />
        </div>
      )}
      <Reinstatement cases={cases.filter((c) => c.state.is !== "quiet")} />
      <Archived listed={listed} asked={cases.find((c) => !over(c)) ?? cases[0]} />
    </>
  );
}

function Reinstatement({ cases }: { cases: readonly MailCase[] }) {
  const t = useT();
  if (cases.length === 0) return null;
  return (
    <section className="mx-5 mt-5 flex flex-col gap-1 rounded-lg border border-border px-4 py-2">
      <h3 className="text-xs text-ink-soft">{t(windowKeys.reviews.reinstatement, "With Google support")}</h3>
      <CaseLinks cases={cases} />
    </section>
  );
}

/** `asked`: the case its gone reviews are asked back in. */
function Archived({ listed, asked }: { listed: Listed; asked: MailCase | undefined }) {
  const t = useT();
  const overview = useResource(reviewArchive, "/me/overview");
  if (overview.status === "loading") return <Skeleton className="m-4 h-96" />;
  if (overview.status === "error")
    return (
      <div className="p-4">
        <ErrorState failure={overview.failure} onRetry={overview.reload} />
      </div>
    );
  const summary = overview.data.flatMap((g) => g.gmbs).find((g) => g.id === listed.gmb.id);
  // a GBP the archive was never told of (a draft, one with no place) has no track
  if (summary === undefined)
    return (
      <div className="p-6">
        <EmptyState title={t(windowKeys.reviews.unknown, "The archive does not track this GBP")} description={t(windowKeys.reviews.unknownWhy, "Only a verified GBP on Maps has its reviews archived.")} />
      </div>
    );
  return <Tracked gmb={listed.gmb.id} track={summary.track} reloadTrack={overview.reload} asked={asked} />;
}

function Tracked({ gmb, track, reloadTrack, asked }: { gmb: number; track: LocationSummary; reloadTrack: () => void; asked: MailCase | undefined }) {
  const t = useT();
  const locale = useLocale();
  const path = { gmb, target: track.target.id };
  const board = useResource(reviewArchive, "/me/gmbs/{gmb}/tracks/{target}/board", { params: { path } });
  const runs = useResource(reviewArchive, "/me/gmbs/{gmb}/tracks/{target}/runs", { params: { path, query: { limit: 30 } } });
  const spend = useSpendConfirm();
  const [query, setQuery] = useState("");
  const [on, setOn] = useState<ReadonlySet<Filter>>(new Set());
  const [all, setAll] = useState(false);
  const [scanning, setScanning] = useState(false);
  const [now] = useState(() => Date.now());
  if (board.status === "loading") return <Skeleton className="m-4 h-96" />;
  if (board.status === "error")
    return (
      <div className="p-4">
        <ErrorState failure={board.failure} onRetry={board.reload} />
      </div>
    );
  const b: Board = board.data;
  const everything = [...b.snapshotted, ...b.suspended, ...b.reinstating, ...b.removed];
  const test: Record<Filter, (c: BoardCard) => boolean> = {
    non_five: (c) => c.review.rating === null || c.review.rating < 5,
    unresponded: (c) => c.review.reply === null,
    new_7d: (c) => c.review.published_est !== null && now - Date.parse(c.review.published_est) <= WEEK_MS,
  };
  const needle = query.trim().toLowerCase();
  const keep = (c: BoardCard) => [...on].every((f) => test[f](c)) && (needle === "" || `${c.review.author} ${c.review.text ?? ""}`.toLowerCase().includes(needle));
  const [pending, answered] = [b.snapshotted.filter((c) => keep(c) && c.review.reply === null), b.snapshotted.filter((c) => keep(c) && c.review.reply !== null)];
  const filterName = {
    non_five: t(windowKeys.reviews.nonFive, "Non-5★"),
    unresponded: t(windowKeys.reviews.unresponded, "Unresponded"),
    new_7d: t(windowKeys.reviews.new7d, "New 7d"),
  } satisfies Record<Filter, string>;
  const scan = () =>
    void spend.ask(track.scan_cost ?? undefined, async () => {
      setScanning(true);
      try {
        await ok(reviewArchive.POST("/me/gmbs/{gmb}/tracks/{target}/scan", { params: { path } }));
        toast(t(windowKeys.reviews.scanQueued, "Scan queued"));
      } catch (e) {
        notifyFailure(e, t);
      } finally {
        setScanning(false);
      }
      reloadTrack();
    });
  const scanBlocked = track.scan !== null ? t(windowKeys.reviews.scanBusy, "A scan is already under way") : track.scan_cost === null ? t(windowKeys.reviews.scanUnpriced, "No scan starts now: walks are unpriced, or the first screen is past the hour's limit") : undefined;
  const chip = (active: boolean) => cn("flex items-center gap-1.5 rounded-full border px-2.5 py-0.5 text-xs whitespace-nowrap outline-none focus-visible:ring-2 focus-visible:ring-ring", active ? "border-ink bg-ink text-background" : "border-border text-ink hover:bg-hover");
  const dot = track.last_run_status === "ok" ? "bg-positive" : track.last_run_status === "partial" ? "bg-accent-warn" : "bg-accent-error";
  return (
    <div className="flex flex-col gap-4 p-5">
      {spend.dialog}
      <div className="flex flex-wrap items-center gap-2">
        <span className="rounded-md border border-border px-2.5 py-1 text-xs text-ink" title={track.target.label}>
          {track.target.lang} · {track.target.kind === "maps" ? "Google Maps" : "Business Profile"}
        </span>
        <span className="relative">
          <Search aria-hidden className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-ink-soft" />
          <Input className="h-7 w-44 pl-8" value={query} onChange={(e) => setQuery(e.target.value)} placeholder={t(windowKeys.reviews.search, "Text or author")} aria-label={t(windowKeys.reviews.search, "Text or author")} />
        </span>
        {FILTERS.map((f) => (
          <button key={f} type="button" aria-pressed={on.has(f)} className={chip(on.has(f))} onClick={() => setOn(on.has(f) ? new Set([...on].filter((x) => x !== f)) : new Set([...on, f]))}>
            {filterName[f]} <span className="tabular-nums opacity-70">{everything.filter(test[f]).length}</span>
          </button>
        ))}
        <span className="flex-1" />
        {track.last_run_at !== null && track.last_run_status !== null && (
          <span className="flex items-center gap-1.5 text-xs text-ink-soft" title={track.last_run_error ?? undefined}>
            <span aria-hidden className={cn("size-1.5 rounded-full", dot)} />
            {t(windowKeys.reviews.lastRun, "last run {ago} · {status}", { ago: ago(track.last_run_at, locale), status: track.last_run_status })}
          </span>
        )}
        <Button size="sm" variant="outline" disabled={scanning} {...(scanBlocked !== undefined && { disabledReason: scanBlocked })} onClick={scan}>
          <RefreshCw aria-hidden className={cn(scanning && "animate-spin")} />
          {t(windowKeys.reviews.scan, "Scan now")}
          {track.scan_cost !== null && <span className="text-ink-soft">{t(windowKeys.reviews.scanCost, "· {n} tokens", { n: track.scan_cost })}</span>}
        </Button>
      </div>
      <div className="grid grid-cols-[repeat(2,minmax(15rem,1fr))_16rem] items-start gap-4 overflow-x-auto">
        <section className="flex min-w-0 flex-col gap-2">
          <ColumnHead tone="bg-positive" name={t(windowKeys.reviews.active, "Active")} n={b.snapshotted.length} note={track.listed !== null ? t(windowKeys.reviews.listed, "listed by Google {n}", { n: track.listed }) : undefined} />
          <Group tone="bg-accent-warn" name={t(windowKeys.reviews.notResponded, "Not responded")} n={pending.length}>
            <Button size="xs" variant="outline" planned={t(windowKeys.reviews.draftBoth, "Draft a reply to each unresponded review")}>
              {t(windowKeys.reviews.draftBothButton, "Draft both replies")}
            </Button>
          </Group>
          <List cards={pending} all={all} onAll={() => setAll(true)} gmb={gmb} reload={board.reload} track={reloadTrack} />
          <Group tone="bg-ink-faint" name={t(windowKeys.reviews.responded, "Responded")} n={answered.length} />
          <List cards={answered} compact all={all} onAll={() => setAll(true)} gmb={gmb} reload={board.reload} track={reloadTrack} />
        </section>
        <section className="flex min-w-0 flex-col gap-2">
          <ColumnHead tone="bg-accent-error" name={t(windowKeys.reviews.needsAction, "Suspended · Reinstating · To remove")} n={b.suspended.length + b.reinstating.length + b.removed.length} note={t(windowKeys.reviews.needsActionNote, "gone from Google, or flagged by us")} />
          <Button size="sm" variant="outline" planned={t(windowKeys.reviews.appealAll, "Send an appeal for every suspended review")}>
            {t(windowKeys.reviews.appealAllButton, "Appeal {n} suspended", { n: b.suspended.length })}
          </Button>
          <Group tone="bg-accent-error" name={t(windowKeys.reviews.suspended, "Suspended")} n={b.suspended.length}>
            {b.suspended.length > 0 && <Asked c={asked} />}
          </Group>
          <List cards={b.suspended.filter(keep)} all={all} onAll={() => setAll(true)} gmb={gmb} reload={board.reload} track={reloadTrack} />
          <Group tone="bg-accent-warn" name={t(windowKeys.reviews.reinstating, "Reinstating")} n={b.reinstating.length}>
            {b.reinstating.length > 0 && <Asked c={asked} />}
          </Group>
          <List cards={b.reinstating.filter(keep)} all={all} onAll={() => setAll(true)} gmb={gmb} reload={board.reload} track={reloadTrack} />
          <Group tone="bg-ink-faint" name={t(windowKeys.reviews.toRemove, "To remove")} n={b.removed.length} />
          <List cards={b.removed.filter(keep)} all={all} onAll={() => setAll(true)} gmb={gmb} reload={board.reload} track={reloadTrack} />
        </section>
        <Stats track={track} runs={runs.status === "ok" ? runs.data : null} gmb={gmb} />
      </div>
    </div>
  );
}

function ColumnHead({ tone, name, n, note }: { tone: string; name: string; n: number; note?: string | undefined }) {
  return (
    <div className="flex flex-col gap-0.5">
      <h3 className="flex items-center gap-2 text-sm font-semibold text-ink">
        <span aria-hidden className={cn("size-2 rounded-full", tone)} />
        {name}
        <span className="font-normal text-ink-soft tabular-nums">{n}</span>
      </h3>
      {note && <p className="text-xs text-ink-soft">{note}</p>}
    </div>
  );
}

function Group({ tone, name, n, children }: { tone: string; name: string; n: number; children?: React.ReactNode }) {
  return (
    <div className="flex items-center gap-2 pt-1 font-mono text-xs text-ink-soft uppercase">
      <span aria-hidden className={cn("size-1.5 rounded-full", tone)} />
      {name}
      <span className="tabular-nums">{n}</span>
      <span className="flex-1" />
      {children}
    </div>
  );
}

/** The way from reviews gone to their case with Google. */
function Asked({ c }: { c: MailCase | undefined }) {
  const t = useT();
  if (c === undefined) return null;
  return (
    <ViewLink to={{ view: "email", params: { case: c.id } }} className="flex items-center gap-1 rounded-sm font-sans text-xs text-ink-soft normal-case outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-ring">
      {t(windowKeys.reviews.reinstatement, "With Google support")}
      <ChevronRight aria-hidden className="size-3" />
    </ViewLink>
  );
}

function List({ cards, compact = false, all, onAll, gmb, reload, track }: { cards: BoardCard[]; compact?: boolean; all: boolean; onAll: () => void; gmb: number; reload: () => void; track: () => void }) {
  const t = useT();
  const shown = all ? cards : cards.slice(0, SHOWN);
  return (
    <>
      {shown.map((c) => (
        <Card key={c.review.id} card={c} compact={compact} gmb={gmb} reload={reload} track={track} />
      ))}
      {shown.length < cards.length && (
        <button type="button" onClick={onAll} className="self-start text-xs text-ink-soft hover:text-ink">
          {t(windowKeys.reviews.more, "+ {n} more", { n: cards.length - shown.length })}
        </button>
      )}
    </>
  );
}

/** A review: by its state a card with what can be done to it, or a line. */
function Card({ card: { review: r, reinstatement }, compact, gmb, reload, track }: { card: BoardCard; compact: boolean; gmb: number; reload: () => void; track: () => void }) {
  const t = useT();
  const locale = useLocale();
  const [busy, setBusy] = useState(false);
  const age = r.published_est !== null ? ago(r.published_est, locale) : r.published_raw;
  const appeal = async () => {
    setBusy(true);
    try {
      await ok(reviewArchive.PUT("/me/gmbs/{gmb}/reinstatements/{review}", { params: { path: { gmb, review: r.id } } }));
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
    }
    reload();
    track();
  };
  const stars = r.rating === null ? t(windowKeys.reviews.unrated, "no stars") : `★${r.rating}`;
  if (compact)
    return (
      <p className="flex items-baseline gap-2 border-b border-border-subtle py-1 text-xs">
        <span className="shrink-0 text-ink">{r.author}</span>
        <span className="shrink-0 text-ink-soft tabular-nums">{stars}</span>
        <span className="min-w-0 flex-1 truncate text-ink-soft">{r.text}</span>
        <span className="shrink-0 text-ink-faint">{age}</span>
      </p>
    );
  const gone = r.gone_at !== null;
  return (
    <article className="flex flex-col gap-1.5 rounded-lg border border-border p-3">
      <header className="flex items-baseline gap-2 text-sm">
        {r.author_url !== null ? (
          <a href={r.author_url} target="_blank" rel="noreferrer" className="min-w-0 truncate font-medium text-ink hover:underline">
            {r.author}
          </a>
        ) : (
          <span className="min-w-0 truncate font-medium text-ink">{r.author}</span>
        )}
        <span className={cn("tabular-nums", r.rating === 5 ? "text-ink" : "text-accent-warn")}>{stars}</span>
        <span className="flex-1" />
        <span className="shrink-0 text-xs text-ink-soft">{age}</span>
      </header>
      {r.text !== null && <p className="line-clamp-4 text-sm text-ink">{r.text}</p>}
      {r.reply !== null && (
        <p className="line-clamp-3 border-l-2 border-border pl-2 text-xs text-ink-soft">
          <span className="text-ink">{t(windowKeys.reviews.reply, "Owner")}: </span>
          {r.reply}
        </p>
      )}
      <footer className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-ink-soft">
        {r.capture_url !== null ? (
          <a href={`/api/review_archive${r.capture_url}`} target="_blank" rel="noreferrer" className="flex items-center gap-1 hover:text-ink">
            {t(windowKeys.reviews.snapshot, "Snapshot")}
            <ExternalLink aria-hidden className="size-3" />
          </a>
        ) : (
          r.capture_pending && <span>{t(windowKeys.reviews.snapshotPending, "snapshot pending")}</span>
        )}
        {gone && reinstatement === null && r.gone_at !== null && <span>{t(windowKeys.reviews.goneAt, "gone {ago}", { ago: ago(r.gone_at, locale) })}</span>}
        {reinstatement !== null && <span>{t(windowKeys.reviews.appealSent, "appeal sent {ago}", { ago: ago(reinstatement.requested_at, locale) })}</span>}
        <span className="flex-1" />
        {!gone && r.reply === null && (
          <Button size="xs" variant="ghost" planned={t(windowKeys.reviews.draft, "Draft a reply with an agent")}>
            {t(windowKeys.reviews.draftReply, "Draft reply")}
          </Button>
        )}
        {gone && reinstatement === null && r.rating === 5 && (
          <>
            <Button size="xs" variant="ghost" planned={t(windowKeys.reviews.appealPlanned, "Send the appeal to Google from here")}>
              {t(windowKeys.reviews.appealButton, "Appeal")}
            </Button>
            <Button size="xs" variant="outline" disabled={busy} onClick={() => void appeal()}>
              {t(windowKeys.reviews.appealDone, "Appeal sent")}
            </Button>
          </>
        )}
        {gone && reinstatement === null && r.rating !== 5 && (
          <Button size="xs" variant="ghost" planned={t(windowKeys.reviews.removalPlanned, "Ask Google to remove the review")}>
            {t(windowKeys.reviews.removal, "Request removal")}
          </Button>
        )}
      </footer>
    </article>
  );
}

function Stats({ track, runs, gmb }: { track: LocationSummary; runs: RunDto[] | null; gmb: number }) {
  const t = useT();
  const locale = useLocale();
  const { tags } = useGmbTags();
  const archive = tags.archive?.get(gmb);
  const tiles: [string, number][] = [
    [t(windowKeys.reviews.snapshotted, "Snapshotted"), track.live],
    [t(windowKeys.reviews.suspended, "Suspended"), track.suspended],
    [t(windowKeys.reviews.reinstating, "Reinstating"), track.reinstating],
    [t(windowKeys.reviews.removed, "Removed"), track.removed],
  ];
  const board = `/grafana/a/evmain-tilers-app/d/ra-location?var-gmb=${gmb}&var-target=${track.target.id}&var-api=${encodeURIComponent(`${window.location.origin}/api/review_archive/`)}`;
  const tone = { ok: "bg-positive", partial: "bg-accent-warn", failed: "bg-accent-error" };
  return (
    <aside className="flex min-w-0 flex-col gap-3">
      <h3 className="flex items-baseline gap-2 text-sm font-semibold text-ink">
        {t(windowKeys.reviews.stats, "Stats")}
        <span className="text-xs font-normal text-ink-soft">{t(windowKeys.reviews.statsPeriod, "30 d")}</span>
        <span className="flex-1" />
        <a href={board} target="_blank" rel="noreferrer" className="flex items-center gap-1 text-xs font-normal text-ink-soft hover:text-ink">
          Grafana
          <ExternalLink aria-hidden className="size-3" />
        </a>
      </h3>
      <div className="grid grid-cols-2 gap-2">
        {tiles.map(([name, n]) => (
          <div key={name} className="rounded-lg border border-border px-3 py-2">
            <p className="text-xs text-ink-soft">{name}</p>
            <p className="text-xl text-ink tabular-nums">{n}</p>
          </div>
        ))}
      </div>
      <div className="flex flex-col gap-1.5 rounded-lg border border-border p-3">
        <p className="text-xs text-ink-soft">{t(windowKeys.reviews.liveTitle, "Live reviews")}</p>
        {archive && <LiveReviews days={archive.live_30d} />}
      </div>
      <Planned what={t(windowKeys.reviews.perDay, "New reviews per day")}>
        <div aria-hidden className="flex h-16 items-end gap-0.5">
          {[15, 4, 26, 15, 4, 4, 37, 15, 26, 4, 15, 48, 26, 15, 4, 15, 26, 37, 15, 4, 26, 15, 4, 15, 59, 26, 15, 4, 15, 26].map((h, i) => (
            <span key={i} className="w-1 rounded-sm bg-accent-info" style={{ height: h }} />
          ))}
        </div>
      </Planned>
      <div className="flex flex-col gap-1.5 rounded-lg border border-border p-3">
        <p className="text-xs text-ink-soft">{t(windowKeys.reviews.runs, "Runs")}</p>
        {runs !== null && runs.length > 0 && (
          <div className="flex gap-0.5">
            {[...runs].reverse().map((r) => (
              <span
                key={r.id}
                className={cn("h-2.5 flex-1 rounded-sm", r.status === null ? "bg-hover" : tone[r.status])}
                title={t(windowKeys.reviews.run, "{when} · {seen} seen, {new} new, {gone} gone", { when: ago(r.started_at, locale), seen: r.seen, new: r.new, gone: r.gone })}
              />
            ))}
          </div>
        )}
      </div>
      <Planned what={t(windowKeys.reviews.shown, "The rating Google shows, over time")}>
        <p className="flex justify-between text-xs text-ink">
          {t(windowKeys.reviews.shown, "The rating Google shows, over time")}
          <span className="tabular-nums">4.9 → 4.9</span>
        </p>
        <svg aria-hidden viewBox="0 0 270 24" className="h-6 w-full text-accent-info">
          <polyline fill="none" stroke="currentColor" strokeWidth="1.5" points="0,12 40,10 80,12 120,8 160,9 200,6 270,8" />
        </svg>
      </Planned>
    </aside>
  );
}
