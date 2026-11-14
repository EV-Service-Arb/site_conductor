"use client";

import { Hotbar, type HotbarAction, StatusItem, type StatusTone, useBinding } from "@evinvest/uikit";
import { Layers, MapPin, Satellite } from "lucide-react";
import { usePathname } from "next/navigation";

import { type T, useT } from "@/shared/i18n";
import type { LiveStatus } from "@/shared/lib/live";
import { RunningBrowsers } from "@/features/browser-session";
import { MapPanel, MapPreview } from "@/features/placement-overlay";
import { setBase, showPlaces, useBase, usePlacesShown } from "@/shared/map";

const LIVE: Record<LiveStatus, { tone: StatusTone; label: (t: T) => string }> = {
  live: { tone: "positive", label: (t) => t("Live") },
  connecting: { tone: "neutral", label: (t) => t("Connecting") },
  reconnecting: { tone: "warn", label: (t) => t("Reconnecting") },
  offline: { tone: "error", label: (t) => t("Offline") },
  paused: { tone: "muted", label: (t) => t("Paused") },
  closed: { tone: "error", label: (t) => t("Disconnected") },
};

/** The strip along the bottom of every signed-in screen: the screen's actions, and how the tab is connected. */
export function PanelHotbar({ live }: { live: LiveStatus }) {
  const t = useT();
  const pathname = usePathname();
  const places = usePlacesShown();
  const onMap = pathname === "/";
  const satellite = useBase() === "satellite";
  const togglePlaces = () => showPlaces(!places);
  const toggleBase = () => setBase(satellite ? "schematic" : "satellite");
  useBinding({ on: { key: "p" }, run: { id: "places", label: t("Google's places"), run: onMap ? togglePlaces : null } });
  useBinding({ on: { key: "b" }, run: { id: "base", label: t("Satellite"), run: onMap ? toggleBase : null } });
  const actions: HotbarAction[] = onMap
    ? [
        { id: "map", label: t("Map"), icon: Layers, panel: <MapPanel />, preview: <MapPreview /> },
        { id: "base", label: t("Satellite"), icon: Satellite, binding: "base", pressed: satellite, onSelect: toggleBase },
        { id: "places", label: t("Google's places"), icon: MapPin, binding: "places", pressed: places, onSelect: togglePlaces },
      ]
    : [];
  return (
    <Hotbar
      actions={actions}
      status={
        <>
          <RunningBrowsers />
          <StatusItem tone={LIVE[live].tone}>{LIVE[live].label(t)}</StatusItem>
        </>
      }
      labels={{ region: t("Hotbar"), customize: t("Customize"), title: t("Customize hotbar"), description: t("Pinned actions show in the bar."), reset: t("Reset to default"), done: t("Done"), hide: t("Hide from hotbar") }}
    />
  );
}
