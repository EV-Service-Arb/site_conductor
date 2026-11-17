import { type Readiness, mustHaveValues } from "@/shared/api/schema";

/** How many must-haves are ready: neither missing nor unknown (sites unread). */
export const readyCount = (r: Readiness) => mustHaveValues.length - r.missing.length - r.unknown.length;
