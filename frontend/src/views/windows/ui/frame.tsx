"use client";

import { Button, FloatingPanel } from "@evinvest/uikit";
import { Maximize2, Minimize2 } from "lucide-react";
import { useEffect } from "react";

import { type WinKey, closeWindow, floatWindow, keepOpen, pageOf, pageWindow, shownAt, subjectOf, useShown, windowKeys } from "@/entities/gmb";
import { useT } from "@/shared/i18n";
import { type At, go } from "@/shared/lib/location";
import { DESKTOP_QUERY, useMediaQuery } from "@/shared/lib/use-media-query";
import { useButtonSize } from "@/shared/ui/touch";

import { faceOf } from "./faces";

const TOP_PX = 64;

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
  const page = JSON.stringify(pageOf(k));
  useEffect(() => {
    if (!desk) go(JSON.parse(page) as At, "replace");
  }, [desk, page]);
  if (!desk) return null;
  return (
    <FloatingPanel
      title={<face.Title k={k} page={false} />}
      at={{ x: Math.max(16, (window.innerWidth - face.width) / 2), y: TOP_PX }}
      onClose={() => closeWindow(k)}
      closeLabel={t(windowKeys.close, "Close the window")}
      actions={
        <Button variant="ghost" size={button("sm")} icon aria-label={t(windowKeys.toPage, "Its own page")} title={t(windowKeys.toPage, "Its own page")} onClick={() => pageWindow(k)}>
          <Maximize2 aria-hidden />
        </Button>
      }
    >
      <div className="max-w-full" style={{ width: face.width }}>
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
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <header className="flex items-center gap-2 border-b border-border px-4 py-2 text-sm">
        <span className="flex min-w-0 flex-1 items-center">
          <face.Title k={k} page />
        </span>
        {desk && (
          <Button variant="ghost" size={button("sm")} onClick={() => floatWindow(k)}>
            <Minimize2 aria-hidden />
            {t(windowKeys.toMap, "Over the map")}
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
