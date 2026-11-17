import { ViewError, parse } from "@evinvest/views";

import { VIEWS } from "@/shared/api/views";

/**
 * The href for a chip link (`ev:navigate`'s `detail.href`) when it is one of the panel's views,
 * for a soft move; null when the browser should follow the link itself (`/auth`, another origin).
 */
export function panelRouteFor(href: unknown): string | null {
  if (typeof href !== "string" || !href.startsWith("/") || href.startsWith("//") || href.startsWith("/\\")) return null;
  try {
    parse(VIEWS, href);
  } catch (e) {
    // no view of the panel's: a page of the browser's to load
    if (e instanceof ViewError) return null;
    throw e;
  }
  return href;
}

/** `panelRouteFor` of an `ev:navigate` event's `detail.href`; a missing or malformed detail is null. */
export function routeOfNavigateEvent(event: Event): string | null {
  const detail: unknown = event instanceof CustomEvent ? event.detail : undefined;
  const href = typeof detail === "object" && detail !== null && "href" in detail ? detail.href : undefined;
  return panelRouteFor(href);
}
