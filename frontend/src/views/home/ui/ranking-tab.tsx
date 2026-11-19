"use client";

import { Planned } from "@evinvest/uikit";

import type { Listed } from "@/entities/gmb";

/** Where it ranks (02b): its category's keywords, grid scans. */
export function RankingTab({ listed }: { listed: Listed }) {
  return (
    <div className="p-6">
      <Planned what={`${listed.gmb.name}: Geo-grid scans of its category keywords (#94)`} />
    </div>
  );
}
