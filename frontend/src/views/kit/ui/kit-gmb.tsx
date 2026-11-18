"use client";

import { useResource } from "@evinvest/api/react";
import { Badge, Button, Skeleton, cn, useBinding } from "@evinvest/uikit";
import { ChevronDown, ChevronUp, QrCode } from "lucide-react";

import { type KitAt, KitTargetSegment, kit as kitKeys } from "@/entities/gmb";
import { api } from "@/shared/api";
import type { KitDraft, KitMember, KitRanked } from "@/shared/api/schema";
import { useT } from "@/shared/i18n";
import { go as goTo } from "@/shared/lib/location";
import { ErrorState } from "@/shared/ui/error-state";
import { Qr } from "@/shared/ui/qr";
import { useButtonSize } from "@/shared/ui/touch";

import { sinceOf, usePeriod } from "../model/kit";
import { Chip, HINT, PeriodPicker, SECTION_HEAD, Tally, Thumb } from "./parts";
import { useKit } from "./use-kit";

/** A GBP of a kit (13m, 13o): where its QR leads, what its pool offers here now, what went here, and its tally; ↑↓ through the kit, q its QR. */
export function KitGmb({ slug, gmb, go }: { slug: string; gmb: number; go: (at: KitAt) => void }) {
  const t = useT();
  const kit = useKit(slug);
  if (kit.status === "loading") return <Skeleton className="m-4 h-64" />;
  if (kit.status === "error") return <ErrorState failure={kit.failure} onRetry={kit.reload} />;
  const at = kit.data.gmbs.findIndex((g) => g.id === gmb);
  if (at < 0) return <p className="p-5 text-sm text-ink-soft">{t(kitKeys.gmb.notIn, "Not in this kit any more.")}</p>;
  return <Member key={gmb} slug={slug} members={kit.data.gmbs} at={at} go={go} onSet={kit.reload} />;
}

function Member({ slug, members, at, go, onSet }: { slug: string; members: readonly KitMember[]; at: number; go: (at: KitAt) => void; onSet: () => void }) {
  const t = useT();
  const button = useButtonSize();
  const period = usePeriod();
  const g = members[at]!;
  const res = useResource(api, "/gmbs/{id}/kit", { params: { path: { id: g.id }, query: { since: sinceOf(period) } } });
  const cat = g.category;
  const step = (by: number) => go({ kit: slug, gmb: members[(at + by + members.length) % members.length]!.id });
  const showQr = g.url === null ? null : () => goTo({ view: "kit_qr", params: { slug, gmb: g.id } });
  useBinding({ on: { key: "q" }, id: "kit-qr", run: { label: t(kitKeys.showQr, "Show QR"), run: showQr } });
  useBinding({ on: { key: "ArrowDown" }, id: "kit-next", run: { label: t(kitKeys.next, "Next GBP"), run: members.length > 1 ? () => step(1) : null } });
  useBinding({ on: { key: "ArrowUp" }, id: "kit-prev", run: { label: t(kitKeys.prev, "Previous GBP"), run: members.length > 1 ? () => step(-1) : null } });

  return (
    <div className="flex flex-col gap-5 p-5">
      <header className="flex flex-wrap items-center gap-2">
        <div className="flex min-w-0 flex-1 flex-col">
          <h2 className="truncate text-lg font-semibold text-ink">{g.name}</h2>
          <p className="truncate text-sm text-ink-soft">{g.address}</p>
        </div>
        {members.length > 1 && (
          <span className="flex items-center gap-1 font-mono text-xs text-ink-soft tabular-nums">
            {at + 1} / {members.length}
            <Button variant="ghost" size={button("sm")} icon aria-label={t(kitKeys.prev, "Previous GBP")} onClick={() => step(-1)}>
              <ChevronUp aria-hidden />
            </Button>
            <Button variant="ghost" size={button("sm")} icon aria-label={t(kitKeys.next, "Next GBP")} onClick={() => step(1)}>
              <ChevronDown aria-hidden />
            </Button>
          </span>
        )}
        <Button size={button("sm")} disabled={showQr === null} onClick={showQr ?? undefined}>
          <QrCode aria-hidden />
          {t(kitKeys.showQr, "Show QR")}
        </Button>
      </header>
      {res.status === "loading" && <Skeleton className="h-64" />}
      {res.status === "error" && <ErrorState failure={res.failure} onRetry={res.reload} />}
      {res.status === "ok" && (
        <>
          <section className="flex flex-col gap-2">
            <h3 className={SECTION_HEAD}>
              {t(kitKeys.qrOpens, "The QR opens")}
              <span className="flex-1" />
              {res.data.page !== null && <span className={cn(HINT, "font-mono")}>{res.data.page}</span>}
            </h3>
            <KitTargetSegment gmb={g.id} kit={res.data} onSet={() => (res.reload(), onSet())} />
            {res.data.url !== null && (
              <div className="hidden items-center gap-4 pt-2 md:flex">
                <Qr text={res.data.url} className="size-28 shrink-0 rounded-md p-1" />
                <p className="min-w-0 font-mono text-xs break-all text-ink-soft">{res.data.url}</p>
              </div>
            )}
          </section>
          {cat === null ? <p className="text-sm text-ink-soft">{t(kitKeys.gmb.noCategory, "No category: nothing to offer but Google's form.")}</p> : <Offered drafts={res.data.drafts} cat={cat} brand={res.data.brand?.id ?? null} />}
          <section className="flex flex-col gap-2">
            <div className="flex items-center gap-2">
              <h3 className={cn(SECTION_HEAD, "flex-1")}>{t(kitKeys.tally.title, "Collected")}</h3>
              <PeriodPicker />
            </div>
            <Tally tally={res.data.tally} />
          </section>
        </>
      )}
    </div>
  );
}

function Offered({ drafts, cat, brand }: { drafts: readonly KitRanked[]; cat: NonNullable<KitMember["category"]>; brand: number | null }) {
  const t = useT();
  const pool = useResource(api, "/trades/{id}/{country}/kit", { params: { path: { id: cat.trade, country: cat.country } } });
  const tags = new Map<number, KitDraft>(pool.status === "ok" ? pool.data.drafts.map((d) => [d.id, d]) : []);
  const went = drafts.filter((d) => d.went_here);
  return (
    <>
      <Drafts title={t(kitKeys.drafts, "Drafts")} hint={t(kitKeys.drafts.offerOrder, "least used first · the pool is the category's")} drafts={drafts.filter((d) => !d.went_here)} tags={tags} brand={brand} empty={t(kitKeys.drafts.noneLeft, "None left to offer here: its page answers with Google's form.")} />
      {went.length > 0 && <Drafts title={t(kitKeys.wentHere, "Went here")} hint={t(kitKeys.wentHere.why, "copied on this GBP: never offered here again")} drafts={went} tags={tags} brand={brand} />}
    </>
  );
}

function Drafts({ title, hint, drafts, tags, brand, empty }: { title: string; hint: string; drafts: readonly KitRanked[]; tags: Map<number, KitDraft>; brand: number | null; empty?: string }) {
  const t = useT();
  return (
    <section className="flex flex-col gap-2">
      <h3 className={SECTION_HEAD}>
        {title} <span className="font-normal tabular-nums">{drafts.length}</span>
        <span className="flex-1" />
        <span className={HINT}>{hint}</span>
      </h3>
      {drafts.length === 0 && empty !== undefined && <p className="text-sm text-ink-soft">{empty}</p>}
      <ul className="flex flex-col divide-y divide-border border-y border-border">
        {drafts.map((d) => {
          const tagged = tags.get(d.id);
          return (
            <li key={d.id} className={cn("flex items-center gap-3 py-2.5", d.went_here && "opacity-50")}>
              <span className="flex min-w-0 flex-1 flex-col gap-1">
                <span className="line-clamp-2 text-sm text-ink">{d.text}</span>
                {tagged && (
                  <span className="flex flex-wrap gap-1">
                    {tagged.keywords.map((k) => (
                      <Chip key={k}>{k}</Chip>
                    ))}
                    {tagged.places.map((p) => (
                      <Chip key={p} tone="place">
                        {p}
                      </Chip>
                    ))}
                  </span>
                )}
              </span>
              {brand !== null && (
                <span className="flex shrink-0 gap-1">
                  {d.photos.map((p) => (
                    <span key={p.id} title={p.shows ?? undefined}>
                      <Thumb brand={brand} photo={p.id} className="size-9" />
                    </span>
                  ))}
                </span>
              )}
              <span className="flex w-28 shrink-0 flex-col items-end gap-0.5">
                <span className="font-mono text-sm text-ink tabular-nums">{t(kitKeys.uses, "{n} uses", { n: d.uses })}</span>
                {d.offered && <Badge tone="positive">{t(kitKeys.offered, "offered now")}</Badge>}
                {d.held && <Badge tone="warn">{t(kitKeys.held, "held by an open page")}</Badge>}
              </span>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
