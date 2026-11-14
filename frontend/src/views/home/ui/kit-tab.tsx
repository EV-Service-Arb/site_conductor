"use client";

import { Badge, Skeleton, cn } from "@evinvest/uikit";

import { type KitRanked, KitTargetSegment, type Listed, fetchGmbKit, kit, photoSrc } from "@/entities/gmb";
import { useT } from "@/shared/i18n";
import { useResource } from "@/shared/lib/use-resource";
import { ErrorState } from "@/shared/ui/error-state";
import { Qr } from "@/shared/ui/qr";

const SECTION = "flex flex-col gap-2";
const SECTION_HEAD = "flex items-baseline gap-2 text-xs font-semibold tracking-wide text-ink-soft uppercase";
const HINT = "font-normal tracking-normal normal-case";

/** A GBP's review kit (13g): where its QR leads, what its category's pool offers here now, and the last 30 days. */
export function KitTab({ listed }: { listed: Listed }) {
  const t = useT();
  const { gmb } = listed;
  const res = useResource(`kit.gmb.${gmb.id}`, () => fetchGmbKit(gmb.id), `kit.gmb.${gmb.id}`, { focus: true });
  if (res.status === "loading") return <Skeleton className="h-64 w-full max-w-3xl" />;
  if (res.status === "error") return <ErrorState failure={res.failure} onRetry={res.reload} />;
  const k = res.data;
  const country = gmb.country?.code;
  const src = (photo: number) => (gmb.trade_id === null || country === undefined ? undefined : photoSrc(gmb.trade_id, country, photo));
  const open = k.drafts.filter((d) => !d.went_here);
  const went = k.drafts.filter((d) => d.went_here);
  const funnel = [
    [k.funnel.shown, t(kit.funnel.shown, "shown")],
    [k.funnel.scanned, t(kit.funnel.scanned, "scanned")],
    [k.funnel.opened, t(kit.funnel.opened, "opened")],
    [k.funnel.copied, t(kit.funnel.copied, "copied")],
    [k.funnel.photos_saved, t(kit.funnel.photosSaved, "photos saved")],
  ] as const;

  return (
    <div className="flex max-w-3xl flex-col gap-6">
      <section className={SECTION}>
        <h3 className={SECTION_HEAD}>{t(kit.qrOpens, "The QR opens")}</h3>
        <KitTargetSegment gmb={gmb.id} kit={k} onSet={res.reload} />
        {k.url !== null && (
          <div className="flex items-center gap-4 pt-2">
            <Qr text={k.url} className="size-36 shrink-0 rounded-md p-1" />
            <p className="min-w-0 font-mono text-xs break-all text-ink-soft">{k.url}</p>
          </div>
        )}
      </section>

      <section className={SECTION}>
        <h3 className={SECTION_HEAD}>
          {t(kit.drafts, "Drafts")} <span className="font-normal tabular-nums">{open.length}</span>
          <span className="flex-1" />
          <span className={HINT}>{t(kit.drafts.offerOrder, "in the offer's order · the pool is the trade's")}</span>
        </h3>
        {open.length === 0 ? <p className="text-sm text-ink-soft">{t(kit.drafts.noneLeft, "None left to offer here: its page answers with Google's form.")}</p> : <Drafts drafts={open} src={src} />}
      </section>

      {went.length > 0 && (
        <section className={SECTION}>
          <h3 className={SECTION_HEAD}>
            {t(kit.wentHere, "Went here")} <span className="font-normal tabular-nums">{went.length}</span>
            <span className="flex-1" />
            <span className={HINT}>{t(kit.wentHere.why, "copied on this GBP: never offered here again")}</span>
          </h3>
          <Drafts drafts={went} src={src} />
        </section>
      )}

      <section className={SECTION}>
        <h3 className={SECTION_HEAD}>{t(kit.funnel, "Last 30 days")}</h3>
        <dl className="grid grid-cols-5 gap-3">
          {funnel.map(([n, label]) => (
            <div key={label} className="flex flex-col-reverse">
              <dt className="text-xs text-ink-soft">{label}</dt>
              <dd className="font-mono text-xl text-ink tabular-nums">{n}</dd>
            </div>
          ))}
        </dl>
      </section>
    </div>
  );
}

function Drafts({ drafts, src }: { drafts: readonly KitRanked[]; src: (photo: number) => string | undefined }) {
  const t = useT();
  return (
    <ul className="flex flex-col divide-y divide-border border-y border-border">
      {drafts.map((d) => (
        <li key={d.id} className={cn("flex items-center gap-3 py-2.5", d.went_here && "opacity-50")}>
          <p className="line-clamp-2 min-w-0 flex-1 text-sm text-ink">{d.text}</p>
          <span className="flex shrink-0 gap-1">
            {d.photos.map((p) => (
              <span key={p.id} className={cn(p.went_here && "opacity-40")} title={p.went_here ? t(kit.photos.wentHere, "saved here already") : t(kit.uses, "{n} uses", { n: p.uses })}>
                {/* eslint-disable-next-line @next/next/no-img-element -- the API's own JPEG, behind the session */}
                <img src={src(p.id)} alt="" loading="lazy" className="size-9 rounded-md object-cover" />
              </span>
            ))}
          </span>
          <span className="flex w-28 shrink-0 flex-col items-end gap-0.5">
            <span className="font-mono text-sm text-ink tabular-nums">{t(kit.uses, "{n} uses", { n: d.uses })}</span>
            {d.offered && <Badge className="text-positive">{t(kit.offered, "offered now")}</Badge>}
            {d.held && <Badge className="text-accent-warn">{t(kit.held, "held by an open page")}</Badge>}
          </span>
        </li>
      ))}
    </ul>
  );
}
