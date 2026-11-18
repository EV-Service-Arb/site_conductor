"use client";

import { useResource } from "@evinvest/api/react";
import { Button, Skeleton } from "@evinvest/uikit";
import { ChevronLeft, ChevronRight } from "lucide-react";
import { useEffect, useRef } from "react";

import { KitTargetSegment, kit as kitKeys } from "@/entities/gmb";
import { api, ok } from "@/shared/api";
import type { KitMember } from "@/shared/api/schema";
import { useT } from "@/shared/i18n";
import { go } from "@/shared/lib/location";
import { ErrorState } from "@/shared/ui/error-state";
import { notifyFailure } from "@/shared/ui/notify";
import { Qr } from "@/shared/ui/qr";

import { sinceOf } from "../model/kit";
import { useKit } from "./use-kit";

const SWIPE_PX = 60;

/** A kit's GBP's QR for the reviewer's camera, full screen and light whatever the theme; a swipe or the arrows to the kit's next one that has a QR. */
export function KitQr({ slug, gmb }: { slug: string; gmb: number }) {
  const t = useT();
  const kit = useKit(slug);
  if (kit.status === "loading") return <Skeleton className="min-h-svh rounded-none" />;
  if (kit.status === "error")
    return (
      <div className="p-4">
        <ErrorState failure={kit.failure} onRetry={kit.reload} />
      </div>
    );
  const shown = kit.data.gmbs.filter((g) => g.url !== null || g.id === gmb);
  const at = shown.findIndex((g) => g.id === gmb);
  if (at < 0) return <p className="p-5 text-sm text-ink-soft">{t(kitKeys.gmb.notIn, "Not in this kit any more.")}</p>;
  return <Shown key={gmb} slug={slug} kit={kit.data.name} gmbs={shown} at={at} onSet={kit.reload} />;
}

function Shown({ slug, kit, gmbs, at, onSet }: { slug: string; kit: string; gmbs: readonly KitMember[]; at: number; onSet: () => void }) {
  const t = useT();
  const gmb = gmbs[at]!;
  const res = useResource(api, "/gmbs/{id}/kit", { params: { path: { id: gmb.id }, query: { since: sinceOf("today") } } });
  // the kit's QR, not the GBP's own: what it opens is tagged with the kit
  const url = gmb.url;
  const start = useRef<number | null>(null);
  const step = (by: number) => {
    const next = gmbs[(at + by + gmbs.length) % gmbs.length];
    if (next !== undefined && next.id !== gmb.id) go({ view: "kit_qr", params: { slug, gmb: next.id } }, "replace");
  };
  useEffect(() => {
    if (url === null) return;
    ok(api.POST("/gmbs/{id}/kit/shown", { params: { path: { id: gmb.id }, query: { kit: slug } } })).catch((e: unknown) => notifyFailure(e, t));
  }, [gmb.id, slug, url, t]);
  const lead = res.status !== "ok" || res.data.target === null ? null : res.data.target === "google" ? t(kitKeys.page.lead.google, "Google's review form") : res.data.target === "one_tap" ? t(kitKeys.page.lead.oneTap, "one tap copies a drafted review, then Google") : t(kitKeys.page.lead.drafts, "a drafted review to start from, then Google");
  return (
    <div
      className="flex min-h-svh touch-pan-y flex-col bg-background"
      onPointerDown={(e) => {
        start.current = e.clientX;
      }}
      onPointerUp={(e) => {
        if (start.current === null) return;
        const dx = e.clientX - start.current;
        start.current = null;
        if (Math.abs(dx) >= SWIPE_PX) step(dx < 0 ? 1 : -1);
      }}
    >
      <header className="flex items-center gap-1 px-2 py-2 text-sm text-ink-soft">
        <Button variant="ghost" size="touch" onClick={() => window.history.back()}>
          <ChevronLeft aria-hidden />
          {kit}
        </Button>
        <span className="flex-1" />
        {gmbs.length > 1 && (
          <>
            <Button variant="ghost" size="touch" icon aria-label={t(kitKeys.prev, "Previous GBP")} onClick={() => step(-1)}>
              <ChevronLeft aria-hidden />
            </Button>
            <span className="font-mono tabular-nums">
              {at + 1} / {gmbs.length}
            </span>
            <Button variant="ghost" size="touch" icon aria-label={t(kitKeys.next, "Next GBP")} onClick={() => step(1)}>
              <ChevronRight aria-hidden />
            </Button>
          </>
        )}
      </header>
      <main className="flex flex-1 flex-col items-center gap-4 bg-white px-6 py-8 text-center text-black">
        <h1 className="text-2xl font-bold">{gmb.name}</h1>
        {gmb.address !== null && <p className="text-sm text-neutral-600">{gmb.address}</p>}
        {res.status === "error" ? (
          <ErrorState failure={res.failure} onRetry={res.reload} />
        ) : url === null ? (
          res.status !== "loading" ? <p className="py-24 text-base text-neutral-600">{t(kitKeys.page.off, "Its kit is off: pick where its QR leads below.")}</p> : <Skeleton className="aspect-square w-full max-w-sm" />
        ) : (
          <>
            <Qr text={url} className="aspect-square w-full max-w-sm" />
            <p className="text-lg font-semibold">{t(kitKeys.page.scan, "Scan to leave a review")}</p>
            {lead !== null && <p className="text-sm text-neutral-600">{lead}</p>}
          </>
        )}
      </main>
      <footer className="flex flex-col gap-2 px-3 pt-3 pb-4">
        {res.status === "ok" && <KitTargetSegment gmb={gmb.id} kit={res.data} onSet={() => (res.reload(), onSet())} />}
        {url !== null && <p className="truncate px-1 font-mono text-xs text-ink-soft">{url}</p>}
      </footer>
    </div>
  );
}
