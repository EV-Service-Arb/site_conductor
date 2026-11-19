"use client";

import { History, Plug, Send, Smartphone, Store } from "lucide-react";
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
} satisfies Record<Tab, (t: T) => string>;

export const TAB_ICON: Record<Tab, ReactNode> = {
  profile: <Store aria-hidden className="size-4" />,
  log: <History aria-hidden className="size-4" />,
  launch: <Send aria-hidden className="size-4" />,
  browser: <Smartphone aria-hidden className="size-4" />,
  connections: <Plug aria-hidden className="size-4" />,
};
