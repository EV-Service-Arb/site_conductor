"use client";

import { Hotbar, type HotbarAction, StatusItem, type StatusTone, useBinding } from "@evinvest/uikit";
import { Layers, MapPin, Maximize2, QrCode, Satellite } from "lucide-react";
import { useState } from "react";

import { closeWindow, floatWindow, hideWindow, openWindow, subjectOf, useOpenWindows, useShown } from "@/entities/gmb";
import { useAt } from "@/shared/lib/location";

import { type T, useT, hotbar, placement } from "@/shared/i18n";
import { live as liveKeys } from "../model/i18n";
import { type LiveStatus, useLiveStatus } from "@evinvest/api/react";
import { RunningBrowsers } from "@/features/browser-session";
import { MapPanel, MapPreview } from "@/features/placement-overlay";
import { setBase, showPlaces, useBase, usePlacesShown } from "@/shared/map";
import { KitsList } from "@/views/kit";
import { FACES, useTrackOpen, useWindowLabel } from "@/views/windows";

const LIVE_NAME = {
  live: (t) => t(liveKeys.live, "Live"),
  connecting: (t) => t(liveKeys.connecting, "Connecting"),
  reconnecting: (t) => t(liveKeys.reconnecting, "Reconnecting"),
  offline: (t) => t(liveKeys.offline, "Offline"),
  paused: (t) => t(liveKeys.paused, "Paused"),
  closed: (t) => t(liveKeys.closed, "Disconnected"),
} satisfies Record<LiveStatus, (t: T) => string>;

const LIVE_TONE: Record<LiveStatus, StatusTone> = { live: "positive", connecting: "neutral", reconnecting: "warn", offline: "error", paused: "muted", closed: "error" };

/** The strip along the bottom of every signed-in screen: the screen's actions, then the windows this tab has open, and how it is connected. */
export function PanelHotbar() {
  const live = useLiveStatus();
  const t = useT();
  const places = usePlacesShown();
  const at = useAt();
  const onMap = at.view === "home";
  const shown = useShown();
  const open = useOpenWindows();
  const label = useWindowLabel();
  useTrackOpen();
  const [kits, showKits] = useState(false);
  const satellite = useBase() === "satellite";
  const togglePlaces = () => showPlaces(!places);
  const toggleBase = () => setBase(satellite ? "schematic" : "satellite");
  useBinding({ on: { key: "p" }, id: "places", run: { label: t(hotbar.places, "Google's places"), run: onMap ? togglePlaces : null } });
  useBinding({ on: { key: "b" }, id: "base", run: { label: t(hotbar.base, "Satellite"), run: onMap ? toggleBase : null } });
  useBinding({ on: { key: "k" }, id: "kit-list", run: { label: t(hotbar.kits, "Review kits"), run: () => showKits(!kits) } });
  // shown over the map: pressed, a click hides it; hidden: a click shows it again; as its page: a click floats it back
  const windows: HotbarAction[] = open.map((k) => {
    const here = shown !== null && subjectOf(shown.key) === subjectOf(k) ? shown : null;
    return {
      id: `window:${subjectOf(k)}`,
      label: label(k),
      icon: here?.page ? Maximize2 : FACES[k.kind].icon,
      pressed: here !== null && !here.page,
      onSelect: () => (here !== null && !here.page ? hideWindow() : floatWindow(here?.key ?? k)),
      onClose: () => closeWindow(k),
    };
  });
  const actions: HotbarAction[] = [
    ...(onMap
      ? [
          { id: "map", label: t(placement.title, "Map"), icon: Layers, panel: <MapPanel />, preview: <MapPreview /> },
          { id: "base", label: t(hotbar.base, "Satellite"), icon: Satellite, binding: "base", pressed: satellite, onSelect: toggleBase },
          { id: "places", label: t(hotbar.places, "Google's places"), icon: MapPin, binding: "places", pressed: places, onSelect: togglePlaces },
        ]
      : []),
    {
      id: "kit-list",
      label: t(hotbar.kits, "Review kits"),
      icon: QrCode,
      binding: "kit-list",
      open: kits,
      onOpenChange: showKits,
      panel: (
        // every button in the list picks: a kit, or New
        <div className="max-h-[60vh] w-80 overflow-y-auto" onClick={(e) => (e.target as Element).closest("button") !== null && showKits(false)}>
          <KitsList go={(at) => openWindow({ kind: "kit", at })} />
        </div>
      ),
    },
    ...windows.filter((w) => w.id !== "window:kit:"),
  ];
  return (
    <Hotbar
      actions={actions}
      status={
        <>
          <RunningBrowsers />
          {live !== null && <StatusItem tone={LIVE_TONE[live]}>{LIVE_NAME[live](t)}</StatusItem>}
        </>
      }
      labels={{ region: t(hotbar, "Hotbar"), customize: t(hotbar.customize, "Customize"), title: t(hotbar.title, "Customize hotbar"), description: t(hotbar.description, "Pinned actions show in the bar."), reset: t(hotbar.reset, "Reset to default"), done: t(hotbar.done, "Done"), hide: t(hotbar.hide, "Hide from hotbar") }}
    />
  );
}
