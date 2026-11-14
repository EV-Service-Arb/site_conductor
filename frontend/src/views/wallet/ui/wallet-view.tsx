"use client";

import { PageFrame } from "@evinvest/uikit";
import { PANEL } from "@config";
import { WalletElement } from "@/features/wallet-element";
import { useT } from "@/shared/i18n";
import { EmptyState } from "@/shared/ui/empty-state";

/** The wallet screen; with the flag off the route exists in the export but shows no wallet. */
export function WalletView() {
  const t = useT();
  return (
    <PageFrame breakpoint="md" title={t("Wallet")} {...(PANEL.walletPage ? { description: t("Balances and recent activity of the Service-Arb compartment.") } : {})} width="content">
      {PANEL.walletPage ? <WalletElement /> : <EmptyState title={t("Wallet is unavailable")} description={t("The wallet is not enabled for this panel yet.")} />}
    </PageFrame>
  );
}
