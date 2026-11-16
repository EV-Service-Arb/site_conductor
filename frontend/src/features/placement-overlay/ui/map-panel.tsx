"use client";

import { Button, NativeSelect, NativeSelectGroup, NativeSelectOption, Slider, ToggleGroup, ToggleGroupItem, cn } from "@evinvest/uikit";
import { Car, ChevronRight, Crosshair, Eye, EyeOff, Flame, Grid3x3, MapPin, Settings2, SquareDashed, Store, TriangleAlert, Users, Wallet, Waves } from "lucide-react";
import { type ComponentType, type ReactNode, useState } from "react";
import { PANEL } from "@config";

import { covers, openCategories, openTrade, useScope, useTrades } from "@/entities/gmb";
import type { Country } from "@/shared/api/schema";
import { may, useMe } from "@/entities/session";
import { type T, useT, placement as placementKeys, state, hotbar } from "@/shared/i18n";
import { type Base, setBase, showPlaces, useBase, usePlacesShown } from "@/shared/map";

import { type CategoryKey, type Data, type Layer, type Legend, type MapView, type Refusal, type Studied, categoryKey, pull, setMapView, useMapView, usePlacement, useStudied } from "../model/placement";

const LAYOUT = {
  column: "flex w-80 flex-col",
  columns: "grid w-160 grid-cols-2 gap-x-2",
} satisfies Record<typeof PANEL.mapPanel, string>;

/** The leading trade layers (Demand, Underserved) and location layers (Population, income, cars) a row each; the rest under More. */
const FEATURED = { trade: 2, location: 3 };
const ICONS: Record<string, ComponentType<{ className?: string }>> = { demand: Flame, "underserved-demand": Crosshair, population: Users, "standard-of-living-yr": Wallet, "estimated-cars": Car };

/** Upper edges in days, each a period a person names; the last bar holds everything older. */
const AGE_EDGES: readonly (readonly [number, string])[] = [
  [1, "1d"],
  [3, "3d"],
  [7, "1w"],
  [14, "2w"],
  [30, "1mo"],
  [61, "2mo"],
  [91, "3mo"],
  [182, "6mo"],
  [365, "1y"],
  [730, "2y"],
  [1460, "4y"],
];

const FROM_NAME = {
  pick: (t) => t(placementKeys.from.pick, "your pick"),
  scope: (t) => t(placementKeys.from.scope, "scope"),
} satisfies Record<"pick" | "scope", (t: T) => string>;

const STUDIES_NAME = {
  scope: (t) => t(placementKeys.studies.scope, "In scope"),
  category: (t) => t(placementKeys.studies.category, "Categories with a study"),
} satisfies Record<"scope" | "category", (t: T) => string>;

const REFUSAL = {
  "no study": (t) => t(placementKeys.refused.study, "The category links no study"),
  "no google category": (t) => t(placementKeys.refused.google, "The category has no Google category: it names the competitors"),
  "no keywords": (t) => t(placementKeys.refused.keywords, "The category has no keywords: they weigh the competitors"),
  "another country": (t) => t(placementKeys.refused.country, "The area is in another country than the category"),
} satisfies Record<Refusal, (t: T) => string>;

/** The hotbar's Map panel: the study (a category over an area), its heat, the overlays, what the data is, and the base. */
export function MapPanel() {
  const t = useT();
  const placement = usePlacement();
  const view = useMapView();
  const studied = useStudied(view);
  const base = useBase();
  const places = usePlacesShown();

  const ok = placement.status === "ok" ? placement : null;
  const data = ok?.data ?? null;
  const legend = ok?.legend ?? null;
  const drawn = studied.status === "ok" ? studied.drawn : null;
  // why the trade overlays draw nothing, whatever their eye says
  const untraded =
    studied.status === "ok" && studied.area === null
      ? t(placementKeys.noArea, "no area picked")
      : drawn !== null && "refused" in drawn
        ? REFUSAL[drawn.refused](t)
        : view.study === null
          ? t(placementKeys.pickTrade, "pick a trade")
          : data !== null && data.trade === null
            ? t(placementKeys.noCompetitors, "no competitor bought yet")
            : null;

  return (
    <div className="flex max-h-[70vh] flex-col overflow-y-auto py-1 text-body-s">
      {placement.status === "loading" && <Note>{t(state.loading, "Loading…")}</Note>}
      {placement.status === "unconnected" && <Note>{t(placementKeys.unconnected, "Placement not connected")}</Note>}
      {placement.status === "error" && <Note tone="error">{placement.message}</Note>}
      {studied.status === "error" && <Note tone="error">{studied.message}</Note>}
      <div className={LAYOUT[PANEL.mapPanel]}>
        <div className="flex flex-col">
          {ok && studied.status === "ok" && (
            <>
              <Section title={t(placementKeys.study, "Study")}>
                <StudyPicker view={view} studied={studied} trades={ok.trades} />
                <AreaPicker view={view} studied={studied} />
                {drawn !== null && "refused" in drawn && <Note tone="warn">{REFUSAL[drawn.refused](t)}</Note>}
                {studied.area === null && view.area !== "" && <Note tone="warn">{t(placementKeys.pickArea, "Pick an area, or draw one: a study is bought and kept per area")}</Note>}
              </Section>
              {data !== null && <Heat view={view} data={data} legend={legend} />}
            </>
          )}
        </div>
        <div className="flex flex-col">
          {ok && (
            <Section title={t(placementKeys.overlays, "Overlays")}>
              <Row
                icon={Waves}
                label={t(placementKeys.pressure, "Competitor pressure")}
                warn={untraded ?? (view.pressure !== null && legend?.pressure?.levels === null ? t(placementKeys.noPressure, "nothing in view under pressure") : undefined)}
                on={view.pressure !== null}
                onToggle={() => setMapView({ pressure: view.pressure === null ? {} : null })}
              />
              {view.pressure !== null && data?.trade != null && <Radius view={view} fallback={data.trade.lambda_m} />}
              <Row
                icon={Store}
                label={t(placementKeys.competitors, "Competitors")}
                warn={untraded ?? (view.competitors && legend?.competitors?.in_view === 0 ? t(placementKeys.noneInView, "none in view") : undefined)}
                on={view.competitors}
                onToggle={() => setMapView({ competitors: !view.competitors })}
              />
              {data?.trade != null && <Tiers view={view} trade={data.trade} />}
              <Row icon={MapPin} label={t(hotbar.places, "Google's places")} on={places} onToggle={() => showPlaces(!places)} />
            </Section>
          )}
          {ok && data !== null && <DataSection view={view} data={data} notice={ok.notice} pulling={ok.pulling} pullFailed={ok.pullFailed} />}
          {ok && data === null && ok.notice !== null && <Note tone="warn">{ok.notice.text}</Note>}
          <Section title={t(placementKeys.base, "Base")}>
            <ToggleGroup type="single" size="sm" value={base} onValueChange={(v) => typeof v === "string" && v !== "" && setBase(v as Base)} className="px-2">
              <ToggleGroupItem value="schematic">{t(placementKeys.base.schematic, "Schematic")}</ToggleGroupItem>
              <ToggleGroupItem value="satellite">{t(placementKeys.base.satellite, "Satellite")}</ToggleGroupItem>
            </ToggleGroup>
          </Section>
        </div>
      </div>
    </div>
  );
}

type Option = { key: CategoryKey; label: string; group: "scope" | "category" };

/** Categories whose study gop serves, those in the outline's scope first; none: the area's grid alone. */
function StudyPicker({ view, studied, trades: served }: { view: MapView; studied: Extract<Studied, { status: "ok" }>; trades: readonly string[] }) {
  const t = useT();
  const trades = useTrades();
  const { categories: scope } = useScope();
  const options: Option[] = (trades.status === "ok" ? trades.data : []).flatMap((tr) =>
    tr.categories.flatMap((c) =>
      c.study !== null && served.includes(c.study)
        ? [{ key: categoryKey(tr.id, c.country), label: `${tr.name} · ${c.country}`, group: scope !== null && scope.some((p) => covers(p, tr.id, c.country)) ? ("scope" as const) : ("category" as const) }]
        : [],
    ),
  );
  const picked = studied.picked;
  return (
    <label className="flex h-(--control-h-sm) items-center gap-2 px-2">
      <span className="flex shrink-0 items-center gap-1.5 text-ink-mid">
        {t(placementKeys.trade, "Trade")}
        {view.from !== null && <span className="rounded bg-hover px-1 text-caption text-ink-soft">{FROM_NAME[view.from](t)}</span>}
      </span>
      <NativeSelect
        size="sm"
        wrapperClassName="min-w-0 flex-1"
        value={view.study ?? ""}
        onChange={(e) => {
          const study = e.currentTarget.value === "" ? null : (e.currentTarget.value as CategoryKey);
          setMapView({ study, ...(study !== null && view.heat === null && { heat: "demand" }) });
        }}
      >
        <NativeSelectOption value="">{t(placementKeys.gridOnly, "none · the grid alone")}</NativeSelectOption>
        {(["scope", "category"] as const).map((group) => {
          const shown = options.filter((o) => o.group === group);
          return (
            shown.length > 0 && (
              <NativeSelectGroup key={group} label={STUDIES_NAME[group](t)}>
                {shown.map((o) => (
                  <NativeSelectOption key={o.key} value={o.key}>
                    {o.label}
                  </NativeSelectOption>
                ))}
              </NativeSelectGroup>
            )
          );
        })}
      </NativeSelect>
      <CategoryButton picked={picked === null ? null : { trade: picked.trade.id, country: picked.category.country, label: `${picked.trade.name} · ${picked.category.country}` }} />
    </label>
  );
}

/** Where a study is defined: the picked category (its keywords, Google's categories, its study), else every category. */
function CategoryButton({ picked }: { picked: { trade: number; country: Country; label: string } | null }) {
  const t = useT();
  const label = picked === null ? t(placementKeys.categories, "Categories: where a study is linked") : t(placementKeys.category, "{category}: keywords, Google's categories, study", { category: picked.label });
  return (
    <Button variant="ghost" size="sm" icon aria-label={label} title={label} onClick={() => (picked === null ? openCategories(true) : openTrade(picked.trade, picked.country))}>
      <Settings2 aria-hidden />
    </Button>
  );
}

/** The areas a study can be drawn over: the picked category's country's, every one with no category; or a new one drawn. */
function AreaPicker({ view, studied }: { view: MapView; studied: Extract<Studied, { status: "ok" }> }) {
  const t = useT();
  const country = studied.picked?.category.country ?? null;
  const areas = studied.areas.filter((a) => country === null || a.country === country || a.slug === view.area).toSorted((a, b) => a.name.localeCompare(b.name));
  return (
    <label className="flex h-(--control-h-sm) items-center gap-2 px-2">
      <span className="shrink-0 text-ink-mid">{t(placementKeys.area, "Area")}</span>
      <NativeSelect size="sm" wrapperClassName="min-w-0 flex-1" value={view.area === null || view.area === "" ? "-" : view.area} onChange={(e) => setMapView({ area: e.currentTarget.value === "-" ? null : e.currentTarget.value })}>
        <NativeSelectOption value="-">{view.area === "" ? t(placementKeys.drawing, "drawing one…") : t(placementKeys.pickAreaShort, "pick an area")}</NativeSelectOption>
        {areas.map((a) => (
          <NativeSelectOption key={a.slug} value={a.slug}>
            {a.name} · {a.country}
          </NativeSelectOption>
        ))}
      </NativeSelect>
      <Button variant="ghost" size="sm" icon aria-label={t(placementKeys.drawArea, "Draw a new area")} title={t(placementKeys.drawArea, "Draw a new area")} aria-pressed={view.area === ""} onClick={() => setMapView({ area: "" })}>
        <SquareDashed aria-hidden />
      </Button>
    </label>
  );
}

function Heat({ view, data, legend }: { view: MapView; data: Data; legend: Legend | null }) {
  const t = useT();
  const [more, setMore] = useState(false);
  const trade = data.layers.filter((l) => l.trade);
  const location = data.layers.filter((l) => !l.trade);
  const featured = [...trade.slice(0, FEATURED.trade), ...location.slice(0, FEATURED.location)];
  const rest = [...trade.slice(FEATURED.trade), ...location.slice(FEATURED.location)];
  const row = (l: Layer) => {
    const on = view.heat === l.id;
    return (
      <Row
        key={l.id}
        icon={ICONS[l.id] ?? Grid3x3}
        label={l.name}
        warn={on && legend?.heat?.cells === 0 ? t(placementKeys.noCells, "no cell of the area in view") : undefined}
        on={on}
        onToggle={() => setMapView({ heat: on ? null : l.id })}
      />
    );
  };
  return (
    <Section title={t(placementKeys.heat, "Heat")}>
      {featured.map(row)}
      {rest.length > 0 && (
        <button type="button" onClick={() => setMore(!more)} className="flex h-(--control-h-sm) items-center gap-2 rounded-control px-2 text-ink-soft hover:bg-hover hover:text-ink">
          <ChevronRight className={cn("size-4 transition-transform", more && "rotate-90")} />
          {t(placementKeys.more, "More")}
        </button>
      )}
      {more && rest.map(row)}
      {legend?.heat && <p className="px-2 pt-1 text-caption text-ink-soft">{legend.heat.note}</p>}
    </Section>
  );
}

type TradeData = NonNullable<Data["trade"]>;

/** Each tier of competitor: shown or not, and how much one weighs in the pressure. */
function Tiers({ view, trade }: { view: MapView; trade: TradeData }) {
  const t = useT();
  const tune = (name: string, change: Partial<{ weight: number; show: boolean }>) => {
    const own = trade.tiers.find((x) => x.name === name)!;
    const now = view.tiers?.[name] ?? { weight: own.weight, show: true };
    setMapView({ tiers: { ...view.tiers, [name]: { ...now, ...change } } });
  };
  return (
    <div className="flex flex-col pl-6">
      {trade.tiers.map((tier) => {
        const tuned = view.tiers?.[tier.name] ?? { weight: tier.weight, show: true };
        const Shown = tuned.show ? Eye : EyeOff;
        return (
          <div key={tier.name} className="flex items-center gap-2 px-2 py-0.5">
            <button type="button" aria-pressed={tuned.show} aria-label={t(placementKeys.tier.show, "Show {tier}", { tier: tier.name })} onClick={() => tune(tier.name, { show: !tuned.show })} className="text-ink-soft hover:text-ink">
              <Shown className="size-4" />
            </button>
            <span className={cn("w-24 shrink-0 truncate", !tuned.show && "text-ink-soft")}>
              {tier.name} <span className="tabular-nums text-ink-soft">({tier.fetched_s.length})</span>
            </span>
            <Slider min={0} max={2} step={0.05} value={tuned.weight} onValueChange={(w) => tune(tier.name, { weight: w })} aria-label={t(placementKeys.tier.weight, "Weight of one {tier}", { tier: tier.name })} />
            <span className="w-8 shrink-0 text-right tabular-nums text-ink-soft">{tuned.weight.toFixed(2)}</span>
          </div>
        );
      })}
    </div>
  );
}

function DataSection({ view, data, notice, pulling, pullFailed }: { view: MapView; data: Data; notice: { text: string; missing: number | null } | null; pulling: boolean; pullFailed: string | null }) {
  const t = useT();
  const me = useMe();
  const [now] = useState(() => Date.now() / 1000);
  const trade = data.trade;
  const ages = trade === null ? [] : trade.tiers.flatMap((tier) => (view.tiers?.[tier.name]?.show === false ? [] : tier.fetched_s.map((s) => (now - s) / 86400)));
  const oldest = ages.length === 0 ? null : Math.max(...ages);
  return (
    <Section title={t(placementKeys.data, "Data")}>
      {notice !== null && <Note tone="warn">{notice.text}</Note>}
      {trade !== null && oldest !== null && (
        <Fold title={t(placementKeys.age, "Data age · oldest {days} d", { days: Math.round(oldest) })}>
          <AgeChart ages={ages} />
        </Fold>
      )}
      {trade !== null && may(me, "sa:placement:study:spend") && (
        <div className="px-2 py-1">
          <Button size="sm" variant="secondary" className="w-full" disabled={pulling} onClick={() => void pull()} title={t(placementKeys.pull.what, "re-asks Google the Places searches this map was built from")}>
            {t(placementKeys.pull, "Pull latest · {n} searches", { n: trade.searches })}
          </Button>
        </div>
      )}
      {pullFailed !== null && <Note tone="error">{pullFailed}</Note>}
      {trade !== null && trade.categories.length > 0 && (
        <Fold title={t(placementKeys.googleCategories, "Google categories")} open>
          {trade.categories.map((c) => {
            const n = c.counts.filter((_, i) => view.tiers?.[trade.tiers[i]!.name]?.show !== false).reduce((a, b) => a + b, 0);
            return (
              n > 0 && (
                <span key={c.kind} className="flex items-center gap-2 px-2 py-0.5">
                  <span className="size-2.5 shrink-0 rounded-full" style={{ background: c.color }} />
                  <span className="flex-1 truncate">{c.label}</span>
                  <span className="tabular-nums text-ink-soft">{n}</span>
                </span>
              )
            );
          })}
        </Fold>
      )}
      {trade !== null && trade.queries.length > 0 && (
        <Fold title={t(placementKeys.queries, "Search queries · {n}", { n: trade.queries.length })}>
          {trade.queries.map((q) => (
            <span key={q.text} className="flex items-center gap-2 px-2 py-0.5">
              <span className="flex-1 truncate">{q.text}</span>
              <span className="tabular-nums text-ink-soft">{q.hits}</span>
            </span>
          ))}
        </Fold>
      )}
      {trade !== null && (trade.terms.length > 0 || trade.unweighted.length > 0) && (
        <Fold title={t(placementKeys.keywords, "Keywords · by search volume")}>
          {trade.terms.map((k) => (
            <span key={k.text} className="flex items-center gap-2 px-2 py-0.5">
              <span className="w-28 shrink-0 truncate">{k.text}</span>
              <span className="h-1.5 rounded-full bg-ink-soft" style={{ width: `${k.weight * 6}rem` }} />
            </span>
          ))}
          {trade.unweighted.map((k) => (
            <span key={k} className="flex items-center gap-2 px-2 py-0.5 text-accent-warn" title={t(placementKeys.unweighted, "No search volume for it: left out of the ranking")}>
              <TriangleAlert className="size-3.5 shrink-0" />
              <span className="truncate">{k}</span>
            </span>
          ))}
        </Fold>
      )}
      <button
        type="button"
        aria-pressed={view.hideImputed}
        onClick={() => setMapView({ hideImputed: !view.hideImputed })}
        className="flex h-(--control-h-sm) items-center gap-2 rounded-control px-2 text-left text-ink-mid hover:bg-hover hover:text-ink"
      >
        {view.hideImputed ? <EyeOff className="size-4 shrink-0" /> : <Eye className="size-4 shrink-0" />}
        <span className="flex-1">{t(placementKeys.imputed, "Imputed cells · {n} of {cells}", { n: data.imputed, cells: data.cells })}</span>
      </button>
      <div className="flex items-center gap-2 px-2 py-1">
        <span className="w-24 shrink-0 text-ink-mid">{t(placementKeys.opacity, "Opacity {pct}%", { pct: Math.round((view.opacity ?? 1) * 100) })}</span>
        <Slider min={0} max={1} step={0.05} value={view.opacity ?? 1} onValueChange={(v) => setMapView({ opacity: v })} />
      </div>
    </Section>
  );
}

/** Competitors per age bucket over a logged axis, so a month does not dwarf the day before it; a bar greys as it gets older. */
function AgeChart({ ages }: { ages: readonly number[] }) {
  const bucket = (d: number) => {
    const i = AGE_EDGES.findIndex(([e]) => d < e);
    return i === -1 ? AGE_EDGES.length - 1 : i;
  };
  const counts = new Array<number>(bucket(Math.max(...ages)) + 1).fill(0);
  for (const d of ages) counts[bucket(d)]! += 1;
  const top = AGE_EDGES[counts.length - 1]![0];
  const x = (d: number) => (100 * Math.log1p(d)) / Math.log1p(top);
  const max = Math.max(...counts);
  return (
    <div className="relative mx-2 mt-1 mb-4 h-12">
      {counts.map((n, i) => {
        if (n === 0) return null;
        const lo = i === 0 ? 0 : AGE_EDGES[i - 1]![0];
        const hi = AGE_EDGES[i]![0];
        const chroma = 0.16 * (1 - x((lo + hi) / 2) / 100);
        return (
          <div
            key={i}
            className="absolute bottom-0 flex items-start justify-center text-caption text-background tabular-nums"
            style={{ left: `${x(lo)}%`, width: `calc(${x(hi) - x(lo)}% - 1px)`, height: `${(100 * n) / max}%`, background: `oklch(0.74 ${chroma.toFixed(3)} 235)` }}
          >
            {n}
          </div>
        );
      })}
      {AGE_EDGES.slice(0, counts.length).map(([e, label], i) =>
        [0, 2, 4, 8].includes(i) || i === counts.length - 1 ? (
          <span key={label} className="absolute -bottom-4 -translate-x-1/2 text-caption text-ink-soft" style={{ left: `${x(e)}%` }}>
            {label}
          </span>
        ) : null,
      )}
    </div>
  );
}

/** One line on the hotbar item: what the map is drawing. */
export function MapPreview() {
  const t = useT();
  const placement = usePlacement();
  const view = useMapView();
  if (placement.status !== "ok") return placement.status === "unconnected" ? t(placementKeys.unconnected, "Placement not connected") : t(placementKeys.title, "Map");
  const heat = placement.data?.layers.find((l) => l.id === view.heat)?.name ?? t(placementKeys.noHeat, "No heat");
  return [heat, placement.data?.name ?? t(placementKeys.noArea, "no area picked"), view.pressure !== null && placement.data?.trade != null && t(placementKeys.pressureOn, "pressure on")].filter(Boolean).join(" · ");
}

function Radius({ view, fallback }: { view: MapView; fallback: number }) {
  const t = useT();
  const lambda = view.pressure?.lambda ?? fallback;
  return (
    <div className="flex items-center gap-2 px-2 py-1 pl-8">
      <span className="w-24 shrink-0 text-ink-mid">{t(placementKeys.radius, "Radius λ {km} km", { km: (lambda / 1000).toFixed(1) })}</span>
      <Slider min={250} max={10000} step={250} value={lambda} onValueChange={(v) => setMapView({ pressure: { lambda: v } })} />
    </div>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="flex flex-col px-1 pt-2">
      <p className="px-2 pb-1 text-caption font-medium tracking-wide text-ink-soft uppercase">{title}</p>
      {children}
    </section>
  );
}

function Fold({ title, open: initial = false, children }: { title: string; open?: boolean; children: ReactNode }) {
  const [open, setOpen] = useState(initial);
  return (
    <>
      <button type="button" onClick={() => setOpen(!open)} className="flex h-(--control-h-sm) items-center gap-2 rounded-control px-2 text-ink-mid hover:bg-hover hover:text-ink">
        <ChevronRight className={cn("size-4 transition-transform", open && "rotate-90")} />
        {title}
      </button>
      {open && <div className="flex flex-col pb-1 pl-6">{children}</div>}
    </>
  );
}

/** `warn`: on or off, it draws nothing here, and why. */
function Row({ icon: Icon, label, warn, on, onToggle }: { icon: ComponentType<{ className?: string }>; label: string; warn?: string | undefined; on: boolean; onToggle: () => void }) {
  const Shown = on ? Eye : EyeOff;
  return (
    <button
      type="button"
      aria-pressed={on}
      onClick={onToggle}
      title={warn}
      className={cn("flex h-(--control-h-sm) items-center gap-2 rounded-control px-2 text-left text-ink-mid hover:bg-hover hover:text-ink", on && "text-ink", warn !== undefined && "text-accent-warn hover:text-accent-warn")}
    >
      {warn === undefined ? <Icon className="size-4 shrink-0" /> : <TriangleAlert className="size-4 shrink-0" />}
      <span className="flex-1 truncate">{label}</span>
      {warn !== undefined && <span className="max-w-32 truncate text-caption">{warn}</span>}
      <Shown className={cn("size-4 shrink-0", !on && "text-ink-soft")} />
    </button>
  );
}

function Note({ children, tone }: { children: ReactNode; tone?: "warn" | "error" }) {
  return <p className={cn("px-3 py-1.5 text-ink-soft", tone === "warn" && "text-accent-warn", tone === "error" && "text-accent-error")}>{children}</p>;
}
