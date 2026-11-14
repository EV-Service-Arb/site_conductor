import { cn, toast } from "@evinvest/uikit";
import { PersonStanding } from "lucide-react";
import { useEffect, useState } from "react";
import { createPortal } from "react-dom";

import { http, ignoreBody } from "@/shared/api";
import { streetView, useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";

/** Past this many px a press on the pegman is a drag. */
const DRAG = 4;

// on globalThis beside the kept map: its controls are pushed once per page session
const kept = globalThis as typeof globalThis & { saStreetView?: { box: HTMLDivElement; coverage: google.maps.StreetViewCoverageLayer; pixels: google.maps.OverlayView } };

function controls(map: google.maps.Map) {
  if (kept.saStreetView !== undefined) return kept.saStreetView;
  const box = document.createElement("div");
  box.style.margin = "0 10px 10px 0";
  Object.assign(box, { index: -1 }); // below the zoom, as Google's own pegman: a negative index goes before the built-in controls
  const corner = map.controls[google.maps.ControlPosition.RIGHT_BOTTOM];
  if (corner === undefined) throw new Error("the map has no bottom-right controls");
  corner.push(box);
  const pixels = new google.maps.OverlayView();
  pixels.onAdd = () => {};
  pixels.draw = () => {};
  pixels.onRemove = () => {};
  pixels.setMap(map);
  kept.saStreetView = { box, coverage: new google.maps.StreetViewCoverageLayer(), pixels };
  return kept.saStreetView;
}

/**
 * The map's pegman, in place of Google's: a press shows where Street View is (pressed again, hides
 * it); dragged, it shows it while held, and dropped where there is a panorama, buys it
 * (`POST /api/v1/map/street_view`, tokens, refused past the month's free ones) before opening it.
 */
export function StreetView({ map }: { map: google.maps.Map }) {
  const t = useT();
  const { box, coverage, pixels } = controls(map);
  const [shown, setShown] = useState(coverage.getMap() != null);
  const [held, setHeld] = useState<{ x: number; y: number } | null>(null);
  useEffect(() => {
    coverage.setMap(shown || held !== null ? map : null);
  }, [coverage, map, shown, held !== null]);

  const drop = async (x: number, y: number) => {
    const box = map.getDiv().getBoundingClientRect();
    if (x < box.left || x > box.right || y < box.top || y > box.bottom) return;
    const at = pixels.getProjection().fromContainerPixelToLatLng(new google.maps.Point(x - box.left, y - box.top));
    if (at === null) throw new Error("a pixel on the map has no point");
    // a panorama looked up is free; only one opened is billed
    const found = await new google.maps.StreetViewService().getPanorama({ location: at, radius: 50 }).catch((e: unknown) => {
      if (e instanceof google.maps.MapsRequestError && e.code === google.maps.StreetViewStatus.ZERO_RESULTS) return null;
      throw e;
    });
    if (found === null) {
      toast.error(t(streetView.none, "No Street View here"));
      return;
    }
    const pano = found.data.location;
    if (pano?.pano == null || pano.latLng == null) throw new Error("Street View found a panorama without its id or point");
    await http.send("POST", "/api/v1/map/street_view", pano.latLng.toJSON(), ignoreBody);
    const panorama = map.getStreetView();
    panorama.setPano(pano.pano);
    panorama.setVisible(true);
  };

  return createPortal(
    <>
      <button
        type="button"
        aria-pressed={shown}
        aria-label={t(streetView, "Street View: where it is; drag onto the map to open it")}
        title={t(streetView, "Street View: where it is; drag onto the map to open it")}
        onPointerDown={(e) => {
          e.currentTarget.setPointerCapture(e.pointerId);
          const from = { x: e.clientX, y: e.clientY };
          let dragged = false;
          const target = e.currentTarget;
          const move = (m: PointerEvent) => {
            dragged ||= Math.hypot(m.clientX - from.x, m.clientY - from.y) > DRAG;
            if (dragged) setHeld({ x: m.clientX, y: m.clientY });
          };
          target.addEventListener("pointermove", move);
          target.addEventListener(
            "pointerup",
            (u: PointerEvent) => {
              target.removeEventListener("pointermove", move);
              setHeld(null);
              if (!dragged) setShown((s) => !s);
              else drop(u.clientX, u.clientY).catch((err: unknown) => notifyFailure(err, t));
            },
            { once: true },
          );
        }}
        className={cn(
          "flex size-10 cursor-grab touch-none items-center justify-center rounded-sm bg-card text-[#fbbc04] shadow-overlay ring-1 ring-border transition-colors hover:bg-muted",
          shown && "bg-muted ring-ring",
        )}
      >
        <PersonStanding aria-hidden className="size-6" strokeWidth={2.75} />
      </button>
      {held !== null &&
        createPortal(
          <PersonStanding aria-hidden className="pointer-events-none fixed z-50 size-8 -translate-x-1/2 -translate-y-full text-[#fbbc04] drop-shadow" strokeWidth={2.75} style={{ left: held.x, top: held.y }} />,
          document.body,
        )}
    </>,
    box,
  );
}
