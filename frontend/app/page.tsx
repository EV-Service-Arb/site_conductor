"use client";

import { Empty, EmptyHeader, EmptyTitle, Skeleton } from "@evinvest/uikit";
import type { ReactNode } from "react";

import { AccessView } from "@/views/access";
import { AccountView } from "@/views/account";
import { AdminTokensView } from "@/views/admin-tokens";
import { AlertsView } from "@/views/alerts";
import { AssistantView } from "@/views/assistant";
import { Home } from "@/views/home";
import { KitView } from "@/views/kit";
import { ReviewArchiveView } from "@/views/review-archive";
import { PanelShell } from "@/views/shell";
import { SitesView } from "@/views/sites";
import { SourcesView } from "@/views/sources";
import { TokensView } from "@/views/tokens";
import { UsageView } from "@/views/usage";
import { WalletView } from "@/views/wallet";
import { state, useT } from "@/shared/i18n";
import { type At, useHere } from "@/shared/lib/location";

/** Every view is this one page (the server answers each with it); which one is the location's, by `VIEWS`. */
export default function Page() {
  const here = useHere();
  if (here === null) return <Skeleton className="min-h-svh rounded-none" />;
  if ("error" in here) return <NoView />;
  const { at } = here;
  // Outside the shell: whoever lands here may hold nothing it shows.
  if (at.view === "access") return <AccessView need={at.params.need} continueTo={at.params.continue} />;
  if (at.view === "area")
    return (
      <PanelShell bare>
        <KitView slug={at.params.area} />
      </PanelShell>
    );
  return <PanelShell>{framed(at)}</PanelShell>;
}

function framed(at: Exclude<At, { view: "access" | "area" }>): ReactNode {
  switch (at.view) {
    case "home":
      return <Home />;
    case "assistant":
      return <AssistantView />;
    case "account":
      return <AccountView />;
    case "sources":
      return <SourcesView />;
    case "tokens":
      return <TokensView />;
    case "admin_tokens":
      return <AdminTokensView />;
    case "usage":
      return <UsageView />;
    case "wallet":
      return <WalletView />;
    case "alerts":
      return <AlertsView />;
    default:
      if (at.view === "sites" || at.view.startsWith("sites.")) return <SitesView />;
      at.view satisfies `review_archive${string}` | `sites${string}`;
      return <ReviewArchiveView />;
  }
}

function NoView() {
  const t = useT();
  return (
    <Empty className="min-h-svh">
      <EmptyHeader>
        <EmptyTitle>{t(state.noView, "Nothing is at this address")}</EmptyTitle>
      </EmptyHeader>
    </Empty>
  );
}
