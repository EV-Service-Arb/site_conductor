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

const SIX_DAYS = 6 * 24 * 60 * MINUTE;

/** "14:32" today, "Thu 10:30" within six days, "Nov 26" beyond. */
export function when(iso: string, locale: Locale, now: Date): string {
  const at = new Date(iso);
  const time = { hour: "2-digit", minute: "2-digit" } as const;
  if (at.toDateString() === now.toDateString()) return at.toLocaleTimeString(locale, time);
  if (Math.abs(at.getTime() - now.getTime()) < SIX_DAYS) return at.toLocaleString(locale, { weekday: "short", ...time });
  return at.toLocaleDateString(locale, { month: "short", day: "numeric", ...(at.getFullYear() !== now.getFullYear() && { year: "numeric" }) });
}

/** "2 hours ago". */
export function ago(iso: string, locale: Locale): string {
  const { unit, n } = step(iso);
  return new Intl.RelativeTimeFormat(locale, { numeric: "auto" }).format(-n, unit);
}
