"use client";

import { Button, Planned } from "@evinvest/uikit";
import { Plus, Sparkles } from "lucide-react";

import { type Listed, categoryName, categoryOf, googleName, openWindow, useGmbTags, useTrade, windowKeys } from "@/entities/gmb";
import { useLocale, useT } from "@/shared/i18n";
import { useButtonSize } from "@/shared/ui/touch";

const MOCK_SCANS = [
  ["Nov 2", "3.2"],
  ["Oct 26", "3.9"],
  ["Oct 19", "4.6"],
  ["Oct 12", "5.1"],
] as const;

/** Where it ranks (02b): its category's keywords, grid scans. */
export function RankingTab({ listed: { gmb } }: { listed: Listed }) {
  const t = useT();
  const button = useButtonSize();
  const { tags } = useGmbTags();
  const locale = useLocale();
  const trade = useTrade(gmb.trade, gmb.country?.code ?? null);
  const placed = tags.categories === null || (gmb.trade !== null && trade === null) ? null : categoryOf(gmb, tags.categories);
  const tile = "flex flex-col gap-1 rounded-lg border border-border px-3 py-2";
  return (
    <div className="flex flex-col gap-4 p-5">
      <Planned what={t(windowKeys.ranking.scan.what, "Geo-grid scans (#94): the grid and scan to read, a scan to compare, what a scan costs")}>
        <div className="flex gap-2 text-sm text-ink">
          <span className="rounded-md border border-border px-3 py-1.5">Grid 7×7 · 5 km</span>
          <span className="rounded-md border border-border px-3 py-1.5">Scan Nov 2, 23:11</span>
          <span className="rounded-md border border-border px-3 py-1.5">Compare Oct 12</span>
        </div>
        <p className="rounded-md bg-hover px-3 py-2 text-center text-sm text-ink">{"Scan \u2018plombier lyon\u2019 7×7 · 49 pts · 12 tokens"}</p>
      </Planned>

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <section className="flex flex-col gap-2">
          <h3 className="flex items-baseline justify-between text-sm font-semibold text-ink">
            {t(windowKeys.ranking.keywords, "Keywords")}
            {placed?.status === "set_up" && <span className="text-xs font-normal text-ink-soft">{t(windowKeys.ranking.tracked, "{n} tracked", { n: placed.category.head.keywords.length })}</span>}
          </h3>
          {placed === null && <p className="text-sm text-ink-soft">{t(windowKeys.ranking.trades.unread, "Its category is not read yet.")}</p>}
          {placed?.status === "no_trade" && <p className="text-sm text-ink-soft">{t(windowKeys.ranking.noTrade, "No trade: its keywords are its category's.")}</p>}
          {placed?.status === "no_country" && <p className="text-sm text-ink-soft">{t(windowKeys.ranking.noCountry, "No country: {trade}'s keywords are per country.", { trade: googleName(trade!, locale) })}</p>}
          {placed?.status === "not_set_up" && (
            <p className="flex flex-wrap items-center gap-2 text-sm text-ink-soft">
              {t(windowKeys.ranking.notSetUp, "{category} is not set up: it has no keywords.", { category: categoryName(trade!, placed.country, locale) })}
              <Button variant="outline" size={button("sm")} onClick={() => openWindow({ kind: "trade", trade: placed.trade, country: placed.country })}>
                {t(windowKeys.ranking.setUp, "Set it up")}
              </Button>
            </p>
          )}
          {placed?.status === "set_up" &&
            (placed.category.head.keywords.length === 0 ? (
              <p className="text-sm text-ink-soft">{t(windowKeys.ranking.none, "{category} tracks no keywords yet.", { category: categoryName(placed.category.trade, placed.category.country, locale) })}</p>
            ) : (
              <ul className="flex flex-col divide-y divide-border rounded-lg border border-border">
                {placed.category.head.keywords.map((k) => (
                  <li key={k.text} className="px-3 py-2 text-sm text-ink">
                    {k.text}
                  </li>
                ))}
              </ul>
            ))}
          <Planned what={t(windowKeys.ranking.suggested.what, "Keywords worth tracking, from the category's searches and its competitors")}>
            <p className="flex items-center gap-2 text-sm font-medium text-ink">
              <Sparkles aria-hidden className="size-4" />
              {t(windowKeys.ranking.suggested, "Suggested keywords")}
            </p>
            {[
              ["chauffagiste lyon", "880 / mo · in your categories"],
              ["plombier pas cher lyon", "320 / mo · competitors rank"],
            ].map(([k, why]) => (
              <p key={k} className="flex items-center gap-2 text-sm text-ink">
                <span className="flex min-w-0 flex-1 flex-col">
                  {k}
                  <span className="text-xs text-ink-soft">{why}</span>
                </span>
                <Plus aria-hidden className="size-4" />
              </p>
            ))}
          </Planned>
        </section>

        <div className="flex flex-col gap-4">
          <Planned what={t(windowKeys.ranking.metrics.what, "What the latest scan says: average rank, average top rank, share of local voice, each against the compared scan")}>
            <div className={tile}>
              <span className="text-xs text-ink-soft">ARP</span>
              <span className="text-2xl text-ink">3.2 <span className="text-xs text-positive">−1.9</span></span>
            </div>
            <div className={tile}>
              <span className="text-xs text-ink-soft">ATRP</span>
              <span className="text-2xl text-ink">5.8 <span className="text-xs text-positive">−2.3</span></span>
            </div>
            <div className={tile}>
              <span className="text-xs text-ink-soft">SoLV</span>
              <span className="text-2xl text-ink">41% <span className="text-xs text-positive">+12</span></span>
            </div>
          </Planned>
          <Planned what={t(windowKeys.ranking.history.what, "Every scan of this location, newest first, with its ARP and what is running")}>
            <h3 className="text-sm font-semibold text-ink">{t(windowKeys.ranking.history, "Scans")}</h3>
            {MOCK_SCANS.map(([day, arp]) => (
              <p key={day} className="flex justify-between font-mono text-xs text-ink">
                <span>{day}</span>
                <span>ARP {arp}</span>
              </p>
            ))}
          </Planned>
        </div>
      </div>
    </div>
  );
}
