"use client";

import { Button, Card, CardContent, PageFrame, Settled, Skeleton, Table, TableBody, TableCell, TableHead, TableHeader, TableRow, cn } from "@evinvest/uikit";
import { useState } from "react";

import { useMe } from "@/entities/session";
import { tokens } from "@/entities/tokens";
import { api, ok } from "@/shared/api";
import type { TokenHolder } from "@/shared/api/schema";
import { MetersCard } from "./meters-card";
import { GrantDialog } from "@/features/grant-tokens";
import { PricesCard } from "@/features/price-tokens";
import { useT } from "@/shared/i18n";
import { useResource } from "@/shared/lib/use-resource";
import { EmptyState } from "@/shared/ui/empty-state";
import { ErrorState } from "@/shared/ui/error-state";
import { EDGE_CELL, TABLE_HEAD } from "@/shared/ui/table";
import { useButtonSize } from "@/shared/ui/touch";
import { mayOpen } from "@evinvest/views";
import { VIEWS } from "@/shared/api/views";

const HEAD = cn(TABLE_HEAD, EDGE_CELL);

/** Prices, everyone's spending and balances, for whoever grants tokens. */
export function AdminTokensView() {
  const t = useT();
  const me = useMe();
  const button = useButtonSize();
  const holders = useResource("admin.tokens", async () => (await ok(api.GET("/admin/tokens"))).users, "admin.tokens", { live: ["admin"] });
  const prices = useResource("admin.prices", async () => (await ok(api.GET("/admin/tokens/prices"))).prices, "admin.prices", { live: ["admin"] });
  const meters = useResource("admin.meters", async () => ok(api.GET("/admin/meters")));
  const [changing, setChanging] = useState<TokenHolder | null>(null);

  if (!mayOpen(VIEWS, "admin_tokens", me.permissions)) {
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
          src={`/grafana/d/sa-tokens/?kiosk&hideLogo&_dash.hideLinks&theme=dark`}
          className="h-screen w-full rounded-lg border border-border"
        />
      </section>
      <Settled loading={holders.status === "loading"} skeleton={<Skeleton className="h-40 w-full" />}>
        {holders.status === "error" && <ErrorState failure={holders.failure} onRetry={holders.reload} />}
        {holders.status === "ok" && holders.data.length === 0 && <EmptyState title={t(tokens.admin.empty, "Nobody has signed in yet.")} description="" />}
        {holders.status === "ok" && holders.data.length > 0 && (
          <Card className="overflow-hidden py-0">
            <CardContent className="p-0">
              <Table>
                <TableHeader>
                  <TableRow className="hover:bg-transparent">
                    <TableHead className={HEAD}>{t(tokens.admin.user, "Person")}</TableHead>
                    <TableHead className={cn(HEAD, "text-right")}>{t(tokens.admin.balance, "Tokens")}</TableHead>
                    <TableHead className={HEAD}>
                      <span className="sr-only">{t(tokens.admin.change, "Change")}</span>
                    </TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {holders.data.map((h) => (
                    <TableRow key={h.user_id}>
                      <TableCell className={EDGE_CELL}>
                        <span className="text-ink">{h.name || h.email}</span>
                        {h.name && <span className="ml-2 text-ink-soft">{h.email}</span>}
                      </TableCell>
                      <TableCell className={cn(EDGE_CELL, "text-right tabular-nums")}>{h.balance}</TableCell>
                      <TableCell className={cn(EDGE_CELL, "text-right")}>
                        <Button variant="ghost" size={button("sm")} onClick={() => setChanging(h)}>
                          {t(tokens.admin.change, "Change")}
                        </Button>
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </CardContent>
          </Card>
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
