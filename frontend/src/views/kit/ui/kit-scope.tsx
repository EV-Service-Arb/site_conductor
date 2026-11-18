"use client";

import { useResource } from "@evinvest/api/react";
import { Button, FloatingPanel } from "@evinvest/uikit";
import { ChevronLeft, Maximize2, Minimize2 } from "lucide-react";
import { useEffect } from "react";

import { type KitAt, floatKit, kit as kitKeys, useKitFloat } from "@/entities/gmb";
import { api } from "@/shared/api";
import { useT } from "@/shared/i18n";
import { type At, go } from "@/shared/lib/location";
import { DESKTOP_QUERY, useMediaQuery } from "@/shared/lib/use-media-query";
import { useButtonSize } from "@/shared/ui/touch";

import { pageOf, poolOf } from "../model/kit";
import { KitEdit } from "./kit-edit";
import { KitGmb } from "./kit-gmb";
import { KitOverview } from "./kit-overview";
import { KitPool } from "./kit-pool";
import { KitsList } from "./kits-list";
import { useKit } from "./use-kit";

const WIDTH_PX = 896;

/** What a kit shows at `at`, the same over the map and as a page; `go` moves within it, whichever it is. */
function KitBody({ at, go, phone }: { at: KitAt; go: (at: KitAt) => void; phone: boolean }) {
  if (at.kit === "") return <KitsList go={go} />;
  if (at.gmb !== undefined) return <KitGmb slug={at.kit} gmb={at.gmb} go={go} />;
  if (at.pool !== undefined) return <PoolOf slug={at.kit} pool={at.pool} />;
  return <KitOverview slug={at.kit} go={go} phone={phone} />;
}

function PoolOf({ slug, pool }: { slug: string; pool: string }) {
  const kit = useKit(slug);
  const { trade, country } = poolOf(pool);
  return <KitPool trade={trade} country={country} members={kit.status === "ok" ? kit.data.gmbs : []} />;
}

/** One level up: a GBP or a pool to its kit, a kit to the list. */
const up = (at: KitAt): KitAt => (at.gmb !== undefined || at.pool !== undefined ? { kit: at.kit } : { kit: "" });

function Title({ at, go }: { at: KitAt; go: (at: KitAt) => void }) {
  const t = useT();
  const button = useButtonSize();
  const kits = useResource(api, "/kits");
  const name = kits.status === "ok" ? kits.data.kits.find((k) => k.slug === at.kit)?.name : undefined;
  return (
    <span className="flex min-w-0 items-center gap-1">
      {at.kit !== "" && (
        <Button variant="ghost" size={button("sm")} icon aria-label={t(kitKeys.up, "Up")} onClick={() => go(up(at))}>
          <ChevronLeft aria-hidden />
        </Button>
      )}
      <span className="truncate">{at.kit === "" ? t(kitKeys.list, "Review kits") : t(kitKeys.window, "Review kit · {name}", { name: name ?? at.kit })}</span>
    </span>
  );
}

/** A kit over the map (13k), from md up; below it, its page. Its editor is a map window of its own. */
export function KitFloat({ map }: { map: google.maps.Map | null }) {
  const t = useT();
  const at = useKitFloat();
  const desk = useMediaQuery(DESKTOP_QUERY);
  const button = useButtonSize();
  const page = at === null || at.edit ? null : pageOf(at);
  const key = page === null ? null : JSON.stringify(page);
  useEffect(() => {
    if (!desk && key !== null) go(JSON.parse(key) as At, "replace");
  }, [desk, key]);
  if (at === null) return null;
  if (at.edit) return map === null ? null : <KitEdit map={map} slug={at.kit} />;
  if (!desk || page === null) return null;
  return (
    <FloatingPanel
      title={<Title at={at} go={floatKit} />}
      at={{ x: Math.max(16, (window.innerWidth - WIDTH_PX) / 2), y: 72 }}
      onClose={() => floatKit(null)}
      closeLabel={t(kitKeys.close, "Close the kit")}
      actions={
        <Button variant="ghost" size={button("sm")} icon aria-label={t(kitKeys.toPage, "Its own page")} title={t(kitKeys.toPage, "Its own page")} onClick={() => go(page)}>
          <Maximize2 aria-hidden />
        </Button>
      }
    >
      <div className="w-224 max-w-full">
        <KitBody at={at} go={floatKit} phone={false} />
      </div>
    </FloatingPanel>
  );
}

/** A kit as its own page (13l): the kits beside it from md up; on a phone (13n) always this. */
export function KitPage({ at }: { at: KitAt }) {
  const t = useT();
  const desk = useMediaQuery(DESKTOP_QUERY);
  const button = useButtonSize();
  const goPage = (k: KitAt) => go(pageOf(k));
  return (
    <div className="flex min-h-0 flex-1">
      {desk && (
        <aside className="w-64 shrink-0 overflow-y-auto border-r border-border">
          <KitsList go={goPage} current={at.kit} />
        </aside>
      )}
      <div className="flex min-w-0 flex-1 flex-col overflow-y-auto">
        <header className="flex items-center gap-2 border-b border-border px-3 py-2 text-sm">
          <span className="flex-1">
            <Title at={at} go={goPage} />
          </span>
          {desk && at.kit !== "" && (
            <Button variant="ghost" size={button("sm")} onClick={() => floatKit(at)}>
              <Minimize2 aria-hidden />
              {t(kitKeys.toMap, "Over the map")}
            </Button>
          )}
        </header>
        {at.kit === "" && desk ? <p className="p-5 text-sm text-ink-soft">{t(kitKeys.pickOne, "Pick a kit, or make one with New.")}</p> : <KitBody at={at} go={goPage} phone={!desk} />}
      </div>
    </div>
  );
}
