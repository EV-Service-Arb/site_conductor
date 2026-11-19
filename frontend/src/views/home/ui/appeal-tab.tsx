"use client";

import { Planned } from "@evinvest/uikit";

import type { Listed } from "@/entities/gmb";

/** A suspended GBP's appeal (05h). */
export function AppealTab({ listed }: { listed: Listed }) {
  return (
    <div className="p-6">
      <Planned what={`${listed.gmb.name}: What it is linked to, documents, sent appeals`} />
    </div>
  );
}
