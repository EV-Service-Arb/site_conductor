"use client";

import { Skeleton, cn } from "@evinvest/uikit";
import { useRouter } from "next/navigation";
import { useEffect, useRef, useState } from "react";

import { type LatLng, type Listed, type MapConfig, type Tags, fetchMapConfig, issueOf, narrowed, peek, setOnMap, useGmails, useGmbTags, useOnMap, usePeeked, useSelection } from "@/entities/gmb";
import { PlacingBar } from "@/features/place-gmb";
import { PlacementOverlay } from "@/features/placement-overlay";
import { takeReturnPage } from "@/shared/api";
import { useT } from "@/shared/i18n";
import { setQuery, useQuery } from "@/shared/lib/query";
import { useResource } from "@/shared/lib/use-resource";
import { pin, useTheMap } from "@/shared/map";
import { EmptyState } from "@/shared/ui/empty-state";
import { ErrorState } from "@/shared/ui/error-state";

import { Peek } from "./peek";

/** `/`: the caller's GMBs on a map, unless they come back from the sign-in dialog to the page they were on. */
export function Home() {
  const t = useT();
  const router = useRouter();
  // Taken once: the read clears the key.
  const back = useRef<string | null | undefined>(undefined);
  useEffect(() => {
    back.current ??= takeReturnPage();
    if (back.current !== null && back.current !== "/") router.replace(back.current);
  }, [router]);
  const config = useResource("map", fetchMapConfig);

  return (
    <div className="relative min-h-0 flex-1">
      {config.status === "loading" && <Skeleton className="absolute inset-0 rounded-none" />}
      {config.status === "error" && (
        <div className="p-6">
          <ErrorState failure={config.failure} onRetry={config.reload} />
        </div>
      )}
      {config.status === "ok" && config.data === null && <EmptyState className="m-6" title={t("home.noMap")} description={t("home.noMap.why")} />}
      {config.status === "ok" && config.data !== null && <GmbMap config={config.data} />}
    </div>
  );
}

/** Fitted to the GMBs once per page session, unless `?at` names where: the map outlives the page, and so does where it was left. */
let fitted = false;

function GmbMap({ config }: { config: MapConfig }) {
  const gmails = useGmails();
  const selection = useSelection();
  const { hovered, placing } = useOnMap();
  const peeked = usePeeked();
  // `?at` as the page opened: what it says after is where the map has since moved
  const [left] = useState(useQuery("at"));
  const { tags } = useGmbTags();
  const { host, map } = useTheMap(config.key);
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

  useEffect(() => () => setOnMap({ inView: null, hovered: null }), []);

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
      setQuery({ at: `${centre.lat().toFixed(5)},${centre.lng().toFixed(5)},${zoom}` });
    };
    const idle = m.addListener("idle", update);
    update();
    return () => idle.remove();
  }, [map, shown]);

  useEffect(() => {
    if (map.status !== "ok" || left === null) return;
    const [lat, lng, zoom] = left.split(",").map(Number);
    if (lat === undefined || lng === undefined || zoom === undefined || ![lat, lng, zoom].every(Number.isFinite)) throw new Error(`?at=${left}: not <lat>,<lng>,<zoom>`);
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
      dot.addEventListener("click", () => peek(l.gmb.id));
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
  useEffect(() => {
    const at = peekedListed?.gmb.location;
    if (map.status === "ok" && at) map.map.panTo(at);
  }, [map, peekedListed?.gmb.location]);

  return (
    <>
      <div ref={host} className="absolute inset-0" />
      {map.status === "ok" && <PlacementOverlay map={map.map} />}
      {map.status === "error" && (
        <div className="absolute inset-x-6 top-6">
          <ErrorState failure={{ kind: "invalid", message: map.message }} />
        </div>
      )}
      {peekedListed && placed === undefined && <Peek listed={peekedListed} onClose={() => peek(null)} />}
      {map.status === "ok" && placed !== undefined && placedAt !== null && <PlacingBar gmb={placed} map={map.map} at={placedAt} onMove={(at) => setMoved({ gmb: placed.id, at })} />}
    </>
  );
}

// a teardrop standing on its point, apart from the competitors' dots the placement overlay draws
const TEARDROP = `<svg viewBox="0 0 24 32" class="block h-7 w-5 drop-shadow-md" aria-hidden="true"><path d="M12 31s-10-11.6-10-19a10 10 0 0 1 20 0c0 7.4-10 19-10 19z" stroke-width="2"/><circle cx="12" cy="12" r="3.5"/></svg>`;

/** A pin is red while its GMB needs the operator, faded while it is off, and hollow where its point was placed by hand. */
function styleDots(
  made: Map<number, { dot: HTMLSpanElement; listed: Listed }>,
  { hovered, peeked, tags }: { hovered: number | null; peeked: number | null; tags: Tags },
) {
  for (const [id, { dot, listed }] of made) {
    const lit = id === hovered || id === peeked;
    const issue = issueOf(listed.gmb, tags);
    const tone = issue === "off" ? "ink-soft" : issue !== null ? "negative" : "positive";
    const hollow = listed.gmb.location?.placed === true;
    dot.className = cn(
      "block origin-bottom cursor-pointer transition-transform",
      { negative: "text-accent-error", positive: "text-positive", "ink-soft": "text-ink-soft" }[tone],
      hollow ? "[&_path]:fill-background [&_path]:stroke-current [&_circle]:fill-current" : "[&_path]:fill-current [&_path]:stroke-background [&_circle]:fill-background",
      issue === "off" && !lit && "opacity-50",
      lit && "scale-125",
      id === peeked && "[&_path]:stroke-ink",
    );
    dot.style.zIndex = lit ? "2" : issue === "off" ? "0" : "1";
  }
}
