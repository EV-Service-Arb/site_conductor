"use client";

import { PageFrame, Skeleton } from "@evinvest/uikit";
import { ACCOUNT_SECTIONS } from "@evinvest/concierge";
import { AccountPage } from "@evinvest/concierge/react";
import { useEffect, useState } from "react";

import { panelRouteFor } from "@/features/account-chip";
import { useSignOut } from "@/features/sign-out";
import { platformMfe } from "@/shared/api";
import { go, useAt } from "@/shared/lib/location";
import { useLocale, useT, nav, account } from "@/shared/i18n";
import { EmptyState } from "@/shared/ui/empty-state";

import { SpendingCard } from "./spending-card";

const TIMEOUT_MS = 8000;

/**
 * Concierge's account element, inline. On this origin it shows who is signed in; what acts on
 * the platform session (security, notifications) it links to evinvest.ltd's account page.
 */
export function AccountView() {
  const t = useT();
  const locale = useLocale();
  const signOut = useSignOut();
  const at = useAt();
  if (at.view !== "account") throw new Error(`the account's page on ${at.view}`);
  const section = ACCOUNT_SECTIONS.find((s) => s === at.params.section);
  // undefined while the registry is asked, null when it gave no account entry.
  const [scriptUrl, setScriptUrl] = useState<string | null | undefined>(undefined);
  useEffect(() => {
    let stale = false;
    void platformMfe.scriptUrl("account", TIMEOUT_MS).then((url) => {
      if (!stale) setScriptUrl(url);
    });
    return () => {
      stale = true;
    };
  }, []);

  const loading = <Skeleton className="h-64 w-full" />;
  const onNavigate = (href: string): boolean => {
    const route = panelRouteFor(href);
    if (route !== null) go(route);
    return route !== null;
  };
  return (
    <PageFrame breakpoint="md" title={t(nav.account, "Profile")} width="content">
      {scriptUrl === undefined ? (
        loading
      ) : scriptUrl === null ? (
        <EmptyState title={t(account.unavailable, "Your account could not be loaded.")} />
      ) : (
        <AccountPage
          // The element reads `<html lang>` only when it renders.
          key={locale}
          scriptUrl={scriptUrl}
          {...(section === undefined ? {} : { section })}
          onSection={(s) => go({ view: "account", params: { section: s } }, "replace")}
          onNavigate={onNavigate}
          onSignOut={signOut}
          // useSignOut ends the platform session itself, after the panel's.
          platformSignOut={false}
          fallback={loading}
        />
      )}
      <SpendingCard />
    </PageFrame>
  );
}
