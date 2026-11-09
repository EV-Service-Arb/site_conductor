import type { Gmail, Gmails, Gmb } from "./gmb";
import { type Scope, inScope } from "./scope";

export interface Listed {
  gmb: Gmb;
  /** `null`: a draft no gmail manages yet. */
  gmail: Gmail | null;
}

/** The GBPs in `scope` (`null`: every one, the drafts under no gmail too) whose name, address, place id or gmail holds `filter`. */
export function narrowed(data: Gmails, scope: Scope | null, filter: string): Listed[] {
  const needle = filter.trim().toLowerCase();
  const gmails = new Map(data.gmails.map((g) => [g.id, g]));
  return data.gmbs
    .filter((gmb) => scope === null || inScope(gmb, scope))
    .map((gmb) => {
      const gmail = gmb.gmail_id === null ? null : gmails.get(gmb.gmail_id);
      if (gmail === undefined) throw new Error(`GBP ${gmb.id} is under gmail ${gmb.gmail_id}, which is not listed`);
      return { gmb, gmail };
    })
    .filter(({ gmb, gmail }) => needle === "" || [gmb.name, gmb.address, gmb.place_id, gmail?.gmail].some((s) => s?.toLowerCase().includes(needle)));
}

/** The GBPs `gmail` manages. */
export function under(data: Gmails, gmail: number): Gmb[] {
  return data.gmbs.filter((b) => b.gmail_id === gmail);
}

/** In the viewport on, in it off, and elsewhere: those without a point too. */
export function byView(listed: readonly Listed[], inView: ReadonlySet<number>): { inView: Listed[]; offInView: Listed[]; elsewhere: Listed[] } {
  return {
    inView: listed.filter((l) => inView.has(l.gmb.id) && l.gmb.enabled),
    offInView: listed.filter((l) => inView.has(l.gmb.id) && !l.gmb.enabled),
    elsewhere: listed.filter((l) => !inView.has(l.gmb.id)),
  };
}
