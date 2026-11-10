"use client";

import type { NavSection } from "@evinvest/uikit";
import { Globe, MapPin, Settings } from "lucide-react";
import { usePathname } from "next/navigation";

import { seesSites, useMe } from "@/entities/session";
import { SitesElement } from "@/features/sites-element";
import { ROUTES } from "@/shared/config/routes";
import { useT } from "@/shared/i18n";

export interface App {
  href: string;
  label: string;
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
            label: t("nav.sites"),
            here: pathname.startsWith("/sites"),
            section: {
              id: "sites",
              label: t("nav.sites"),
              icon: Globe,
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
      label: t("nav.reviewArchive"),
      here: pathname.startsWith("/review_archive"),
      section: {
        id: "archive",
        items: [
          { id: "places", href: ROUTES.reviewArchive, label: t("reviewArchive.places"), icon: MapPin },
          { id: "settings", href: ROUTES.reviewArchiveSettings, label: t("reviewArchive.settings"), icon: Settings },
        ],
      },
    },
  ];
}
