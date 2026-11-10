import { describe, expect, it } from "vitest";

import type { Gmails, Gmb } from "@/entities/gmb/model/gmb";
import { byView, narrowed } from "@/entities/gmb/model/narrow";

const gmb = (id: number, name: string, address: string | null, located: boolean, enabled = true, gmail: number | null = Math.floor(id / 10)): Gmb => ({
  id,
  place_id: `ChIJplace${id}`,
  name,
  address,
  gmail_id: gmail,
  state: "verified",
  next: ["suspended", "verifying"],
  country: { code: "FR", by_hand: false },
  phone: null,
  trade_id: null,
  areas: [],
  hidden_address: null,
  customers_visit: null,
  film: null,
  created_at: "2026-11-05T00:00:00Z",
  enabled,
  location: located ? { lat: 45.76, lng: 4.83, placed: false } : null,
});

const gmails: Gmails = {
  gmails: [
    { id: 1, gmail: "eric@aquafix.top", role: null, created_at: "2026-11-05T00:00:00Z", browser: false },
    { id: 2, gmail: "sameday.ny", role: null, created_at: "2026-11-05T00:00:00Z", browser: false },
  ],
  gmbs: [
    { ...gmb(10, "Aquafix · Vaise", "Lyon 9e", true), trade_id: 1 },
    { ...gmb(11, "Aquafix · Gerland", "Lyon 7e", false), trade_id: 2 },
    gmb(12, "Aquafix · Croix-Rousse", "Lyon 4e", true, false),
    { ...gmb(20, "Same Day Plumbing", null, true), trade_id: 1, country: { code: "US", by_hand: false } },
    { ...gmb(30, "Aquafix · Part-Dieu", null, false, true, null), place_id: null, state: "draft", country: null },
  ],
};

const ids = (listed: { gmb: Gmb }[]) => listed.map((l) => l.gmb.id);

describe("the outline's GMBs", () => {
  it("are the scoped gmail's, matched on name, address, place id or gmail; a draft under no gmail is under every gmail only", () => {
    for (const [scope, filter, want] of [
      [null, "", [10, 11, 12, 20, 30]],
      [new Set([1, 2]), "", [10, 11, 12, 20]],
      [new Set([2]), "", [20]],
      [null, "lyon 7E", [11]],
      [null, "ChIJplace20", [20]],
      [null, "aquafix.top", [10, 11, 12]],
      [null, "part-dieu", [30]],
      [new Set([1]), "same day", []],
    ] as const) {
      expect(ids(narrowed(gmails, scope && { gmails: scope, categories: null }, filter)), `${scope && [...scope]} ${filter}`).toEqual(want);
    }
  });

  it("under category patterns are those any pattern takes, and the gmails narrow them in turn; one with no trade is under none", () => {
    for (const [categories, scope, want] of [
      [[{ trade: 1, country: "FR" }], null, [10]],
      [[{ trade: 1, country: "*" }], null, [10, 20]],
      [[{ trade: "*", country: "FR" }], null, [10, 11]],
      [[{ trade: 1, country: "FR" }, { trade: 2, country: "FR" }, { trade: 1, country: "US" }], null, [10, 11, 20]],
      [[{ trade: 1, country: "*" }], new Set([2]), [20]],
    ] as const) {
      expect(ids(narrowed(gmails, { gmails: scope, categories }, "")), JSON.stringify(categories)).toEqual(want);
    }
  });

  it("split by the viewport put the unlocated elsewhere, and the off ones in it apart", () => {
    const { inView, offInView, elsewhere } = byView(narrowed(gmails, null, ""), new Set([10, 12]));
    expect([ids(inView), ids(offInView), ids(elsewhere)]).toEqual([[10], [12], [11, 20, 30]]);
  });
});
