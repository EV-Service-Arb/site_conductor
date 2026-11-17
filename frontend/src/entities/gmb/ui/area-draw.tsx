"use client";

import { Button, Command, CommandInput, CommandItem, CommandList, Input, ToggleGroup, ToggleGroupItem } from "@evinvest/uikit";
import { Crosshair, MapPin } from "lucide-react";
import { type ReactNode, useCallback, useEffect, useRef, useState } from "react";

import { ApiError, api, ok } from "@/shared/api";
import type { Area, LatLng, Locality, PutArea, Suggestion } from "@/shared/api/schema";
import { useLocale, useT } from "@/shared/i18n";
import { formatDistance } from "@/shared/lib/format";
import { pin } from "@/shared/map";
import { failureText } from "@/shared/ui/failure-text";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize, useControlSize } from "@/shared/ui/touch";

import { localityAt } from "../api/areas";
import { isSlug, slugOf } from "../model/areas";
import { areaKeys } from "../model/i18n";

const PAUSE_MS = 350;
const MIN_INPUT = 2;
const RADII = [2000, 5000, 10000, 20000] as const;
const M_PER_DEG = 111_320;

export type Circle = { center: LatLng; radius_m: number };

/** `circle` on `map` while mounted, the map fitted to it once per change; `left` px of the map stay under a window. */
export function useOutline(map: google.maps.Map, circle: Circle | null, left: number) {
  const key = JSON.stringify(circle === null ? null : { center: circle.center, radius_m: circle.radius_m });
  useEffect(() => {
    if (circle === null) return;
    const drawn = new google.maps.Circle({ map, center: circle.center, radius: circle.radius_m, strokeColor: "#ffffff", strokeWeight: 2, fillColor: "#ffffff", fillOpacity: 0.06, clickable: false });
    const bounds = drawn.getBounds();
    if (bounds === null) throw new Error("a circle on the map has no bounds");
    map.fitBounds(bounds, { top: 48, right: 48, bottom: 48, left: left + 48 });
    return () => drawn.setMap(null);
    // `key` is `circle`, by value
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [map, key, left]);
}

/** Half the city's viewport tall, to the half km: a first circle that roughly holds it. */
const radiusOf = (l: Locality) => Math.max(500, Math.round(((l.viewport.north - l.viewport.south) * M_PER_DEG) / 2 / 500) * 500);

/** A new area, a circle around a point: a city searched for (its centre), or a point clicked on `map`; its centre dragged anywhere after. Saved under its link, then `onSaved`. */
export function AreaDraw({ map, left, onSaved }: { map: google.maps.Map; left: number; onSaved: (area: Area) => void }) {
  const t = useT();
  const locale = useLocale();
  const size = useControlSize();
  const button = useButtonSize();
  const [center, setCenter] = useState<LatLng | null>(null);
  const [city, setCity] = useState<Locality | null>(null);
  const [radius, setRadius] = useState(5000);
  const [named, setNamed] = useState<string | null>(null);
  const [slugTyped, setSlugTyped] = useState<string | null>(null);
  const [refused, setRefused] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const pick = useCallback((at: LatLng, locality: Locality | null) => {
    setCenter(at);
    setCity(locality);
    setNamed(null);
    setSlugTyped(null);
    setRefused(null);
  }, []);

  useEffect(() => {
    const click = map.addListener("click", (e: google.maps.MapMouseEvent) => {
      if (e.latLng === null) return; // a click Google gives no point (off the world)
      const at = e.latLng.toJSON();
      localityAt(at).then(
        (locality) => pick(at, locality),
        (err: unknown) => notifyFailure(err, t),
      );
    });
    map.setOptions({ draggableCursor: "crosshair" });
    return () => {
      click.remove();
      map.setOptions({ draggableCursor: null });
    };
  }, [map, pick, t]);

  const centre = useRef<ReturnType<typeof pin> | null>(null);
  const placed = center !== null;
  useEffect(() => {
    if (center === null) return;
    const dot = document.createElement("div");
    dot.className = "block size-6 rounded-full border-4 border-white bg-primary shadow-lg ring-2 ring-ink";
    dot.style.zIndex = "3";
    dot.setAttribute("aria-label", t(areaKeys.centre, "The area's centre: drag it"));
    const p = pin(map, dot, center, { onDrag: (to) => (setCenter(to), setRefused(null)) });
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

  useOutline(map, center === null ? null : { center, radius_m: radius }, left);
  const name = named ?? city?.name ?? "";
  const slug = slugTyped ?? (name === "" ? "" : city !== null ? slugOf(name) : `${slugOf(name)}-${radius / 1000}km`);
  const body: PutArea | null = center !== null && name.trim() !== "" ? { name: name.trim(), center, radius_m: radius, city: city?.place_id ?? null } : null;

  const save = async () => {
    if (body === null) throw new Error("saved with nothing drawn");
    setBusy(true);
    try {
      onSaved(await ok(api.PUT("/areas/{slug}", { params: { path: { slug } }, body })));
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
          void ok(api.GET("/kit/cities/{place_id}", { params: { path: { place_id: placeId } } })).then(
            (locality) => {
              pick(locality.location, locality);
              setRadius(radiusOf(locality));
            },
            (e: unknown) => notifyFailure(e, t),
          )
        }
      />
      {center === null ? (
        <p className="flex items-center gap-2 text-xs text-ink-soft">
          <Crosshair aria-hidden className="size-3.5" />
          {t(areaKeys.orClick, "or click the map; drag the centre after")}
        </p>
      ) : (
        <p className="flex items-center gap-2 text-sm text-ink">
          {city !== null ? <MapPin aria-hidden className="size-4 text-ink-soft" /> : <Crosshair aria-hidden className="size-4 text-ink-soft" />}
          {city !== null && `${city.name} · ${city.country}`}
          <span className="font-mono text-xs text-ink-soft tabular-nums">
            {center.lat.toFixed(4)}, {center.lng.toFixed(4)}
          </span>
        </p>
      )}

      {center !== null && (
        <>
          <Row label={t(areaKeys.radius, "radius")}>
            <Presets values={RADII} value={radius} onPick={(r) => (setRadius(r), setSlugTyped(null))} locale={locale} />
            <Input size={size} type="number" min={0.1} step={0.1} value={radius / 1000} onChange={(e) => Number(e.target.value) > 0 && (setRadius(Math.round(Number(e.target.value) * 1000)), setSlugTyped(null))} aria-label={t(areaKeys.radiusKm, "Radius, km")} className="w-20 tabular-nums" />
          </Row>
          <Row label={t(areaKeys.name, "its name")}>
            <Input size={size} value={name} onChange={(e) => (setNamed(e.target.value), setSlugTyped(null))} placeholder={t(areaKeys.namePlaceholder, "Chassieu")} aria-label={t(areaKeys.name, "its name")} className="min-w-0 flex-1" />
          </Row>
          <Row label={t(areaKeys.link, "its link")}>
            <span className="flex min-w-0 flex-1 items-center gap-1 font-mono text-sm">
              /
              <Input size={size} value={slug} onChange={(e) => (setSlugTyped(e.target.value.toLowerCase()), setRefused(null))} aria-label={t(areaKeys.link, "its link")} aria-invalid={slug !== "" && !isSlug(slug)} className="min-w-0 flex-1 font-mono" />/
            </span>
          </Row>
        </>
      )}
      {slug !== "" && !isSlug(slug) && <p className="text-xs text-accent-error">{t(areaKeys.badSlug, "Lowercase letters, digits, - and _; up to 64.")}</p>}
      {refused !== null && <p className="text-xs text-accent-error">{refused}</p>}
      {center !== null && (
        <Button size={button("sm")} className="self-end" disabled={busy || body === null || !isSlug(slug)} onClick={() => void save()}>
          {t(areaKeys.save, "Save the area")}
        </Button>
      )}
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
    <Command shouldFilter={false} search={input} onSearchChange={setInput} className="h-auto overflow-visible rounded-md border border-border">
      <CommandInput placeholder={t(areaKeys.search, "City or town")} autoCapitalize="none" />
      {found !== null && (
        <CommandList>
          {found.length === 0 && <p className="px-3 py-2 text-sm text-ink-soft">{t(areaKeys.noneFound, "No city found.")}</p>}
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
