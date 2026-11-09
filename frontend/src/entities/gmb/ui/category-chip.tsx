"use client";

import { cn } from "@evinvest/uikit";

import { useT } from "@/shared/i18n";

import { categoryName, categoryOf } from "../model/category";
import type { Gmb } from "../model/gmb";
import { useGmbTags } from "../model/gmails-context";
import { openTrade } from "../model/on-map";

/** A GBP's category, opening its trade's window on its country; what it lacks in the warning tone. Nothing while it has no trade. */
export function CategoryChip({ gmb }: { gmb: Gmb }) {
  const t = useT();
  const { tags } = useGmbTags();
  const placed = tags.trades === null ? null : categoryOf(gmb, tags.trades);
  if (placed === null || placed.status === "no_trade") return null;
  const country = placed.status === "set_up" ? placed.category.country : placed.status === "not_set_up" ? placed.country : undefined;
  return (
    <button
      type="button"
      onClick={() => openTrade(placed.trade.id, country)}
      title={t("window.category.open")}
      className={cn(
        "shrink-0 rounded-full border px-2 py-0.5 text-xs outline-none hover:bg-hover/50 focus-visible:ring-2 focus-visible:ring-ring",
        placed.status === "set_up" ? "border-border text-ink" : "border-accent-warn/40 text-accent-warn",
      )}
    >
      {country === undefined
        ? t("window.category.noCountry", { trade: placed.trade.name })
        : placed.status === "not_set_up"
          ? t("window.category.notSetUp", { category: categoryName(placed.trade, country) })
          : categoryName(placed.trade, country)}
    </button>
  );
}
