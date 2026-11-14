export function formatDateTime(iso: string, locale: string): string {
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleString(locale, { dateStyle: "short", timeStyle: "short" });
}

/** Metres as a person reads a distance: `350 m`, `2.1 km`. */
export function formatDistance(m: number, locale: string): string {
  return m < 1000 ? `${Math.round(m)} m` : `${(m / 1000).toLocaleString(locale, { maximumFractionDigits: 1 })} km`;
}
