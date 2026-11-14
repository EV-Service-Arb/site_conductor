"use client";

import { Button, NativeSelect, NativeSelectGroup, NativeSelectOption, Slider, ToggleGroup, ToggleGroupItem, cn } from "@evinvest/uikit";
import { Car, ChevronRight, Crosshair, Eye, EyeOff, Flame, Grid3x3, MapPin, Store, Users, Wallet, Waves } from "lucide-react";
import { type ComponentType, type ReactNode, useState } from "react";

import { covers, useScope, useTrades } from "@/entities/gmb";
import { may, useMe } from "@/entities/session";
import { type T, useT, placement as placementKeys, state, hotbar } from "@/shared/i18n";
import { type Base, setBase, showPlaces, useBase, usePlacesShown } from "@/shared/map";

import { type Layer, type MapView, pull, setMapView, useMapView, usePlacement } from "../model/placement";

/** The leading trade layers (Demand, Underserved) and location layers (Population, income, cars) a row each; the rest under More. */
const FEATURED = { trade: 2, location: 3 };
const ICONS: Record<string, ComponentType<{ className?: string }>> = { demand: Flame, "underserved-demand": Crosshair, population: Users, "standard-of-living-yr": Wallet, "estimated-cars": Car };

/** The hotbar's Map panel: the heat, the overlays and the base, as openmarket's "show on chart". */
const FROM_NAME = {
  pick: (t) => t(placementKeys.from.pick, "your pick"),
  scope: (t) => t(placementKeys.from.scope, "scope"),
} satisfies Record<"pick" | "scope", (t: T) => string>;

const STUDIES_NAME = {
  scope: (t) => t(placementKeys.studies.scope, "In scope"),
  category: (t) => t(placementKeys.studies.category, "Categories with a study"),
  stem: (t) => t(placementKeys.studies.stem, "Studies no category links"),
} satisfies Record<"scope" | "category" | "stem", (t: T) => string>;

export function MapPanel() {
  const t = useT();
  const placement = usePlacement();
  const view = useMapView();
  const base = useBase();
  const places = usePlacesShown();
  const me = useMe();
  const [more, setMore] = useState(false);

  const ok = placement.status === "ok" ? placement : null;
  const studies = useStudies(ok?.trades ?? []);
  const layers = ok?.layers ?? [];
  const trade = layers.filter((l) => l.trade);
  const location = layers.filter((l) => !l.trade);
  const featured = [...trade.slice(0, FEATURED.trade), ...location.slice(0, FEATURED.location)];
  const rest = [...trade.slice(FEATURED.trade), ...location.slice(FEATURED.location)];
  const heatRow = (l: Layer) => (
    <Row key={l.id} icon={ICONS[l.id] ?? Grid3x3} label={l.name} on={view.heat === l.id} onToggle={() => setMapView({ heat: view.heat === l.id ? null : l.id })} />
  );

  return (
    <div className="flex max-h-[70vh] flex-col overflow-y-auto py-1 text-body-s">
      <p className="px-3 pt-2 pb-1 text-body font-medium">{t(placementKeys.title, "Map")}</p>
      {placement.status === "loading" && <Note>{t(state.loading, "Loading…")}</Note>}
      {placement.status === "unconnected" && <Note>{t(placementKeys.unconnected, "Placement not connected")}</Note>}
      {placement.status === "error" && <Note tone="error">{placement.message}</Note>}
      {ok && (
        <>
          <Section title={t(placementKeys.heat, "Heat")}>
            <label className="flex h-(--control-h-sm) items-center gap-2 px-2">
              <span className="flex flex-1 items-center gap-1.5 text-ink-mid">
                {t(placementKeys.trade, "Trade")}
                {view.from !== null && <span className="rounded bg-hover px-1 text-caption text-ink-soft">{FROM_NAME[view.from](t)}</span>}
              </span>
              <NativeSelect
                size="sm"
                wrapperClassName="w-48"
                value={studies.find((o) => o.study === view.study)?.key ?? ""}
                onChange={(e) => {
                  const study = studies.find((o) => o.key === e.currentTarget.value)?.study ?? null;
                  setMapView({ study, ...(study !== null && view.heat === null && { heat: "demand" }) });
                }}
              >
                <NativeSelectOption value="">{t(placementKeys.pickTrade, "pick a trade")}</NativeSelectOption>
                {(["scope", "category", "stem"] as const).map((group) => {
                  const options = studies.filter((o) => o.group === group);
                  return (
                    options.length > 0 && (
                      <NativeSelectGroup key={group} label={STUDIES_NAME[group](t)}>
                        {options.map((o) => (
                          <NativeSelectOption key={o.key} value={o.key}>
                            {o.label}
                          </NativeSelectOption>
                        ))}
                      </NativeSelectGroup>
                    )
                  );
                })}
              </NativeSelect>
            </label>
            {ok.inView === "zoom" && <Note>{t(placementKeys.zoomIn, "Zoom in to see the grid")}</Note>}
            {ok.inView !== "zoom" && ok.inView.length === 0 && <Note>{t(placementKeys.noGrid, "No grid here")}</Note>}
            {ok.inView !== "zoom" && ok.inView.length > 0 && (
              <>
                {view.study === null && <Row icon={Flame} label={t(placementKeys.demand, "Demand")} hint={t(placementKeys.pickTrade, "pick a trade")} />}
                {featured.map(heatRow)}
                {rest.length > 0 && (
                  <button type="button" onClick={() => setMore(!more)} className="flex h-(--control-h-sm) items-center gap-2 rounded-control px-2 text-ink-soft hover:bg-hover hover:text-ink">
                    <ChevronRight className={cn("size-4 transition-transform", more && "rotate-90")} />
                    {t(placementKeys.more, "More")}
                  </button>
                )}
                {more && rest.map(heatRow)}
              </>
            )}
          </Section>
          <Section title={t(placementKeys.overlays, "Overlays")}>
            <Row
              icon={Waves}
              label={t(placementKeys.pressure, "Competitor pressure")}
              hint={view.study === null ? t(placementKeys.pickTrade, "pick a trade") : undefined}
              on={view.pressure !== null}
              onToggle={view.study === null ? undefined : () => setMapView({ pressure: view.pressure === null ? {} : null })}
            />
            {view.study !== null && view.pressure !== null && ok.lambda !== null && <Radius view={view} fallback={ok.lambda} />}
            <Row
              icon={Store}
              label={t(placementKeys.competitors, "Competitors")}
              hint={view.study === null ? t(placementKeys.pickTrade, "pick a trade") : undefined}
              on={view.competitors}
              onToggle={view.study === null ? undefined : () => setMapView({ competitors: !view.competitors })}
            />
            <Row icon={MapPin} label={t(hotbar.places, "Google's places")} on={places} onToggle={() => showPlaces(!places)} />
          </Section>
          {ok.notices.length > 0 && (
            <Section title={t(placementKeys.notices, "Data")}>
              {ok.notices.map((n) => (
                <div key={`${n.trade}×${n.location}`} className="flex flex-col gap-1 px-2 py-1">
                  <span className="text-ink-mid">{n.text}</span>
                  {n.missing !== null && may(me, "sa:placement:study:spend") && (
                    <Button size="sm" variant="secondary" disabled={ok.pulling} onClick={() => void pull()}>
                      {t(placementKeys.pull, "Pull latest · {n} searches", { n: n.missing })}
                    </Button>
                  )}
                </div>
              ))}
              {ok.pullFailed !== null && <Note tone="error">{ok.pullFailed}</Note>}
            </Section>
          )}
        </>
      )}
      <Section title={t(placementKeys.base, "Base")}>
        <ToggleGroup type="single" size="sm" value={base} onValueChange={(v) => typeof v === "string" && v !== "" && setBase(v as Base)} className="px-2">
          <ToggleGroupItem value="schematic">{t(placementKeys.base.schematic, "Schematic")}</ToggleGroupItem>
          <ToggleGroupItem value="satellite">{t(placementKeys.base.satellite, "Satellite")}</ToggleGroupItem>
        </ToggleGroup>
      </Section>
      {ok?.legend?.heat && <p className="px-3 pt-2 pb-1 text-caption text-ink-soft">{ok.legend.heat.note}</p>}
    </div>
  );
}

/** Placement's trades as the picker lists them: by the categories that study them, those in the outline's scope first, then a trade no category links under its own name. */
function useStudies(stems: readonly string[]): { key: string; label: string; study: string; group: "scope" | "category" | "stem" }[] {
  const trades = useTrades();
  const { categories: scope } = useScope();
  const linked = (trades.status === "ok" ? trades.data : []).flatMap((tr) =>
    tr.categories.flatMap((c) =>
      c.study !== null && stems.includes(c.study)
        ? [{ key: `${tr.id}.${c.country}`, label: `${tr.name} · ${c.country}`, study: c.study, group: scope !== null && scope.some((p) => covers(p, tr.id, c.country)) ? ("scope" as const) : ("category" as const) }]
        : [],
    ),
  );
  return [...linked, ...stems.filter((s) => !linked.some((l) => l.study === s)).map((s) => ({ key: `study:${s}`, label: s, study: s, group: "stem" as const }))];
}

/** One line on the hotbar item: what the map is drawing. */
export function MapPreview() {
  const t = useT();
  const placement = usePlacement();
  const view = useMapView();
  if (placement.status !== "ok") return placement.status === "unconnected" ? t(placementKeys.unconnected, "Placement not connected") : t(placementKeys.title, "Map");
  const heat = placement.layers.find((l) => l.id === view.heat)?.name ?? t(placementKeys.noHeat, "No heat");
  const where = placement.inView === "zoom" ? t(placementKeys.zoomIn, "Zoom in to see the grid") : placement.inView.length === 0 ? t(placementKeys.noGrid, "No grid here") : placement.inView.join(", ");
  return [heat, view.study === null ? where : `${view.study} × ${where}`, view.pressure !== null && view.study !== null && t(placementKeys.pressureOn, "pressure on")].filter(Boolean).join(" · ");
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

function Row({ icon: Icon, label, hint, on, onToggle }: { icon: ComponentType<{ className?: string }>; label: string; hint?: string | undefined; on?: boolean; onToggle?: (() => void) | undefined }) {
  const Shown = on ? Eye : EyeOff;
  return (
    <button
      type="button"
      disabled={onToggle === undefined}
      aria-pressed={on}
      onClick={onToggle}
      className={cn("flex h-(--control-h-sm) items-center gap-2 rounded-control px-2 text-left text-ink-mid enabled:hover:bg-hover enabled:hover:text-ink disabled:opacity-60", on && "text-ink")}
    >
      <Icon className="size-4 shrink-0" />
      <span className="flex-1 truncate">{label}</span>
      {hint !== undefined && <span className="text-caption text-ink-soft">{hint}</span>}
      {onToggle !== undefined && <Shown className={cn("size-4 shrink-0", !on && "text-ink-soft")} />}
    </button>
  );
}

function Note({ children, tone }: { children: ReactNode; tone?: "error" }) {
  return <p className={cn("px-3 py-1.5 text-ink-soft", tone === "error" && "text-accent-error")}>{children}</p>;
}
