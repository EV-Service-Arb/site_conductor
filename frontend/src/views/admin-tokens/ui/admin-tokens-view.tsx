"use client";

import { PageFrame, Settled, Skeleton } from "@evinvest/uikit";
import { useState } from "react";

import { may, useMe } from "@/entities/session";
import { tokens } from "@/entities/tokens";
import { api, ok } from "@/shared/api";
import type { TokenHolder } from "@/shared/api/schema";
import { HoldersCard } from "./holders-card";
import { MetersCard } from "./meters-card";
import { GrantDialog } from "@/features/grant-tokens";
import { PricesCard } from "@/features/price-tokens";
import { ROUTES } from "@/shared/config/routes";
import { useT } from "@/shared/i18n";
import { useResource } from "@/shared/lib/use-resource";
import { EmptyState } from "@/shared/ui/empty-state";
import { ErrorState } from "@/shared/ui/error-state";

/** Prices, everyone's spending and balances, for whoever grants tokens. */
export function AdminTokensView() {
  const t = useT();
  const me = useMe();
  const holders = useResource("admin.tokens", async () => (await ok(api.GET("/admin/tokens"))).users, "admin.tokens", { live: ["admin"] });
  const prices = useResource("admin.prices", async () => (await ok(api.GET("/admin/tokens/prices"))).prices, "admin.prices", { live: ["admin"] });
  const meters = useResource("admin.meters", async () => ok(api.GET("/admin/meters")));
  const [changing, setChanging] = useState<TokenHolder | null>(null);

  if (!may(me, "sa:admin:tokens:grant")) {
    return (
      <PageFrame breakpoint="md" title={t(tokens.admin.title, "Tokens of everyone")}>
        <ErrorState failure={{ kind: "forbidden", message: "" }} />
      </PageFrame>
    );
  }

  return (
    <PageFrame breakpoint="md" title={t(tokens.admin.title, "Tokens of everyone")}>
      {prices.status === "error" && <ErrorState failure={prices.failure} onRetry={prices.reload} />}
      {meters.status === "error" && <ErrorState failure={meters.failure} onRetry={meters.reload} />}
      {meters.status === "ok" && <MetersCard meters={meters.data} />}
      {prices.status === "ok" && <PricesCard prices={prices.data} onSaved={prices.reload} />}
      <section className="flex flex-col gap-2">
        <h2 className="text-sm font-semibold text-ink">{t(tokens.admin.spending, "Spending")}</h2>
        <iframe
          title={t(tokens.admin.spending, "Spending")}
          src={`${ROUTES.grafana}d/sa-tokens/?kiosk&hideLogo&_dash.hideLinks&theme=dark`}
          className="h-screen w-full rounded-lg border border-border"
        />
      </section>
      <Settled loading={holders.status === "loading"} skeleton={<Skeleton className="h-40 w-full" />}>
        {holders.status === "error" && <ErrorState failure={holders.failure} onRetry={holders.reload} />}
        {holders.status === "ok" && holders.data.length === 0 && <EmptyState title={t(tokens.admin.empty, "Nobody has signed in yet.")} description="" />}
        {holders.status === "ok" && holders.data.length > 0 && (
          <HoldersCard holders={holders.data} onChange={setChanging} />
        )}
      </Settled>
      {changing && (
        <GrantDialog
          holder={changing}
          onClose={() => setChanging(null)}
          onDone={() => {
            setChanging(null);
            holders.reload();
          }}
        />
      )}
    </PageFrame>
  );
}
