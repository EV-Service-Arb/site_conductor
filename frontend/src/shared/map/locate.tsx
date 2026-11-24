import { cn, toast } from "@evinvest/uikit";
import { LoaderCircle, LocateFixed } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

import { locate, useT } from "@/shared/i18n";

import { corner, pin } from "./the-map";

const CLOSE_ZOOM = 15;

/** The map's "where am I": the browser's position (asking for it the first time), centred on and marked. */
export function Locate({ map }: { map: google.maps.Map }) {
  const t = useT();
  const box = corner(map, "locate");
  const [busy, setBusy] = useState(false);
  const mark = useRef<{ move: (to: google.maps.LatLngLiteral) => void; remove: () => void } | null>(null);
  useEffect(
    () => () => {
      mark.current?.remove();
      mark.current = null;
    },
    [map],
  );

  const find = () => {
    // absent off a secure origin (http but localhost)
    if (!("geolocation" in navigator)) {
      toast.error(t(locate.unavailable, "This browser gives no location here"));
      return;
    }
    setBusy(true);
    navigator.geolocation.getCurrentPosition(
      ({ coords }) => {
        setBusy(false);
        const at = { lat: coords.latitude, lng: coords.longitude };
        if (mark.current === null) {
          const dot = document.createElement("span");
          dot.className = "block size-4 rounded-full border-2 border-background bg-primary shadow-md";
          mark.current = pin(map, dot, at);
        } else mark.current.move(at);
        map.panTo(at);
        map.setZoom(Math.max(map.getZoom() ?? CLOSE_ZOOM, CLOSE_ZOOM));
      },
      (e) => {
        setBusy(false);
        // once denied, the browser asks no more: only its site settings undo it
        if (e.code === e.PERMISSION_DENIED) toast.error(t(locate.denied, "Location is blocked for this site: allow it in the browser's site settings"));
        else toast.error(t(locate.failed, "Could not find your location. {detail}", { detail: e.message }));
      },
      { enableHighAccuracy: true, timeout: 15_000, maximumAge: 60_000 },
    );
  };

  return createPortal(
    <button
      type="button"
      disabled={busy}
      aria-label={t(locate, "Centre on my location")}
      title={t(locate, "Centre on my location")}
      onClick={find}
      className={cn("flex size-10 items-center justify-center rounded-sm bg-card text-ink shadow-overlay ring-1 ring-border transition-colors hover:bg-muted", busy && "cursor-progress")}
    >
      {busy ? <LoaderCircle aria-hidden className="size-5 animate-spin" /> : <LocateFixed aria-hidden className="size-5" />}
    </button>,
    box,
  );
}
