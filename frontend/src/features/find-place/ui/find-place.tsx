"use client";

import { cn, useDismissableLayer } from "@evinvest/uikit";
import { Search } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

import { SuggestSearch, frameOf, placesLike } from "@/entities/gmb";
import { find, useT } from "@/shared/i18n";
import { corner, pin } from "@/shared/map";
import { notifyFailure } from "@/shared/ui/notify";

/** The map's search: a place picked is framed and marked, the mark there until clicked or the next pick. */
export function FindPlace({ map }: { map: google.maps.Map }) {
  const t = useT();
  const box = corner(map, "find");
  const [open, setOpen] = useState(false);
  const button = useRef<HTMLButtonElement>(null);
  const layer = useDismissableLayer({ enabled: open, onDismiss: () => setOpen(false), exclude: [button] });
  const mark = useRef<{ remove: () => void } | null>(null);
  const unmark = () => {
    mark.current?.remove();
    mark.current = null;
  };
  useEffect(() => unmark, [map]);

  const go = async (place_id: string) => {
    setOpen(false);
    try {
      const { name, location, viewport } = await frameOf(place_id);
      unmark();
      const dot = document.createElement("button");
      dot.type = "button";
      dot.title = t(find.unmark, "{name} · click to remove the mark", { name });
      dot.className = "block size-5 rounded-full border-4 border-primary bg-background shadow-md ring-2 ring-background";
      dot.style.zIndex = "3";
      dot.addEventListener("click", unmark);
      mark.current = pin(map, dot, location);
      map.fitBounds(viewport);
    } catch (e) {
      notifyFailure(e, t);
    }
  };

  return createPortal(
    <div className="relative">
      <button
        ref={button}
        type="button"
        aria-expanded={open}
        aria-label={t(find, "Find a place")}
        title={t(find, "Find a place")}
        onClick={() => setOpen(!open)}
        className={cn("flex size-10 items-center justify-center rounded-sm bg-card text-ink shadow-overlay ring-1 ring-border transition-colors hover:bg-muted", open && "bg-muted ring-ring")}
      >
        <Search aria-hidden className="size-5" />
      </button>
      {open && (
        <div ref={layer} className="absolute top-0 right-full mr-2 w-80 max-w-[70vw]">
          <SuggestSearch autoFocus placeholder={t(find.placeholder, "An address, a town or a business")} none={t(find.none, "No place found.")} ask={placesLike} onPick={(s) => void go(s.place_id)} className="bg-card shadow-overlay" />
        </div>
      )}
    </div>,
    box,
  );
}
