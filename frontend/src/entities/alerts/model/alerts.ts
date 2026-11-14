import type { Alerts } from "@/shared/api/schema";

/** Whether nothing reaches the caller: the bot is there, and no own chat is linked, or a destination is dead. */
export function unreached(a: Alerts): boolean {
  return a.enabled && (!a.destinations.some((d) => d.kind === "private") || a.destinations.some((d) => d.dead != null));
}
