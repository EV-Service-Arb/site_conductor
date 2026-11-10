"use client";

import { KeymapProvider } from "@evinvest/uikit";
import type { ReactNode } from "react";
import { useEffect } from "react";
import { usePathname } from "next/navigation";

import { PANEL } from "@config";

import { GmailsProvider, PublishSelection } from "@/entities/gmb";
import { seesSites, useMe } from "@/entities/session";
import { Assistant } from "@/features/assistant";
import { goToSignIn } from "@/shared/api";
import { useT } from "@/shared/i18n";
import { ROUTES } from "@/shared/config/routes";
import { useLiveSocket } from "@/shared/lib/live";

import { useSidebarFolded } from "../model/sidebar";
import { followTitleCount } from "../model/title-count";
import { NavKeys } from "./nav-keys";
import { Outline } from "./outline";
import { PanelHotbar } from "./panel-hotbar";
import { PanelTopBar } from "./panel-top-bar";

/** Option on macOS too: the keymap reads the key's Latin label when Option composes `ß`, and a field keeps typing it. */
const LEADER = { key: "s", alt: true };

/** CONTRACT §9: an element's API answered 401. */
const UNAUTHENTICATED_EVENT = "sa:unauthenticated";

/** Every signed-in screen between the top bar and the hotbar, beside the outline; the tab's live socket and the elements' window events. */
export function ShellFrame({ onForbidden, children }: { onForbidden: () => void; children: ReactNode }) {
  const t = useT();
  const me = useMe();
  const pathname = usePathname();
  const folded = useSidebarFolded();
  const live = useLiveSocket({ onUnauthenticated: goToSignIn, onForbidden });
  const leads = t("title.leads");
  useEffect(() => followTitleCount(window, document, leads), [leads]);
  useEffect(() => {
    window.addEventListener(UNAUTHENTICATED_EVENT, goToSignIn);
    return () => window.removeEventListener(UNAUTHENTICATED_EVENT, goToSignIn);
  }, []);
  return (
    <KeymapProvider leader={LEADER} labels={{ title: t("keys.help") }}>
      <GmailsProvider seesSites={seesSites(me)}>
        <PublishSelection />
        <NavKeys />
        <div className="flex h-svh flex-col">
          <PanelTopBar />
          <div className="relative flex min-h-0 flex-1">
            {/* `hidden`, not unmounted, on a phone: the outline keeps `mfe-sites-nav` mounted */}
            <Outline folded={folded} className="hidden shrink-0 md:flex" />
            <main className="flex min-w-0 flex-1 flex-col overflow-y-auto">{children}</main>
            {PANEL.assistant && !`${pathname}/`.startsWith(ROUTES.assistant) && <Assistant />}
          </div>
          <PanelHotbar live={live} />
        </div>
      </GmailsProvider>
    </KeymapProvider>
  );
}
