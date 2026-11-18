"use client";

import { ToggleGroup, ToggleGroupItem, cn } from "@evinvest/uikit";

import { kit as kitKeys, photoSrc } from "@/entities/gmb";
import type { KitBlock, KitTally } from "@/shared/api/schema";
import type { T } from "@/shared/i18n";
import { useT } from "@/shared/i18n";

import { PERIODS, type Period, setPeriod, usePeriod } from "../model/kit";

export const SECTION_HEAD = "flex items-baseline gap-2 text-xs font-semibold tracking-wide text-ink-soft uppercase";
export const HINT = "font-normal tracking-normal normal-case";

export const BLOCK_NAME = {
  draft: (t) => t(kitKeys.block.draft, "not on Google yet"),
  verifying: (t) => t(kitKeys.block.verifying, "kit waits for Verified"),
  suspended: (t) => t(kitKeys.block.suspended, "no kit while suspended"),
  off: (t) => t(kitKeys.block.off, "kit off"),
  no_page: (t) => t(kitKeys.block.noPage, "no landing page: Google's form only"),
} satisfies Record<KitBlock, (t: T) => string>;

const PERIOD_NAME = {
  today: (t) => t(kitKeys.period.today, "Today"),
  "7d": (t) => t(kitKeys.period.week, "7 d"),
  "30d": (t) => t(kitKeys.period.month, "30 d"),
  all: (t) => t(kitKeys.period.all, "All"),
} satisfies Record<Period, (t: T) => string>;

export function PeriodPicker() {
  const t = useT();
  const period = usePeriod();
  return (
    <ToggleGroup type="single" variant="outline" size="sm" value={period} onValueChange={(v) => PERIODS.includes(v as Period) && setPeriod(v as Period)} aria-label={t(kitKeys.period, "Period")}>
      {PERIODS.map((p) => (
        <ToggleGroupItem key={p} value={p} className="px-2 text-xs">
          {PERIOD_NAME[p](t)}
        </ToggleGroupItem>
      ))}
    </ToggleGroup>
  );
}

/** What a kit or a GBP collected: the reviewer's way from the QR to Google. */
export function Tally({ tally, className }: { tally: KitTally; className?: string }) {
  const t = useT();
  const cells = [
    [tally.shown, t(kitKeys.tally.shown, "QR shown"), ""],
    [tally.scanned, t(kitKeys.tally.scanned, "scanned"), ""],
    [tally.copied, t(kitKeys.tally.copied, "copied a draft"), ""],
    [tally.on_google, t(kitKeys.tally.onGoogle, "on Google"), "text-positive"],
    [tally.gone, t(kitKeys.tally.gone, "gone since"), tally.gone > 0 ? "text-accent-error" : ""],
  ] as const;
  return (
    <div className={cn("flex flex-col gap-1", className)}>
      <dl className="grid grid-cols-5 gap-3">
        {cells.map(([n, label, tone]) => (
          <div key={label} className="flex flex-col-reverse">
            <dt className="text-xs text-ink-soft">{label}</dt>
            <dd className={cn("font-mono text-xl text-ink tabular-nums", tone)}>{n}</dd>
          </div>
        ))}
      </dl>
      {tally.other_reviews > 0 && <p className="text-xs text-ink-soft">{t(kitKeys.tally.other, "+{n} reviews not from drafts", { n: tally.other_reviews })}</p>}
    </div>
  );
}

export function Thumb({ brand, photo, className }: { brand: number; photo: number; className?: string }) {
  // eslint-disable-next-line @next/next/no-img-element -- the API's own JPEG, behind the session: nothing for next/image to optimise in an export
  return <img src={photoSrc(brand, photo)} alt="" loading="lazy" className={cn("size-12 rounded-md object-cover", className)} />;
}

/** One dot per GBP of the kit, filled where it went. */
export function Dots({ went, of, title }: { went: number; of: number; title?: string }) {
  if (of === 0) return null;
  return (
    <span className="flex gap-0.5" title={title}>
      {Array.from({ length: of }, (_, i) => (
        <span key={i} className={cn("size-1.5 rounded-full border", i < went ? "border-positive bg-positive" : "border-ink-faint")} />
      ))}
    </span>
  );
}

export function Chip({ children, tone, onRemove }: { children: React.ReactNode; tone?: "place" | "warn"; onRemove?: () => void }) {
  return (
    <span className={cn("inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-xs", tone === "place" ? "bg-accent-warn/15 text-accent-warn" : tone === "warn" ? "text-accent-warn" : "bg-hover text-ink-soft")}>
      {children}
      {onRemove && (
        <button type="button" onClick={onRemove} className="text-ink-soft hover:text-ink" aria-label="×">
          ×
        </button>
      )}
    </span>
  );
}
