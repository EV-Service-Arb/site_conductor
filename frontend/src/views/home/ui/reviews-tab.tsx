"use client";

import { Planned } from "@evinvest/uikit";

import type { Listed } from "@/entities/gmb";

/** Its reviews as review_archive tracks them (02c). */
export function ReviewsTab({ listed }: { listed: Listed }) {
  return (
    <div className="p-6">
      <Planned what={`${listed.gmb.name}: The review board: active, suspended, reinstating, to remove`} />
    </div>
  );
}
