import type { Locale } from "@/shared/i18n";

const MINUTE = 60_000;
const STEPS = [
  { unit: "minute", ms: MINUTE, below: 60 },
  { unit: "hour", ms: 60 * MINUTE, below: 24 },
  { unit: "day", ms: 24 * 60 * MINUTE, below: 7 },
  { unit: "week", ms: 7 * 24 * 60 * MINUTE, below: 52 },
  { unit: "year", ms: 365 * 24 * 60 * MINUTE, below: Infinity },
] as const;

function step(iso: string) {
  const ms = Math.max(0, Date.now() - Date.parse(iso));
  const s = STEPS.find((x) => ms / x.ms < x.below)!; // the last step is below Infinity
  return { unit: s.unit, n: Math.max(1, Math.floor(ms / s.ms)) };
}

/** "2h", "3w": a history row's age. Months are left out: narrow, they read as minutes. */
export function age(iso: string, locale: Locale): string {
  const { unit, n } = step(iso);
  return new Intl.NumberFormat(locale, { style: "unit", unit, unitDisplay: "narrow" }).format(n);
}

/** "2 hours ago". */
export function ago(iso: string, locale: Locale): string {
  const { unit, n } = step(iso);
  return new Intl.RelativeTimeFormat(locale, { numeric: "auto" }).format(-n, unit);
}
