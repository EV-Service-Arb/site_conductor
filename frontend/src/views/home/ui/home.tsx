"use client";

import { useResource } from "@evinvest/api/react";
import { Button, Skeleton, cn, useDismissableLayer } from "@evinvest/uikit";
import { type ReactNode, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

import { type Listed, type Tags, issueOf, narrowed, openOrFocus, peek, setOnMap, move, useGmails, useGmbTags, useKitEdit, useOnMap, usePeeked, useSelection, useShown } from "@/entities/gmb";
import { DraftHere } from "@/features/add-gmb";
import { FindPlace } from "@/features/find-place";
import { PlacingBar } from "@/features/place-gmb";
import { useSpendConfirm, useTokens } from "@/entities/tokens";
import { PlacementOverlay, StudyAreaWindow, StudyAt } from "@/features/placement-overlay";
import type { LatLng, MapConfig, State } from "@/shared/api/schema";
import { api, takeReturnPage } from "@/shared/api";
import { useT, home } from "@/shared/i18n";
import { go, setHome, useHome } from "@/shared/lib/location";
import { Locate, StreetView, pin, useTheMap } from "@/shared/map";
import { EmptyState } from "@/shared/ui/empty-state";
import { useButtonSize } from "@/shared/ui/touch";
import { ErrorState } from "@/shared/ui/error-state";
import { KitEdit } from "@/views/kit";

import { Peek } from "./peek";

/** `/`: the caller's GMBs on a map, unless they come back from the sign-in dialog to the page they were on. */
export function Home() {
  const t = useT();
  // Taken once: the read clears the key.
  const back = useRef<string | null | undefined>(undefined);
  useEffect(() => {
    back.current ??= takeReturnPage();
    if (back.current !== null && back.current !== "/") go(back.current, "replace");
  }, []);
  const config = useResource(api, "/map");

  return (
    <div className="relative min-h-0 flex-1">
      {config.status === "loading" && <Skeleton className="absolute inset-0 rounded-none" />}
      {config.status === "error" && config.failure.code !== "not_found" && (
        <div className="p-6">
          <ErrorState failure={config.failure} onRetry={config.reload} />
        </div>
      )}
      {/* not found: the conductor has no browser key, so there is no map */}
      {config.status === "error" && config.failure.code === "not_found" && <EmptyState className="m-6" title={t(home.noMap, "No map here")} description={t(home.noMap.why, "No Google account in the conductor's config has a browser key. Your GMBs are in the list beside.")} />}
      {config.status === "ok" && <GmbMap config={config.data} />}
    </div>
  );
}

/** Fitted to the GMBs once per page session, unless `?at` names where: the map outlives the page, and so does where it was left. */
let fitted = false;

function GmbMap({ config }: { config: MapConfig }) {
  const gmails = useGmails();
  const selection = useSelection();
  const hovered = useOnMap((s) => s.hovered);
  const placing = useOnMap((s) => s.placing);
  const picking = useOnMap((s) => s.picking);
  const drafting = useOnMap((s) => s.drafting);
  const marks = useOnMap((s) => s.marks);
  const shownWindow = useShown();
  const adding = shownWindow?.key.kind === "add";
  const editing = useKitEdit();
  const t = useT();
  const button = useButtonSize();
  const peeked = usePeeked();
  // `?at` as the page opened: what it says after is where the map has since moved
  const view = useHome();
  const [left] = useState(view?.at ?? null);
  const { tags } = useGmbTags();
  const { host, map } = useTheMap(config);
  const listed = selection?.selected ?? [];
  const located = listed.filter((l) => l.gmb.location !== null);
  const locatedRef = useRef(located);
  // first of the effects, so each one after reads this render's
  useEffect(() => {
    locatedRef.current = located;
  });
  const markers = useRef(new Map<number, { remove: () => void; dot: HTMLSpanElement; listed: Listed }>());
  const shown = located.map((l) => `${l.gmb.id}:${l.gmb.enabled}:${l.gmb.location?.lat},${l.gmb.location?.lng},${l.gmb.location?.placed}`).join(" ");
  const styling = useRef({ hovered, peeked, tags });
  useEffect(() => {
    styling.current = { hovered, peeked, tags };
  });

  useEffect(() => () => setOnMap({ inView: null, hovered: null, drafting: null }), []);

  useEffect(() => {
    if (map.status !== "ok") return;
    const m = map.map;
    const update = () => {
      const bounds = m.getBounds();
      if (bounds === undefined) return; // before the first render: the next idle tells
      setOnMap({ inView: new Set(locatedRef.current.filter((l) => l.gmb.location && bounds.contains(l.gmb.location)).map((l) => l.gmb.id)) });
      const centre = m.getCenter();
      const zoom = m.getZoom();
      if (centre === undefined || zoom === undefined) throw new Error("the map has bounds but no centre or zoom");
      setHome({ at: [Number(centre.lat().toFixed(5)), Number(centre.lng().toFixed(5)), zoom] });
    };
    const idle = m.addListener("idle", update);
    update();
    return () => idle.remove();
  }, [map, shown]);

  useEffect(() => {
    if (map.status !== "ok" || left === null) return;
    const [lat, lng, zoom] = left;
    fitted = true;
    map.map.setCenter({ lat, lng });
    map.map.setZoom(zoom);
  }, [map, left]);

  useEffect(() => {
    if (map.status !== "ok" || fitted || left !== null || gmails.status !== "ok") return;
    fitted = true;
    const points = locatedRef.current.flatMap((l) => (l.gmb.location ? [l.gmb.location] : []));
    if (points.length === 1) {
      map.map.setCenter(points[0]!);
      map.map.setZoom(14);
    } else if (points.length > 1) {
      const bounds = new google.maps.LatLngBounds();
      for (const p of points) bounds.extend(p);
      map.map.fitBounds(bounds, 48);
    }
  }, [map, left, gmails.status]);

  useEffect(() => {
    if (map.status !== "ok") return;
    const made = markers.current;
    for (const l of locatedRef.current) {
      const dot = document.createElement("span");
      dot.title = l.gmb.name;
      dot.innerHTML = TEARDROP;
      dot.addEventListener("mouseenter", () => setOnMap({ hovered: l.gmb.id }));
      dot.addEventListener("mouseleave", () => setOnMap({ hovered: null }));
      dot.addEventListener("click", () => openOrFocus(l.gmb, styling.current.peeked === l.gmb.id));
      made.set(l.gmb.id, { remove: pin(map.map, dot, l.gmb.location!, { anchor: "tip" }).remove, dot, listed: l });
    }
    styleDots(made, styling.current);
    return () => {
      for (const { remove } of made.values()) remove();
      made.clear();
    };
  }, [map, shown]);

  useEffect(() => styleDots(markers.current, { hovered, peeked, tags }), [hovered, peeked, tags, shown]);

  const placed = placing === null || gmails.status !== "ok" ? undefined : gmails.data.gmbs.find((b) => b.id === placing);
  const placedRef = useRef(placed);
  useEffect(() => {
    placedRef.current = placed;
  });
  const [moved, setMoved] = useState<{ gmb: number; at: LatLng } | null>(null);
  const placedAt = placed !== undefined && moved?.gmb === placed.id ? moved.at : null;
  const placingPin = useRef<ReturnType<typeof pin> | null>(null);
  useEffect(() => {
    const gmb = placedRef.current;
    if (map.status !== "ok" || gmb === undefined) return;
    const move = (at: LatLng) => setMoved({ gmb: gmb.id, at });
    const click = map.map.addListener("click", (e: google.maps.MapMouseEvent) => e.latLng && move(e.latLng.toJSON()));
    const centre = map.map.getCenter();
    if (centre === undefined) throw new Error("the map has no centre while shown");
    const at = gmb.location ?? centre.toJSON();
    const dot = document.createElement("span");
    dot.title = gmb.name;
    dot.className = "block size-5 rounded-full border-2 border-background bg-accent-error shadow-md ring-2 ring-ink";
    dot.style.zIndex = "3";
    placingPin.current = pin(map.map, dot, at, { onDrag: move });
    move(at);
    return () => {
      click.remove();
      placingPin.current?.remove();
      placingPin.current = null;
    };
  }, [map, placed?.id]);
  useEffect(() => {
    if (placingPin.current && moved) placingPin.current.move(moved.at);
  }, [moved]);

  const peekedListed = gmails.status === "ok" ? narrowed(gmails.data, null, "").find((l) => l.gmb.id === peeked) : undefined;
  // the GBP's window over the map stands in for its card
  const windowed = shownWindow?.key.kind === "gbp";
  const focus = useOnMap((s) => s.focus);
  useEffect(() => {
    if (map.status !== "ok" || focus === null) return;
    const at = peekedListed?.gmb.location;
    if (focus !== peekedListed?.gmb.id || !at) throw new Error(`GMB ${focus} focused while not the peeked one on the map`);
    map.map.setCenter(at);
    map.map.setZoom(Math.max(map.map.getZoom() ?? FOCUS_ZOOM, FOCUS_ZOOM));
    setOnMap({ focus: null });
  }, [map, focus, peekedListed?.gmb.id, peekedListed?.gmb.location]);

  useDismissableLayer({ enabled: picking !== null, onDismiss: () => setOnMap({ picking: null }), pointerOutside: false });
  useEffect(() => {
    if (map.status !== "ok" || picking === null) return;
    const m = map.map;
    m.setOptions({ draggableCursor: "crosshair" });
    const click = m.addListener("click", (e: google.maps.MapMouseEvent) => {
      if (!e.latLng) return;
      setOnMap({ picking: null });
      picking.onPick(e.latLng.toJSON());
    });
    return () => {
      click.remove();
      m.setOptions({ draggableCursor: null });
    };
  }, [map, picking]);

  // a form's picks while one is open, else the peeked draft's spots past its first, which is its pin
  const peekedSpots = peekedListed?.gmb.state === "draft" ? peekedListed.gmb.spots.slice(1) : [];
  const numbered = adding ? marks.map((at, i) => ({ at, n: i + 1 })) : peekedSpots.map(({ lat, lng }, i) => ({ at: { lat, lng }, n: i + 2 }));
  const numberedRef = useRef(numbered);
  useEffect(() => {
    numberedRef.current = numbered;
  });
  const numberedKey = numbered.map(({ at, n }) => `${n}:${at.lat},${at.lng}`).join(" ");
  const markCount = useRef(0);
  useEffect(() => {
    if (map.status !== "ok") return;
    const drawn = numberedRef.current.map(({ at, n }) => {
      const dot = document.createElement("span");
      dot.textContent = String(n);
      dot.className = cn(
        "flex size-7 items-center justify-center rounded-full border-2 text-xs font-semibold shadow-md",
        n === 1 ? "border-background bg-primary text-primary-foreground" : "border-ink bg-background text-ink",
      );
      dot.style.zIndex = "3";
      return pin(map.map, dot, at);
    });
    // a mark just added is shown close enough to place by
    const last = numberedRef.current.at(-1);
    if (last !== undefined && numberedRef.current.length > markCount.current) {
      map.map.panTo(last.at);
      map.map.setZoom(Math.max(map.map.getZoom() ?? CLOSE_ZOOM, CLOSE_ZOOM));
    }
    markCount.current = numberedRef.current.length;
    return () => {
      for (const d of drawn) d.remove();
    };
  }, [map, numberedKey]);

  const windows = placed === undefined && picking === null;
  const held = useTokens();
  const spend = useSpendConfirm();
  const panorama = held.status === "ok" ? held.data.prices["street_view.panorama"] : undefined;
  return (
    <>
      <div ref={host} className="absolute inset-0" />
      {map.status === "ok" && <PlacementOverlay map={map.map} />}
      {map.status === "ok" && <StreetView map={map.map} account={map.account} spend={(open) => spend.ask(panorama, open)} />}
      {spend.dialog}
      {map.status === "ok" && <Locate map={map.map} />}
      {map.status === "ok" && <FindPlace map={map.map} />}
      {map.status === "error" && (
        <div className="absolute inset-x-6 top-6">
          <ErrorState failure={map.failure} />
        </div>
      )}
      {peekedListed && windows && !windowed && map.status === "ok" && peekedListed.gmb.location && (
        <OnPin key={peekedListed.gmb.id} map={map.map} at={peekedListed.gmb.location}>
          <Peek listed={peekedListed} onClose={() => peek(null)} className="pointer-events-auto" />
        </OnPin>
      )}
      {peekedListed && windows && !windowed && !peekedListed.gmb.location && <Peek listed={peekedListed} onClose={() => peek(null)} className="absolute top-4 right-4" />}
      {drafting !== null && windows && editing === null && view?.area !== "" && map.status === "ok" && (
        <OnPin map={map.map} at={drafting}>
          <StudyAt at={drafting}>{(said) => <DraftHere at={drafting} {...said} onClose={() => setOnMap({ drafting: null })} />}</StudyAt>
          <span aria-hidden className="absolute bottom-0 left-1/2 -translate-x-1/2 [&_[data-badge]]:hidden [&_[data-core]]:fill-positive [&_path]:fill-white [&_path]:stroke-ink" dangerouslySetInnerHTML={{ __html: TEARDROP }} />
        </OnPin>
      )}
      {map.status === "ok" && windows && editing !== null && <KitEdit map={map.map} slug={editing} />}
      {map.status === "ok" && windows && <StudyAreaWindow map={map.map} />}
      {picking !== null && (
        <div className="absolute bottom-4 left-4 flex items-center gap-3 rounded-lg border border-border bg-background py-2 pr-2 pl-4 shadow-overlay">
          <p className="text-sm text-ink">{picking.label}</p>
          <kbd className="font-mono text-xs text-ink-faint">Esc</kbd>
          <Button variant="outline" size={button("sm")} onClick={() => setOnMap({ picking: null })}>
            {t(move.cancel, "Cancel")}
          </Button>
        </div>
      )}
      {map.status === "ok" && placed !== undefined && placedAt !== null && <PlacingBar gmb={placed} map={map.map} at={placedAt} onMove={(at) => setMoved({ gmb: placed.id, at })} />}
    </>
  );
}

const FOCUS_ZOOM = 17;
const MARGIN = 16;

/** `children` over the map standing on a GMB's pin, the map panned the least that shows them when they first are. */
function OnPin({ map, at, children }: { map: google.maps.Map; at: LatLng; children: ReactNode }) {
  const [box] = useState(() => {
    const made = document.createElement("div");
    made.style.zIndex = "4";
    made.style.pointerEvents = "none"; // the pin under its padding stays clickable
    made.addEventListener("contextmenu", (e) => e.stopPropagation()); // a right click on it marks nothing under it
    return made;
  });
  const { lat, lng } = at;
  useEffect(() => {
    const placed = pin(map, box, { lat, lng }, { anchor: "tip" });
    // sized once the map has drawn it
    const seen = new ResizeObserver(() => {
      if (box.offsetHeight === 0) return;
      seen.disconnect();
      reveal(map, { lat, lng }, box);
    });
    seen.observe(box);
    return () => {
      seen.disconnect();
      placed.remove();
    };
  }, [map, box, lat, lng]);
  return createPortal(
    <div className="relative pb-14">
      {children}
      <span aria-hidden className="absolute bottom-12 left-1/2 size-3 -translate-x-1/2 rotate-45 border-r border-b border-border bg-card" />
    </div>,
    box,
  );
}

function reveal(map: google.maps.Map, at: LatLng, box: HTMLElement) {
  const projection = map.getProjection();
  const centre = map.getCenter();
  const zoom = map.getZoom();
  if (projection === undefined || centre === undefined || zoom === undefined) throw new Error("the map drew a pin before it had a camera");
  const p = projection.fromLatLngToPoint(at);
  const c = projection.fromLatLngToPoint(centre);
  if (p === null || c === null) throw new Error("a point off the map's projection");
  const frame = map.getDiv();
  const x = (p.x - c.x) * 2 ** zoom + frame.clientWidth / 2;
  const y = (p.y - c.y) * 2 ** zoom + frame.clientHeight / 2;
  const past = (lo: number, hi: number, size: number) => (lo < MARGIN ? lo - MARGIN : hi > size - MARGIN ? hi - size + MARGIN : 0);
  const dx = past(x - box.offsetWidth / 2, x + box.offsetWidth / 2, frame.clientWidth);
  const dy = past(y - box.offsetHeight, y, frame.clientHeight);
  if (dx !== 0 || dy !== 0) map.panBy(dx, dy);
}

const CLOSE_ZOOM = 15;

// ours are white teardrops standing on their point, apart from the competitors' dots the placement overlay draws
const TEARDROP = `<svg viewBox="0 0 28 34" class="block h-8 w-7 overflow-visible drop-shadow-md" aria-hidden="true"><path d="M14 33s-10-11.6-10-19a10 10 0 0 1 20 0c0 7.4-10 19-10 19z" stroke-width="1.5"/><circle data-core cx="14" cy="14" r="4"/><circle data-badge cx="22" cy="5" r="4" stroke-width="1.5"/></svg>`;

const CORE: Record<State, string> = {
  draft: "[&_[data-core]]:fill-positive",
  verifying: "[&_[data-core]]:fill-accent-warn",
  verified: "[&_[data-core]]:fill-positive",
  suspended: "[&_[data-core]]:fill-white",
};

/** A pin's body is white (red while suspended), its core its state; a badge while it needs the operator; grey while off; dashed where its point was placed by hand. */
function styleDots(
  made: Map<number, { dot: HTMLSpanElement; listed: Listed }>,
  { hovered, peeked, tags }: { hovered: number | null; peeked: number | null; tags: Tags },
) {
  for (const [id, { dot, listed }] of made) {
    const lit = id === hovered || id === peeked;
    const issue = issueOf(listed.gmb, tags);
    const off = !listed.gmb.enabled;
    const needs = issue !== null && issue !== "off" && issue !== "draft";
    const hollow = listed.gmb.location?.placed === true;
    const state = listed.gmb.state;
    dot.className = cn(
      "block origin-bottom cursor-pointer transition-transform [&_path]:stroke-background",
      off ? "[&_path]:fill-ink-soft [&_[data-core]]:fill-background" : [state === "suspended" ? "[&_path]:fill-accent-error" : "[&_path]:fill-white", CORE[state]],
      hollow && "[&_path]:[stroke-dasharray:3_2] [&_path]:stroke-ink",
      needs ? "[&_[data-badge]]:fill-accent-error [&_[data-badge]]:stroke-background" : "[&_[data-badge]]:hidden",
      off && !lit && "opacity-60",
      lit && "scale-125",
      id === peeked && "[&_path]:stroke-ink [&_path]:[stroke-width:2.5]",
    );
    dot.style.zIndex = lit ? "2" : off ? "0" : "1";
  }
}
