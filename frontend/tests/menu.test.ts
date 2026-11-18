import { describe, expect, it } from "vitest";

import { panelGroups } from "@/features/account-chip/model/attributes";
import { grants, seesSites } from "@/entities/session/model/access";
import type { Caller, Need, Permission } from "@/shared/api/schema";
import type { T } from "@/shared/i18n";

const t: T = (...args) => args[1];

const holding = (...permissions: Permission[]): Caller => ({ user_id: "u", email: "e", preferred_name: "", permissions, dev_sign_in: false, acting: null });
const alias = (name: Need): Caller => holding(...grants(name));

/** What the top bar offers a caller: Sites, the chip's entries past the profile. */
function check(caller: Caller, expected: { sites: boolean; chip: string[][] }) {
  const chip = panelGroups(caller, t).map((g) => g.items.map((i) => i.id));
  expect({ sites: seesSites(caller), chip }).toEqual(expected);
}

describe("the top bar, by what the caller holds", () => {
  it("offers someone with nothing a chip of their account alone", () => {
    check(holding(), { sites: false, chip: [] });
    check(alias("sa:everyone"), { sites: false, chip: [] });
  });

  it("gives an operator Sites", () => {
    check(alias("sa:operator"), { sites: true, chip: [] });
  });

  it("gives acting as a member to admins alone", () => {
    check(alias("sa:admin"), { sites: true, chip: [["act-as"]] });
  });

  it("shows Sites for either section its nav lists, and nothing for a sites write alone", () => {
    check(holding("sa:sites:analysis:read"), { sites: true, chip: [] });
    check(holding("sa:sites:work:leads:edit", "sa:sites:work:pii:see"), { sites: false, chip: [] });
  });
});
