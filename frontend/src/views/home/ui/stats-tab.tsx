"use client";

import { Planned } from "@evinvest/uikit";

import type { Listed } from "@/entities/gmb";

/** Its output (02a): leads and reviews a day, what changed on it. */
export function StatsTab({ listed }: { listed: Listed }) {
  return (
    <div className="p-6">
      <Planned what={`${listed.gmb.name}: Leads, live reviews and changes over 7/30/90 days`} />
    </div>
  );
}
