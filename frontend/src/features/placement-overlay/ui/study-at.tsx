"use client";

import type { ReactNode } from "react";

import type { Area, LatLng } from "@/shared/api/schema";
import { distanceM } from "@/shared/map";

import { useMapView, useStudied } from "../model/placement";
import { Resolve } from "./map-panel";

/** What the map's study says of point `at`: the trade of the category picked, and the area shown if `at` is in it. Nothing until read. */
export function StudyAt({ at, children }: { at: LatLng; children: (said: { trade: string | null; area: Area | null }) => ReactNode }) {
  const view = useMapView();
  const studied = useStudied(view);
  if (studied.status === "loading") return null;
  // the map's panel says why; the form asks for what it would have said
  if (studied.status === "error") return children({ trade: null, area: null });
  const { area } = studied;
  const inside = area !== null && distanceM(area.center, at) <= area.radius_m ? area : null;
  return (
    <Resolve view={view} studied={studied}>
      {(picked) => (view.study !== null && picked === null ? null : children({ trade: picked?.trade.id ?? null, area: inside }))}
    </Resolve>
  );
}
