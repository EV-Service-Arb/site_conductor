"use client";

import { useEffect, useState } from "react";

import type { LatLng } from "@/shared/api/schema";

/** Where the device is, as it moves; `denied`: the browser will not say. */
export function usePosition(): { at: LatLng | null; denied: boolean } {
  const [state, setState] = useState<{ at: LatLng | null; denied: boolean }>(() => ({ at: null, denied: typeof navigator !== "undefined" && !("geolocation" in navigator) }));
  useEffect(() => {
    if (!("geolocation" in navigator)) return;
    const id = navigator.geolocation.watchPosition(
      (p) => setState({ at: { lat: p.coords.latitude, lng: p.coords.longitude }, denied: false }),
      () => setState((s) => ({ at: s.at, denied: true })),
      { enableHighAccuracy: false, maximumAge: 30_000 },
    );
    return () => navigator.geolocation.clearWatch(id);
  }, []);
  return state;
}

/** Great-circle metres. */
export function metres(a: LatLng, b: LatLng): number {
  const r = Math.PI / 180;
  const h = Math.sin(((b.lat - a.lat) * r) / 2) ** 2 + Math.cos(a.lat * r) * Math.cos(b.lat * r) * Math.sin(((b.lng - a.lng) * r) / 2) ** 2;
  return 2 * 6_371_000 * Math.asin(Math.sqrt(h));
}
