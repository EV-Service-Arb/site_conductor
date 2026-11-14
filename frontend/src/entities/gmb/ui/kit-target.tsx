"use client";

import { ToggleGroup, ToggleGroupItem, cn } from "@evinvest/uikit";
import { useState } from "react";

import { useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";

import { setKitTarget } from "../api/kit";
import { kit as kitKeys } from "../model/i18n";
import { type GmbKit, KIT_TARGETS, TARGET_NAME } from "../model/kit";

const OFF = "off";

/** Where a GBP's QR leads, or the kit off; the page targets only with a landing page. */
export function KitTargetSegment({ gmb, kit, onSet, className }: { gmb: number; kit: GmbKit; onSet: (kit: GmbKit) => void; className?: string }) {
  const t = useT();
  const [busy, setBusy] = useState(false);
  const noPage = t(kitKeys.target.noPage, "No landing page of its: only Google's form");
  return (
    <div className={cn("flex flex-col gap-1", className)}>
      <ToggleGroup
        type="single"
        variant="outline"
        size="sm"
        className="w-full"
        value={kit.target ?? OFF}
        onValueChange={(v) => {
          if (typeof v !== "string" || v === "") return; // the pressed one pressed again: it stays
          setBusy(true);
          setKitTarget(gmb, v === OFF ? null : KIT_TARGETS.find((k) => k === v)!)
            .then(onSet, (e: unknown) => notifyFailure(e, t))
            .finally(() => setBusy(false));
        }}
      >
        <ToggleGroupItem value={OFF} className="min-w-0 flex-1 px-1 text-xs sm:text-sm" disabled={busy}>
          {t(kitKeys.target.off, "Off")}
        </ToggleGroupItem>
        {KIT_TARGETS.map((k) => (
          <ToggleGroupItem key={k} value={k} className="min-w-0 flex-1 px-1 text-xs sm:text-sm" disabled={busy || (k !== "google" && kit.page === null)} title={k !== "google" && kit.page === null ? noPage : undefined}>
            {TARGET_NAME[k](t)}
          </ToggleGroupItem>
        ))}
      </ToggleGroup>
      {kit.page === null && <p className="text-xs text-ink-soft">{noPage}</p>}
    </div>
  );
}
