import { describe, expect, it } from "vitest";

import { clock, nextAt, quietNow } from "@/entities/alerts/model/hours";
import { phoneOf } from "@/entities/alerts/model/text";

const hours = { tz: "Europe/Paris", daily: 540, quiet: [22 * 60, 7 * 60] as [number, number], snoozed_until: null };
const at = (iso: string) => new Date(iso);

describe("a person's hours", () => {
  it("are quiet past midnight, in their time zone", () => {
    expect(quietNow(hours, at("2026-11-24T22:30:00Z"))).toEqual({ by: "hours", until: "07:00" }); // 23:30 in Paris
    expect(quietNow(hours, at("2026-11-25T05:59:00Z"))).toEqual({ by: "hours", until: "07:00" });
    expect(quietNow(hours, at("2026-11-25T06:00:00Z"))).toBeNull();
    expect(quietNow(null, at("2026-11-24T22:30:00Z"))).toBeNull();
  });
  it("are snoozed first", () => {
    expect(quietNow({ ...hours, quiet: null, snoozed_until: "2026-11-24T14:32:00Z" }, at("2026-11-24T13:40:00Z"))).toEqual({ by: "snooze", until: "15:32" });
    expect(quietNow({ ...hours, quiet: null, snoozed_until: "2026-11-24T13:32:00Z" }, at("2026-11-24T13:40:00Z"))).toBeNull();
  });
  it("strike next strictly after now", () => {
    expect(nextAt("Europe/Paris", 18 * 60, at("2026-11-24T13:40:30Z")).toISOString()).toBe("2026-11-24T17:00:00.000Z");
    expect(nextAt("Europe/Paris", 7 * 60, at("2026-11-24T06:00:00Z")).toISOString()).toBe("2026-11-25T06:00:00.000Z");
    expect(clock(450)).toBe("07:30");
  });
});

describe("the number to call", () => {
  it("is a line's, labelled or not, and no other digits", () => {
    expect(phoneOf("New lead\nLéa\nPhone: +33 6 12 48 90 17")).toBe("+33 6 12 48 90 17");
    expect(phoneOf("New lead\n06 12 48 90 17")).toBe("06 12 48 90 17");
    expect(phoneOf("Payment received\nBilled: 1200.00 EUR")).toBeNull();
    expect(phoneOf("Slot booked: 2026-11-25 10:00 (Paris)")).toBeNull();
  });
});
