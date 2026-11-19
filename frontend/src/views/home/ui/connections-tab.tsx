"use client";

import { Planned } from "@evinvest/uikit";

import type { Listed } from "@/entities/gmb";

/** What it is connected to (02d): its verification kit, its gmail's inbox and browser, its photos. */
export function ConnectionsTab({ listed }: { listed: Listed }) {
  return (
    <div className="p-6">
      <Planned what={`${listed.gmb.name}: Kit readiness, the inbox, the browser, brand photos, the playbook`} />
    </div>
  );
}
