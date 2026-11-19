"use client";

import { useResource } from "@evinvest/api/react";
import { Button } from "@evinvest/uikit";
import { ChevronLeft } from "lucide-react";

import { type KitAt, kit as kitKeys, openWindow, poolOf } from "@/entities/gmb";
import { api } from "@/shared/api";
import { useT } from "@/shared/i18n";
import { DESKTOP_QUERY, useMediaQuery } from "@/shared/lib/use-media-query";
import { useButtonSize } from "@/shared/ui/touch";

import { KitOverview } from "./kit-overview";
import { KitGmb } from "./kit-gmb";
import { KitPool } from "./kit-pool";
import { KitsList } from "./kits-list";
import { useKit } from "./use-kit";

const go = (at: KitAt) => openWindow({ kind: "kit", at });

function PoolOf({ slug, pool }: { slug: string; pool: string }) {
  const kit = useKit(slug);
  const { trade, country } = poolOf(pool);
  return <KitPool trade={trade} country={country} members={kit.status === "ok" ? kit.data.gmbs : []} />;
}

/** What a kit shows at `at`, the same over the map and as a page; as a page from md up, the kits beside it. */
export function KitBody({ at, page }: { at: KitAt; page: boolean }) {
  const t = useT();
  const desk = useMediaQuery(DESKTOP_QUERY);
  const body =
    at.kit === "" ? (
      page && desk ? <p className="p-5 text-sm text-ink-soft">{t(kitKeys.pickOne, "Pick a kit, or make one with New.")}</p> : <KitsList go={go} />
    ) : at.gmb !== undefined ? (
      <KitGmb slug={at.kit} gmb={at.gmb} go={go} />
    ) : at.pool !== undefined ? (
      <PoolOf slug={at.kit} pool={at.pool} />
    ) : (
      <KitOverview slug={at.kit} go={go} phone={!desk} />
    );
  if (!page || !desk) return body;
  return (
    <div className="flex min-h-0 flex-1">
      <aside className="w-64 shrink-0 overflow-y-auto border-r border-border">
        <KitsList go={go} current={at.kit} />
      </aside>
      <div className="flex min-w-0 flex-1 flex-col overflow-y-auto">{body}</div>
    </div>
  );
}

/** One level up: a GBP or a pool to its kit, a kit to the list. */
const up = (at: KitAt): KitAt => (at.gmb !== undefined || at.pool !== undefined ? { kit: at.kit } : { kit: "" });

export function KitTitle({ at }: { at: KitAt }) {
  const t = useT();
  const button = useButtonSize();
  const name = useKitName(at.kit);
  return (
    <span className="flex min-w-0 items-center gap-1">
      {at.kit !== "" && (
        <Button variant="ghost" size={button("sm")} icon aria-label={t(kitKeys.up, "Up")} onClick={() => go(up(at))}>
          <ChevronLeft aria-hidden />
        </Button>
      )}
      <span className="truncate">{at.kit === "" ? t(kitKeys.list, "Review kits") : t(kitKeys.window, "Review kit · {name}", { name })}</span>
    </span>
  );
}

/** A kit's name, its slug until the kits are read. */
export function useKitName(slug: string): string {
  const kits = useResource(api, "/kits");
  return (kits.status === "ok" ? kits.data.kits.find((k) => k.slug === slug)?.name : undefined) ?? slug;
}
