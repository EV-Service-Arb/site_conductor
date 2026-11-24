import type { Alert, Gmb } from "@/shared/api/schema";

/** What a row shows of an alert: its first line, then the GBP it is about and the line after. */
export function lines(alert: Alert, gmbs: readonly Gmb[]): { title: string; sub: string } {
  const [title = "", ...rest] = alert.text.split("\n").filter((l) => l.trim() !== "");
  const names = alert.gmbs.map((id) => gmbs.find((b) => b.id === id)?.name).filter((n) => n !== undefined && !title.includes(n));
  const about = names.length === 0 ? [] : [names.length === 1 ? names[0]! : `${names[0]} +${names.length - 1}`];
  return { title, sub: [...about, ...rest.slice(0, 1)].join(" · ") };
}

/** A phone as a note writes one: on a line of its own, or after "label:". */
const PHONE = /^(?:[^\d+]*:\s*)?(\+?\d(?:[\s().-]*\d){8,14})$/;

/** The number to call, when the alert carries one. */
export function phoneOf(text: string): string | null {
  for (const line of text.split("\n")) {
    const found = PHONE.exec(line.trim());
    if (found !== null) return found[1]!;
  }
  return null;
}
