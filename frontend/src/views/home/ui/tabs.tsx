"use client";

import { BarChart3, Gavel, History, Plug, Send, ShieldCheck, Smartphone, Star, Store, TrendingUp } from "lucide-react";
import type { ReactNode } from "react";

import { windowKeys } from "@/entities/gmb";
import type { Tab } from "@/shared/api/schema";
import type { T } from "@/shared/i18n";

/** A window's tab, the GBP's or the gmail's. */
export const TAB_NAME = {
  profile: (t) => t(windowKeys.tab.profile, "Profile"),
  log: (t) => t(windowKeys.tab.log, "Log"),
  launch: (t) => t(windowKeys.tab.launch, "Launch"),
  browser: (t) => t(windowKeys.tab.browser, "Browser"),
  connections: (t) => t(windowKeys.tab.connections, "Connections"),
  stats: (t) => t(windowKeys.tab.stats, "Stats"),
  ranking: (t) => t(windowKeys.tab.ranking, "Ranking"),
  reviews: (t) => t(windowKeys.tab.reviews, "Reviews"),
  verification: (t) => t(windowKeys.tab.verification, "Verification"),
  appeal: (t) => t(windowKeys.tab.appeal, "Appeal"),
} satisfies Record<Tab, (t: T) => string>;

export const TAB_ICON: Record<Tab, ReactNode> = {
  profile: <Store aria-hidden className="size-4" />,
  log: <History aria-hidden className="size-4" />,
  launch: <Send aria-hidden className="size-4" />,
  browser: <Smartphone aria-hidden className="size-4" />,
  connections: <Plug aria-hidden className="size-4" />,
  stats: <BarChart3 aria-hidden className="size-4" />,
  ranking: <TrendingUp aria-hidden className="size-4" />,
  reviews: <Star aria-hidden className="size-4" />,
  verification: <ShieldCheck aria-hidden className="size-4" />,
  appeal: <Gavel aria-hidden className="size-4" />,
};
