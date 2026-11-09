"use client";

import { useEffect, useState } from "react";

import { type Legend, attach, locationsIn, setInView, useMapView, usePlacement } from "../model/placement";

type Attached = NonNullable<Awaited<ReturnType<typeof attach>>>;

/** gmaps_optimal_placement's drawing on `map`, as the panel's URL state says, over the studies in view; and its legend. */
export function PlacementOverlay({ map }: { map: google.maps.Map }) {
  const placement = usePlacement();
  const view = useMapView();
  const [live, setLive] = useState<Attached | null>(null);
  useEffect(() => {
    let mounted = true;
    void attach(map).then((a) => mounted && setLive(a));
    return () => {
      mounted = false;
    };
  }, [map]);

  useEffect(() => {
    if (live === null) return;
    const update = () => {
      const bounds = map.getBounds();
      const zoom = map.getZoom();
      if (bounds === undefined || zoom === undefined) return; // before the first render: the next idle tells
      setInView(locationsIn(live.locations, bounds, zoom));
    };
    const idle = map.addListener("idle", update);
    update();
    return () => idle.remove();
  }, [map, live]);

  const inView = placement.status === "ok" ? placement.inView : "zoom";
  const shown = inView === "zoom" ? "" : inView.join("\n");
  useEffect(() => {
    void live?.handle.show(view.trade, shown === "" ? [] : shown.split("\n"));
  }, [live, view.trade, shown]);
  // the overlay refuses a layer no shown study offers; `?heat` waits for the studies that offer it
  const heat = placement.status === "ok" && placement.layers.some((l) => l.id === view.heat) ? view.heat : null;
  useEffect(() => {
    live?.handle.heat(heat);
  }, [live, heat]);
  const lambda = view.pressure?.lambda;
  const pressure = view.pressure !== null && view.trade !== null;
  useEffect(() => {
    live?.handle.pressure(pressure ? (lambda === undefined ? {} : { lambda }) : null);
  }, [live, pressure, lambda]);
  const competitors = view.competitors && view.trade !== null;
  useEffect(() => {
    live?.handle.competitors(competitors);
  }, [live, competitors]);

  return placement.status === "ok" && placement.legend !== null && shown !== "" ? <LegendStrip legend={placement.legend} /> : null;
}

function LegendStrip({ legend }: { legend: Legend }) {
  if (legend.heat === null && legend.pressure === null) return null;
  return (
    <div className="pointer-events-none absolute bottom-7 left-2 flex w-72 flex-col gap-1.5 rounded-control border border-border bg-background/85 px-2.5 py-2 text-caption text-ink-mid shadow-md backdrop-blur">
      {legend.heat && (
        <div className="flex flex-col gap-1">
          <span className="font-medium text-ink">{legend.heat.name}</span>
          <div className="h-2 rounded-full" style={{ background: `linear-gradient(to right, ${legend.heat.stops.join(", ")})` }} />
          <div className="flex justify-between tabular-nums">
            {legend.heat.ticks.map((tick, i) => (
              <span key={i}>{tick}</span>
            ))}
          </div>
          <span className="text-ink-soft">{legend.heat.note}</span>
        </div>
      )}
      {legend.pressure && (
        <div className="flex items-center gap-2">
          <div className="h-2 w-16 shrink-0 rounded-full" style={{ background: `linear-gradient(to right, ${legend.pressure.stops.join(", ")})` }} />
          <span>{legend.pressure.note}</span>
        </div>
      )}
    </div>
  );
}
