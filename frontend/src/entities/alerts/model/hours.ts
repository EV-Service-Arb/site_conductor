import type { AlertHours } from "@/shared/api/schema";

/** 450 → "07:30". */
export const clock = (minutes: number): string => `${String(Math.floor(minutes / 60)).padStart(2, "0")}:${String(minutes % 60).padStart(2, "0")}`;

/** Minutes since midnight in `tz`. */
export function minutesIn(tz: string, at: Date): number {
  const parts = new Intl.DateTimeFormat("en-GB", { timeZone: tz, hour: "2-digit", minute: "2-digit", hourCycle: "h23" }).formatToParts(at);
  const of = (type: "hour" | "minute") => Number(parts.find((p) => p.type === type)!.value); // both were asked for
  return of("hour") * 60 + of("minute");
}

/** From and to, in minutes: two, as the document bounds them. */
export const quietOf = (hours: AlertHours): [number, number] | null => hours.quiet as [number, number] | null;

/** What holds everything but urgent now, and the wall clock it ends at; `null`: nothing does. */
export function quietNow(hours: AlertHours | null, at: Date): { by: "snooze" | "hours"; until: string } | null {
  if (hours === null) return null;
  const now = minutesIn(hours.tz, at);
  const snoozed = hours.snoozed_until === null ? null : new Date(hours.snoozed_until);
  if (snoozed !== null && snoozed > at) return { by: "snooze", until: clock(minutesIn(hours.tz, snoozed)) };
  const quiet = quietOf(hours);
  if (quiet === null) return null;
  const [from, to] = quiet;
  const inside = from < to ? from <= now && now < to : now >= from || now < to;
  return inside ? { by: "hours", until: clock(to) } : null;
}

/** When `minutes` next strikes in `tz`, strictly after `at`, to the minute. */
export function nextAt(tz: string, minutes: number, at: Date): Date {
  const ahead = (minutes - minutesIn(tz, at) + 1440) % 1440 || 1440;
  // shortcut: an hour off across a DST change, the same way round as the wall clock moves
  return new Date(Math.floor(at.getTime() / 60_000) * 60_000 + ahead * 60_000);
}
