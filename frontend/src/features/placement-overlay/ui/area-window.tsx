"use client";

import { Button, CloseButton } from "@evinvest/uikit";
import { SquareDashed } from "lucide-react";
import { useState } from "react";

import { AreaDraw, type Drawn, areaKeys, saveArea, windowKeys } from "@/entities/gmb";
import { Refused } from "@/shared/api";
import { useT, placement as placementKeys } from "@/shared/i18n";
import { failureText } from "@/shared/ui/failure-text";
import { useButtonSize } from "@/shared/ui/touch";

import { setMapView, useMapView } from "../model/placement";

/** The window over the map's left while `?area=` is empty: an area drawn for the study, picked once saved. */
export function StudyAreaWindow({ map }: { map: google.maps.Map }) {
  const t = useT();
  const button = useButtonSize();
  if (useMapView().area !== "") return null;
  return (
    // the map stays clickable beside it and below it
    <div className="pointer-events-none absolute inset-4 flex items-start">
      <section aria-label={t(placementKeys.areaWindow, "A new area for the study")} className="pointer-events-auto flex max-h-full w-full flex-col overflow-y-auto rounded-lg border border-border bg-background shadow-overlay md:w-104">
        <header className="flex items-center gap-2 border-b border-border px-4 py-3">
          <SquareDashed aria-hidden className="size-4 text-ink-soft" />
          <h2 className="flex-1 text-sm font-semibold text-ink">{t(placementKeys.areaWindow, "A new area for the study")}</h2>
          <CloseButton size={button("sm")} label={t(windowKeys.close, "Close the window")} onClose={() => setMapView({ area: null })} />
        </header>
        <AreaDraw map={map} left={window.matchMedia("(min-width: 48rem)").matches ? 16 + 416 : 0} initial={null}>
          {(drawn) => drawn !== null && <Save drawn={drawn} />}
        </AreaDraw>
      </section>
    </div>
  );
}

function Save({ drawn }: { drawn: Drawn }) {
  const t = useT();
  const button = useButtonSize();
  const [busy, setBusy] = useState(false);
  const [refused, setRefused] = useState<string | null>(null);
  const save = async () => {
    setBusy(true);
    try {
      setMapView({ area: await saveArea(drawn) });
    } catch (e) {
      if (!(e instanceof Refused)) throw e;
      if (e.failure.code !== "unauthenticated") setRefused(failureText(e.failure, t));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="flex items-center gap-2 border-t border-border px-4 py-3">
      <span className="min-w-0 flex-1 truncate font-mono text-xs text-ink-soft">{refused ?? (drawn.area === null ? "" : `/${drawn.area.slug}/`)}</span>
      <Button size={button("sm")} disabled={busy || drawn.area === null} onClick={() => void save()}>
        {t(areaKeys.save, "Save the area")}
      </Button>
    </div>
  );
}
