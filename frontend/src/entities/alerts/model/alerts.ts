import type { Alerts } from "@/shared/api/schema";

/** Whether an urgent alert reaches the caller nowhere but in the panel. */
export function unreached(a: Alerts): boolean {
  return a.methods.length > 0 && !a.destinations.some((d) => d.dead === null && d.cadences.urgent !== "off");
}

/** Whether none reaches them at once: the ones they have wait. */
export function late(a: Alerts): boolean {
  return !unreached(a) && a.methods.length > 0 && !a.destinations.some((d) => d.dead === null && d.cadences.urgent === "now");
}
