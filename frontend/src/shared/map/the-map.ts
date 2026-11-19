"use client";

// One Google map per page session, kept while its page is away and re-inserted when it is back:
// every `google.maps.Map` built is a billed map load, bought before it is built. The script comes
// straight from Google.
// Raster, without a map id: only such a map takes `styles`, which is what hides Google's own places.

import { importLibrary, setOptions } from "@googlemaps/js-api-loader";
import { type RefObject, useEffect, useRef, useState, useSyncExternalStore } from "react";

import { type Failure, Refused, api, ok } from "@/shared/api";
import type { MapConfig } from "@/shared/api/schema";
import { setHome, useHome } from "@/shared/lib/location";

const PLACES_KEY = "sa:map-places";
const CHOSEN_EVENT = "sa:map-view-chosen";
const PLACES_HIDDEN: google.maps.MapTypeStyle[] = [
  { featureType: "poi", stylers: [{ visibility: "off" }] },
  { featureType: "transit", elementType: "labels.icon", stylers: [{ visibility: "off" }] },
];

export type Base = "schematic" | "satellite";

const placesShown = () => localStorage.getItem(PLACES_KEY) === "shown";
const mapTypeId = (base: Base) => (base === "satellite" ? "hybrid" : "roadmap");

// on globalThis: a hot update re-running this module keeps the page's (billed) map, on the account it loaded on
const kept = globalThis as typeof globalThis & { saMap?: { account: string; element: HTMLDivElement; map: Promise<google.maps.Map> } };
const styles = () => (placesShown() ? [] : PLACES_HIDDEN);

// Maps JS takes one key a page load: a config naming another account after is for the next load
function theMap({ key, account }: MapConfig, scheme: string): { account: string; element: HTMLDivElement; map: Promise<google.maps.Map> } {
  if (kept.saMap !== undefined) return kept.saMap;
  if (scheme !== "dark" && scheme !== "light") throw new Error(`--scheme is ${JSON.stringify(scheme)}, not dark or light`);
  setOptions({ key, v: "weekly" });
  const element = document.createElement("div");
  element.style.width = "100%";
  element.style.height = "100%";
  const map = ok(api.POST("/map/load", { body: { account } }))
    .then(() => Promise.all([importLibrary("maps"), importLibrary("core")]))
    .then(([{ Map, RenderingType }, { ColorScheme, ControlPosition }]) =>
      new Map(element, {
        renderingType: RenderingType.RASTER,
        colorScheme: scheme === "dark" ? ColorScheme.DARK : ColorScheme.LIGHT,
        styles: styles(),
        // the default base; `useTheMap` moves it to what `?base` says
        mapTypeId: mapTypeId("satellite"),
        // the world, until the page fits what it shows
        center: { lat: 0, lng: 0 },
        zoom: 2,
        disableDefaultUI: true,
        zoomControl: true,
        zoomControlOptions: { position: ControlPosition.RIGHT_BOTTOM },
        fullscreenControl: true,
        fullscreenControlOptions: { position: ControlPosition.RIGHT_TOP },
        // Google's pegman opens panoramas unpaid: `StreetView` is the map's
        streetViewControl: false,
        clickableIcons: false,
      }),
  );
  // a refused buy builds nothing: the next mount asks again
  void map.catch(() => {
    if (kept.saMap?.map === map) delete kept.saMap;
  });
  kept.saMap = { account, element, map };
  return kept.saMap;
}

export type TheMap = { status: "loading" } | { status: "ok"; map: google.maps.Map; account: string } | { status: "error"; failure: Failure };

/** The page session's map, inside `host`; its polarity is the host's `--scheme`. */
export function useTheMap(config: MapConfig): { host: RefObject<HTMLDivElement | null>; map: TheMap } {
  const host = useRef<HTMLDivElement>(null);
  const [map, setMap] = useState<TheMap>({ status: "loading" });
  useEffect(() => {
    const box = host.current;
    if (box === null) throw new Error("useTheMap: its host is not rendered");
    const { account, element, map } = theMap(config, getComputedStyle(box).getPropertyValue("--scheme").trim());
    box.appendChild(element);
    let live = true;
    map.then(
      (m) => live && setMap({ status: "ok", map: m, account }),
      (e: unknown) => live && setMap({ status: "error", failure: e instanceof Refused ? e.failure : { code: "maps", error: e instanceof Error ? e.message : String(e) } }),
    );
    return () => {
      live = false;
      element.remove();
    };
  }, [config]);
  const base = useBase();
  useEffect(() => {
    if (map.status === "ok") map.map.setMapTypeId(mapTypeId(base));
  }, [map, base]);
  return { host, map };
}

const subscribeChosen = (onChange: () => void) => {
  for (const e of ["storage", CHOSEN_EVENT]) window.addEventListener(e, onChange);
  return () => {
    for (const e of ["storage", CHOSEN_EVENT]) window.removeEventListener(e, onChange);
  };
};

const chosen = () => {
  window.dispatchEvent(new Event(CHOSEN_EVENT));
  void kept.saMap?.map.then((m) => m.setOptions({ styles: styles() }));
};

/** Whether the map shows Google's own places (shops, sights, stops); hidden unless chosen, kept in this browser. */
export function usePlacesShown(): boolean {
  return useSyncExternalStore(subscribeChosen, placesShown, () => false);
}

export function showPlaces(shown: boolean): void {
  if (shown) localStorage.setItem(PLACES_KEY, "shown");
  else localStorage.removeItem(PLACES_KEY);
  chosen();
}

/** Google's imagery or its roads under everything drawn: `?schematic`, satellite without it. */
export function useBase(): Base {
  return useHome()?.schematic === true ? "schematic" : "satellite";
}

export function setBase(base: Base): void {
  setHome({ schematic: base === "schematic" });
}

/** `element` centred on a point of `map` (or standing on it by its bottom tip) until removed, its clicks the map's no more; dragged where `onDrag` is given. */
export function pin(
  map: google.maps.Map,
  element: HTMLElement,
  at: google.maps.LatLngLiteral,
  { anchor = "centre", onDrag, onMoving }: { anchor?: "centre" | "tip"; onDrag?: (to: google.maps.LatLngLiteral) => void; onMoving?: (to: google.maps.LatLngLiteral) => void } = {},
): { move: (to: google.maps.LatLngLiteral) => void; remove: () => void } {
  element.style.position = "absolute";
  element.style.transform = anchor === "tip" ? "translate(-50%, -100%)" : "translate(-50%, -50%)";
  google.maps.OverlayView.preventMapHitsAndGesturesFrom(element);
  const overlay = new (class extends google.maps.OverlayView {
    override onAdd() {
      this.getPanes()!.overlayMouseTarget.appendChild(element);
    }
    override draw() {
      const p = this.getProjection().fromLatLngToDivPixel(at);
      if (p === null) throw new Error("a pin's point has no pixel while drawn");
      element.style.left = `${p.x}px`;
      element.style.top = `${p.y}px`;
    }
    override onRemove() {
      element.remove();
    }
  })();
  // the map draws it once added; before that a move only changes where
  const redraw = () => element.isConnected && overlay.draw();
  if (onDrag) {
    element.style.cursor = "grab";
    element.addEventListener("pointerdown", (e) => {
      element.setPointerCapture(e.pointerId);
      const box = map.getDiv().getBoundingClientRect();
      const follow = (m: PointerEvent) => {
        const to = overlay.getProjection().fromContainerPixelToLatLng(new google.maps.Point(m.clientX - box.left, m.clientY - box.top));
        if (to === null) throw new Error("the pointer's pixel has no point while dragging");
        at = to.toJSON();
        redraw();
        onMoving?.(at);
      };
      element.addEventListener("pointermove", follow);
      element.addEventListener("pointerup", () => {
        element.removeEventListener("pointermove", follow);
        onDrag(at);
      }, { once: true });
    });
  }
  overlay.setMap(map);
  return {
    move: (to) => {
      at = to;
      redraw();
    },
    remove: () => overlay.setMap(null),
  };
}

const EARTH_M = 6_371_008.8;
const rad = (deg: number) => (deg * Math.PI) / 180;

/** Metres between two points along the ground (haversine). */
export function distanceM(a: google.maps.LatLngLiteral, b: google.maps.LatLngLiteral): number {
  const h = Math.sin(rad(b.lat - a.lat) / 2) ** 2 + Math.cos(rad(a.lat)) * Math.cos(rad(b.lat)) * Math.sin(rad(b.lng - a.lng) / 2) ** 2;
  return 2 * EARTH_M * Math.asin(Math.sqrt(h));
}
