"use client";

import { type KeyBinding, useBinding } from "@evinvest/uikit";
import { useRouter } from "next/navigation";

import { PANEL } from "@config";

import { may, ownScreens, seesSites, useMe } from "@/entities/session";
import { ROUTES } from "@/shared/config/routes";
import { useT, admin, keys, nav, usage } from "@/shared/i18n";

/** `leader g …`: every screen the caller may open, by key. */
export function NavKeys() {
  const t = useT();
  const me = useMe();
  const router = useRouter();
  const own = ownScreens(me, PANEL.walletPage);
  const go = (key: string, label: string, href: string, may = true): KeyBinding => ({ on: { key }, id: href, run: { label, run: may ? () => router.push(href) : null } });
  useBinding({
    on: { key: "g" },
    label: t(keys.go, "Go to"),
    children: [
      go("h", t(nav.home, "Home"), "/"),
      go("s", t(nav.sites, "Sites"), ROUTES.sites, seesSites(me)),
      go("r", t(nav.reviewArchive, "Review Archive"), ROUTES.reviewArchive),
      go("c", t(nav.sources, "Sources"), ROUTES.sources, may(me, "sa:admin:sources:manage")),
      go("k", t(admin.tokens, "Tokens"), ROUTES.adminTokens, may(me, "sa:admin:tokens:grant")),
      go("t", t(nav.tokens, "Tokens"), ROUTES.tokens, own.includes("tokens")),
      go("u", t(usage.title, "Usage"), ROUTES.usage, own.includes("usage")),
      go("w", t(nav.wallet, "Wallet"), ROUTES.wallet, own.includes("wallet")),
      go("e", t(nav.alerts, "Alerts"), ROUTES.alerts),
      go("y", t(nav.account, "Profile"), ROUTES.account),
    ],
  });
  return null;
}
