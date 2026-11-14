"use client";

import type { NavIcon, NavSection } from "@evinvest/uikit";
import { Archive, Globe, MapPin, Settings } from "lucide-react";
import { usePathname } from "next/navigation";

import { seesSites, useMe } from "@/entities/session";
import { SitesElement } from "@/features/sites-element";
import { ROUTES } from "@/shared/config/routes";
import { useT } from "@/shared/i18n";

export interface App {
  href: string;
  label: string;
  icon: NavIcon;
  description: string;
  here: boolean;
  /** Its own views: the outline's section while it is open, its top-bar item's menu on hover. */
  section: NavSection;
}

/** The caller's apps, in the top bar's order. */
export function useApps(onNavigate?: () => void): App[] {
  const t = useT();
  const me = useMe();
  const pathname = usePathname();
  return [
    ...(seesSites(me)
      ? [
          {
            href: ROUTES.sites,
            label: t("Sites"),
            icon: Globe,
            description: t("Leads, locations, pricing, invoices"),
            here: pathname.startsWith("/sites"),
            section: {
              id: "sites",
              label: t("Sites"),
              icon: Globe,
              menu: true,
              // A click on one of its links has routed (CONTRACT §9).
              content: (
                <div onClick={() => onNavigate?.()}>
                  <SitesElement tag="mfe-sites-nav" />
                </div>
              ),
            },
          },
        ]
      : []),
    {
      href: ROUTES.reviewArchive,
      label: t("Review Archive"),
      icon: Archive,
      description: t("Every place's reviews and their state"),
      here: pathname.startsWith("/review_archive"),
      section: {
        id: "archive",
        items: [
          { id: "places", href: ROUTES.reviewArchive, label: t("Places"), icon: MapPin, description: t("Each place's reviews, newest first") },
          { id: "settings", href: ROUTES.reviewArchiveSettings, label: t("Settings"), icon: Settings, description: t("How the archive scans and keeps reviews") },
        ],
      },
    },
  ];
}
