"use client";

import { Badge, Card, CardContent, cn } from "@evinvest/uikit";

import { tokens } from "@/entities/tokens";
import type { MeterUse } from "@/shared/api/schema";
import { useT } from "@/shared/i18n";

/** A reading older than this is stale: the sync runs daily. */
const STALE_MS = 36 * 3600 * 1000;

const day = (iso: string) => new Date(iso).toLocaleString(undefined, { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });

/** Each vendor meter this period: the vendor's own count plus ours since it, against its free units. */
export function MetersCard({ meters }: { meters: MeterUse[] }) {
  const t = useT();
  const now = Date.now();
  return (
    <Card className="gap-3 py-4">
      <CardContent className="flex flex-col gap-3 px-4">
        <header className="flex flex-col gap-1">
          <h2 className="text-sm font-semibold text-ink">{t(tokens.meters.title, "Vendors this month")}</h2>
          <p className="text-xs text-ink-soft">
            {t(
              tokens.meters.why,
              "Each one's count is the vendor's own (every project on its billing account, hours behind), plus ours since. Street View stops at its free ones.",
            )}
          </p>
        </header>
        <ul className="flex flex-col divide-y divide-border">
          {meters.map((m) => {
            const share = m.used / m.free;
            const stale = m.reading === null || now - Date.parse(m.reading.synced_at) > STALE_MS;
            return (
              <li key={m.meter} className="flex flex-col gap-1.5 py-2">
                <div className="flex flex-wrap items-baseline gap-2">
                  <span className="text-sm text-ink">{m.name}</span>
                  {m.stop === "at_free" && <Badge tone="neutral">{t(tokens.meters.stops, "stops at free")}</Badge>}
                  {share >= 0.8 && <Badge tone={share >= 1 ? "error" : "warn"}>{t(tokens.meters.near, "{p}% of free", { p: Math.round(share * 100) })}</Badge>}
                  {stale && (
                    <Badge tone="warn">
                      {m.reading === null ? t(tokens.meters.unread, "never read") : t(tokens.meters.stale, "read {at}", { at: day(m.reading.synced_at) })}
                    </Badge>
                  )}
                  <span className="ml-auto text-sm tabular-nums text-ink">
                    {m.used.toLocaleString()} / {m.free.toLocaleString()}
                  </span>
                </div>
                <div className="h-1.5 overflow-hidden rounded-full bg-muted" aria-hidden>
                  <div className={cn("h-full rounded-full", share >= 1 ? "bg-accent-error" : share >= 0.8 ? "bg-accent-warn" : "bg-ink-soft")} style={{ width: `${Math.min(share, 1) * 100}%` }} />
                </div>
                <span className="text-xs text-ink-soft tabular-nums">
                  {t(tokens.meters.split, "{vendor} {baseline} as of {at}, ours {delta} since", { vendor: "Google", baseline: m.baseline.toLocaleString(), at: day(m.since), delta: m.delta.toLocaleString() })}
                </span>
              </li>
            );
          })}
        </ul>
      </CardContent>
    </Card>
  );
}
