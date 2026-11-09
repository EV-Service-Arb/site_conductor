import { describe, expect, it } from "vitest";

import type { ArchiveTag, Gmb, SiteTag, Trade } from "@/entities/gmb/model/gmb";
import { type Issue, type Tags, issueCounts, issueOf, issueText } from "@/entities/gmb/model/issues";
import { readiness } from "@/entities/gmb/model/readiness";
import { translator } from "@/shared/i18n/translate";

const GMAIL = 1;
const gmb = (id: number, { located = true, enabled = true } = {}): Gmb => ({
  id,
  place_id: `ChIJplace${id}`,
  name: `GMB ${id}`,
  address: null,
  gmail_id: GMAIL,
  state: "verified",
  next: ["suspended", "verifying"],
  country: { code: "FR", by_hand: false },
  phone: null,
  trade_id: null,
  areas: [],
  hidden_address: null,
  customers_visit: null,
  film: null,
  profile: null,
  created_at: "2026-11-06T00:00:00Z",
  enabled,
  location: located ? { lat: 45.76, lng: 4.83, placed: false } : null,
});
const archive = (suspended: number, failing: boolean): ArchiveTag => ({ tracked: true, rating: 4.5, reviews: 10, suspended, reinstating: 0, failing });
const site = (overdue: number): SiteTag => ({ brand: "aquafix", location: "lyon-9", leads_7d: 3, overdue });
const PLUMBING = 7;
/** Plumbing, set up in FR with Google's label unless `label` is null. */
const plumbing = (label: string | null = "Plombier"): Trade[] => [
  { id: PLUMBING, name: "Plumbing", created_at: "2026-11-09T00:00:00Z", categories: [{ country: "FR", lang: "fr", keywords: ["plombier"], google_category: label, study: null }] },
];

/** Each GMB's issue under `tags`, by id. */
function check(gmbs: Gmb[], tags: Tags, want: Record<number, Issue | null>) {
  expect(Object.fromEntries(gmbs.map((g) => [g.id, issueOf(g, tags)]))).toEqual(want);
}

describe("a GMB's issue", () => {
  const every = [gmb(1, { located: false }), gmb(2), gmb(3), gmb(4), gmb(5, { located: false, enabled: false })];

  it("is the most pressing one it has; an off one's is only that it is off", () => {
    check(
      every,
      {
        archive: new Map([
          [1, archive(2, true)],
          [2, archive(0, true)],
          [3, archive(0, false)],
          [4, archive(0, false)],
          [5, archive(2, true)],
        ]),
        sites: new Map([
          [1, site(1)],
          [2, site(0)],
          [3, site(0)],
          [5, site(1)],
        ]),
        trades: null,
      },
      { 1: "overdue", 2: "failing", 3: null, 4: "no_site", 5: "off" },
    );
  });

  it("drops what a service that did not answer would have said", () => {
    const tags: Tags = { archive: null, sites: null, trades: null };
    check(every, tags, { 1: "unlocated", 2: null, 3: null, 4: null, 5: "off" });
    expect(issueCounts(every, tags)).toEqual(
      new Map<Issue, number>([
        ["unlocated", 1],
        ["off", 1],
      ]),
    );
  });
});

describe("a GBP's category", () => {
  it("is missing, last before off, while it has no trade or its trade is not set up in its country", () => {
    const us = { ...gmb(3, { located: true }), trade_id: PLUMBING, country: { code: "US" as const, by_hand: false } };
    check([gmb(1), { ...gmb(2), trade_id: PLUMBING }, us, gmb(4, { enabled: false })], { archive: null, sites: null, trades: plumbing() }, { 1: "no_category", 2: null, 3: "no_category", 4: "off" });
  });
});

/** A draft with `fill` set: none of its must-haves but its name otherwise. */
const draft = (id: number, fill: Partial<Gmb> = {}): Gmb => ({ ...gmb(id, { located: false }), place_id: null, gmail_id: null, state: "draft", next: ["verifying"], country: null, ...fill });
const full: Partial<Gmb> = {
  trade_id: PLUMBING,
  country: { code: "FR", by_hand: true },
  areas: ["Lyon 8e"],
  phone: "+33478000001",
  hidden_address: "14 rue Laënnec, Lyon",
  customers_visit: false,
  film: "verif_vid:42",
  gmail_id: GMAIL,
  profile: "mlx:lyon-8",
};

describe("a draft's readiness", () => {
  const ready = (g: Gmb, tags: Tags) =>
    readiness(g, tags)
      .filter((r) => r.ready)
      .map((r) => r.must);

  it("is its name alone until the rest is set, in Google's order", () => {
    expect(ready(draft(1), { archive: null, sites: new Map(), trades: plumbing() })).toEqual(["name"]);
    expect(ready(draft(1, full), { archive: null, sites: new Map(), trades: plumbing() })).toEqual(["name", "category", "areas", "phone", "hidden_address", "film", "gmail", "profile"]);
  });

  it("has its site only where sites links a place to it, and none while sites is unread", () => {
    expect(ready(draft(1, full), { archive: null, sites: new Map([[1, site(0)]]), trades: plumbing() })).toHaveLength(9);
    expect(ready(draft(1, full), { archive: null, sites: null, trades: plumbing() })).not.toContain("site");
  });

  it("has its address only once whether customers visit is decided", () => {
    expect(ready(draft(1, { ...full, customers_visit: null }), { archive: null, sites: null, trades: plumbing() })).not.toContain("hidden_address");
  });

  it("has its category only once its trade is set up in its country with Google's label", () => {
    for (const [why, fill, trades] of [
      ["no country", { ...full, country: null }, plumbing()],
      ["not set up there", { ...full, country: { code: "US", by_hand: true } }, plumbing()],
      ["no label", full, plumbing(null)],
      ["the trades unread", full, null],
    ] as const) {
      expect(ready(draft(1, fill), { archive: null, sites: null, trades }), why).not.toContain("category");
    }
  });

  it("is a draft's issue, after overdue leads and before a missing point, told as its count", () => {
    const tags: Tags = { archive: null, sites: new Map([[2, site(1)]]), trades: plumbing() };
    check([draft(1, full), draft(2), gmb(3, { located: false })], tags, { 1: "draft", 2: "overdue", 3: "unlocated" });
    expect(issueText("draft", draft(1, full), tags, translator("en"))).toBe("8/9");
  });
});
