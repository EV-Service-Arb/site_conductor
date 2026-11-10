"use client";

import { type NavSection, ShellNav, cn } from "@evinvest/uikit";
import { Store } from "lucide-react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import type { CSSProperties } from "react";

import { useT } from "@/shared/i18n";

import { toggleSidebar } from "../model/sidebar";

import { useApps } from "./apps";
import { GmbTree } from "./gmb-tree";

const WIDTH = { "--shell-rail-w": "18rem" } as CSSProperties;

/**
 * The left of every page: the open app's own views, then the caller's GMBs. `mfe-sites-nav` stays
 * mounted, only hidden, off the sites (CONTRACT §9): it keeps the tab title's count on every page.
 */
export function Outline({ folded, onNavigate, className }: { folded: boolean; onNavigate?: () => void; className?: string }) {
  const t = useT();
  const pathname = usePathname();
  const sections: NavSection[] = [
    ...useApps(onNavigate).flatMap((a) => (a.here ? [a.section] : "content" in a.section ? [{ ...a.section, hidden: true }] : [])),
    { id: "gbps", label: t("nav.gbps"), icon: Store, content: <GmbTree {...(onNavigate && { onNavigate })} /> },
  ];
  return (
    <ShellNav
      groups={sections}
      pathname={pathname}
      linkComponent={Link}
      unfold={folded ? toggleSidebar : undefined}
      labels={{ primary: t("nav.outline") }}
      {...(onNavigate && { onNavigate: () => onNavigate() })}
      style={WIDTH}
      className={cn("gap-4 px-2 pt-2 pb-2", className)}
    />
  );
}
