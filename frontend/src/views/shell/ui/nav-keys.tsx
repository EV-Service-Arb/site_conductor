"use client";

import { type KeyBinding, useBinding } from "@evinvest/uikit";
import { useRouter } from "next/navigation";

import { PANEL } from "@config";

import { ownScreens, seesSites, useMe } from "@/entities/session";
import { ROUTES } from "@/shared/config/routes";
import { useT } from "@/shared/i18n";

/** `leader g …`: every screen the caller may open, by key. */
export function NavKeys() {
  const t = useT();
  const me = useMe();
  const router = useRouter();
  const own = ownScreens(me, PANEL.walletPage);
  const go = (key: string, label: string, href: string, may = true): KeyBinding => ({ on: { key }, run: { label, run: may ? () => router.push(href) : null } });
  useBinding({
    on: { key: "g" },
    label: t("Go to"),
    children: [
      go("h", t("Home"), "/"),
      go("s", t("Sites"), ROUTES.sites, seesSites(me)),
      go("r", t("Review Archive"), ROUTES.reviewArchive),
      go("c", t("Sources"), ROUTES.sources, own.includes("sources")),
      go("t", t("Tokens"), ROUTES.tokens, own.includes("tokens")),
      go("u", t("Usage"), ROUTES.usage, own.includes("usage")),
      go("w", t("Wallet"), ROUTES.wallet, own.includes("wallet")),
      go("e", t("Alerts"), ROUTES.alerts),
      go("y", t("Profile"), ROUTES.account),
    ],
  });
  return null;
}
