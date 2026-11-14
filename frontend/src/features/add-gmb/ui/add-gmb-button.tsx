"use client";

import { Button } from "@evinvest/uikit";
import { Plus } from "lucide-react";
import { usePathname, useRouter } from "next/navigation";

import { addHref, gmbs, openAdd } from "@/entities/gmb";
import { useT } from "@/shared/i18n";
import { useButtonSize } from "@/shared/ui/touch";

/** Opens the Add a GBP panel over the home map, of a trade (`trade` first, the outline's scope's), under `gmail` if given. */
export function AddGmbButton({ gmail, trade, labeled }: { gmail: number | null; trade: number | null; labeled?: boolean }) {
  const t = useT();
  const button = useButtonSize();
  const router = useRouter();
  const pathname = usePathname();
  const label = t(gmbs.gmb.add, "Add a GBP");
  return (
    <Button
      variant="ghost"
      size={button("sm")}
      icon={!labeled}
      aria-label={label}
      title={labeled ? undefined : label}
      className={labeled ? "justify-start text-ink-soft" : undefined}
      onClick={() => {
        if (pathname === "/") openAdd({ gmail, trade });
        else router.push(addHref({ gmail, trade }));
      }}
    >
      <Plus aria-hidden />
      {labeled && label}
    </Button>
  );
}
