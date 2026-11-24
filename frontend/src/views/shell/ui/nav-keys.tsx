"use client";

import { type KeyBinding, useBinding } from "@evinvest/uikit";
import { mayOpen } from "@evinvest/views";

import { PANEL } from "@config";

import { seesSites, useMe } from "@/entities/session";
import { VIEWS } from "@/shared/api/views";
import { type T, useT, admin, keys, nav, usage } from "@/shared/i18n";
import { type At, go } from "@/shared/lib/location";

type Keyed = { [N in keyof typeof VIEWS.views]: (typeof VIEWS.views)[N] extends { key: string } ? N : never }[keyof typeof VIEWS.views];

const LABEL: Record<Keyed, (t: T) => string> = {
  home: (t) => t(nav.home, "Home"),
  sites: (t) => t(nav.sites, "Sites"),
  review_archive: (t) => t(nav.reviewArchive, "Review Archive"),
  sources: (t) => t(nav.sources, "Sources"),
  admin_tokens: (t) => t(admin.tokens, "Tokens"),
  tokens: (t) => t(nav.tokens, "Tokens"),
  usage: (t) => t(usage.title, "Usage"),
  wallet: (t) => t(nav.wallet, "Wallet"),
  members: (t) => t(nav.members, "View as member"),
  alerts: (t) => t(nav.alerts, "Alerts"),
  email: (t) => t(nav.email, "Email"),
  account: (t) => t(nav.account, "Profile"),
  kits: (t) => t(nav.kits, "Review kits"),
};

/** `leader g …`: every screen the caller may open, by its view's key. */
export function NavKeys() {
  const t = useT();
  const me = useMe();
  const opens = (view: Keyed) => (view === "sites" ? seesSites(me) : (view !== "wallet" || PANEL.walletPage) && mayOpen(VIEWS, view, me.permissions));
  useBinding({
    on: { key: "g" },
    label: t(keys.go, "Go to"),
    children: (Object.keys(LABEL) as Keyed[]).map((view): KeyBinding => {
      const entry = VIEWS.views[view];
      // a nest's own page is the root of the service's views under it
      const at = { view, params: "nest" in entry ? { below: "" } : {} } as At;
      return { on: { key: entry.key }, id: view, run: { label: LABEL[view](t), run: opens(view) ? () => go(at) : null } };
    }),
  });
  return null;
}
