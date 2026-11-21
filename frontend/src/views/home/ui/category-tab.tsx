"use client";

import { useResource } from "@evinvest/api/react";
import { Button, Input, NativeSelect, NativeSelectOption, Slider, Tabs, TabsList, TabsTrigger, Textarea, cn } from "@evinvest/uikit";
import { History as HistoryIcon, LayoutGrid, Lock, Map as MapIcon, Plus, Settings2, Upload, X } from "lucide-react";
import { type ReactNode, useState } from "react";

import { BrandMark, HeadBadges, PublishDialog, STATE_NAME, VersionDiff, VersionRead, adoptRelease, categoryName, googleName, kit, normalized, openWindow, revertCategory, select, trades as tradesKeys, useBrands, useCategories } from "@/entities/gmb";
import { setMapView } from "@/features/placement-overlay";
import { Refused, api, ok } from "@/shared/api";
import { type Category, type CategoryMove, type Demand, type Excluded, type Gmb, type Keyword, type State, type Why, scaleValues, stateValues } from "@/shared/api/schema";
import { type T, state, useLocale, useT } from "@/shared/i18n";
import { formatDateTime } from "@/shared/lib/format";
import { failureText } from "@/shared/ui/failure-text";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize, useControlSize } from "@/shared/ui/touch";
import { KitPool } from "@/views/kit";

import { GoogleCategories, type Tier } from "./google-categories";
import { useSplit } from "./split";

const MAX_KEYWORDS = 50;
const SECTION = "flex flex-col gap-2";
const SECTION_HEAD = "flex items-baseline gap-2 text-xs font-semibold tracking-wide text-ink-soft uppercase";
const HINT = "font-normal tracking-normal normal-case";
/** STATE_TONE's, as a fill. */
const FILL: Record<State, string> = { verified: "bg-positive", verifying: "bg-accent-warn", suspended: "bg-accent-error", draft: "bg-ink-soft" };

const WHY_NAME = {
  edit: (t) => t(tradesKeys.history.edit, "Edited"),
  adopt: (t) => t(tradesKeys.history.adopt, "Adopted"),
  update: (t) => t(tradesKeys.history.update, "Updated"),
  revert: (t) => t(tradesKeys.history.revert, "Reverted"),
} satisfies Record<Why, (t: T) => string>;

/** What the category's studies read, as staged: its additional Google categories' weights possibly not given yet. */
type Draft = { additional: Tier[]; keywords: Keyword[]; excluded: Excluded; demand: Demand | null };
type Tile = "overview" | "definition" | "history" | "kit";

const draftOf = (c: Category): Draft => ({ additional: c.head.additional, keywords: c.head.keywords, excluded: c.head.excluded, demand: c.head.demand });

/** A set-up category: what holds it now, and what Google and its studies read; its history and its review kit's drafts beside. A page wide enough splits Overview beside the rest, as a GBP's window does. */
export function CategoryTab({ category, here, page }: { category: Category; here: readonly Gmb[]; page: boolean }) {
  const t = useT();
  const locale = useLocale();
  const [setBox, wide] = useSplit(page);
  const [tile, setTile] = useState<Tile>("overview");
  const [draft, setDraft] = useState<Draft | null>(null);
  const brands = useBrands();
  const areas = useAreasOf(category, here);
  const path = { params: { path: { trade: category.trade.id, country: category.country } } };
  const pool = useResource(api, "/categories/{trade}/{country}/kit", path);
  const history = useMoves(category);
  const base = draftOf(category);
  const staged = draft === null ? 0 : unapplied(base, draft);
  const nBrands = brands.status === "ok" ? brands.data.filter((b) => b.trade === category.trade.id && b.country === category.country).length : null;
  const label = {
    overview: (
      <>
        <LayoutGrid aria-hidden />
        {t(tradesKeys.overview, "Overview")}
        {nBrands !== null && areas !== null && <span className="font-mono text-xs text-ink-soft tabular-nums">{t(tradesKeys.overview.counts, "{brands} brands · {areas} areas", { brands: nBrands, areas: areas.length })}</span>}
      </>
    ),
    definition: (
      <>
        <Settings2 aria-hidden />
        {t(tradesKeys.definition, "Definition")}
        {staged > 0 && <span className="font-mono text-xs text-accent-warn tabular-nums">{t(tradesKeys.definition.unapplied, "{n} unapplied", { n: staged })}</span>}
      </>
    ),
    history: (
      <>
        <HistoryIcon aria-hidden />
        {history.status === "ok" ? t(tradesKeys.history.counted, "History {n}", { n: history.data.moves.length }) : t(tradesKeys.history, "History")}
      </>
    ),
    kit: pool.status === "ok" ? t(kit.pane.draftsCounted, "Drafts {drafts}", { drafts: pool.data.drafts.length }) : t(kit.pane.drafts, "Drafts"),
  } satisfies Record<Tile, ReactNode>;
  const body = (shown: Tile) => {
    switch (shown) {
      case "overview":
        return <Overview category={category} here={here} areas={areas} />;
      case "definition":
        return <Definition key={`${category.trade.id}.${category.country}`} category={category} base={base} draft={draft ?? base} staged={staged} setDraft={setDraft} />;
      case "history":
        return <History category={category} moves={history} />;
      case "kit":
        return (
          <div className="p-6">
            <KitPool trade={category.trade.id} country={category.country} />
          </div>
        );
    }
  };
  const strip = (tiles: readonly Tile[], value: Tile) => (
    <div className="flex h-10 shrink-0 overflow-x-auto border-b border-border">
      <Tabs value={value} onValueChange={(v) => setTile(v as Tile)} className="min-w-0 flex-1 justify-center">
        <TabsList aria-label={t(tradesKeys.tiles, "Sections")} className="max-w-full overflow-x-auto">
          {tiles.map((x) => (
            <TabsTrigger key={x} value={x} className="flex-none">
              {label[x]}
            </TabsTrigger>
          ))}
        </TabsList>
      </Tabs>
    </div>
  );
  const right: Tile = tile === "overview" ? "definition" : tile;
  return (
    <div ref={setBox} className="flex min-h-0 flex-1 flex-col">
      <Head category={category} onHistory={() => setTile("history")} />
      {wide ? (
        <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,36rem)_minmax(0,1fr)] divide-x divide-border">
          <section className="flex min-h-0 flex-col overflow-y-auto">
            <h3 className="flex h-10 shrink-0 items-center gap-2 border-b border-border px-4 text-sm font-semibold text-ink [&_svg]:size-4">
              {label.overview}
              <span className="flex-1" />
              <span className="text-xs font-normal text-ink-soft">{t(tradesKeys.overview.what, "what holds {category} now", { category: categoryName(category.trade, category.country, locale) })}</span>
            </h3>
            {body("overview")}
          </section>
          <section className="flex min-h-0 flex-col overflow-y-auto">
            {strip(["definition", "history", "kit"], right)}
            {body(right)}
          </section>
        </div>
      ) : (
        <div className="flex min-h-0 flex-1 flex-col">
          {strip(["overview", "definition", "history", "kit"], tile)}
          {body(tile)}
        </div>
      )}
    </div>
  );
}

/** Where its head stands, and Publish…; a newer release of the listing it came from, offered after its diff and never taken unasked. */
function Head({ category, onHistory }: { category: Category; onHistory: () => void }) {
  const t = useT();
  const locale = useLocale();
  const button = useButtonSize();
  const categories = useCategories();
  const [publishing, setPublishing] = useState(false);
  const [reviewing, setReviewing] = useState(false);
  const { lineage } = category;
  const upstream = lineage?.upstream ?? null;
  const update = async () => {
    if (upstream === null) throw new Error("an update with no newer release");
    try {
      await adoptRelease(category.trade.id, category.country, upstream);
      setReviewing(false);
      categories.reload();
    } catch (e) {
      notifyFailure(e, t);
    }
  };
  return (
    <div className="flex flex-col gap-2 border-b border-border px-6 py-2">
      <span className="flex flex-wrap items-center gap-2 text-sm">
        <span className="text-ink-soft">{t(tradesKeys.head, "Head")}</span>
        <button type="button" onClick={onHistory} className="rounded font-mono text-xs text-ink-soft outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-ring">
          {t(tradesKeys.head.at, "v{id} · {at}", { id: category.head.id, at: formatDateTime(category.head.at, locale) })}
        </button>
        <HeadBadges category={category} />
        {lineage !== null && (
          <button type="button" onClick={() => openWindow({ kind: "library", listing: lineage.listing })} className="rounded text-xs text-ink-soft underline-offset-2 outline-none hover:text-ink hover:underline focus-visible:ring-2 focus-visible:ring-ring">
            {t(tradesKeys.head.listingOpen, "its listing ↗")}
          </button>
        )}
        <span className="flex-1" />
        <Button variant="outline" size={button("sm")} onClick={() => setPublishing(true)}>
          <Upload aria-hidden />
          {t(tradesKeys.release.open, "Publish…")}
        </Button>
      </span>
      {upstream !== null && lineage !== null && (
        <div className="flex flex-col gap-2 rounded-lg border border-accent-warn/40 bg-accent-warn/5 px-3 py-2">
          <span className="flex flex-wrap items-center gap-2 text-sm text-ink">
            <span className="min-w-0 flex-1">
              {lineage.edited
                ? t(tradesKeys.upstream.edited, "{listing} v{n} is out; yours is v{from}, edited since.", { listing: lineage.title, n: upstream.n, from: lineage.from.n })
                : t(tradesKeys.upstream.what, "{listing} v{n} is out; yours is v{from}.", { listing: lineage.title, n: upstream.n, from: lineage.from.n })}
            </span>
            <Button variant="outline" size={button("sm")} aria-pressed={reviewing} onClick={() => setReviewing(!reviewing)}>
              {t(tradesKeys.upstream.review, "Review diff")}
            </Button>
            {reviewing && (
              <Button size={button("sm")} onClick={() => void update()}>
                {t(tradesKeys.upstream.update, "Update to v{n}", { n: upstream.n })}
              </Button>
            )}
          </span>
          {upstream.notes !== "" && <p className="text-xs text-ink-soft">{upstream.notes}</p>}
          {reviewing && <VersionRead id={upstream.version}>{(v) => <VersionDiff from={category.head} to={v} />}</VersionRead>}
        </div>
      )}
      {lineage?.withdrawn === true && <p className="text-xs text-accent-warn">{t(tradesKeys.head.withdrawnWhat, "{listing} is withdrawn: no newer version will come of it.", { listing: lineage.title })}</p>}
      {publishing && <PublishDialog category={category} fork={null} onClose={() => setPublishing(false)} />}
    </div>
  );
}

/** Every move of its head, newest first; one off the head goes back with Revert, after its diff. */
function History({ category, moves }: { category: Category; moves: ReturnType<typeof useMoves> }) {
  const t = useT();
  const locale = useLocale();
  const button = useButtonSize();
  const categories = useCategories();
  const [open, setOpen] = useState<number | null>(null);
  if (moves.status === "loading") return <p className="p-6 text-sm text-ink-soft">{t(state.loading, "Loading…")}</p>;
  if (moves.status === "error") return <p className="p-6 text-sm text-accent-error">{failureText(moves.failure, t)}</p>;
  const revert = async (m: CategoryMove) => {
    try {
      await revertCategory(category.trade.id, category.country, m.to);
      setOpen(null);
      categories.reload();
      moves.reload();
    } catch (e) {
      notifyFailure(e, t);
    }
  };
  return (
    <ol className="flex flex-col divide-y divide-border p-6">
      {moves.data.moves.toSorted((a, b) => b.at.localeCompare(a.at)).map((m) => {
        const head = m.to === category.head.id;
        return (
          <li key={m.id} className="flex flex-col gap-2 py-2">
            <span className="flex items-center gap-3 text-sm">
              <span className="w-24 shrink-0 font-medium text-ink">{WHY_NAME[m.why](t)}</span>
              <span className="font-mono text-xs text-ink-soft tabular-nums">{m.from === null ? `v${m.to}` : `v${m.from} → v${m.to}`}</span>
              {m.listing_version !== null && <span className="text-xs text-ink-soft">{t(tradesKeys.history.library, "off the library")}</span>}
              <span className="flex-1" />
              <span className="text-xs text-ink-soft">{formatDateTime(m.at, locale)}</span>
              {head ? (
                <span className="w-20 text-right text-xs text-ink-soft">{t(tradesKeys.history.head, "the head")}</span>
              ) : (
                <Button variant="ghost" size={button("sm")} className="w-20" aria-pressed={open === m.id} onClick={() => setOpen(open === m.id ? null : m.id)}>
                  {t(tradesKeys.history.diff, "Diff")}
                </Button>
              )}
            </span>
            {open === m.id && (
              <div className="flex flex-col gap-2 pl-24">
                <VersionRead id={m.to}>{(v) => <VersionDiff from={category.head} to={v} />}</VersionRead>
                <Button size={button("sm")} className="self-start" onClick={() => void revert(m)}>
                  {t(tradesKeys.history.revertTo, "Revert to v{id}", { id: m.to })}
                </Button>
              </div>
            )}
          </li>
        );
      })}
    </ol>
  );
}

const useMoves = (c: Category) => useResource(api, "/categories/{trade}/{country}/history", { params: { path: { trade: c.trade.id, country: c.country } } });

/** The areas the category's GBPs stand in, most held first, each with how many. */
function useAreasOf(category: Category, here: readonly Gmb[]): { slug: string; name: string; gbps: number }[] | null {
  const read = useResource(api, "/areas");
  if (read.status !== "ok") return null;
  return read.data.areas
    .filter((a) => a.country === category.country)
    .map((a) => ({ slug: a.slug, name: a.name, gbps: here.filter((b) => b.location !== null && metres(b.location, a.center) <= a.radius_m).length }))
    .filter((a) => a.gbps > 0)
    .toSorted((a, b) => b.gbps - a.gbps);
}

/** Great-circle distance, haversine. */
function metres(a: { lat: number; lng: number }, b: { lat: number; lng: number }): number {
  const rad = Math.PI / 180;
  const h = Math.sin(((b.lat - a.lat) * rad) / 2) ** 2 + Math.cos(a.lat * rad) * Math.cos(b.lat * rad) * Math.sin(((b.lng - a.lng) * rad) / 2) ** 2;
  return 2 * 6_371_000 * Math.asin(Math.sqrt(h));
}

function Overview({ category, here, areas }: { category: Category; here: readonly Gmb[]; areas: { slug: string; name: string; gbps: number }[] | null }) {
  const t = useT();
  const brands = useBrands();
  const mine = brands.status === "ok" ? brands.data.filter((b) => b.trade === category.trade.id && b.country === category.country) : null;
  const brandless = here.filter((b) => b.brand_id === null);
  const byState = stateValues.map((s) => ({ state: s, gbps: here.filter((b) => b.state === s) })).filter((x) => x.gbps.length > 0);
  const row = "flex w-full items-center gap-3 border-t border-border py-2.5 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring";
  return (
    <div className="flex flex-col gap-6 p-6">
      <section className={SECTION}>
        <p className="flex items-baseline gap-2">
          <span className="text-3xl font-semibold text-ink tabular-nums">{here.length}</span>
          <span className="text-sm text-ink-soft">{t(tradesKeys.overview.gbps, "GBPs")}</span>
        </p>
        {here.length > 0 && (
          <>
            <span className="flex h-1.5 gap-0.5 overflow-hidden rounded-full">
              {byState.map(({ state, gbps }) => (
                <span key={state} className={FILL[state]} style={{ flexGrow: gbps.length }} />
              ))}
            </span>
            <span className="flex flex-wrap items-center gap-1.5">
              {byState.map(({ state, gbps }) => (
                <button
                  key={state}
                  type="button"
                  onClick={() => select(gbps.map((b) => b.id))}
                  className="flex items-center gap-1.5 rounded-full border border-border px-2 py-0.5 text-xs text-ink outline-none hover:bg-hover focus-visible:ring-2 focus-visible:ring-ring"
                >
                  <span className={cn("size-1.5 rounded-full", FILL[state])} />
                  {STATE_NAME[state](t)} <span className="text-ink-soft tabular-nums">{gbps.length}</span>
                </button>
              ))}
              <span className="flex-1" />
              <span className="text-xs text-ink-soft">{t(tradesKeys.overview.chip, "a chip scopes the outline")}</span>
            </span>
          </>
        )}
      </section>

      <section className="flex flex-col">
        <h3 className={cn(SECTION_HEAD, "pb-2")}>
          {t(tradesKeys.overview.brands, "Brands")}
          <span className="flex-1" />
          <span className={HINT}>{t(tradesKeys.overview.brands.what, "name, logo and site its GBPs show")}</span>
        </h3>
        {mine !== null && mine.length === 0 && brandless.length === 0 && <p className="text-sm text-ink-soft">{t(tradesKeys.overview.brands.none, "No brand yet: a GBP's Profile names one.")}</p>}
        {(mine ?? []).map((b) => (
          <button key={b.id} type="button" onClick={() => openWindow({ kind: "brand", brand: b.id })} className={cn(row, "hover:bg-hover/40")}>
            <BrandMark brand={b} className="size-8" />
            <span className="flex min-w-0 flex-1 flex-col">
              <span className="truncate text-sm font-medium text-ink">{b.name}</span>
              <span className="truncate font-mono text-xs text-ink-soft">{b.site}</span>
            </span>
            <span className="text-sm text-ink-soft tabular-nums">{t(tradesKeys.overview.brands.counts, "{gbps} GBPs · {photos} photos", { gbps: b.gmbs, photos: b.photos })}</span>
          </button>
        ))}
        {brandless.length > 0 && (
          <button type="button" onClick={() => select(brandless.map((b) => b.id))} className={cn(row, "hover:bg-accent-warn/5")}>
            <span className="size-8 shrink-0 rounded-md border border-dashed border-border" />
            <span className="flex min-w-0 flex-1 flex-col">
              <span className="text-sm font-medium text-accent-warn">{t(tradesKeys.overview.brands.brandless, "No brand")}</span>
              <span className="text-xs text-ink-soft">{t(tradesKeys.overview.brands.brandless.what, "their profiles carry no logo nor site")}</span>
            </span>
            <span className="text-sm text-accent-warn tabular-nums">{t(tradesKeys.overview.brands.brandless.n, "{n} GBPs · give them one", { n: brandless.length })}</span>
          </button>
        )}
      </section>

      <section className="flex flex-col">
        <h3 className={cn(SECTION_HEAD, "pb-2")}>
          {t(tradesKeys.overview.areas, "Areas")}
          <span className="flex-1" />
          <span className={HINT}>{t(tradesKeys.overview.areas.what, "where its GBPs are · the map opens on its study")}</span>
        </h3>
        {areas !== null && areas.length === 0 && <p className="text-sm text-ink-soft">{t(tradesKeys.overview.areas.none, "None of its GBPs stands in an area yet: draw one on the map.")}</p>}
        {(areas ?? []).map((a) => (
          <div key={a.slug} className="flex items-center gap-3 border-t border-border py-2">
            <span className="min-w-0 flex-1 truncate text-sm text-ink">{a.name}</span>
            <span className="w-20 text-sm text-ink-soft tabular-nums">{t(tradesKeys.overview.areas.gbps, "{n} GBPs", { n: a.gbps })}</span>
            <Button variant="ghost" size="sm" onClick={() => setMapView({ area: a.slug, study: { kind: "head", trade: category.trade.id, country: category.country } })}>
              <MapIcon aria-hidden />
              {t(tradesKeys.overview.areas.map, "Map")}
            </Button>
          </div>
        ))}
      </section>
    </div>
  );
}

/** Edits staged against `base`: how many, by piece. */
function unapplied(base: Draft, d: Draft): number {
  const weights = (ks: readonly Keyword[]) => new Map(ks.map((k) => [k.text, k.weight]));
  const [was, now] = [weights(base.keywords), weights(d.keywords)];
  const keywords = [...now].filter(([k, w]) => was.get(k) !== w || !was.has(k)).length + [...was.keys()].filter((k) => !now.has(k)).length;
  const tiers = (ts: readonly Tier[]) => ts.map((x) => `${x.google.id}:${x.weight}`);
  const [tw, tn] = [tiers(base.additional), tiers(d.additional)];
  const additional = tn.filter((x) => !tw.includes(x)).length + tw.filter((x) => !tn.includes(x)).length || Number(tw.join() !== tn.join());
  const apart = (a: readonly string[], b: readonly string[]) => a.filter((x) => !b.includes(x)).length + b.filter((x) => !a.includes(x)).length;
  return keywords + additional + apart(base.excluded.words, d.excluded.words) + apart(base.excluded.types, d.excluded.types) + Number(JSON.stringify(base.demand) !== JSON.stringify(d.demand));
}

/** What Google and every study of the category read, staged until applied: the whole of it is PUT at once, a new version its head moves onto. */
function Definition({ category, base, draft, staged, setDraft }: { category: Category; base: Draft; draft: Draft; staged: number; setDraft: (d: Draft | null) => void }) {
  const t = useT();
  const locale = useLocale();
  const button = useButtonSize();
  const categories = useCategories();
  const [refused, setRefused] = useState<string | null>(null);
  const [applying, setApplying] = useState(false);
  const { trade, country, head } = category;
  const lang = new Intl.DisplayNames([locale], { type: "language" }).of(head.lang) ?? head.lang;
  const name = categoryName(trade, country, locale);
  const change = (c: Partial<Draft>) => {
    setRefused(null);
    setDraft({ ...draft, ...c });
  };
  const unweighed = draft.additional.filter((a) => a.weight === null);
  const added = draft.additional.some((a) => !base.additional.some((b) => b.google.id === a.google.id));
  const apply = async () => {
    setApplying(true);
    try {
      await ok(
        api.PUT("/categories/{trade}/{country}", {
          params: { path: { trade: trade.id, country } },
          body: {
            additional: draft.additional.map((a) => {
              if (a.weight === null) throw new Error(`${a.google.id} unweighed: Apply is disabled until each tier has a weight`);
              return { id: a.google.id, weight: a.weight };
            }),
            keywords: draft.keywords,
            excluded: draft.excluded,
            demand: draft.demand,
          },
        }),
      );
      setDraft(null);
      categories.reload();
    } catch (e) {
      if (!(e instanceof Refused)) throw e;
      if (e.failure.code !== "unauthenticated") setRefused(failureText(e.failure, t));
    } finally {
      setApplying(false);
    }
  };

  return (
    <div className="@container flex flex-1 flex-col">
      <div className="grid gap-8 p-6 @4xl:grid-cols-2">
        <div className="flex flex-col gap-6">
          <Heading title={t(tradesKeys.definition.google, "To Google")} what={t(tradesKeys.definition.google.what, "What its GBPs are filed under and scanned on.")} />
          <section className={SECTION}>
            <h4 className={SECTION_HEAD}>
              {t(tradesKeys.google.categories, "Google categories")}
              <span className="flex-1" />
              <span className={HINT}>{t(tradesKeys.google.where, "typed into Google's form in {lang}", { lang })}</span>
            </h4>
            <p className="text-sm text-ink">{t(tradesKeys.google.primaryIs, "Primary: {trade}, the trade itself", { trade: googleName(trade, locale) })}</p>
            <GoogleCategories country={country} lang={head.lang} trade={trade} additional={draft.additional} setAdditional={(additional) => change({ additional })} />
          </section>
          <Keywords lang={lang} primary={normalized(googleName(trade, head.lang))} keywords={draft.keywords} set={(keywords) => change({ keywords })} />
        </div>
        <div className="flex flex-col gap-6">
          <Heading title={t(tradesKeys.definition.placement, "To placement")} what={t(tradesKeys.definition.placement.what, "What every area's study of it is built from. The map's sliders try other weights for one view.")} />
          <Competitors primary={googleName(trade, locale)} additional={draft.additional} set={(additional) => change({ additional })} />
          <ExcludedEditor excluded={draft.excluded} set={(excluded) => change({ excluded })} />
          <DemandEditor demand={draft.demand} set={(demand) => change({ demand })} />
        </div>
      </div>
      {(staged > 0 || refused !== null) && (
        <div className="sticky bottom-0 mt-auto flex items-center gap-3 border-t border-border bg-background px-6 py-3">
          <span className="flex min-w-0 flex-1 flex-col">
            <span className="text-sm font-semibold text-ink tabular-nums">{t(tradesKeys.definition.unapplied, "{n} unapplied", { n: staged })}</span>
            {refused !== null ? (
              <span className="text-xs text-accent-error">{refused}</span>
            ) : unweighed.length > 0 ? (
              <span className="text-xs text-accent-warn">{t(tradesKeys.definition.unweighed, "Weigh {categories}'s tier first: a weight is a person's to give", { categories: unweighed.map((a) => googleName(a.google, locale)).join(", ") })}</span>
            ) : (
              <span className="text-xs text-ink-soft">
                {[t(tradesKeys.definition.applies, "rebuilds the studies of {category}'s pulled areas from what is held · no search bought", { category: name }), added && t(tradesKeys.definition.applies.added, "a Google category added: the next pull buys its sweep")].filter(Boolean).join(" · ")}
              </span>
            )}
          </span>
          <Button variant="ghost" size={button("sm")} onClick={() => { setRefused(null); setDraft(null); }}>
            {t(tradesKeys.definition.discard, "Discard")}
          </Button>
          <Button size={button("sm")} disabled={applying || staged === 0 || unweighed.length > 0} onClick={() => void apply()}>
            {t(tradesKeys.definition.apply, "Apply")}
          </Button>
        </div>
      )}
    </div>
  );
}

function Heading({ title, what }: { title: string; what: string }) {
  return (
    <hgroup className="flex flex-col gap-0.5">
      <h3 className="text-sm font-semibold text-ink">{title}</h3>
      <p className="text-xs text-ink-soft">{what}</p>
    </hgroup>
  );
}

/** A weight 0–2; 0 is none: `null`. */
function Weight({ label, value, set, locked }: { label: string; value: number | null; set?: (w: number | null) => void; locked?: boolean }) {
  return (
    <span className="flex w-44 shrink-0 items-center gap-2">
      {locked === true || set === undefined ? (
        <Lock aria-hidden className="ml-auto size-3 text-ink-faint" />
      ) : (
        <Slider min={0} max={2} step={0.05} value={value ?? 0} onValueChange={(w) => set(w === 0 ? null : w)} aria-label={label} className={cn(value === null && "opacity-40")} />
      )}
      <span className={cn("w-9 shrink-0 text-right font-mono text-xs tabular-nums", value === null ? "text-ink-soft" : "text-ink")}>{value === null ? "–" : value.toFixed(2)}</span>
    </span>
  );
}

function Keywords({ lang, primary, keywords, set }: { lang: string; primary: string | null; keywords: readonly Keyword[]; set: (k: Keyword[]) => void }) {
  const t = useT();
  const size = useControlSize();
  const [typed, setTyped] = useState("");
  return (
    <section className={SECTION}>
      <h4 className={SECTION_HEAD}>
        {t(tradesKeys.keywords.weight, "Keywords · weight")}
        <span className="flex-1" />
        <span className={cn(HINT, "font-mono tabular-nums")}>
          {lang} · {keywords.length} / {MAX_KEYWORDS}
        </span>
      </h4>
      <ul className="flex flex-col divide-y divide-border">
        {keywords.map((k) => {
          const reference = k.text === primary;
          return (
            <li key={k.text} className={cn("flex h-8 items-center gap-3 text-sm", k.weight === null && "bg-accent-warn/5")}>
              <span className={cn("min-w-0 flex-1 truncate", k.weight === null ? "text-accent-warn" : "text-ink")}>
                {k.text}
                {reference && <span className="ml-2 text-xs text-ink-soft">{t(tradesKeys.keywords.reference, "the primary's name · the reference")}</span>}
                {k.weight === null && <span className="ml-2 text-xs">{t(tradesKeys.keywords.unscored, "scanned, not scored")}</span>}
              </span>
              <Weight label={t(tradesKeys.keywords.weigh, "Weight of {keyword}", { keyword: k.text })} value={k.weight} locked={reference} set={(w) => set(keywords.map((x) => (x.text === k.text ? { ...x, weight: w } : x)))} />
              {reference ? (
                <span className="size-4" />
              ) : (
                <button type="button" aria-label={t(tradesKeys.keywords.remove, "Remove {keyword}", { keyword: k.text })} onClick={() => set(keywords.filter((x) => x.text !== k.text))} className="rounded text-ink-soft outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-ring">
                  <X aria-hidden className="size-4" />
                </button>
              )}
            </li>
          );
        })}
      </ul>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          const fresh = [...new Set(typed.split(",").map(normalized).filter((k) => k !== "" && !keywords.some((x) => x.text === k)))];
          if (fresh.length > 0) set([...keywords, ...fresh.map((text) => ({ text, weight: null }))]);
          setTyped("");
        }}
      >
        <Input size={size} value={typed} onChange={(e) => setTyped(e.target.value)} placeholder={t(tradesKeys.keywords.add, "Add a keyword in {lang} — no city: the grid point is the place", { lang })} aria-label={t(tradesKeys.keywords.add, "Add a keyword in {lang} — no city: the grid point is the place", { lang })} />
      </form>
      <p className="text-xs text-ink-soft">{t(tradesKeys.keywords.why, "Every GBP of the category is scanned on these. Placement scores a competitor by how it ranks for each, weighed so; a keyword with no weight is scanned, not scored.")}</p>
    </section>
  );
}

/** Its tiers, derived: the primary's, then one per additional Google category in order, each weighed against the primary. */
function Competitors({ primary, additional, set }: { primary: string; additional: readonly Tier[]; set: (t: Tier[]) => void }) {
  const t = useT();
  const locale = useLocale();
  return (
    <section className={SECTION}>
      <h4 className={SECTION_HEAD}>
        {t(tradesKeys.competitors, "Competitors")}
        <span className="flex-1" />
        <span className={HINT}>{t(tradesKeys.competitors.what, "one tier per Google category")}</span>
      </h4>
      <ol className="flex flex-col divide-y divide-border">
        <TierRow n={1} name={primary} kind={t(tradesKeys.competitors.primary, "primary")} what={t(tradesKeys.competitors.primary.what, "what searching its name or a keyword finds")}>
          <Weight label={t(tradesKeys.competitors.reference, "the reference")} value={1} locked />
        </TierRow>
        {additional.map((a, i) => (
          <TierRow key={a.google.id} n={i + 2} name={googleName(a.google, locale)} kind={t(tradesKeys.competitors.additional, "additional")} what={a.weight === null ? t(tradesKeys.competitors.unweighed, "not weighed yet: give it a weight") : t(tradesKeys.competitors.additional.what, "what searching its name finds")} warn={a.weight === null}>
            <Weight label={t(tradesKeys.competitors.weigh, "Weight of one {tier}", { tier: googleName(a.google, locale) })} value={a.weight} set={(w) => set(additional.map((x, j) => (j === i ? { ...x, weight: w } : x)))} />
          </TierRow>
        ))}
      </ol>
      <p className="text-xs text-ink-soft">{t(tradesKeys.competitors.order, "First match wins, in this order. A Google category added on the left is a tier here.")}</p>
    </section>
  );
}

function TierRow({ n, name, kind, what, warn, children }: { n: number; name: string; kind: string; what: string; warn?: boolean; children: ReactNode }) {
  return (
    <li className="flex items-center gap-3 py-2">
      <span className="w-4 shrink-0 text-right font-mono text-xs text-ink-soft tabular-nums">{n}</span>
      <span className="flex min-w-0 flex-1 flex-col">
        <span className="flex items-center gap-2">
          <span className="truncate text-sm font-semibold text-ink">{name}</span>
          <span className="rounded-full border border-border px-1.5 text-caption text-ink-soft">{kind}</span>
        </span>
        <span className={cn("text-xs", warn === true ? "text-accent-warn" : "text-ink-soft")}>{what}</span>
      </span>
      {children}
    </li>
  );
}

function ExcludedEditor({ excluded, set }: { excluded: Excluded; set: (e: Excluded) => void }) {
  const t = useT();
  return (
    <section className={SECTION}>
      <h4 className={SECTION_HEAD}>
        {t(tradesKeys.excluded, "Excluded")}
        <span className="flex-1" />
        <span className={HINT}>{t(tradesKeys.excluded.what, "never a competitor: checked before any tier")}</span>
      </h4>
      <Chips
        values={excluded.words}
        set={(words) => set({ ...excluded, words })}
        clean={normalized}
        label={t(tradesKeys.excluded.words, "Words in a place's name")}
        placeholder={t(tradesKeys.excluded.words.add, "a word, Enter")}
      />
      <Chips
        values={excluded.types}
        set={(types) => set({ ...excluded, types })}
        clean={(s) => s.trim().toLowerCase()}
        mono
        label={t(tradesKeys.excluded.types, "Places types, as a place's primary")}
        placeholder={t(tradesKeys.excluded.types.add, "hardware_store, Enter")}
      />
    </section>
  );
}

function Chips({ values, set, clean, label, placeholder, mono }: { values: readonly string[]; set: (v: string[]) => void; clean: (s: string) => string; label: string; placeholder: string; mono?: boolean }) {
  const t = useT();
  const [typed, setTyped] = useState("");
  return (
    <div className="flex flex-col gap-1">
      <span className="text-xs text-ink-soft">{label}</span>
      <form
        className="flex flex-wrap items-center gap-1.5"
        onSubmit={(e) => {
          e.preventDefault();
          const fresh = [...new Set(typed.split(",").map(clean).filter((w) => w !== "" && !values.includes(w)))];
          if (fresh.length > 0) set([...values, ...fresh]);
          setTyped("");
        }}
      >
        {values.map((v) => (
          <span key={v} className={cn("flex items-center gap-1 rounded-md bg-hover py-0.5 pr-1 pl-2 text-xs text-ink", mono === true && "font-mono")}>
            {v}
            <button type="button" aria-label={t(tradesKeys.excluded.remove, "Remove {value}", { value: v })} onClick={() => set(values.filter((x) => x !== v))} className="rounded text-ink-soft outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-ring">
              <X aria-hidden className="size-3" />
            </button>
          </span>
        ))}
        <input value={typed} onChange={(e) => setTyped(e.target.value)} placeholder={placeholder} aria-label={label} className={cn("min-w-32 flex-1 bg-transparent px-1 py-0.5 text-xs text-ink outline-none placeholder:text-ink-faint", mono === true && "font-mono")} />
      </form>
    </div>
  );
}

/** Its demand surface: an expression over the grid's columns and its own derived ones, and heat layers of its own; `null`, none. */
function DemandEditor({ demand, set }: { demand: Demand | null; set: (d: Demand | null) => void }) {
  const t = useT();
  const size = useControlSize();
  const button = useButtonSize();
  const field = "h-7 min-w-0 rounded-md border border-border bg-transparent px-2 font-mono text-xs text-ink outline-none focus-visible:ring-2 focus-visible:ring-ring";
  return (
    <section className={SECTION}>
      <h4 className={SECTION_HEAD}>
        {t(tradesKeys.demand, "Demand")}
        <span className="flex-1" />
        <span className={HINT}>{t(tradesKeys.demand.what, "per cell of the country's grid")}</span>
      </h4>
      {demand === null ? (
        <span className="flex items-center gap-3">
          <span className="flex-1 text-sm text-ink-soft">{t(tradesKeys.demand.none, "No demand surface.")}</span>
          <Button variant="outline" size={button("sm")} onClick={() => set({ expr: "", columns: [], layers: [] })}>
            <Plus aria-hidden />
            {t(tradesKeys.demand.add, "Demand")}
          </Button>
        </span>
      ) : (
        <>
          <Textarea size={size} value={demand.expr} onChange={(e) => set({ ...demand, expr: e.target.value })} rows={3} aria-label={t(tradesKeys.demand.expr, "Expression")} placeholder="homes * (0.6 + old_share)" className="font-mono text-xs" />
          <span className="text-xs text-ink-soft">{t(tradesKeys.demand.columns, "Derived columns, in order: what the expression reads beside the grid's")}</span>
          {demand.columns.map((c, i) => (
            <span key={i} className="flex items-center gap-1.5">
              <input value={c.name} onChange={(e) => set({ ...demand, columns: demand.columns.map((x, j) => (j === i ? { ...x, name: e.target.value } : x)) })} aria-label={t(tradesKeys.demand.column.name, "Column name")} placeholder="old_share" className={cn(field, "w-32")} />
              <span className="text-ink-soft">=</span>
              <input value={c.expr} onChange={(e) => set({ ...demand, columns: demand.columns.map((x, j) => (j === i ? { ...x, expr: e.target.value } : x)) })} aria-label={t(tradesKeys.demand.column.expr, "Column expression")} className={cn(field, "flex-1")} />
              <Remove label={t(tradesKeys.demand.column.remove, "Remove column {name}", { name: c.name })} onClick={() => set({ ...demand, columns: demand.columns.filter((_, j) => j !== i) })} />
            </span>
          ))}
          <span className="text-xs text-ink-soft">{t(tradesKeys.demand.layers, "Heat layers it adds to the country's")}</span>
          {demand.layers.map((l, i) => {
            const edit = (c: Partial<Demand["layers"][number]>) => set({ ...demand, layers: demand.layers.map((x, j) => (j === i ? { ...x, ...c } : x)) });
            return (
              <span key={i} className="flex flex-col gap-1 rounded-md border border-border p-2">
                <span className="flex items-center gap-1.5">
                  <input value={l.name} onChange={(e) => edit({ name: e.target.value })} aria-label={t(tradesKeys.demand.layer.name, "Layer name")} placeholder={t(tradesKeys.demand.layer.name, "Layer name")} className={cn(field, "flex-1 font-sans")} />
                  <NativeSelect size="sm" value={l.scale} onChange={(e) => edit({ scale: e.currentTarget.value as (typeof scaleValues)[number] })} aria-label={t(tradesKeys.demand.layer.scale, "Scale")}>
                    {scaleValues.map((s) => (
                      <NativeSelectOption key={s} value={s}>
                        {s}
                      </NativeSelectOption>
                    ))}
                  </NativeSelect>
                  <Remove label={t(tradesKeys.demand.layer.remove, "Remove layer {name}", { name: l.name })} onClick={() => set({ ...demand, layers: demand.layers.filter((_, j) => j !== i) })} />
                </span>
                <input value={l.expr} onChange={(e) => edit({ expr: e.target.value })} aria-label={t(tradesKeys.demand.layer.expr, "Layer expression")} className={field} />
                <input value={l.note} onChange={(e) => edit({ note: e.target.value })} aria-label={t(tradesKeys.demand.layer.note, "Note")} placeholder={t(tradesKeys.demand.layer.note, "Note")} className={cn(field, "font-sans")} />
              </span>
            );
          })}
          <span className="flex flex-wrap gap-1.5">
            <Button variant="ghost" size={button("sm")} onClick={() => set({ ...demand, columns: [...demand.columns, { name: "", expr: "" }] })}>
              <Plus aria-hidden />
              {t(tradesKeys.demand.column.add, "Column")}
            </Button>
            <Button variant="ghost" size={button("sm")} onClick={() => set({ ...demand, layers: [...demand.layers, { name: "", expr: "", scale: "percentile", note: "" }] })}>
              <Plus aria-hidden />
              {t(tradesKeys.demand.layer.add, "Layer")}
            </Button>
            <span className="flex-1" />
            <Button variant="ghost" size={button("sm")} onClick={() => set(null)}>
              {t(tradesKeys.demand.remove, "Drop the demand surface")}
            </Button>
          </span>
        </>
      )}
    </section>
  );
}

function Remove({ label, onClick }: { label: string; onClick: () => void }) {
  return (
    <button type="button" aria-label={label} title={label} onClick={onClick} className="rounded p-1 text-ink-soft outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-ring">
      <X aria-hidden className="size-3.5" />
    </button>
  );
}
