"use client";

import { useResource } from "@evinvest/api/react";
import { Button, Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, Field, FieldLabel, Input, NativeSelect, NativeSelectGroup, NativeSelectOption, Slider, ToggleGroup, ToggleGroupItem, cn } from "@evinvest/uikit";
import { Car, ChevronRight, Crosshair, Eye, EyeOff, Flame, Grid3x3, MapPin, Settings2, SquareDashed, Store, TriangleAlert, Upload, Users, Wallet, Waves } from "lucide-react";
import { type ComponentType, type ReactNode, useEffect, useId, useState } from "react";
import { PANEL } from "@config";

import { categoryName, covers, googleName, openWindow, useCategories, useRelease, useScope } from "@/entities/gmb";
import type { Area, Category, CategoryVersion, Country, GoogleCategory, PublishedStudy } from "@/shared/api/schema";
import { useMe } from "@/entities/session";
import { Refused, api, ok } from "@/shared/api";
import { formatDateTime } from "@/shared/lib/format";
import { notifyFailure } from "@/shared/ui/notify";
import { useSpendConfirm, useTokens } from "@/entities/tokens";
import { type T, useLocale, useT, placement as placementKeys, state, hotbar } from "@/shared/i18n";
import { type Base, setBase, showPlaces, useBase, usePlacesShown } from "@/shared/map";

import { type Ages, type Data, type Layer, type Legend, type MapView, type Query, type Refusal, type Studied, parseStudy, pull, setMapView, studiable, studyText, useMapView, usePlacement, useStudied } from "../model/placement";

const LAYOUT = {
  column: "flex w-80 flex-col",
  columns: "grid w-160 grid-cols-2 gap-x-2",
} satisfies Record<typeof PANEL.mapPanel, string>;

/** The leading trade layers (Demand, Underserved) and location layers (Population, income, cars) a row each; the rest under More. */
const FEATURED = { trade: 2, location: 3 };
const ICONS: Record<string, ComponentType<{ className?: string }>> = { demand: Flame, "underserved-demand": Crosshair, population: Users, "standard-of-living-yr": Wallet, "estimated-cars": Car };

const FROM_NAME = {
  pick: (t) => t(placementKeys.from.pick, "your pick"),
  scope: (t) => t(placementKeys.from.scope, "scope"),
} satisfies Record<"pick" | "scope", (t: T) => string>;

const STUDIES_NAME = {
  scope: (t) => t(placementKeys.studies.scope, "In scope"),
  category: (t) => t(placementKeys.studies.category, "Other categories"),
} satisfies Record<"scope" | "category", (t: T) => string>;

const REFUSAL = {
  another_country: (t) => t(placementKeys.refused.country, "The area is in another country than the category"),
  unstudied: (t) => t(placementKeys.refused.unstudied, "Placement does not study this category here"),
} satisfies Record<Refusal, (t: T) => string>;

/** The hotbar's Map panel: the study (a category over an area), its heat, the overlays, what the data is, and the base. */
export function MapPanel() {
  const t = useT();
  const placement = usePlacement();
  const view = useMapView();
  const studied = useStudied(view);
  const base = useBase();
  const places = usePlacesShown();
  const me = useMe();

  const ok = placement.status === "ok" ? placement : null;
  const data = ok?.data ?? null;
  const legend = ok?.legend ?? null;
  const refused = ok?.refused ?? null;
  // why the trade overlays draw nothing, whatever their eye says
  const untraded =
    studied.status === "ok" && studied.area === null
      ? t(placementKeys.noArea, "no area picked")
      : refused !== null && "refused" in refused
        ? REFUSAL[refused.refused](t)
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
                <Resolve view={view} studied={studied}>
                  {(picked) => (
                    <>
                      <StudyPicker view={view} studied={studied} countries={ok.countries} picked={picked} />
                      <AreaPicker view={view} studied={studied} country={picked?.country ?? null} />
                      {refused !== null && ("refused" in refused ? <Note tone="warn">{REFUSAL[refused.refused](t)}</Note> : <Note tone="error">{refused.failed}</Note>)}
                      {studied.area === null && view.area !== "" && <Note tone="warn">{t(placementKeys.pickArea, "Pick an area, or draw one: a study is bought and kept per area")}</Note>}
                      {picked !== null && studied.area !== null && data !== null && refused === null && picked.published === null && api.mayCall("post", "/library/studies", me.permissions) && <PublishStudy picked={picked} area={studied.area} />}
                    </>
                  )}
                </Resolve>
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
                // on at the trade's own radius: none drawn, none to start from
                disabled={view.pressure === null && data?.trade == null}
                onToggle={() => setMapView({ pressure: view.pressure === null ? data!.trade!.lambda_m : null })}
              />
              {view.pressure !== null && <Radius lambda={view.pressure} />}
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
          {ok && data !== null && studied.status === "ok" && (
            <Resolve view={view} studied={studied}>
              {(picked) => <DataSection view={view} data={data} ages={legend?.ages ?? null} notice={ok.notice} pulling={ok.pulling} pullFailed={ok.pullFailed} drawn={studied.drawn} picked={picked?.version ?? null} />}
            </Resolve>
          )}
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

/** What `?study` picks, read: its version, the trade and country of it, the caller's category there, and the published study it is of. */
type Picked = { trade: GoogleCategory; country: Country; version: CategoryVersion; head: Category | null; published: PublishedStudy | null };

/** `?study` read for what draws on it; `null` while none is picked or it is not read yet. */
function Resolve({ view, studied, children }: { view: MapView; studied: Extract<Studied, { status: "ok" }>; children: (p: Picked | null) => ReactNode }) {
  const { study } = view;
  if (study === null) return children(null);
  if (study.kind === "head") {
    const head = studied.head!;
    return children({ trade: head.trade, country: head.country, version: head.head, head, published: null });
  }
  if (study.kind === "version") return <OfVersion id={study.id} published={null}>{children}</OfVersion>;
  return <OfPublished sha={study.sha} area={view.area}>{children}</OfPublished>;
}

function OfVersion({ id, published, children }: { id: number; published: PublishedStudy | null; children: (p: Picked | null) => ReactNode }) {
  const read = useResource(api, "/category-versions/{id}", { params: { path: { id } } });
  const categories = useCategories();
  if (read.status !== "ok" || categories.status !== "ok") return children(null);
  const v = read.data;
  return children({ trade: v.trade, country: v.country, version: v, head: categories.data.find((c) => c.trade.id === v.trade.id && c.country === v.country) ?? null, published });
}

/** A published study, among those the caller may see; the map moved onto its own area. */
function OfPublished({ sha, area, children }: { sha: string; area: string | null; children: (p: Picked | null) => ReactNode }) {
  const read = useResource(api, "/library/studies");
  const study = read.status === "ok" ? read.data.studies.find((s) => s.sha === sha) : undefined;
  useEffect(() => {
    if (study !== undefined && study.area !== area) setMapView({ area: study.area });
  }, [study, area]);
  if (read.status === "ok" && study === undefined) throw new Error(`?study=s${sha}: no published study the caller may see`);
  if (study === undefined) return children(null);
  return <OfVersion id={study.release.version} published={study}>{children}</OfVersion>;
}

type Option = { key: string; label: string; group: "scope" | "category" };

/** A category version to study: the caller's heads (those in the outline's scope first), the picked one's history, the library's versions of its trade, and the studies published of it here; none: the area's grid alone. */
function StudyPicker({ view, studied, countries, picked }: { view: MapView; studied: Extract<Studied, { status: "ok" }>; countries: readonly string[]; picked: Picked | null }) {
  const t = useT();
  const locale = useLocale();
  const categories = useCategories();
  const { categories: scope } = useScope();
  const options: Option[] = (categories.status === "ok" ? categories.data : []).flatMap((c) =>
    studiable(c, countries) ? [{ key: studyText({ kind: "head", trade: c.trade.id, country: c.country }), label: categoryName(c.trade, c.country, locale), group: scope !== null && scope.some((p) => covers(p, c.trade.id, c.country)) ? ("scope" as const) : ("category" as const) }] : [],
  );
  const value = view.study === null ? "" : studyText(view.study);
  const name = picked === null ? null : categoryName(picked.trade, picked.country, locale);
  return (
    <label className="flex h-(--control-h-sm) items-center gap-2 px-2">
      <span className="flex shrink-0 items-center gap-1.5 text-ink-mid">
        {t(placementKeys.trade, "Trade")}
        {view.from !== null && <span className="rounded bg-hover px-1 text-caption text-ink-soft">{FROM_NAME[view.from](t)}</span>}
      </span>
      <NativeSelect
        size="sm"
        wrapperClassName="min-w-0 flex-1"
        value={value}
        onChange={(e) => {
          const study = e.currentTarget.value === "" ? null : parseStudy(e.currentTarget.value);
          setMapView({ study, ...(study !== null && view.heat === null && { heat: "demand" }) });
        }}
      >
        <NativeSelectOption value="">{t(placementKeys.gridOnly, "none · the grid alone")}</NativeSelectOption>
        {value !== "" && !options.some((o) => o.key === value) && <NativeSelectOption value={value}>{picked === null ? "…" : picked.published !== null ? picked.published.title : `${name} · v${picked.version.id}`}</NativeSelectOption>}
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
        {picked?.head != null && <HistoryOptions category={picked.head} label={t(placementKeys.studies.history, "{category}: your history", { category: name! })} />}
        {picked !== null && <LibraryOptions trade={picked.trade} country={picked.country} label={t(placementKeys.studies.library, "{category}: the library", { category: name! })} />}
        {picked !== null && studied.area !== null && <PublishedOptions trade={picked.trade} country={picked.country} area={studied.area} label={t(placementKeys.studies.published, "Published here")} />}
      </NativeSelect>
      <CategoryButton picked={picked === null ? null : { trade: picked.trade.id, country: picked.country, label: name! }} />
    </label>
  );
}

/** The versions its head moved through, the head aside. */
function HistoryOptions({ category, label }: { category: Category; label: string }) {
  const locale = useLocale();
  const read = useResource(api, "/categories/{trade}/{country}/history", { params: { path: { trade: category.trade.id, country: category.country } } });
  if (read.status !== "ok") return null;
  const seen = new Map<number, string>();
  for (const m of read.data.moves.toSorted((a, b) => b.at.localeCompare(a.at))) if (m.to !== category.head.id && !seen.has(m.to)) seen.set(m.to, m.at);
  if (seen.size === 0) return null;
  return (
    <NativeSelectGroup label={label}>
      {[...seen].map(([id, at]) => (
        <NativeSelectOption key={id} value={studyText({ kind: "version", id })}>
          v{id} · {formatDateTime(at, locale)}
        </NativeSelectOption>
      ))}
    </NativeSelectGroup>
  );
}

/** Each listing of the trade there, at its latest version. */
function LibraryOptions({ trade, country, label }: { trade: GoogleCategory; country: Country; label: string }) {
  const read = useResource(api, "/library", { params: { query: { trade: trade.id, country, sort: "rank" } } });
  if (read.status !== "ok" || read.data.listings.length === 0) return null;
  return (
    <NativeSelectGroup label={label}>
      {read.data.listings.map((l) => (
        <NativeSelectOption key={l.id} value={studyText({ kind: "version", id: l.latest.version })}>
          {l.title} v{l.latest.n} · {l.author}
        </NativeSelectOption>
      ))}
    </NativeSelectGroup>
  );
}

/** The studies published of the trade over this area, each named by the release it was studied under. */
function PublishedOptions({ trade, country, area, label }: { trade: GoogleCategory; country: Country; area: Area; label: string }) {
  const read = useResource(api, "/library/studies", { params: { query: { trade: trade.id, country, area: area.slug } } });
  if (read.status !== "ok" || read.data.studies.length === 0) return null;
  return (
    <NativeSelectGroup label={label}>
      {read.data.studies.map((s) => (
        <NativeSelectOption key={s.sha} value={studyText({ kind: "published", sha: s.sha })}>
          {s.title} · {s.listing_title} v{s.release.n} · {s.author}
        </NativeSelectOption>
      ))}
    </NativeSelectGroup>
  );
}

/** Where a study is defined: the picked category, else every category. */
function CategoryButton({ picked }: { picked: { trade: string; country: Country; label: string } | null }) {
  const t = useT();
  const label = picked === null ? t(placementKeys.categories, "Categories: each defines its studies") : t(placementKeys.category, "{category}: its definition", { category: picked.label });
  return (
    <Button variant="ghost" size="sm" icon aria-label={label} title={label} onClick={() => openWindow(picked === null ? { kind: "categories" } : { kind: "trade", trade: picked.trade, country: picked.country })}>
      <Settings2 aria-hidden />
    </Button>
  );
}

/** The areas a study can be drawn over: the picked category's country's, every one with no category; or a new one drawn. */
function AreaPicker({ view, studied, country }: { view: MapView; studied: Extract<Studied, { status: "ok" }>; country: Country | null }) {
  const t = useT();
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

/** The study drawn published, its category version released in the same step when no listing has it yet. */
function PublishStudy({ picked, area }: { picked: Picked; area: Area }) {
  const t = useT();
  const [open, setOpen] = useState(false);
  return (
    <div className="px-2 py-1">
      <Button size="sm" variant="ghost" className="w-full justify-start" onClick={() => setOpen(true)}>
        <Upload aria-hidden />
        {t(placementKeys.publish.open, "Publish this study…")}
      </Button>
      {open && <PublishStudyDialog picked={picked} area={area} onClose={() => setOpen(false)} />}
    </div>
  );
}

function PublishStudyDialog({ picked, area, onClose }: { picked: Picked; area: Area; onClose: () => void }) {
  const t = useT();
  const locale = useLocale();
  const id = useId();
  const lineage = picked.head?.head.id === picked.version.id ? picked.head.lineage : null;
  const [title, setTitle] = useState(`${googleName(picked.trade, locale)} · ${area.name}`);
  // a head off a listing, unedited, is that listing's release; anything else is found out by asking
  const [releasing, setReleasing] = useState(picked.head?.head.id === picked.version.id && (lineage === null || lineage.edited));
  const [busy, setBusy] = useState(false);
  const { fields, release } = useRelease({ trade: picked.trade, country: picked.country, body: picked.version, lineage, fork: null, title: categoryName(picked.trade, picked.country, locale) });
  const named = title.trim() !== "" && title.trim().length <= 100;
  const submit = async () => {
    setBusy(true);
    try {
      const published = await ok(api.POST("/library/studies", { body: { area: area.slug, version: picked.version.id, title: title.trim(), ...(releasing && release !== null && { release }) } }));
      onClose();
      setMapView({ study: { kind: "published", sha: published.sha } });
    } catch (e) {
      if (e instanceof Refused && e.failure.code === "unreleased_category") setReleasing(true);
      else notifyFailure(e, t);
    } finally {
      setBusy(false);
    }
  };
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>{t(placementKeys.publish.title, "Publish this study")}</DialogTitle>
          <DialogDescription>{t(placementKeys.publish.what, "{area} under {category} v{version}: anyone may load it, each load paid by whoever pulls it.", { area: area.name, category: categoryName(picked.trade, picked.country, locale), version: picked.version.id })}</DialogDescription>
        </DialogHeader>
        <Field className="flex flex-col gap-1">
          <FieldLabel htmlFor={`${id}-title`}>{t(placementKeys.publish.name, "The study's title")}</FieldLabel>
          <Input id={`${id}-title`} maxLength={100} value={title} onChange={(e) => setTitle(e.target.value)} />
        </Field>
        {releasing ? (
          <section className="flex flex-col gap-2">
            <p className="text-sm text-ink">{t(placementKeys.publish.release, "Its category version is no listing's yet: it is released in the same step.")}</p>
            {fields}
          </section>
        ) : (
          <p className="text-xs text-ink-soft">{t(placementKeys.publish.public, "A published study is readable by everyone, for good.")}</p>
        )}
        <DialogFooter>
          <Button disabled={busy || !named || (releasing && release === null)} onClick={() => void submit()}>
            {t(placementKeys.publish, "Publish")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
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

function PullButton({ searches, pulling, drawn }: { searches: number; pulling: boolean; drawn: Query }) {
  const t = useT();
  const held = useTokens();
  const spend = useSpendConfirm();
  const price = held.status === "ok" ? held.data.prices["placement.search"] : undefined;
  return (
    <div className="px-2 py-1">
      <Button size="sm" variant="secondary" className="w-full" disabled={pulling || held.status !== "ok"} onClick={() => void spend.ask(price === undefined ? undefined : searches * price, () => pull(drawn))} title={t(placementKeys.pull.what, "re-asks Google the Places searches this map was built from")}>
        {price === undefined ? t(placementKeys.pull, "Pull latest · {n} searches", { n: searches }) : t(placementKeys.pull.priced, "Pull latest · {n} searches · ≈ {tokens} tokens", { n: searches, tokens: searches * price })}
      </Button>
      {spend.dialog}
    </div>
  );
}

function DataSection({ view, data, ages, notice, pulling, pullFailed, drawn, picked }: { view: MapView; data: Data; ages: Ages | null; notice: { text: string; missing: number | null } | null; pulling: boolean; pullFailed: string | null; drawn: Query | null; picked: CategoryVersion | null }) {
  const t = useT();
  const me = useMe();
  const trade = data.trade;
  return (
    <Section title={t(placementKeys.data, "Data")}>
      {notice !== null && <Note tone="warn">{notice.text}</Note>}
      {ages !== null && (
        <Fold title={t(placementKeys.age, "Data age · oldest {days} d", { days: Math.round(ages.oldest_days) })}>
          <AgeChart ages={ages} />
        </Fold>
      )}
      {/* a sweep never bought has no trade yet: what it lacks is the notice's */}
      {drawn !== null && (trade !== null || notice?.missing != null) && api.mayCall("post", "sha" in drawn ? "/placement/studies/{sha}/pull" : "/placement/study/pull", me.permissions) && (
        <PullButton searches={trade?.searches ?? notice!.missing!} pulling={pulling} drawn={drawn} />
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
      {picked !== null && picked.keywords.length > 0 && (
        <Fold title={t(placementKeys.keywords, "Keywords · weight")}>
          {picked.keywords.map((k) =>
            k.weight === null ? (
              <span key={k.text} className="flex items-center gap-2 px-2 py-0.5 text-accent-warn" title={t(placementKeys.unweighted, "Scanned, not scored: no weight")}>
                <TriangleAlert className="size-3.5 shrink-0" />
                <span className="truncate">{k.text}</span>
              </span>
            ) : (
              <span key={k.text} className="flex items-center gap-2 px-2 py-0.5">
                <span className="w-28 shrink-0 truncate">{k.text}</span>
                <span className="h-1.5 rounded-full bg-ink-soft" style={{ width: `${k.weight * 3}rem` }} />
                <span className="tabular-nums text-ink-soft">{k.weight.toFixed(2)}</span>
              </span>
            ),
          )}
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
        <span className="w-24 shrink-0 text-ink-mid">{t(placementKeys.opacity, "Opacity {pct}%", { pct: Math.round(view.opacity * 100) })}</span>
        <Slider min={0} max={1} step={0.05} value={view.opacity} onValueChange={(v) => setMapView({ opacity: v })} />
      </div>
    </Section>
  );
}

/** The overlay's age histogram of the competitors shown: a bar greys as it gets older. */
function AgeChart({ ages }: { ages: Ages }) {
  return (
    <div className="relative mx-2 mt-1 mb-4 h-12">
      {ages.bars.map((b) => (
        <div
          key={b.span}
          title={b.span}
          className="absolute bottom-0 flex items-start justify-center text-caption text-background tabular-nums"
          style={{ left: `${b.left}%`, width: `calc(${b.width}% - 1px)`, height: `${b.height}%`, background: `oklch(0.74 ${(0.16 * b.fresh).toFixed(3)} 235)` }}
        >
          {b.count}
        </div>
      ))}
      {ages.ticks.map((tick) => (
        <span key={tick.label} className="absolute -bottom-4 -translate-x-1/2 text-caption text-ink-soft" style={{ left: `${tick.left}%` }}>
          {tick.label}
        </span>
      ))}
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

function Radius({ lambda }: { lambda: number }) {
  const t = useT();
  return (
    <div className="flex items-center gap-2 px-2 py-1 pl-8">
      <span className="w-24 shrink-0 text-ink-mid">{t(placementKeys.radius, "Radius λ {km} km", { km: (lambda / 1000).toFixed(1) })}</span>
      <Slider min={250} max={10000} step={250} value={lambda} onValueChange={(v) => setMapView({ pressure: v })} />
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
function Row({ icon: Icon, label, warn, on, disabled, onToggle }: { icon: ComponentType<{ className?: string }>; label: string; warn?: string | undefined; on: boolean; disabled?: boolean; onToggle: () => void }) {
  const Shown = on ? Eye : EyeOff;
  return (
    <button
      type="button"
      aria-pressed={on}
      disabled={disabled}
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
