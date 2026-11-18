"use client";

import { useResource } from "@evinvest/api/react";
import { Button, Skeleton, cn } from "@evinvest/uikit";
import { Plus, QrCode } from "lucide-react";

import { type KitAt, floatKit, kit as kitKeys } from "@/entities/gmb";
import { api } from "@/shared/api";
import { useT } from "@/shared/i18n";
import { ErrorState } from "@/shared/ui/error-state";
import { useButtonSize } from "@/shared/ui/touch";

import { SECTION_HEAD } from "./parts";

/** The caller's kits: one opens, `New` draws one on the map. */
export function KitsList({ go, current }: { go: (at: KitAt) => void; current?: string }) {
  const t = useT();
  const button = useButtonSize();
  const kits = useResource(api, "/kits");
  if (kits.status === "loading") return <Skeleton className="m-4 h-32" />;
  if (kits.status === "error") return <ErrorState failure={kits.failure} onRetry={kits.reload} />;
  return (
    <nav aria-label={t(kitKeys.list, "Review kits")} className="flex flex-col gap-2 p-3">
      <div className="flex items-center gap-2 px-1">
        <h2 className={cn(SECTION_HEAD, "flex-1")}>
          {t(kitKeys.list.title, "Kits")} <span className="font-normal tabular-nums">{kits.data.kits.length}</span>
        </h2>
        <Button variant="ghost" size={button("sm")} onClick={() => floatKit({ kit: "", edit: true })}>
          <Plus aria-hidden />
          {t(kitKeys.list.new, "New")}
        </Button>
      </div>
      {kits.data.kits.length === 0 && <p className="px-1 text-sm text-ink-soft">{t(kitKeys.list.none, "No kit yet: a kit is GBPs you collect reviews for together, from an area, the outline or by hand.")}</p>}
      <ul className="flex flex-col gap-0.5">
        {kits.data.kits.map((k) => (
          <li key={k.slug}>
            <button
              type="button"
              onClick={() => go({ kit: k.slug })}
              aria-current={k.slug === current ? "page" : undefined}
              className={cn("flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left outline-none hover:bg-hover focus-visible:ring-2 focus-visible:ring-ring", k.slug === current && "bg-hover")}
            >
              <QrCode aria-hidden className="size-4 shrink-0 text-ink-soft" />
              <span className="flex min-w-0 flex-col">
                <span className="truncate text-sm text-ink">{k.name}</span>
                <span className="truncate text-xs text-ink-soft">{t(kitKeys.list.members, "{n} GBPs", { n: k.members })}</span>
              </span>
            </button>
          </li>
        ))}
      </ul>
    </nav>
  );
}
