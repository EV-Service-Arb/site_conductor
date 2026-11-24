"use client";

import { useEffect, useState } from "react";

import { useT, placement as placementKeys } from "@/shared/i18n";

import type { Overlay } from "@/shared/placement_overlay/placement_overlay";

import { type Legend, type Payload, type Query, attach, landed, tiersShown, useAreaPayload, useMapView, usePlacement, usePublishedPayload, useStudied } from "../model/placement";

/** gmaps_optimal_placement's drawing on `map`, as the panel's URL state says: the picked category's study over the picked area; and its legend. */
export function PlacementOverlay({ map }: { map: google.maps.Map }) {
  const placement = usePlacement();
  const view = useMapView();
  const studied = useStudied(view);
  const [live, setLive] = useState<Overlay | null>(null);
  useEffect(() => {
    let mounted = true;
    void attach(map).then((a) => mounted && setLive(a));
    return () => {
      mounted = false;
    };
  }, [map]);

  const drawn = studied.status === "ok" ? studied.drawn : null;
  useEffect(() => {
    if (drawn !== null) return;
    live?.show(null, false);
    landed(null);
  }, [live, drawn, placement.status]);
  // the overlay refuses a layer the shown study lacks; `?heat` waits for the study that offers it
  const data = placement.status === "ok" ? placement.data : null;
  const heat = data?.layers.some((l) => l.id === view.heat) ? view.heat : null;
  useEffect(() => {
    live?.heat(heat);
  }, [live, heat, data]);
  const traded = data?.trade != null;
  const tuning = JSON.stringify({ tiers: tiersShown(view, data) });
  useEffect(() => {
    if (traded) live?.tune(JSON.parse(tuning) as Parameters<Overlay["tune"]>[0]);
  }, [live, traded, tuning, data]);
  const pressure = view.pressure && traded;
  useEffect(() => {
    live?.pressure(pressure);
  }, [live, pressure]);
  const competitors = view.competitors && traded;
  useEffect(() => {
    live?.competitors(competitors);
  }, [live, competitors]);
  useEffect(() => {
    live?.opacity(view.opacity);
  }, [live, view.opacity]);
  useEffect(() => {
    live?.hideImputed(view.hideImputed);
  }, [live, view.hideImputed]);

  return (
    <>
      {drawn !== null && live !== null && <Study live={live} drawn={drawn} attached={placement.status} />}
      {placement.status === "ok" && placement.legend !== null && drawn !== null && placement.refused === null && <LegendStrip legend={placement.legend} />}
    </>
  );
}

function Study({ live, drawn, attached }: { live: Overlay; drawn: Query; attached: string }) {
  return "sha" in drawn ? <Published live={live} sha={drawn.sha} attached={attached} /> : <OfArea live={live} drawn={drawn} attached={attached} />;
}

function OfArea({ live, drawn, attached }: { live: Overlay; drawn: Extract<Query, { area: string }>; attached: string }) {
  return <Shown live={live} payload={useAreaPayload(drawn)} attached={attached} />;
}

function Published({ live, sha, attached }: { live: Overlay; sha: string; attached: string }) {
  return <Shown live={live} payload={usePublishedPayload(sha)} attached={attached} />;
}

/** The payload of what is drawn, handed to the overlay; why it is not, told to the panel. */
function Shown({ live, payload, attached }: { live: Overlay; payload: Payload; attached: string }) {
  const shown = payload.status === "ok" ? (payload.fresh ? payload.payload : undefined) : payload.status === "loading" ? undefined : null;
  useEffect(() => {
    if (shown !== undefined) live.show(shown, false);
  }, [live, shown]);
  useEffect(() => landed(payload), [payload, attached]);
  return null;
}

function LegendStrip({ legend }: { legend: Legend }) {
  const t = useT();
  if (legend.heat === null && legend.pressure === null) return null;
  return (
    <div className="pointer-events-none absolute bottom-7 left-2 flex w-72 flex-col gap-1.5 rounded-control border border-border bg-background/85 px-2.5 py-2 text-caption text-ink-mid shadow-md backdrop-blur">
      {legend.heat && (
        <div className="flex flex-col gap-1">
          <span className="font-medium text-ink">
            {legend.heat.name}
            {legend.heat.focus !== null && <span className="font-normal text-ink-soft"> — {legend.heat.focus}</span>}
          </span>
          <div className="h-2 rounded-full" style={{ background: `linear-gradient(to right, ${legend.heat.stops.join(", ")})` }} />
          {legend.heat.ticks !== null && (
            <div className="flex justify-between tabular-nums">
              {legend.heat.ticks.map((tick, i) => (
                <span key={i}>{tick}</span>
              ))}
            </div>
          )}
          <span className="text-ink-soft">
            {legend.heat.scale === "linear" ? t(placementKeys.legend.linear, "{n} cells in view · colour by magnitude", { n: legend.heat.cells }) : t(placementKeys.legend.percentile, "{n} cells in view · colour by percentile", { n: legend.heat.cells })}
          </span>
        </div>
      )}
      {legend.pressure && (
        <div className="flex flex-col gap-1">
          <div className="h-2 w-16 shrink-0 rounded-full" style={{ background: `linear-gradient(to right, ${legend.pressure.stops.join(", ")})` }} />
          <span>{legend.pressure.note}</span>
          {legend.pressure.levels !== null && <span className="tabular-nums text-ink-soft">{legend.pressure.levels.map((l) => `p${Math.round(l.q * 100)} ${l.value}`).join(" · ")}</span>}
        </div>
      )}
    </div>
  );
}
