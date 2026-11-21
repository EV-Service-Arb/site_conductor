"use client";

import { Button } from "@evinvest/uikit";
import { Plus } from "lucide-react";

import { gmbs, openWindow } from "@/entities/gmb";
import { useT } from "@/shared/i18n";
import { useButtonSize } from "@/shared/ui/touch";

/** Opens the Add a GBP panel over the home map, of a trade (`trade` first, the outline's scope's), under `gmail` if given. */
export function AddGmbButton({ gmail, trade, labeled }: { gmail: number | null; trade: string | null; labeled?: boolean }) {
  const t = useT();
  const button = useButtonSize();
  const label = t(gmbs.gmb.add, "Add a GBP");
  return (
    <Button
      variant="ghost"
      size={button("sm")}
      icon={!labeled}
      aria-label={label}
      title={labeled ? undefined : label}
      className={labeled ? "justify-start text-ink-soft" : undefined}
      onClick={() => openWindow({ kind: "add", at: { ...(gmail !== null && { gmail }), ...(trade !== null && { trade }) } })}
    >
      <Plus aria-hidden />
      {labeled && label}
    </Button>
  );
}
