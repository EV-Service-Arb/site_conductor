"use client";

import { Badge, Button, Skeleton, cn, toast } from "@evinvest/uikit";
import { ChevronRight, Info, Link2, LocateOff, Pencil, QrCode } from "lucide-react";

import { type KitAt, TARGET_NAME, floatKit, kit as kitKeys } from "@/entities/gmb";
import type { Kit, KitEvent, KitHappening, KitMember } from "@/shared/api/schema";
import { type T, useLocale, useT } from "@/shared/i18n";
import { formatDistance } from "@/shared/lib/format";
import { go as goTo } from "@/shared/lib/location";
import { metres, usePosition } from "@/shared/lib/use-position";
import { ErrorState } from "@/shared/ui/error-state";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize } from "@/shared/ui/touch";

import { BLOCK_NAME, HINT, PeriodPicker, SECTION_HEAD, Tally } from "./parts";
import { useKit } from "./use-kit";

const HAPPENED = {
  shown: (t) => t(kitKeys.latest.shown, "QR shown"),
  scanned: (t) => t(kitKeys.latest.scanned, "scanned"),
  opened: (t) => t(kitKeys.latest.opened, "opened its page"),
  copied: (t) => t(kitKeys.latest.copied, "copied a draft"),
  saved: (t) => t(kitKeys.latest.saved, "saved a photo"),
  posted: (t) => t(kitKeys.latest.posted, "on Google, from a draft"),
  reviewed: (t) => t(kitKeys.latest.reviewed, "on Google, not from a draft"),
  gone: (t) => t(kitKeys.latest.gone, "a review from a draft gone"),
} satisfies Record<KitHappening, (t: T) => string>;

export const kitLink = (slug: string) => `${window.location.origin}/kit/${slug}`;

/** A kit (13k, 13l): what it collected over the period, its GBPs, its pools and the latest; on a phone (13n) its GBPs nearest first, a tap their QR. */
export function KitOverview({ slug, go, phone }: { slug: string; go: (at: KitAt) => void; phone: boolean }) {
  const res = useKit(slug);
  if (res.status === "loading") return <Skeleton className="m-4 h-64" />;
  if (res.status === "error") return <ErrorState failure={res.failure} onRetry={res.reload} />;
  return phone ? <Phone kit={res.data} go={go} /> : <Desk kit={res.data} go={go} />;
}

function Header({ kit }: { kit: Kit }) {
  const t = useT();
  const button = useButtonSize();
  const cats = [...new Set(kit.pools.map((p) => `${p.name} · ${p.country}`))].join(", ");
  return (
    <header className="flex flex-wrap items-start gap-3">
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <h2 className="flex items-center gap-2 text-xl font-semibold text-ink">
          {kit.name}
          <Button variant="ghost" size={button("sm")} icon aria-label={t(kitKeys.edit, "Edit the kit")} title={t(kitKeys.edit, "Edit the kit")} onClick={() => floatKit({ kit: kit.slug, edit: true })}>
            <Pencil aria-hidden />
          </Button>
        </h2>
        <p className="truncate text-sm text-ink-soft">{[t(kitKeys.gbps, "{n} GBPs", { n: kit.gmbs.length }), cats, kit.area !== null && t(kitKeys.area, "area {area}", { area: kit.area })].filter(Boolean).join(" · ")}</p>
      </div>
      <PeriodPicker />
    </header>
  );
}

function Desk({ kit, go }: { kit: Kit; go: (at: KitAt) => void }) {
  const t = useT();
  const button = useButtonSize();
  const name = new Map(kit.gmbs.map((g) => [g.id, g.name]));
  return (
    <div className="@container flex flex-col gap-5 p-5">
      <Header kit={kit} />
      <Tally tally={kit.tally} className="rounded-lg border border-border p-4" />
      <section className="flex flex-col gap-1">
        <h3 className={SECTION_HEAD}>
          {t(kitKeys.gbps.title, "GBPs")} <span className="font-normal tabular-nums">{kit.gmbs.length}</span>
          <span className="flex-1" />
          <span className={HINT}>{t(kitKeys.gbps.hint, "a row opens it · q in it shows its QR")}</span>
        </h3>
        <div className="overflow-x-auto">
        <table className="w-full min-w-xl text-sm">
          <thead className="text-xs text-ink-soft">
            <tr className="[&>th]:px-2 [&>th]:py-1 [&>th]:text-right [&>th]:font-normal">
              <th className="!text-left">{t(kitKeys.gbps.name, "GBP")}</th>
              <th className="!text-left">{t(kitKeys.gbps.qrOpens, "QR opens")}</th>
              <th>{t(kitKeys.tally.shownShort, "shown")}</th>
              <th>{t(kitKeys.tally.scansShort, "scans")}</th>
              <th>{t(kitKeys.tally.copiedShort, "copied")}</th>
              <th>{t(kitKeys.tally.onGoogle, "on Google")}</th>
              <th className="!text-left">{t(kitKeys.left, "left")}</th>
              <th />
            </tr>
          </thead>
          <tbody className="divide-y divide-border border-y border-border">
            {kit.gmbs.map((g) => (
              <tr key={g.id} onClick={() => go({ kit: kit.slug, gmb: g.id })} className={cn("cursor-pointer hover:bg-hover [&>td]:px-2 [&>td]:py-2", g.blocked !== null && g.blocked !== "no_page" && "opacity-50")}>
                <td>
                  <span className="flex flex-col">
                    <span className="text-ink">{g.name}</span>
                    <span className="text-xs text-ink-soft">{g.address}</span>
                  </span>
                </td>
                <td>{g.target === null ? <span className="text-xs text-ink-soft">{g.blocked && BLOCK_NAME[g.blocked](t)}</span> : <Badge>{TARGET_NAME[g.target](t)}</Badge>}</td>
                <td className="text-right font-mono tabular-nums">{g.tally.shown}</td>
                <td className="text-right font-mono tabular-nums">{g.tally.scanned}</td>
                <td className="text-right font-mono tabular-nums">{g.tally.copied}</td>
                <td className="text-right font-mono text-positive tabular-nums">{g.tally.on_google}</td>
                <td className="text-xs">
                  <Left g={g} />
                </td>
                <td>
                  <ChevronRight aria-hidden className="size-4 text-ink-soft" />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        </div>
      </section>
      {kit.pools.length > 0 && (
        <section className="flex flex-col gap-2">
          <h3 className={SECTION_HEAD}>
            {t(kitKeys.pools, "Pools")}
            <span className={HINT}>{t(kitKeys.pools.hint, "what the GBPs draw from · each draft and photo once per GBP")}</span>
          </h3>
          <div className="grid grid-cols-1 gap-3 @2xl:grid-cols-2">
            {kit.pools.map((p) => (
              <button key={`${p.trade}.${p.country}`} type="button" onClick={() => go({ kit: kit.slug, pool: `${p.trade}.${p.country}` })} className="flex flex-col gap-1 rounded-lg border border-border p-3 text-left outline-none hover:bg-hover focus-visible:ring-2 focus-visible:ring-ring">
                <span className="flex items-baseline gap-2">
                  <span className="flex-1 text-sm font-medium text-ink">
                    {p.name} · {p.country}
                  </span>
                  <span className="text-xs text-ink-soft">{t(kitKeys.pools.open, "Open pool →")}</span>
                </span>
                <span className="text-xs text-ink-soft">{t(kitKeys.pools.counts, "{d} drafts · {p} photos", { d: p.drafts, p: p.photos })}</span>
                {p.warnings.map((w) => (
                  <span key={w} className="text-xs text-accent-warn">
                    {w}
                  </span>
                ))}
              </button>
            ))}
          </div>
        </section>
      )}
      {kit.latest.length > 0 && <Latest latest={kit.latest} name={name} />}
      <footer className="flex flex-wrap gap-2">
        <Button variant="outline" size={button("sm")} onClick={() => floatKit({ kit: kit.slug, edit: true })}>
          <Pencil aria-hidden />
          {t(kitKeys.editGbps, "Add or drop GBPs")}
        </Button>
        <span className="flex-1" />
        <CopyLink slug={kit.slug} />
      </footer>
    </div>
  );
}

function Left({ g }: { g: KitMember }) {
  const t = useT();
  if (g.blocked !== null && g.blocked !== "no_page") return <span className="text-ink-soft">{BLOCK_NAME[g.blocked](t)}</span>;
  if (g.target === "google") return <span className="text-ink-soft">{t(kitKeys.left.google, "no drafts needed")}</span>;
  return (
    <span className={cn(g.left.drafts <= 1 || g.left.photos === 0 ? "text-accent-warn" : "text-ink-soft")}>
      {t(kitKeys.left.counts, "{d} drafts · {p} photos", { d: g.left.drafts, p: g.left.photos })}
    </span>
  );
}

function Latest({ latest, name }: { latest: readonly KitEvent[]; name: Map<number, string> }) {
  const t = useT();
  const locale = useLocale();
  return (
    <section className="flex flex-col gap-1">
      <h3 className={SECTION_HEAD}>
        {t(kitKeys.latest, "Latest")}
        <span className={HINT}>{t(kitKeys.latest.hint, "every scan, copy and review, newest first")}</span>
      </h3>
      <ul className="flex flex-col gap-0.5 text-sm">
        {latest.map((e, i) => (
          <li key={i} className={cn("flex gap-3", (e.kind === "posted" || e.kind === "reviewed") && "text-positive", e.kind === "gone" && "text-accent-error")}>
            <span className="w-12 shrink-0 font-mono text-xs text-ink-soft tabular-nums">{new Date(e.at).toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit" })}</span>
            <span className="text-ink">{name.get(e.gmb)}</span>
            <span>
              {HAPPENED[e.kind](t)}
              {(e.kind === "posted" || e.kind === "reviewed" || e.kind === "gone") && e.detail !== null && ` ★${e.detail}`}
            </span>
          </li>
        ))}
      </ul>
    </section>
  );
}

function CopyLink({ slug }: { slug: string }) {
  const t = useT();
  const button = useButtonSize();
  return (
    <Button
      variant="outline"
      size={button("sm")}
      onClick={() =>
        void navigator.clipboard.writeText(kitLink(slug)).then(
          () => toast.positive(t(kitKeys.copied, "Link copied")),
          (e: unknown) => notifyFailure(e, t),
        )
      }
    >
      <Link2 aria-hidden />
      {t(kitKeys.copy, "Copy link")}
    </Button>
  );
}

/** The operator in the street: nearest first while the phone says where it is, a tap the QR. */
function Phone({ kit, go }: { kit: Kit; go: (at: KitAt) => void }) {
  const t = useT();
  const locale = useLocale();
  const { at, denied } = usePosition();
  const rows = kit.gmbs.map((g) => ({ g, m: at !== null && g.location !== null ? metres(at, g.location) : null }));
  if (at !== null) rows.sort((a, b) => (a.m ?? Infinity) - (b.m ?? Infinity));
  return (
    <div className="flex flex-col gap-3 p-3">
      <header className="flex items-baseline gap-2 px-1">
        <h1 className="flex-1 text-xl font-semibold text-ink">{kit.name}</h1>
        {denied && (
          <span className="flex items-center gap-1 text-xs text-ink-soft">
            <LocateOff aria-hidden className="size-3.5" />
            {t(kitKeys.phone.locationOff, "location off")}
          </span>
        )}
      </header>
      <Tally tally={kit.tally} />
      <h2 className={SECTION_HEAD}>{at !== null ? t(kitKeys.phone.nearest, "Nearest first") : t(kitKeys.phone.order, "In the kit's order")}</h2>
      <ul className="flex flex-col divide-y divide-border overflow-hidden rounded-xl border border-border">
        {rows.map(({ g, m }) => (
          <li key={g.id} className={cn("flex items-center", g.url === null && "opacity-50")}>
            <button
              type="button"
              disabled={g.url === null}
              onClick={() => goTo({ view: "kit_qr", params: { slug: kit.slug, gmb: g.id } })}
              className="flex min-h-16 min-w-0 flex-1 items-center gap-3 px-4 py-3 text-left outline-none active:bg-hover focus-visible:ring-2 focus-visible:ring-ring"
            >
              <QrCode aria-hidden className="size-5 shrink-0 text-ink-soft" />
              <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                <span className="truncate text-base font-medium text-ink">{g.name}</span>
                <span className="truncate text-sm text-ink-soft">{g.blocked !== null ? BLOCK_NAME[g.blocked](t) : g.target !== null && TARGET_NAME[g.target](t)}</span>
              </span>
              {m !== null && <span className="font-mono text-sm text-ink tabular-nums">{formatDistance(m, locale)}</span>}
            </button>
            <Button variant="ghost" size="touch" icon aria-label={t(kitKeys.phone.info, "Its drafts and tally")} onClick={() => go({ kit: kit.slug, gmb: g.id })}>
              <Info aria-hidden />
            </Button>
          </li>
        ))}
      </ul>
    </div>
  );
}
