"use client";

import { Button, FloatingPanel, type FloatingPanelHandle, type Placement, useBinding, useKeymap } from "@evinvest/uikit";
import { Maximize2, Minimize2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { type WinKey, closeWindow, floatWindow, hideWindow, keepOpen, pageOf, pageWindow, shownAt, subjectOf, useShown, windowKeys } from "@/entities/gmb";
import { useT } from "@/shared/i18n";
import { type At, go } from "@/shared/lib/location";
import { DESKTOP_QUERY, useMediaQuery } from "@/shared/lib/use-media-query";
import { useFloatie } from "@/shared/ui/floatie";
import { useButtonSize } from "@/shared/ui/touch";

import { faceOf } from "./faces";
const TOP_PX = 64;

/** Where each window was left over the map, by its subject, so it comes back there from the hotbar (sessionStorage, as the open set). */
const PLACES = "sa-window-places";

function places(): Record<string, Placement> {
  const raw = sessionStorage.getItem(PLACES);
  const all: unknown = raw === null ? {} : JSON.parse(raw);
  if (typeof all !== "object" || all === null || Array.isArray(all)) throw new Error(`sessionStorage["${PLACES}"] is not a map of placements: ${raw}`);
  return all as Record<string, Placement>;
}

/** The window shown over the map, from md up; below it, its page instead. */
export function Floats({ hidden }: { hidden: boolean }) {
  const shown = useShown();
  if (shown === null || shown.page || hidden) return null;
  return <WindowFrame key={subjectOf(shown.key)} k={shown.key} />;
}

function WindowFrame({ k }: { k: WinKey }) {
  const t = useT();
  const desk = useMediaQuery(DESKTOP_QUERY);
  const button = useButtonSize();
  const face = faceOf(k);
  const width = typeof face.width === "number" ? face.width : face.width(k);
  const page = JSON.stringify(pageOf(k));
  const body = useRef<HTMLDivElement>(null);
  const keys = useFloatie(body, desk ? () => pageWindow(k) : null);
  const toPage = keys === undefined ? t(windowKeys.toPage, "Its own page") : `${t(windowKeys.toPage, "Its own page")} · ${keys}`;
  const subject = subjectOf(k);
  const [placed] = useState(() => places()[subject]);
  const panel = useRef<FloatingPanelHandle>(null);
  const centerName = t(windowKeys.center, "To the middle");
  useBinding({ on: { key: "c" }, id: "window-center", run: { label: centerName, run: desk ? () => panel.current!.center() : null } });
  const centerKeys = useKeymap().bound.get("window-center")?.sequence;
  useEffect(() => {
    if (!desk) go(JSON.parse(page) as At, "replace");
  }, [desk, page]);
  if (!desk) return null;
  return (
    <FloatingPanel
      handle={panel}
      title={<face.Title k={k} page={false} />}
      at={placed ?? { x: Math.max(16, (window.innerWidth - width) / 2), y: TOP_PX }}
      size={placed?.size ?? null}
      width={width}
      onPlaced={(p) => sessionStorage.setItem(PLACES, JSON.stringify({ ...places(), [subject]: p }))}
      onClose={() => closeWindow(k)}
      onMinimize={hideWindow}
      closeLabel={t(windowKeys.close, "Close the window")}
      minimizeLabel={t(windowKeys.minimize, "To the hotbar")}
      centerLabel={centerKeys === undefined ? centerName : `${centerName} · ${centerKeys}`}
      resizeLabel={t(windowKeys.resize, "Resize")}
      actions={
        <Button variant="ghost" size={button("sm")} icon aria-label={t(windowKeys.toPage, "Its own page")} title={toPage} onClick={() => pageWindow(k)}>
          <Maximize2 aria-hidden />
        </Button>
      }
    >
      <div ref={body}>
        <face.Body k={k} page={false} />
      </div>
    </FloatingPanel>
  );
}

/** A window as its own page: on a phone always, from md up when it wants the room. */
export function WindowPage({ at }: { at: At }) {
  const t = useT();
  const desk = useMediaQuery(DESKTOP_QUERY);
  const button = useButtonSize();
  const shown = shownAt(at);
  if (shown === null || !shown.page) throw new Error(`${at.view} is no window's page`);
  const k = shown.key;
  const face = faceOf(k);
  const el = useRef<HTMLDivElement>(null);
  const keys = useFloatie(el, desk ? () => floatWindow(k) : null);
  return (
    <div ref={el} className="flex min-h-0 flex-1 flex-col">
      <header className="flex items-center gap-2 border-b border-border px-4 py-2 text-sm">
        <span className="flex min-w-0 flex-1 items-center">
          <face.Title k={k} page />
        </span>
        {desk && (
          <Button
            variant="ghost"
            size={button("sm")}
            icon
            aria-label={t(windowKeys.toMap, "Over the map")}
            title={keys === undefined ? t(windowKeys.toMap, "Over the map") : `${t(windowKeys.toMap, "Over the map")} · ${keys}`}
            onClick={() => floatWindow(k)}
          >
            <Minimize2 aria-hidden />
          </Button>
        )}
      </header>
      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
        <face.Body k={k} page />
      </div>
    </div>
  );
}

/** The shown window joins the open set, its entry following where it is (its tab, its country). */
export function useTrackOpen(): void {
  const shown = useShown();
  const key = shown === null ? null : JSON.stringify(shown.key);
  useEffect(() => {
    if (key !== null) keepOpen(JSON.parse(key) as WinKey);
  }, [key]);
}
