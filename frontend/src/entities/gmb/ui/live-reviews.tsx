"use client";

import { cn } from "@evinvest/uikit";

import { useT } from "@/shared/i18n";

import type { ArchiveTag } from "../model/gmb";
import { gmbs as gmbsKeys } from "../model/i18n";

const WEEK = 7;
const BAR = 12;

/** A GBP's live reviews now, their change over the week, and a bar a day over 30; a day nothing collected is an empty slot, not a zero. Nothing while no day was collected. */
export function LiveReviews({ days }: { days: ArchiveTag["live_30d"] }) {
  const t = useT();
  const collected = days.flatMap((d) => (d === null ? [] : [d.live]));
  const latest = days.findLast((d) => d !== null);
  if (latest === undefined) return null;
  const base = days.slice(0, days.length - WEEK).findLast((d) => d !== null);
  const delta = base === undefined ? null : latest.live - base.live;
  const [min, max] = [Math.min(...collected), Math.max(...collected)];
  return (
    <span className="flex shrink-0 flex-col items-end gap-0.5 py-1.5" title={t(gmbsKeys.live.title, "Live reviews over the last 30 days")}>
      <span className="text-xs tabular-nums">
        <span className="text-ink">{latest.live}</span>
        {delta !== null && <span className={cn("ml-1", delta > 0 ? "text-accent-info" : delta < 0 ? "text-accent-error" : "text-ink-soft")}>{delta > 0 ? `+${delta}` : delta < 0 ? `−${-delta}` : "+0"}</span>}
        <span className="ml-1 text-ink-faint">{t(gmbsKeys.live.week, "7d")}</span>
      </span>
      <span aria-hidden className="flex h-3.5 items-end gap-px">
        {days.map((d, i) => (
          <span key={i} className="flex w-0.5 flex-col justify-end">
            {d === null ? (
              <span className="h-px bg-border-subtle" />
            ) : (
              <>
                {d.lost > 0 && <span className="h-0.5 bg-accent-error" />}
                <span className={i >= days.length - WEEK ? "bg-accent-info" : "bg-ink-faint"} style={{ height: 2 + Math.round(((d.live - min) / (max - min || 1)) * (BAR - 2)) }} />
              </>
            )}
          </span>
        ))}
      </span>
    </span>
  );
}
