import { describe, expect, it } from "vitest";

import type { ArchiveTag, SiteTag } from "@/entities/gmb/model/gmb";
import { type Issue, type Tags, issueCounts, issueOf, issueText } from "@/entities/gmb/model/issues";
import type { Gmail, Gmb, GoogleCategory, Trade } from "@/shared/api/schema";
import { translator } from "@evinvest/i18n";

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
  brand_id: null,
  areas: [],
  hidden_address: null,
  customers_visit: null,
  film: null,
  role: null,
  owner_gmail: null,
  created_at: "2026-11-06T00:00:00Z",
  enabled,
  location: located ? { lat: 45.76, lng: 4.83, placed: false } : null,
  spots: [],
  website: null,
  hours: [],
  google_primary: null,
    synced_at: null,
  photos: [],
  readiness: null,
  tabs: [],
});
const archive = (suspended: number, failing: boolean): ArchiveTag => ({ tracked: true, rating: 4.5, reviews: 10, suspended, reinstating: 0, failing, live_30d: Array(30).fill(null) });
const site = (overdue: number): SiteTag => ({ brand: "aquafix", location: "lyon-9", leads_7d: 3, overdue });
const PLUMBING = 7;
/** Plumbing, set up in FR with Google's primary category unless `primary` is null. */
const plumbing = (primary: GoogleCategory | null = { id: "gcid:plumber", names: { en: "Plumber", fr: "Plombier", ru: "Сантехник" } }): Trade[] => [
  { id: PLUMBING, name: "Plumbing", created_at: "2026-11-09T00:00:00Z", categories: [{ country: "FR", lang: "fr", keywords: ["plombier"], google_primary: primary, google_additional: [], study: null }] },
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

/** A draft with `fill` set: none of its must-haves but its name otherwise, as the server judges them. */
const draft = (id: number, fill: Partial<Gmb> = {}): Gmb => ({
  ...gmb(id, { located: false }),
  place_id: null,
  gmail_id: null,
  state: "draft",
  next: ["verifying"],
  country: null,
  readiness: { missing: ["areas", "phone", "film", "gmail", "category", "site", "hidden_address", "profile"], unknown: [] },
  tabs: [
    { tab: "launch", closed: null },
    { tab: "browser", closed: null },
  ],
  ...fill,
});
const full: Partial<Gmb> = {
  trade_id: PLUMBING,
  country: { code: "FR", by_hand: true },
  areas: ["Lyon 8e"],
  phone: "+33478000001",
  hidden_address: "14 rue Laënnec, Lyon",
  customers_visit: false,
  film: "verif_vid:42",
  gmail_id: GMAIL,
};
/** The draft as listed: under a gmail whose browser is `browser`, when it has one. */
const listed = (g: Gmb, browser = true): { gmb: Gmb; gmail: Gmail | null } => ({
  gmb: g,
  gmail: g.gmail_id == null ? null : { id: g.gmail_id, gmail: "ops@example.test", created_at: "2026-11-06T00:00:00Z", browser, tabs: ["connections", "browser"] },
});

describe("a draft", () => {
  it("is a draft's issue, after overdue leads and before a missing point, told as its count of must-haves ready", () => {
    const tags: Tags = { archive: null, sites: new Map([[2, site(1)]]), trades: plumbing() };
    const all = { ...full, readiness: { missing: ["site" as const], unknown: [] } };
    check([draft(1, all), draft(2), gmb(3, { located: false })], tags, { 1: "draft", 2: "overdue", 3: "unlocated" });
    expect(issueText("draft", listed(draft(1, all)), tags, translator({}, "en"))).toBe("8/9");
    expect(issueText("draft", listed(draft(1, { ...all, readiness: { missing: [], unknown: ["site"] } })), tags, translator({}, "en"))).toBe("8/9");
  });
});
