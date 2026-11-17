import openapi from "@/shared/api/openapi.json";
import type { ConnectedSites as Answer, BrandReads, Probe } from "@/shared/api/schema";

/** A site key's origin as `sa_core::ids::SiteOrigin` takes it; the backend has the last word. */
export function isOrigin(s: string): boolean {
  try {
    const u = new URL(s);
    return u.origin === s && (u.protocol === "https:" || ["localhost", "127.0.0.1"].includes(u.hostname));
  } catch {
    return false;
  }
}

/** What sites' `/api/internal` serves a brand, as the document's `BrandReads` names them. */
const ENDPOINTS = Object.keys(openapi.components.schemas.BrandReads.properties) as (keyof BrandReads)[];

/** `null`: not since sites began counting; `undefined`: sites could not be asked. */
type Seen = string | null | undefined;

export interface ConnectedSite {
  keyId: string;
  brands: string[];
  origin: string | null;
  report: Probe;
  lastIngest: Seen;
  /** The latest read of the key's brands from sites' `/api/internal`, and each one read. */
  lastRead: Seen;
  reads: { brand: string; endpoint: keyof BrandReads; at: string }[];
}

export interface ConnectedSites {
  sites: ConnectedSite[];
  /** What sites answered instead, once for the page. */
  sitesFailures: string[];
}

export function joinConnectedSites(raw: Answer): ConnectedSites {
  const { received, internal_reads: internal } = raw;
  const sites = raw.sites.map((s) => {
    const reads =
      internal.state === "read"
        ? s.brands.flatMap((brand) => ENDPOINTS.flatMap((endpoint) => {
            const at = internal.value.reads[brand]?.[endpoint];
            return at ? [{ brand, endpoint, at }] : [];
          }))
        : [];
    const latest = reads.reduce<string | null>((l, r) => (l === null || Date.parse(r.at) > Date.parse(l) ? r.at : l), null);
    return {
      keyId: s.key_id,
      brands: s.brands,
      origin: s.origin,
      report: s.report,
      lastIngest: received.state === "read" ? (received.value[s.key_id] ?? null) : undefined,
      lastRead: internal.state === "read" ? latest : undefined,
      reads,
    };
  });
  const sitesFailures = [received, internal].flatMap((r) => (r.state === "failed" ? [r.detail] : []));
  return { sites, sitesFailures };
}
