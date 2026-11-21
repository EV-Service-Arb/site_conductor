"use client";

import { cn } from "@evinvest/uikit";

import type { Gmb } from "@/shared/api/schema";
import { useLocale, useT } from "@/shared/i18n";

import { categoryOf } from "../model/category";
import { useGmbTags } from "../model/gmails-context";
import { googleName } from "../model/gmb";
import { useTrade } from "../model/google";
import { openWindow } from "../model/windows";
import { windowKeys } from "../model/i18n";

/** A GBP's category, opening its trade's window on its country; what it lacks in the warning tone. Nothing while it has no trade. */
export function CategoryChip({ gmb }: { gmb: Gmb }) {
  if (gmb.trade === null) return null;
  return <Chip gmb={gmb} trade={gmb.trade} />;
}

function Chip({ gmb, trade: gcid }: { gmb: Gmb; trade: string }) {
  const t = useT();
  const locale = useLocale();
  const { tags } = useGmbTags();
  const country = gmb.country?.code ?? null;
  const trade = useTrade(gcid, country);
  if (tags.categories === null || trade === null) return null;
  const placed = categoryOf(gmb, tags.categories);
  const name = googleName(trade, locale);
  return (
    <button
      type="button"
      onClick={() => openWindow({ kind: "trade", trade: gcid, country })}
      title={t(windowKeys.category.open, "Open its trade")}
      className={cn(
        "shrink-0 rounded-full border px-2 py-0.5 text-xs outline-none hover:bg-hover/50 focus-visible:ring-2 focus-visible:ring-ring",
        placed.status === "set_up" ? "border-border text-ink" : "border-accent-warn/40 text-accent-warn",
      )}
    >
      {country === null
        ? t(windowKeys.category.noCountry, "{trade} · no country", { trade: name })
        : placed.status === "not_set_up"
          ? t(windowKeys.category.notSetUp, "{category} · not set up", { category: `${name} · ${country}` })
          : `${name} · ${country}`}
    </button>
  );
}
