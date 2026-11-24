"use client";

import { NavBadge, type NavIcon, type NavSection } from "@evinvest/uikit";
import { Archive, Bot, Coins, Globe, Inbox, KeyRound, Mail, MapPin, Settings, ShieldCheck } from "lucide-react";
import type { ReactNode } from "react";
import { href, mayOpen } from "@evinvest/views";

import { mail, shownIn, useCases } from "@/entities/mail";
import { seesSites, useMe } from "@/entities/session";
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
  /** What waits there. */
  badge?: ReactNode;
  /** Its own views: the outline's section while it is open, its top-bar item's menu on hover. */
  section: NavSection;
}

/** The caller's apps, in the top bar's order. */
export function useApps(onNavigate?: () => void): App[] {
  const t = useT();
  const me = useMe();
  const at = useAt();
  const carried = useCarried();
  const { shown } = useCases();
  const badge = <NavBadge count={shown === null ? 0 : shownIn(shown, "needs_you").length} label={(n) => t(mail.count, "{n} need you", { n })} />;
  const link = (to: At) => href(VIEWS, to, carried);
  const adminItems = [
    ...(mayOpen(VIEWS, "admin_tokens", me.permissions)
      ? [{ id: "tokens", href: link({ view: "admin_tokens", params: {} }), label: t(admin.tokens, "Tokens"), icon: Coins, description: t(admin.tokens.description, "Prices, spending and everyone's balance") }]
      : []),
    ...(mayOpen(VIEWS, "sources", me.permissions) ? [{ id: "sources", href: link({ view: "sources", params: {} }), label: t(nav.sources, "Sources"), icon: KeyRound, description: t(admin.sources.description, "Source keys and the sites they connect") }] : []),
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
    {
      href: link({ view: "email", params: {} }),
      label: t(nav.email, "Email"),
      icon: Mail,
      description: t(nav.email.description, "Google's mail about the GBPs, and the replies"),
      here: at.view === "email",
      badge,
      section: {
        id: "email",
        items: [
          { id: "threads", href: link({ view: "email", params: {} }), label: t(mail.tab.threads, "Threads"), icon: Inbox, badge, description: t(mail.tab.threads.description, "Each conversation and notice, and whose turn it is"), match: () => at.view === "email" && at.params.show !== "automatic" },
          { id: "automatic", href: link({ view: "email", params: { show: "automatic" } }), label: t(mail.tab.automatic, "Automatic replies"), icon: Bot, description: t(mail.tab.automatic.description, "Which GBPs' mail is answered by itself"), match: () => at.view === "email" && at.params.show === "automatic" },
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
