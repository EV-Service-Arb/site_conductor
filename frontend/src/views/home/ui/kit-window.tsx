"use client";

import { Button, Command, CommandInput, CommandItem, CommandList, Input, Skeleton, ToggleGroup, ToggleGroupItem, buttonVariants, toast } from "@evinvest/uikit";
import { Copy, Crosshair, ExternalLink, MapPin, Plus, QrCode, X } from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";

import {
  type KitArea,
  type KitNearby,
  type LatLng,
  type Locality,
  type PutKitArea,
  type Suggestion,
  TARGET_NAME,
  fetchAreaGmbs,
  fetchAreas,
  fetchCity,
  isSlug,
  kit,
  localityAt,
  open,
  openKit,
  putArea,
  slugOf,
  suggestCities,
  windowKeys,
} from "@/entities/gmb";
import { ApiError } from "@/shared/api";
import { useLocale, useT } from "@/shared/i18n";
import { formatDistance } from "@/shared/lib/format";
import { useResource } from "@/shared/lib/use-resource";
import { ErrorState } from "@/shared/ui/error-state";
import { failureText } from "@/shared/ui/failure-text";
import { notifyFailure } from "@/shared/ui/notify";
import { Qr } from "@/shared/ui/qr";
import { useButtonSize, useControlSize } from "@/shared/ui/touch";

const PAUSE_MS = 350;
const MIN_INPUT = 2;
const MARGINS = [0, 1000, 2000, 5000] as const;
const RADII = [5000, 10000, 20000] as const;
const SECTION_HEAD = "flex items-baseline gap-2 text-xs font-semibold tracking-wide text-ink-soft uppercase";
const M_PER_DEG = 111_320;

type Outline = ({ kind: "city"; viewport: { south: number; west: number; north: number; east: number } } | { kind: "circle"; center: LatLng; radius_m: number }) & { margin_m: number };

/** The review kit's areas (`?kit`), over the map's left so the area stays in sight: `""` draws one, a slug shows that one. */
export function KitWindow({ map, area }: { map: google.maps.Map; area: string }) {
  const t = useT();
  const button = useButtonSize();
  return (
    // the map stays clickable beside it and below it
    <div className="pointer-events-none absolute inset-4 flex items-start">
    <section aria-label={t(kit.window, "Review kit")} className="pointer-events-auto flex max-h-full w-full flex-col overflow-y-auto rounded-lg border border-border bg-background shadow-overlay md:w-104">
      <header className="flex items-center gap-2 border-b border-border px-4 py-3">
        <QrCode aria-hidden className="size-4 text-ink-soft" />
        <h2 className="flex-1 text-sm font-semibold text-ink">{area === "" ? t(kit.area.new, "Review kit · a new area") : t(kit.window, "Review kit")}</h2>
        <Button variant="ghost" size={button("sm")} icon aria-label={t(windowKeys.close, "Close the window")} title={t(windowKeys.close, "Close the window")} onClick={() => openKit(null)}>
          <X aria-hidden />
        </Button>
      </header>
      {area === "" ? <Draw map={map} /> : <Area key={area} map={map} slug={area} />}
    </section>
    </div>
  );
}

/** `outline` on `map` while mounted, the map fitted to it once per change. */
function useOutline(map: google.maps.Map, outline: Outline | null) {
  const key = JSON.stringify(outline);
  useEffect(() => {
    if (outline === null) return;
    const line = { strokeColor: "#ffffff", clickable: false };
    // the window's width beside it from md up (left-4 + w-104), and a margin
    const padding = { top: 48, right: 48, bottom: 48, left: window.matchMedia("(min-width: 48rem)").matches ? 16 + 416 + 48 : 48 };
    const drawn: (google.maps.Rectangle | google.maps.Circle)[] = [];
    if (outline.kind === "city") {
      const v = outline.viewport;
      const dLat = outline.margin_m / M_PER_DEG;
      const dLng = outline.margin_m / (M_PER_DEG * Math.cos((((v.north + v.south) / 2) * Math.PI) / 180));
      const outer = { south: v.south - dLat, north: v.north + dLat, west: v.west - dLng, east: v.east + dLng };
      drawn.push(new google.maps.Rectangle({ map, bounds: v, ...line, strokeWeight: 2, fillColor: "#ffffff", fillOpacity: 0.06 }));
      if (outline.margin_m > 0) drawn.push(new google.maps.Rectangle({ map, bounds: outer, ...line, strokeWeight: 1, strokeOpacity: 0.8, fillOpacity: 0 }));
      map.fitBounds(outer, padding);
    } else {
      const inner = new google.maps.Circle({ map, center: outline.center, radius: outline.radius_m, ...line, strokeWeight: 2, fillColor: "#ffffff", fillOpacity: 0.06 });
      drawn.push(inner);
      const outer = new google.maps.Circle({ map, center: outline.center, radius: outline.radius_m + outline.margin_m, ...line, strokeWeight: 1, strokeOpacity: outline.margin_m > 0 ? 0.8 : 0, fillOpacity: 0 });
      drawn.push(outer);
      const bounds = outer.getBounds();
      if (bounds === null) throw new Error("a circle on the map has no bounds");
      map.fitBounds(bounds, padding);
    }
    return () => {
      for (const d of drawn) d.setMap(null);
    };
    // `key` is `outline`, by value
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [map, key]);
}

type Picked = { kind: "city"; locality: Locality } | { kind: "point"; at: LatLng; locality: Locality | null };

/** By a city searched for, or a point clicked on the map: its city, or a circle around it. */
function Draw({ map }: { map: google.maps.Map }) {
  const t = useT();
  const locale = useLocale();
  const size = useControlSize();
  const button = useButtonSize();
  const [picked, setPicked] = useState<Picked | null>(null);
  const [asCircle, setAsCircle] = useState(false);
  const [margin, setMargin] = useState(2000);
  const [radius, setRadius] = useState(10000);
  const [named, setNamed] = useState<string | null>(null);
  const [slugTyped, setSlugTyped] = useState<string | null>(null);
  const [refused, setRefused] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    const click = map.addListener("click", (e: google.maps.MapMouseEvent) => {
      if (e.latLng === null) return; // a click Google gives no point (off the world)
      const at = e.latLng.toJSON();
      localityAt(at).then(
        (locality) => {
          setPicked({ kind: "point", at, locality });
          setAsCircle(locality === null);
          setNamed(null);
          setSlugTyped(null);
          setRefused(null);
        },
        (err: unknown) => notifyFailure(err, t),
      );
    });
    map.setOptions({ draggableCursor: "crosshair" });
    return () => {
      click.remove();
      map.setOptions({ draggableCursor: null });
    };
  }, [map, t]);

  const city = picked === null ? null : picked.kind === "city" ? picked.locality : asCircle ? null : picked.locality;
  const circleAt = picked?.kind === "point" && city === null ? picked.at : null;
  const name = named ?? (picked?.locality?.name ?? "");
  const slug = slugTyped ?? (city !== null ? slugOf(city.name) : name === "" ? "" : `${slugOf(name)}-${radius / 1000}km`);
  const outline: Outline | null = city !== null ? { kind: "city", viewport: city.viewport, margin_m: margin } : circleAt !== null ? { kind: "circle", center: circleAt, radius_m: radius, margin_m: margin } : null;
  useOutline(map, outline);
  const body: PutKitArea | null = city !== null ? { kind: "city", place_id: city.place_id, margin_m: margin } : circleAt !== null && name.trim() !== "" ? { kind: "circle", name: name.trim(), lat: circleAt.lat, lng: circleAt.lng, radius_m: radius, margin_m: margin } : null;

  const save = async () => {
    if (body === null) throw new Error("saved with nothing drawn");
    setBusy(true);
    try {
      const saved = await putArea(slug, body);
      openKit(saved.slug);
    } catch (e) {
      if (!(e instanceof ApiError)) throw e;
      if (e.failure.kind !== "unauthenticated") setRefused(failureText(e.failure, t));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex flex-col gap-4 p-4">
      <CitySearch
        onPick={(placeId) =>
          void fetchCity(placeId).then(
            (locality) => {
              setPicked({ kind: "city", locality });
              setNamed(null);
              setSlugTyped(null);
              setRefused(null);
            },
            (e: unknown) => notifyFailure(e, t),
          )
        }
      />
      {picked?.kind === "city" && (
        <p className="flex items-center gap-2 text-sm text-ink">
          <MapPin aria-hidden className="size-4 text-ink-soft" />
          {picked.locality.name}
        </p>
      )}
      {picked?.kind === "point" ? (
        <div className="flex flex-col gap-2">
          <p className="flex items-center gap-2 text-sm text-ink">
            <Crosshair aria-hidden className="size-4 text-ink-soft" />
            {t(kit.area.picked, "Picked on the map")}
            <span className="font-mono text-xs text-ink-soft tabular-nums">
              {picked.at.lat.toFixed(4)}, {picked.at.lng.toFixed(4)}
            </span>
          </p>
          {picked.locality !== null ? (
            <ToggleGroup type="single" variant="outline" size="sm" className="w-full" value={asCircle ? "circle" : "city"} onValueChange={(v) => (v === "city" || v === "circle") && (setAsCircle(v === "circle"), setSlugTyped(null))}>
              <ToggleGroupItem value="city" className="flex-1">
                {t(kit.area.itsCity, "Its city · {name}", { name: picked.locality.name })}
              </ToggleGroupItem>
              <ToggleGroupItem value="circle" className="flex-1">
                {t(kit.area.circle, "A circle around the point")}
              </ToggleGroupItem>
            </ToggleGroup>
          ) : (
            <p className="text-xs text-ink-soft">{t(kit.area.noCity, "No city there: a circle around the point.")}</p>
          )}
        </div>
      ) : (
        <p className="flex items-center gap-2 text-xs text-ink-soft">
          <Crosshair aria-hidden className="size-3.5" />
          {t(kit.area.orClick, "or click the map: its city, else a circle around the point")}
        </p>
      )}

      {circleAt !== null && (
        <Row label={t(kit.area.radius, "radius")}>
          <Presets values={RADII} value={radius} onPick={(r) => (setRadius(r), setSlugTyped(null))} locale={locale} />
          <Input size={size} type="number" min={0.1} step={0.1} value={radius / 1000} onChange={(e) => Number(e.target.value) > 0 && (setRadius(Math.round(Number(e.target.value) * 1000)), setSlugTyped(null))} aria-label={t(kit.area.radiusKm, "Radius, km")} className="w-20 tabular-nums" />
        </Row>
      )}
      {outline !== null && (
        <Row label={t(kit.area.margin, "and next to it, within")}>
          <Presets values={MARGINS} value={margin} onPick={setMargin} locale={locale} />
        </Row>
      )}
      {circleAt !== null && (
        <Row label={t(kit.area.name, "its name")}>
          <Input size={size} value={name} onChange={(e) => (setNamed(e.target.value), setSlugTyped(null))} placeholder={t(kit.area.namePlaceholder, "Chassieu")} aria-label={t(kit.area.name, "its name")} className="min-w-0 flex-1" />
        </Row>
      )}
      {outline !== null && (
        <Row label={t(kit.area.link, "its link")}>
          <span className="flex min-w-0 flex-1 items-center gap-1 font-mono text-sm">
            /
            <Input size={size} value={slug} onChange={(e) => (setSlugTyped(e.target.value.toLowerCase()), setRefused(null))} aria-label={t(kit.area.link, "its link")} aria-invalid={slug !== "" && !isSlug(slug)} className="min-w-0 flex-1 font-mono" />/
          </span>
        </Row>
      )}
      {slug !== "" && !isSlug(slug) && <p className="text-xs text-accent-error">{t(kit.area.badSlug, "Lowercase letters, digits, - and _; up to 64.")}</p>}
      {refused !== null && <p className="text-xs text-accent-error">{refused}</p>}
      {outline !== null && (
        <Button size={button("sm")} className="self-end" disabled={busy || body === null || !isSlug(slug)} onClick={() => void save()}>
          {t(kit.area.save, "Save the area")}
        </Button>
      )}
      <Areas />
    </div>
  );
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex flex-wrap items-center gap-2 text-sm text-ink-soft">
      <span className="w-28 shrink-0">{label}</span>
      {children}
    </div>
  );
}

function Presets({ values, value, onPick, locale }: { values: readonly number[]; value: number; onPick: (v: number) => void; locale: string }) {
  return (
    <ToggleGroup type="single" variant="outline" size="sm" value={String(value)} onValueChange={(v) => typeof v === "string" && v !== "" && onPick(Number(v))}>
      {values.map((v) => (
        <ToggleGroupItem key={v} value={String(v)} className="tabular-nums">
          {v === 0 ? "0" : formatDistance(v, locale)}
        </ToggleGroupItem>
      ))}
    </ToggleGroup>
  );
}

/** Google's cities and towns as one types. */
function CitySearch({ onPick }: { onPick: (placeId: string) => void }) {
  const t = useT();
  const [input, setInput] = useState("");
  const [answered, setAnswered] = useState<{ q: string; found: Suggestion[] } | null>(null);
  const q = input.trim();
  const found = answered !== null && answered.q === q ? answered.found : null;
  useEffect(() => {
    if (q.length < MIN_INPUT) return;
    let live = true;
    const timer = setTimeout(() => {
      suggestCities(q).then(
        (found) => live && setAnswered({ q, found }),
        (e: unknown) => live && notifyFailure(e, t),
      );
    }, PAUSE_MS);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [q, t]);
  return (
    <Command shouldFilter={false} search={input} onSearchChange={setInput} className="h-auto overflow-visible rounded-md border border-border">
      <CommandInput placeholder={t(kit.area.search, "City or town")} autoCapitalize="none" />
      {found !== null && (
        <CommandList>
          {found.length === 0 && <p className="px-3 py-2 text-sm text-ink-soft">{t(kit.area.noneFound, "No city found.")}</p>}
          {found.map((s) => (
            <CommandItem
              key={s.place_id}
              value={s.place_id}
              onSelect={() => {
                onPick(s.place_id);
                setInput("");
              }}
            >
              <MapPin aria-hidden className="text-ink-soft" />
              <span className="flex min-w-0 flex-col">
                <span className="truncate text-sm text-ink">{s.main}</span>
                {s.secondary !== null && <span className="truncate text-xs text-ink-soft">{s.secondary}</span>}
              </span>
            </CommandItem>
          ))}
        </CommandList>
      )}
    </Command>
  );
}

/** Every area drawn, anyone's: areas are geography. */
function Areas() {
  const t = useT();
  const areas = useResource("kit.areas", fetchAreas);
  if (areas.status !== "ok" || areas.data.length === 0) return null;
  return (
    <section className="flex flex-col gap-2 border-t border-border pt-3">
      <h3 className={SECTION_HEAD}>{t(kit.areas, "Areas")}</h3>
      <span className="flex flex-wrap gap-1.5">
        {areas.data.map((a) => (
          <button key={a.slug} type="button" onClick={() => openKit(a.slug)} className="flex items-center gap-1.5 rounded-md bg-hover px-2 py-1 text-xs text-ink outline-none hover:bg-hover/70 focus-visible:ring-2 focus-visible:ring-ring">
            {a.name}
            <span className="font-mono text-ink-soft">/{a.slug}/</span>
          </button>
        ))}
      </span>
    </section>
  );
}

function Area({ map, slug }: { map: google.maps.Map; slug: string }) {
  const t = useT();
  const locale = useLocale();
  const button = useButtonSize();
  const res = useResource(`kit.area.${slug}`, () => fetchAreaGmbs(slug), `kit.area.${slug}`, { focus: true });
  const area = res.status === "ok" ? res.data.area : null;
  useOutline(map, area === null ? null : outlineOf(area));
  if (res.status === "loading") return <Skeleton className="m-4 h-64" />;
  if (res.status === "error")
    return (
      <div className="flex flex-col gap-3 p-4">
        {res.failure.kind === "not_found" ? <p className="text-sm text-ink-soft">{t(kit.area.notFound, "No area is at /{slug}/.", { slug })}</p> : <ErrorState failure={res.failure} onRetry={res.reload} />}
        <NewArea />
      </div>
    );
  const { gmbs } = res.data;
  const a = res.data.area;
  const link = `${window.location.origin}/${a.slug}/`;
  const inside = gmbs.filter((g) => g.inside);
  const near = gmbs.filter((g) => !g.inside);
  const shape = a.shape.kind === "city" ? t(kit.area.isCity, "the city") : t(kit.area.isCircle, "a circle of {radius}", { radius: formatDistance(a.shape.radius_m, locale) });
  return (
    <div className="flex flex-col gap-4 p-4">
      <div className="flex flex-col gap-0.5">
        <p className="text-lg font-semibold text-ink">{a.name}</p>
        <p className="text-sm text-ink-soft">{a.margin_m > 0 ? t(kit.area.summary, "{shape}, and {margin} around it", { shape, margin: formatDistance(a.margin_m, locale) }) : shape}</p>
      </div>
      <Group title={t(kit.area.inIt, "In it")} gmbs={inside} locale={locale} />
      <Group title={t(kit.area.nextToIt, "Next to it")} gmbs={near} locale={locale} />
      {gmbs.length === 0 && <p className="text-sm text-ink-soft">{t(kit.area.noGbps, "None of your GBPs with a point is in it or next to it.")}</p>}
      <div className="flex items-center gap-3 rounded-lg border border-border p-3">
        <Qr text={link} className="size-24 shrink-0 rounded p-1" />
        <div className="flex min-w-0 flex-col gap-1">
          <p className="text-sm font-medium text-ink">{t(kit.area.kit, "Review kit · {n} places", { n: inside.length + near.length })}</p>
          <p className="truncate font-mono text-xs text-ink-soft">{link}</p>
          <p className="text-xs text-ink-soft">{t(kit.area.scan, "Scan with your phone to take it out; signed-in only.")}</p>
        </div>
      </div>
      <div className="flex flex-wrap gap-2">
        <a href={`/${a.slug}/`} target="_blank" rel="noreferrer" className={buttonVariants({ size: button("sm") })}>
          <ExternalLink aria-hidden />
          {t(kit.area.open, "Open /{slug}/", { slug: a.slug })}
        </a>
        <Button
          variant="outline"
          size={button("sm")}
          onClick={() =>
            void navigator.clipboard.writeText(link).then(
              () => toast.positive(t(kit.area.copied, "Link copied")),
              (e: unknown) => notifyFailure(e, t),
            )
          }
        >
          <Copy aria-hidden />
          {t(kit.area.copy, "Copy link")}
        </Button>
        <span className="flex-1" />
        <NewArea />
      </div>
    </div>
  );
}

function NewArea() {
  const t = useT();
  const button = useButtonSize();
  return (
    <Button variant="ghost" size={button("sm")} onClick={() => openKit("")}>
      <Plus aria-hidden />
      {t(kit.area.another, "Another area")}
    </Button>
  );
}

const outlineOf = (a: KitArea): Outline => (a.shape.kind === "city" ? { kind: "city", viewport: a.shape.viewport, margin_m: a.margin_m } : { kind: "circle", center: a.center, radius_m: a.shape.radius_m, margin_m: a.margin_m });

function Group({ title, gmbs, locale }: { title: string; gmbs: readonly KitNearby[]; locale: string }) {
  const t = useT();
  if (gmbs.length === 0) return null;
  return (
    <section className="flex flex-col gap-1">
      <h3 className={SECTION_HEAD}>
        {title} <span className="font-normal tabular-nums">{gmbs.length}</span>
      </h3>
      <ul className="flex flex-col">
        {gmbs.map((g) => (
          <li key={g.id}>
            <button type="button" onClick={() => open(g)} className="flex w-full items-center gap-3 rounded-md px-2 py-1.5 text-left outline-none hover:bg-hover focus-visible:ring-2 focus-visible:ring-ring">
              <span className="flex min-w-0 flex-1 flex-col">
                <span className="truncate text-sm text-ink">{g.name}</span>
                <span className="truncate text-xs text-ink-soft">{[g.address, g.target === null ? t(kit.off, "kit off") : TARGET_NAME[g.target](t)].filter(Boolean).join(" · ")}</span>
              </span>
              <span className="font-mono text-xs text-ink-soft tabular-nums">{formatDistance(g.distance_m, locale)}</span>
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}
