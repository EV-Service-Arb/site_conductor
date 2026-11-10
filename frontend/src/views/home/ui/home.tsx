"use client";

import { Skeleton, cn } from "@evinvest/uikit";
import { useRouter } from "next/navigation";
import { useEffect, useRef, useState } from "react";

import {
  type LatLng,
  type Listed,
  type MapConfig,
  type State,
  TABS,
  type Tags,
  fetchMapConfig,
  issueOf,
  narrowed,
  open,
  openAccount,
  peek,
  setOnMap,
  under,
  useAccountWindow,
  useGmails,
  useGmbTags,
  useOnMap,
  usePeeked,
  useSelection,
  useTab,
  useTradeWindow,
  useTrades,
} from "@/entities/gmb";
import { PlacingBar } from "@/features/place-gmb";
import { PlacementOverlay } from "@/features/placement-overlay";
import { takeReturnPage } from "@/shared/api";
import { useT } from "@/shared/i18n";
import { setQuery, useQuery } from "@/shared/lib/query";
import { useResource } from "@/shared/lib/use-resource";
import { pin, useTheMap } from "@/shared/map";
import { EmptyState } from "@/shared/ui/empty-state";
import { ErrorState } from "@/shared/ui/error-state";

import { AccountWindow } from "./account-window";
import { Peek } from "./peek";
import { TradeWindow } from "./trade-window";
import { LocationWindow } from "./window";

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
  const hovered = useOnMap((s) => s.hovered);
  const placing = useOnMap((s) => s.placing);
  const peeked = usePeeked();
  const tab = useTab();
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
      dot.addEventListener("click", () => open(l.gmb));
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
  const tabs = peekedListed && TABS[peekedListed.gmb.state];
  // a GBP whose state moved since its window opened: its state's first tab, or its card alone
  useEffect(() => {
    if (tabs !== undefined && tab !== null && !tabs.includes(tab)) setQuery({ tab: tabs[0] ?? null });
  }, [tabs, tab]);
  const windowed = peekedListed !== undefined && tab !== null && tabs?.includes(tab) === true;
  const tradeWindow = useTradeWindow();
  const trades = useTrades();
  const opened = tradeWindow !== null && trades.status === "ok" ? trades.data.find((tr) => tr.id === tradeWindow.trade) : undefined;
  // a trade deleted, here or in another tab: its window closes
  const gone = tradeWindow !== null && trades.status === "ok" && opened === undefined;
  useEffect(() => {
    if (gone) setQuery({ trade: null, tab: null });
  }, [gone]);
  const accountWindow = useAccountWindow();
  const account = accountWindow !== null && gmails.status === "ok" ? gmails.data.gmails.find((g) => g.id === accountWindow.gmail) : undefined;
  // a gmail removed, here or in another tab: its window closes
  const accountGone = accountWindow !== null && gmails.status === "ok" && account === undefined;
  useEffect(() => {
    if (accountGone) openAccount(null);
  }, [accountGone]);
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
      {peekedListed && placed === undefined && !windowed && <Peek listed={peekedListed} onClose={() => peek(null)} />}
      {peekedListed && placed === undefined && windowed && <LocationWindow listed={peekedListed} tab={tab} />}
      {opened && tradeWindow && gmails.status === "ok" && placed === undefined && <TradeWindow trade={opened} tab={tradeWindow.tab} gmbs={gmails.data.gmbs} gmails={gmails.data.gmails} />}
      {account && accountWindow && gmails.status === "ok" && placed === undefined && <AccountWindow gmail={account} tab={accountWindow.tab} gbps={under(gmails.data, account.id).length} />}
      {map.status === "ok" && placed !== undefined && placedAt !== null && <PlacingBar gmb={placed} map={map.map} at={placedAt} onMove={(at) => setMoved({ gmb: placed.id, at })} />}
    </>
  );
}

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
    const tone = issue === "off" ? "ink-soft" : issue !== null && issue !== "draft" ? "negative" : "positive";
    const hollow = listed.gmb.location?.placed === true;
    const state = listed.gmb.state;
    dot.className = cn(
      "block origin-bottom cursor-pointer transition-transform [&_path]:stroke-background",
      issue === "off" ? "[&_path]:fill-ink-soft [&_[data-core]]:fill-background" : [state === "suspended" ? "[&_path]:fill-accent-error" : "[&_path]:fill-white", CORE[state]],
      hollow && "[&_path]:[stroke-dasharray:3_2] [&_path]:stroke-ink",
      tone === "negative" ? "[&_[data-badge]]:fill-accent-error [&_[data-badge]]:stroke-background" : "[&_[data-badge]]:hidden",
      issue === "off" && !lit && "opacity-60",
      lit && "scale-125",
      id === peeked && "[&_path]:stroke-ink [&_path]:[stroke-width:2.5]",
    );
    dot.style.zIndex = lit ? "2" : issue === "off" ? "0" : "1";
  }
}
