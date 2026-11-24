"use client";

import { Button } from "@evinvest/uikit";
import { useState } from "react";

import { SuggestSearch, frameOf, peek, placeGmb, placesLike, setOnMap, useGmails, useUndoable, move } from "@/entities/gmb";
import type { Gmb, LatLng } from "@/shared/api/schema";
import { find, home, useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize } from "@/shared/ui/touch";

/** Over the map's bottom edge while a GMB's point is placed: a place search that moves the map and the pin, and the point set where the pin is. */
export function PlacingBar({ gmb, map, at, onMove }: { gmb: Gmb; map: google.maps.Map; at: LatLng; onMove: (at: LatLng) => void }) {
  const t = useT();
  const button = useButtonSize();
  const gmails = useGmails();
  const undoable = useUndoable();
  const [busy, setBusy] = useState(false);

  const set = async () => {
    setBusy(true);
    try {
      undoable(await placeGmb(gmb, at, t));
      gmails.reload();
      setOnMap({ placing: null });
      peek(gmb.id);
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="absolute inset-x-4 bottom-4 flex flex-wrap items-center gap-2 rounded-lg border border-border bg-background p-3 shadow-overlay">
      <p className="min-w-0 flex-1 basis-60 text-sm text-ink">{t(home.placing, "Placing {name} · click the map or drag the pin", { name: gmb.name ?? gmb.place_id })}</p>
      <SuggestSearch
        placeholder={t(find.placeholder, "An address, a town or a business")}
        none={t(find.none, "No place found.")}
        ask={placesLike}
        onPick={(s) =>
          void frameOf(s.place_id).then(
            ({ location, viewport }) => {
              map.fitBounds(viewport);
              onMove(location);
            },
            (e: unknown) => notifyFailure(e, t),
          )
        }
        className="min-w-0 flex-1 basis-60"
      />
      <Button variant="outline" size={button()} onClick={() => setOnMap({ placing: null })}>
        {t(move.cancel, "Cancel")}
      </Button>
      <Button size={button()} disabled={busy} onClick={() => void set()}>
        {t(home.placing.set, "Set here")}
      </Button>
    </div>
  );
}
