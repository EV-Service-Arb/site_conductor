"use client";

import { Planned } from "@evinvest/uikit";

import type { Listed } from "@/entities/gmb";

/** A verifying GBP's road to Verified (05e). */
export function VerificationTab({ listed }: { listed: Listed }) {
  return (
    <div className="p-6">
      <Planned what={`${listed.gmb.name}: Google's emails, the kit, attempts`} />
    </div>
  );
}
