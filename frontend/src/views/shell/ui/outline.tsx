"use client";

import { type NavSection, ShellNav, cn } from "@evinvest/uikit";
import { Globe, MapPin, Settings, Store } from "lucide-react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import type { CSSProperties } from "react";

import { seesSites, useMe } from "@/entities/session";
import { SitesElement } from "@/features/sites-element";
import { ROUTES } from "@/shared/config/routes";
import { useT } from "@/shared/i18n";

import { GmbTree } from "./gmb-tree";

const WIDTH = { "--shell-rail-w": "18rem" } as CSSProperties;

/**
 * The left of every page: the open app's own views, then the caller's GMBs. `mfe-sites-nav` stays
 * mounted, only hidden, off the sites (CONTRACT §9): it keeps the tab title's count on every page.
 */
export function Outline({ folded, onNavigate, className }: { folded: boolean; onNavigate?: () => void; className?: string }) {
  const t = useT();
  const me = useMe();
  const pathname = usePathname();
  const app = pathname.startsWith("/sites") ? "sites" : pathname.startsWith("/review_archive") ? "archive" : null;
  const sections: NavSection[] = [
    ...(seesSites(me)
      ? [
          {
            id: "sites",
            label: t("nav.sites"),
            icon: Globe,
            hidden: app !== "sites",
            // A click on one of its links has routed (CONTRACT §9).
            content: (
              <div onClick={() => onNavigate?.()}>
                <SitesElement tag="mfe-sites-nav" />
              </div>
            ),
          },
        ]
      : []),
    ...(app === "archive"
      ? [
          {
            id: "archive",
            items: [
              { id: "places", href: ROUTES.reviewArchive, label: t("reviewArchive.places"), icon: MapPin },
              { id: "settings", href: ROUTES.reviewArchiveSettings, label: t("reviewArchive.settings"), icon: Settings },
            ],
          },
        ]
      : []),
    { id: "gbps", label: t("nav.gbps"), icon: Store, content: <GmbTree {...(onNavigate && { onNavigate })} /> },
  ];
  return (
    <ShellNav
      groups={sections}
      pathname={pathname}
      linkComponent={Link}
      folded={folded}
      labels={{ primary: t("nav.outline") }}
      {...(onNavigate && { onNavigate: () => onNavigate() })}
      style={WIDTH}
      className={cn("gap-4 px-2 pt-2 pb-2", className)}
    />
  );
}
