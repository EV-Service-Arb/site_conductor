"use client";

import type { NavIcon, NavSection } from "@evinvest/uikit";
import { Archive, Coins, Globe, KeyRound, MapPin, Settings, ShieldCheck } from "lucide-react";
import { href } from "@evinvest/views";

import { may, seesSites, useMe } from "@/entities/session";
import { SitesElement } from "@/features/sites-element";
import { VIEWS } from "@/shared/api/views";
import { useT, admin, nav, reviewArchive } from "@/shared/i18n";
import { type At, useAt, useCarried } from "@/shared/lib/location";

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
  const at = useAt();
  const carried = useCarried();
  const link = (to: At) => href(VIEWS, to, carried);
  const adminItems = [
    ...(may(me, "sa:admin:tokens:grant")
      ? [{ id: "tokens", href: link({ view: "admin_tokens", params: {} }), label: t(admin.tokens, "Tokens"), icon: Coins, description: t(admin.tokens.description, "Prices, spending and everyone's balance") }]
      : []),
    ...(may(me, "sa:admin:sources:manage") ? [{ id: "sources", href: link({ view: "sources", params: {} }), label: t(nav.sources, "Sources"), icon: KeyRound, description: t(admin.sources.description, "Source keys and the sites they connect") }] : []),
  ];
  return [
    ...(seesSites(me)
      ? [
          {
            href: link({ view: "sites", params: { below: "" } }),
            label: t(nav.sites, "Sites"),
            icon: Globe,
            description: t(nav.sites.description, "Leads, locations, pricing, invoices"),
            here: at.view === "sites",
            section: {
              id: "sites",
              label: t(nav.sites, "Sites"),
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
      href: link({ view: "review_archive.mine.home", params: {} }),
      label: t(nav.reviewArchive, "Review Archive"),
      icon: Archive,
      description: t(nav.reviewArchive.description, "Every place's reviews and their state"),
      here: at.view.startsWith("review_archive"),
      section: {
        id: "archive",
        items: [
          { id: "places", href: link({ view: "review_archive.mine.home", params: {} }), label: t(reviewArchive.places, "Places"), icon: MapPin, description: t(reviewArchive.places.description, "Each place's reviews, newest first") },
          { id: "settings", href: link({ view: "review_archive.mine.settings", params: {} }), label: t(reviewArchive.settings, "Settings"), icon: Settings, description: t(reviewArchive.settings.description, "How the archive scans and keeps reviews") },
        ],
      },
    },
    ...(adminItems[0] !== undefined
      ? [
          {
            href: adminItems[0].href,
            label: t(nav.admin, "Admin"),
            icon: ShieldCheck,
            description: t(nav.admin.description, "Prices, everyone's tokens, source keys"),
            here: at.view === "admin_tokens" || at.view === "sources",
            section: { id: "admin", items: adminItems },
          },
        ]
      : []),
  ];
}
