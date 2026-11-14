"use client";

import { Button, Command, CommandInput, CommandItem, CommandList, Input, ToggleGroup, ToggleGroupItem } from "@evinvest/uikit";
import { Crosshair, MapPin } from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";

import { ApiError } from "@/shared/api";
import { useLocale, useT } from "@/shared/i18n";
import { formatDistance } from "@/shared/lib/format";
import { failureText } from "@/shared/ui/failure-text";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize, useControlSize } from "@/shared/ui/touch";

import { fetchCity, localityAt, putArea, suggestCities } from "../api/areas";
import { type Area, type Locality, type PutArea, isSlug, slugOf } from "../model/areas";
import type { LatLng, Suggestion } from "../model/gmb";
import { areaKeys } from "../model/i18n";

const PAUSE_MS = 350;
const MIN_INPUT = 2;
const MARGINS = [0, 1000, 2000, 5000] as const;
const RADII = [5000, 10000, 20000] as const;
const M_PER_DEG = 111_320;

export type Outline = ({ kind: "city"; viewport: { south: number; west: number; north: number; east: number } } | { kind: "circle"; center: LatLng; radius_m: number }) & { margin_m: number };

export const outlineOf = (a: Area): Outline => (a.shape.kind === "city" ? { kind: "city", viewport: a.shape.viewport, margin_m: a.margin_m } : { kind: "circle", center: a.center, radius_m: a.shape.radius_m, margin_m: a.margin_m });

/** `outline` on `map` while mounted, the map fitted to it once per change; `left` px of the map stay under a window. */
export function useOutline(map: google.maps.Map, outline: Outline | null, left: number) {
  const key = JSON.stringify(outline);
  useEffect(() => {
    if (outline === null) return;
    const line = { strokeColor: "#ffffff", clickable: false };
    const padding = { top: 48, right: 48, bottom: 48, left: left + 48 };
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
  }, [map, key, left]);
}

type Picked = { kind: "city"; locality: Locality } | { kind: "point"; at: LatLng; locality: Locality | null };

/** A new area, by a city searched for or a point clicked on `map`: its city, or a circle around it; saved under its link, then `onSaved`. */
export function AreaDraw({ map, left, onSaved }: { map: google.maps.Map; left: number; onSaved: (area: Area) => void }) {
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
  useOutline(map, outline, left);
  const body: PutArea | null = city !== null ? { kind: "city", place_id: city.place_id, margin_m: margin } : circleAt !== null && name.trim() !== "" ? { kind: "circle", name: name.trim(), lat: circleAt.lat, lng: circleAt.lng, radius_m: radius, margin_m: margin } : null;

  const save = async () => {
    if (body === null) throw new Error("saved with nothing drawn");
    setBusy(true);
    try {
      onSaved(await putArea(slug, body));
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
          {picked.locality.name} · {picked.locality.country}
        </p>
      )}
      {picked?.kind === "point" ? (
        <div className="flex flex-col gap-2">
          <p className="flex items-center gap-2 text-sm text-ink">
            <Crosshair aria-hidden className="size-4 text-ink-soft" />
            {t(areaKeys.picked, "Picked on the map")}
            <span className="font-mono text-xs text-ink-soft tabular-nums">
              {picked.at.lat.toFixed(4)}, {picked.at.lng.toFixed(4)}
            </span>
          </p>
          {picked.locality !== null ? (
            <ToggleGroup type="single" variant="outline" size="sm" className="w-full" value={asCircle ? "circle" : "city"} onValueChange={(v) => (v === "city" || v === "circle") && (setAsCircle(v === "circle"), setSlugTyped(null))}>
              <ToggleGroupItem value="city" className="flex-1">
                {t(areaKeys.itsCity, "Its city · {name}", { name: picked.locality.name })}
              </ToggleGroupItem>
              <ToggleGroupItem value="circle" className="flex-1">
                {t(areaKeys.circle, "A circle around the point")}
              </ToggleGroupItem>
            </ToggleGroup>
          ) : (
            <p className="text-xs text-ink-soft">{t(areaKeys.noCity, "No city there: a circle around the point.")}</p>
          )}
        </div>
      ) : (
        <p className="flex items-center gap-2 text-xs text-ink-soft">
          <Crosshair aria-hidden className="size-3.5" />
          {t(areaKeys.orClick, "or click the map: its city, else a circle around the point")}
        </p>
      )}

      {circleAt !== null && (
        <Row label={t(areaKeys.radius, "radius")}>
          <Presets values={RADII} value={radius} onPick={(r) => (setRadius(r), setSlugTyped(null))} locale={locale} />
          <Input size={size} type="number" min={0.1} step={0.1} value={radius / 1000} onChange={(e) => Number(e.target.value) > 0 && (setRadius(Math.round(Number(e.target.value) * 1000)), setSlugTyped(null))} aria-label={t(areaKeys.radiusKm, "Radius, km")} className="w-20 tabular-nums" />
        </Row>
      )}
      {outline !== null && (
        <Row label={t(areaKeys.margin, "and next to it, within")}>
          <Presets values={MARGINS} value={margin} onPick={setMargin} locale={locale} />
        </Row>
      )}
      {circleAt !== null && (
        <Row label={t(areaKeys.name, "its name")}>
          <Input size={size} value={name} onChange={(e) => (setNamed(e.target.value), setSlugTyped(null))} placeholder={t(areaKeys.namePlaceholder, "Chassieu")} aria-label={t(areaKeys.name, "its name")} className="min-w-0 flex-1" />
        </Row>
      )}
      {outline !== null && (
        <Row label={t(areaKeys.link, "its link")}>
          <span className="flex min-w-0 flex-1 items-center gap-1 font-mono text-sm">
            /
            <Input size={size} value={slug} onChange={(e) => (setSlugTyped(e.target.value.toLowerCase()), setRefused(null))} aria-label={t(areaKeys.link, "its link")} aria-invalid={slug !== "" && !isSlug(slug)} className="min-w-0 flex-1 font-mono" />/
          </span>
        </Row>
      )}
      {slug !== "" && !isSlug(slug) && <p className="text-xs text-accent-error">{t(areaKeys.badSlug, "Lowercase letters, digits, - and _; up to 64.")}</p>}
      {refused !== null && <p className="text-xs text-accent-error">{refused}</p>}
      {outline !== null && (
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
