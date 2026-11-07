"use client";

import { Hotbar, type HotbarAction, StatusItem, type StatusTone, useBinding } from "@evinvest/uikit";
import { MapPin } from "lucide-react";
import { usePathname } from "next/navigation";

import { useT } from "@/shared/i18n";
import type { LiveStatus } from "@/shared/lib/live";
import { showPlaces, usePlacesShown } from "@/shared/map";

const LIVE_TONE: Record<LiveStatus, StatusTone> = { live: "positive", connecting: "neutral", reconnecting: "warn", offline: "error", paused: "muted", closed: "error" };

/** The strip along the bottom of every signed-in screen: the screen's actions, and how the tab is connected. */
export function PanelHotbar({ live }: { live: LiveStatus }) {
  const t = useT();
  const pathname = usePathname();
  const places = usePlacesShown();
  const onMap = pathname === "/";
  const togglePlaces = () => showPlaces(!places);
  useBinding({ on: { key: "p" }, run: { id: "places", label: t("hotbar.places"), run: onMap ? togglePlaces : null } });
  const actions: HotbarAction[] = onMap ? [{ id: "places", label: t("hotbar.places"), icon: MapPin, binding: "places", pressed: places, onSelect: togglePlaces }] : [];
  return (
    <Hotbar
      actions={actions}
      status={<StatusItem tone={LIVE_TONE[live]}>{t(`live.${live}`)}</StatusItem>}
      labels={{ region: t("hotbar"), customize: t("hotbar.customize"), title: t("hotbar.title"), description: t("hotbar.description"), reset: t("hotbar.reset"), done: t("hotbar.done") }}
    />
  );
}
