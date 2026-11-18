"use client";

import { Hotbar, type HotbarAction, StatusItem, type StatusTone, useBinding } from "@evinvest/uikit";
import { useResource } from "@evinvest/api/react";
import { Layers, MapPin, QrCode, Satellite } from "lucide-react";
import { floatKit, useKitFloat } from "@/entities/gmb";
import { api } from "@/shared/api";
import { go, useAt } from "@/shared/lib/location";

import { type T, useT, hotbar, placement } from "@/shared/i18n";
import { live as liveKeys } from "../model/i18n";
import { type LiveStatus, useLiveStatus } from "@evinvest/api/react";
import { RunningBrowsers } from "@/features/browser-session";
import { MapPanel, MapPreview } from "@/features/placement-overlay";
import { setBase, showPlaces, useBase, usePlacesShown } from "@/shared/map";
import { kitOfPage } from "@/views/kit";

const LIVE_NAME = {
  live: (t) => t(liveKeys.live, "Live"),
  connecting: (t) => t(liveKeys.connecting, "Connecting"),
  reconnecting: (t) => t(liveKeys.reconnecting, "Reconnecting"),
  offline: (t) => t(liveKeys.offline, "Offline"),
  paused: (t) => t(liveKeys.paused, "Paused"),
  closed: (t) => t(liveKeys.closed, "Disconnected"),
} satisfies Record<LiveStatus, (t: T) => string>;

const LIVE_TONE: Record<LiveStatus, StatusTone> = { live: "positive", connecting: "neutral", reconnecting: "warn", offline: "error", paused: "muted", closed: "error" };

/** The strip along the bottom of every signed-in screen: the screen's actions, and how the tab is connected. */
export function PanelHotbar() {
  const live = useLiveStatus();
  const t = useT();
  const places = usePlacesShown();
  const at = useAt();
  const onMap = at.view === "home";
  const kits = useResource(api, "/kits");
  const floated = useKitFloat();
  const kitOpen = (onMap ? floated : kitOfPage(at))?.kit ?? null;
  // over the map from it, its page from anywhere else
  const openKit = (slug: string) => (onMap ? floatKit({ kit: slug }) : go(slug === "" ? { view: "kits", params: {} } : { view: "kit", params: { slug } }));
  const satellite = useBase() === "satellite";
  const togglePlaces = () => showPlaces(!places);
  const toggleBase = () => setBase(satellite ? "schematic" : "satellite");
  useBinding({ on: { key: "p" }, id: "places", run: { label: t(hotbar.places, "Google's places"), run: onMap ? togglePlaces : null } });
  useBinding({ on: { key: "b" }, id: "base", run: { label: t(hotbar.base, "Satellite"), run: onMap ? toggleBase : null } });
  useBinding({ on: { key: "k" }, id: "kits", run: { label: t(hotbar.kits, "Review kits"), run: () => openKit("") } });
  const kitActions: HotbarAction[] = kits.status === "ok" ? kits.data.kits.map((k) => ({ id: `kit:${k.slug}`, label: k.name, icon: QrCode, pressed: kitOpen === k.slug, onSelect: () => openKit(k.slug) })) : [];
  const actions: HotbarAction[] = [
    ...(onMap
      ? [
          { id: "map", label: t(placement.title, "Map"), icon: Layers, panel: <MapPanel />, preview: <MapPreview /> },
          { id: "base", label: t(hotbar.base, "Satellite"), icon: Satellite, binding: "base", pressed: satellite, onSelect: toggleBase },
          { id: "places", label: t(hotbar.places, "Google's places"), icon: MapPin, binding: "places", pressed: places, onSelect: togglePlaces },
        ]
      : []),
    { id: "kits", label: t(hotbar.kits, "Review kits"), icon: QrCode, binding: "kits", pressed: kitOpen === "", onSelect: () => openKit("") },
    ...kitActions,
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
