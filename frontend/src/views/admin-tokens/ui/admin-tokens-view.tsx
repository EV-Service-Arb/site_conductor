"use client";

import { useResource } from "@evinvest/api/react";
import { PageFrame, Settled, Skeleton } from "@evinvest/uikit";
import { useState } from "react";

import { useMe } from "@/entities/session";
import { tokens } from "@/entities/tokens";
import { api } from "@/shared/api";
import type { TokenHolder } from "@/shared/api/schema";
import { HoldersCard } from "./holders-card";
import { MetersCard } from "./meters-card";
import { GrantDialog } from "@/features/grant-tokens";
import { PricesCard } from "@/features/price-tokens";
import { useT } from "@/shared/i18n";
import { EmptyState } from "@/shared/ui/empty-state";
import { ErrorState } from "@/shared/ui/error-state";
import { mayOpen } from "@evinvest/views";
import type { PATHS } from "@/shared/api/paths";
import { VIEWS } from "@/shared/api/views";

const GRAFANA: (typeof PATHS)[number] = "/grafana";

/** Prices, everyone's spending and balances, for whoever grants tokens. */
export function AdminTokensView() {
  const t = useT();
  const me = useMe();
  const holders = useResource(api, "/admin/tokens");
  const prices = useResource(api, "/admin/tokens/prices");
  const meters = useResource(api, "/admin/meters");
  const [changing, setChanging] = useState<TokenHolder | null>(null);

  if (!mayOpen(VIEWS, "admin_tokens", me.permissions)) {
    return (
      <PageFrame breakpoint="md" title={t(tokens.admin.title, "Tokens of everyone")}>
        <ErrorState failure={{ code: "forbidden", error: "" }} />
      </PageFrame>
    );
  }

  return (
    <PageFrame breakpoint="md" title={t(tokens.admin.title, "Tokens of everyone")}>
      {prices.status === "error" && <ErrorState failure={prices.failure} onRetry={prices.reload} />}
      {meters.status === "error" && <ErrorState failure={meters.failure} onRetry={meters.reload} />}
      {meters.status === "ok" && <MetersCard meters={meters.data} />}
      {prices.status === "ok" && <PricesCard prices={prices.data.prices} tokenUsd={prices.data.token_usd} onSaved={prices.reload} />}
      <section className="flex flex-col gap-2">
        <h2 className="text-sm font-semibold text-ink">{t(tokens.admin.spending, "Spending")}</h2>
        <iframe
          title={t(tokens.admin.spending, "Spending")}
          src={`${GRAFANA}/d/sa-tokens/?kiosk&hideLogo&_dash.hideLinks&theme=dark`}
          className="h-screen w-full rounded-lg border border-border"
        />
      </section>
      <Settled loading={holders.status === "loading"} skeleton={<Skeleton className="h-40 w-full" />}>
        {holders.status === "error" && <ErrorState failure={holders.failure} onRetry={holders.reload} />}
        {holders.status === "ok" && holders.data.users.length === 0 && <EmptyState title={t(tokens.admin.empty, "Nobody has signed in yet.")} description="" />}
        {holders.status === "ok" && holders.data.users.length > 0 && (
          <HoldersCard holders={holders.data.users} onChange={setChanging} />
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
