"use client";

import { useResource } from "@evinvest/api/react";
import { Button, Command, CommandInput, CommandItem, CommandList, Input, Skeleton, ToggleGroup, ToggleGroupItem, cn } from "@evinvest/uikit";
import { Crosshair, MapPin, Search, X } from "lucide-react";
import { type ReactNode, useCallback, useEffect, useRef, useState } from "react";

import { api, ok } from "@/shared/api";
import type { Area, LatLng, Locality, Suggestion } from "@/shared/api/schema";
import { useLocale, useT } from "@/shared/i18n";
import { formatDistance } from "@/shared/lib/format";
import { distanceM, pin } from "@/shared/map";
import { ErrorState } from "@/shared/ui/error-state";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize, useControlSize } from "@/shared/ui/touch";

import { localityAt } from "../api/areas";
import { isSlug, slugOf } from "../model/areas";
import { areaKeys } from "../model/i18n";

const PAUSE_MS = 350;
const MIN_INPUT = 2;
const RADII = [1000, 2000, 5000, 10000, 20000];
const FIRST_RADIUS = 5000;
const M_PER_DEG = 111_320;
const EARTH_M = 6_371_008.8;
const RING_POINTS = 120;
const SLUG_MAX = 64;

export type Circle = { center: LatLng; radius_m: number };

/** What is drawn: the circle; its city while the centre is in the city's circle; once named, the area it is saved as (`saved`: an equal one holds that link already). */
export type Drawn = Circle & { city: Locality | null; area: { name: string; slug: string; saved: boolean } | null };

type City = { locality: Locality; secondary: string | null };
type Seed = { center: LatLng; radius_m: number; city: City | null; name: string };

/** The city's circle: half its viewport tall, to the half km. */
const radiusOf = (l: Locality) => Math.max(500, Math.round(((l.viewport.north - l.viewport.south) * M_PER_DEG) / 2 / 500) * 500);

/** Km as a name carries them, the same in every locale: a name is its link's source. */
const km = (m: number) => `${Number((m / 1000).toFixed(1))} km`;

/** Points around `c` at `r` metres, closed. */
function ring(c: LatLng, r: number): LatLng[] {
  const d = r / EARTH_M;
  const [lat, lng] = [(c.lat * Math.PI) / 180, (c.lng * Math.PI) / 180];
  return Array.from({ length: RING_POINTS + 1 }, (_, i) => {
    const a = (2 * Math.PI * i) / RING_POINTS;
    const lat2 = Math.asin(Math.sin(lat) * Math.cos(d) + Math.cos(lat) * Math.sin(d) * Math.cos(a));
    const lng2 = lng + Math.atan2(Math.sin(a) * Math.sin(d) * Math.cos(lat), Math.cos(d) - Math.sin(lat) * Math.sin(lat2));
    return { lat: (lat2 * 180) / Math.PI, lng: (lng2 * 180) / Math.PI };
  });
}

/** The first of `base`, `base-2`, … no area holds, or one holding an equal circle; an area is never drawn over from here. */
function linkOf(base: string, circle: Circle, areas: readonly Area[]): { slug: string; saved: boolean } {
  for (let n = 1; ; n++) {
    const slug = n === 1 ? base : `${base.slice(0, SLUG_MAX - String(n).length - 1)}-${n}`;
    const held = areas.find((a) => a.slug === slug);
    if (held === undefined) return { slug, saved: false };
    if (held.center.lat === circle.center.lat && held.center.lng === circle.center.lng && held.radius_m === circle.radius_m) return { slug, saved: true };
  }
}

/**
 * An area drawn on `map`: a city searched for or clicked in (centred on it), else a circle around a point clicked; its centre dragged anywhere, its radius picked.
 * Named `<city> × <radius>` while the centre is in the city's circle, else by hand. `children` get what is drawn, and `reread` for after it is saved.
 */
export function AreaDraw({ map, left, initial, children }: { map: google.maps.Map; left: number; initial: Area | null; children: (drawn: Drawn | null, reread: () => void) => ReactNode }) {
  const areas = useResource(api, "/areas");
  if (areas.status === "loading") return <Skeleton className="m-4 h-9" />;
  if (areas.status === "error")
    return (
      <div className="p-4">
        <ErrorState failure={areas.failure} onRetry={areas.reload} />
      </div>
    );
  const draw = (seed: Seed | null) => <Drawing map={map} left={left} seed={seed} areas={areas.data.areas} reread={areas.reload} render={children} />;
  if (initial === null) return draw(null);
  if (initial.city === null) return draw({ center: initial.center, radius_m: initial.radius_m, city: null, name: initial.name });
  return <SeededCity placeId={initial.city} initial={initial} draw={draw} />;
}

/** An area drawn from a city: the city read first, for its circle. */
function SeededCity({ placeId, initial, draw }: { placeId: string; initial: Area; draw: (seed: Seed) => ReactNode }) {
  const city = useResource(api, "/kit/cities/{place_id}", { params: { path: { place_id: placeId } } });
  if (city.status === "loading") return <Skeleton className="m-4 h-9" />;
  if (city.status === "error")
    return (
      <div className="p-4">
        <ErrorState failure={city.failure} onRetry={city.reload} />
      </div>
    );
  return draw({ center: initial.center, radius_m: initial.radius_m, city: { locality: city.data, secondary: city.data.country }, name: "" });
}

function Drawing({ map, left, seed, areas, reread, render }: { map: google.maps.Map; left: number; seed: Seed | null; areas: readonly Area[]; reread: () => void; render: (drawn: Drawn | null, reread: () => void) => ReactNode }) {
  const t = useT();
  const locale = useLocale();
  const size = useControlSize();
  const button = useButtonSize();
  const [center, setCenter] = useState<LatLng | null>(seed?.center ?? null);
  const [city, setCity] = useState<City | null>(seed?.city ?? null);
  const [radius, setRadius] = useState(seed?.radius_m ?? FIRST_RADIUS);
  const [custom, setCustom] = useState(!RADII.includes(radius));
  const [named, setNamed] = useState(seed?.name ?? "");
  // bumped as a base is picked: the map is fitted to the circle again, not while its centre is dragged
  const [framed, setFramed] = useState(0);

  const pickCity = useCallback((locality: Locality, secondary: string | null) => {
    const r = radiusOf(locality);
    setCity({ locality, secondary });
    setCenter(locality.location);
    setRadius(r);
    setCustom(!RADII.includes(r));
    setFramed((f) => f + 1);
  }, []);

  useEffect(() => {
    const click = map.addListener("click", (e: google.maps.MapMouseEvent) => {
      if (e.latLng === null) return; // a click Google gives no point (off the world)
      const at = e.latLng.toJSON();
      localityAt(at).then(
        (locality) => {
          if (locality !== null) return pickCity(locality, locality.country);
          setCity(null);
          setCenter(at);
          setFramed((f) => f + 1);
        },
        (err: unknown) => notifyFailure(err, t),
      );
    });
    map.setOptions({ draggableCursor: "crosshair" });
    return () => {
      click.remove();
      map.setOptions({ draggableCursor: null });
    };
  }, [map, pickCity, t]);

  const placed = center !== null;
  const circle = useRef<google.maps.Circle | null>(null);
  useEffect(() => {
    if (!placed) return;
    const drawn = new google.maps.Circle({ map, strokeColor: "#ffffff", strokeWeight: 2, fillColor: "#ffffff", fillOpacity: 0.08, clickable: false });
    circle.current = drawn;
    return () => {
      circle.current = null;
      drawn.setMap(null);
    };
  }, [map, placed]);
  useEffect(() => {
    if (center !== null) circle.current?.setOptions({ center, radius });
  }, [center, radius]);
  useEffect(() => {
    const drawn = circle.current;
    if (drawn === null) return;
    const bounds = drawn.getBounds();
    if (bounds === null) throw new Error("a circle on the map has no bounds");
    map.fitBounds(bounds, { top: 48, right: 48, bottom: 48, left: left + 48 });
  }, [map, placed, framed, radius, left]);

  const centre = useRef<ReturnType<typeof pin> | null>(null);
  useEffect(() => {
    if (center === null) return;
    const dot = document.createElement("div");
    dot.className = "block size-8 rounded-full border-4 border-white bg-primary shadow-lg ring-2 ring-ink active:cursor-grabbing";
    dot.style.zIndex = "3";
    dot.setAttribute("aria-label", t(areaKeys.centre, "The area's centre: drag it"));
    const p = pin(map, dot, center, { onMoving: setCenter, onDrag: setCenter });
    centre.current = p;
    return () => {
      centre.current = null;
      p.remove();
    };
    // the pin follows `center` through `move`: it is rebuilt only when it comes or goes
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [map, placed, t]);
  useEffect(() => {
    if (center !== null) centre.current?.move(center);
  }, [center]);

  const inCity = city !== null && center !== null && distanceM(center, city.locality.location) <= radiusOf(city.locality);
  useEffect(() => {
    if (!inCity) return;
    const edge = new google.maps.Polyline({
      map,
      path: ring(city.locality.location, radiusOf(city.locality)),
      clickable: false,
      strokeOpacity: 0,
      icons: [{ icon: { path: "M 0,-1 0,1", strokeColor: "#ffffff", strokeOpacity: 0.7, scale: 2 }, offset: "0", repeat: "12px" }],
    });
    return () => edge.setMap(null);
    // `city` is its place, by value
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [map, inCity, city?.locality.place_id]);

  const name = inCity ? `${city.locality.name} × ${km(radius)}` : named.trim();
  const base = slugOf(name).slice(0, SLUG_MAX);
  const drawn: Drawn | null =
    center === null
      ? null
      : { center, radius_m: radius, city: inCity ? city.locality : null, area: name === "" || !isSlug(base) ? null : { name, ...linkOf(base, { center, radius_m: radius }, areas) } };

  return (
    <>
      <div className="flex flex-col gap-3 p-4">
        {city !== null ? (
          <div className="flex h-9 items-center gap-2 rounded-md border border-border px-3 text-sm">
            <Search aria-hidden className="size-4 shrink-0 text-ink-soft" />
            <span className="truncate text-ink">{city.locality.name}</span>
            {city.secondary !== null && <span className="min-w-0 flex-1 truncate text-ink-soft">{city.secondary}</span>}
            <Button variant="ghost" size={button("xs")} icon className="ml-auto" aria-label={t(areaKeys.clear, "Pick another")} onClick={() => (setCity(null), setCenter(null))}>
              <X aria-hidden />
            </Button>
          </div>
        ) : (
          <CitySearch
            onPick={(s) =>
              void ok(api.GET("/kit/cities/{place_id}", { params: { path: { place_id: s.place_id } } })).then(
                (locality) => pickCity(locality, s.secondary),
                (e: unknown) => notifyFailure(e, t),
              )
            }
          />
        )}
        {center === null ? (
          <p className="flex items-center gap-2 text-xs text-ink-soft">
            <Crosshair aria-hidden className="size-3.5" />
            {t(areaKeys.orClick, "or click the map: its city, else a circle around the point")}
          </p>
        ) : (
          !inCity && (
            <p className="flex items-center gap-2 text-xs text-ink-soft">
              <Crosshair aria-hidden className="size-3.5 shrink-0" />
              <span className="flex-1">{city === null ? t(areaKeys.aroundPoint, "A circle around the point") : t(areaKeys.leftCity, "The centre is out of {city}: name the area", { city: city.locality.name })}</span>
              <span className="font-mono tabular-nums">
                {center.lat.toFixed(4)}, {center.lng.toFixed(4)}
              </span>
              {city === null && (
                <Button variant="ghost" size={button("xs")} icon aria-label={t(areaKeys.clear, "Pick another")} onClick={() => setCenter(null)}>
                  <X aria-hidden />
                </Button>
              )}
            </p>
          )
        )}
        {center !== null && (
          <div className="flex flex-wrap items-center gap-2 text-xs text-ink-soft">
            <span>{t(areaKeys.radius, "radius")}</span>
            <ToggleGroup type="single" variant="outline" size="sm" value={custom ? "" : String(radius)} onValueChange={(v) => v !== "" && (setRadius(Number(v)), setCustom(false))}>
              {RADII.map((r) => (
                <ToggleGroupItem key={r} value={String(r)} className="tabular-nums">
                  {formatDistance(r, locale)}
                </ToggleGroupItem>
              ))}
            </ToggleGroup>
            {custom ? (
              <Input size={size} type="number" min={0.1} max={100} step={0.1} value={radius / 1000} onChange={(e) => Number(e.target.value) > 0 && setRadius(Math.round(Number(e.target.value) * 1000))} aria-label={t(areaKeys.radiusKm, "Radius, km")} className="w-20 tabular-nums" />
            ) : (
              <Button variant="outline" size={button("sm")} aria-label={t(areaKeys.otherRadius, "Another radius")} onClick={() => setCustom(true)}>
                …
              </Button>
            )}
          </div>
        )}
        {center !== null && !inCity && <Input size={size} value={named} onChange={(e) => setNamed(e.target.value)} placeholder={t(areaKeys.namePlaceholder, "Chassieu")} aria-label={t(areaKeys.name, "Its name")} aria-invalid={named.trim() === "" || !isSlug(base)} />}
        {name !== "" && !isSlug(base) && <p className="text-xs text-accent-error">{t(areaKeys.badName, "A name needs a Latin letter or a digit: its link is made of them.")}</p>}
      </div>
      {render(drawn, reread)}
    </>
  );
}

/** Google's cities and towns as one types. */
function CitySearch({ onPick }: { onPick: (s: Suggestion) => void }) {
  const t = useT();
  const [input, setInput] = useState("");
  const [answered, setAnswered] = useState<{ q: string; found: Suggestion[] } | null>(null);
  const q = input.trim();
  const found = answered !== null && answered.q === q ? answered.found : null;
  useEffect(() => {
    if (q.length < MIN_INPUT) return;
    let live = true;
    const timer = setTimeout(() => {
      ok(api.GET("/kit/cities", { params: { query: { input: q } } })).then(
        ({ suggestions: found }) => live && setAnswered({ q, found }),
        (e: unknown) => live && notifyFailure(e, t),
      );
    }, PAUSE_MS);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [q, t]);
  return (
    <Command shouldFilter={false} search={input} onSearchChange={setInput} className={cn("h-auto overflow-visible rounded-md border border-border")}>
      <CommandInput placeholder={t(areaKeys.search, "City or town")} autoCapitalize="none" />
      {found !== null && (
        <CommandList>
          {found.length === 0 && <p className="px-3 py-2 text-sm text-ink-soft">{t(areaKeys.noneFound, "No city found.")}</p>}
          {found.map((s) => (
            <CommandItem
              key={s.place_id}
              value={s.place_id}
              onSelect={() => {
                onPick(s);
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
