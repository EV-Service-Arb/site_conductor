"use client";

import { useResource } from "@evinvest/api/react";
import { Button, ConfirmDialog, Skeleton, cn, toast } from "@evinvest/uikit";
import { Check, CircleSlash, Copy, Printer, Smartphone, X } from "lucide-react";
import { type ReactNode, useState } from "react";
import { createPortal } from "react-dom";

import { AreaDraw, type Drawn, categoryName, googleName, useTrade, closeWindow, editKit, kit as kitKeys, move, openWindow, pageOf, saveArea, useGmails, useGmbTags, windowKeys } from "@/entities/gmb";
import { Refused, api, ok } from "@/shared/api";
import type { Area, Gmb, Kit } from "@/shared/api/schema";
import { type T, useLocale, useT } from "@/shared/i18n";
import { formatDistance } from "@/shared/lib/format";
import { linkTo } from "@/shared/lib/location";
import { distanceM } from "@/shared/map";
import { ErrorState } from "@/shared/ui/error-state";
import { failureText } from "@/shared/ui/failure-text";
import { Qr } from "@/shared/ui/qr";
import { useButtonSize } from "@/shared/ui/touch";

import { sinceOf } from "../model/kit";
import { useKit } from "./use-kit";

// the panel's width beside it from md up (left-4 + w-100)
const beside = () => (window.matchMedia("(min-width: 48rem)").matches ? 16 + 400 : 0);
// a GBP's kit is read here for what its QR does, not for its tally
const SINCE = sinceOf("today");

/** Verified and on Maps: a QR there can be reviewed. */
const reviewable = (g: Gmb) => g.state === "verified" && g.place_id !== null;

/** Why a GBP can't be reviewed. */
function whyNot(g: Gmb, t: T): string {
  switch (g.state) {
    case "draft":
      return t(kitKeys.edit.draft, "Draft · not on Google yet");
    case "verifying":
      return t(kitKeys.edit.verifying, "Verifying · not on Maps yet, nothing to review");
    case "suspended":
      return t(kitKeys.edit.suspended, "Suspended · nothing to review while suspended");
    case "verified":
      return t(kitKeys.edit.offMaps, "Not on Maps yet, nothing to review");
  }
}

const keyOf = (ids: Iterable<number>) => [...ids].sort((a, b) => a - b).join(",");

/** A review kit made or changed at the desk (Figma 13a): an area on the map, the owner's GBPs it holds, then its QR to take it out. */
export function KitEdit({ map, slug }: { map: google.maps.Map; slug: string }) {
  const t = useT();
  if (slug === "")
    return (
      <Panel title={t(kitKeys.edit.new, "New review kit")} onClose={() => editKit(null)}>
        <Owned map={map} kit={null} area={null} />
      </Panel>
    );
  return <Existing key={slug} map={map} slug={slug} />;
}

function Panel({ title, onClose, children }: { title: string; onClose: () => void; children: ReactNode }) {
  const t = useT();
  const button = useButtonSize();
  return (
    // the map stays clickable beside it and below it
    <div className="pointer-events-none absolute inset-4 flex items-start">
      <section aria-label={title} className="pointer-events-auto flex max-h-full w-full flex-col overflow-y-auto rounded-lg border border-border bg-background shadow-overlay md:w-100">
        <header className="flex items-center gap-2 px-4 pt-3">
          <h2 className="flex-1 truncate font-mono text-xs tracking-wider text-ink-soft uppercase">{title}</h2>
          <Button variant="ghost" size={button("xs")} icon aria-label={t(windowKeys.close, "Close the window")} onClick={onClose}>
            <X aria-hidden />
          </Button>
        </header>
        {children}
      </section>
    </div>
  );
}

function Failed({ read }: { read: { status: "error"; failure: Parameters<typeof ErrorState>[0]["failure"]; reload: () => void } }) {
  return (
    <div className="p-4">
      <ErrorState failure={read.failure} onRetry={read.reload} />
    </div>
  );
}

function Existing({ map, slug }: { map: google.maps.Map; slug: string }) {
  const t = useT();
  const kit = useResource(api, "/kits/{slug}", { params: { path: { slug }, query: { since: SINCE } } });
  const title = kit.status === "ok" ? t(kitKeys.edit.title, "Review kit · {name}", { name: kit.data.name }) : slug;
  return (
    <Panel title={title} onClose={() => openWindow({ kind: "kit", at: { kit: slug } })}>
      {kit.status === "loading" ? <Skeleton className="m-4 h-9" /> : kit.status === "error" ? <Failed read={kit} /> : kit.data.area === null ? <Owned map={map} kit={kit.data} area={null} /> : <OfArea map={map} kit={kit.data} slug={kit.data.area} />}
    </Panel>
  );
}

function OfArea({ map, kit, slug }: { map: google.maps.Map; kit: Kit; slug: string }) {
  const area = useResource(api, "/areas/{slug}", { params: { path: { slug } } });
  if (area.status === "loading") return <Skeleton className="m-4 h-9" />;
  if (area.status === "error") return <Failed read={area} />;
  return <Owned map={map} kit={kit} area={area.data} />;
}

/** The owner's GBPs read first: a kit's ticks are seeded against its area from them. */
function Owned({ map, kit, area }: { map: google.maps.Map; kit: Kit | null; area: Area | null }) {
  const gmails = useGmails();
  if (gmails.status === "loading") return <Skeleton className="m-4 h-9" />;
  if (gmails.status === "error") return <Failed read={gmails} />;
  return <Editor map={map} own={gmails.data.gmbs} kit={kit} area={area} />;
}

function Editor({ map, own, kit, area }: { map: google.maps.Map; own: readonly Gmb[]; kit: Kit | null; area: Area | null }) {
  // members = (reviewable ∩ inside − excluded) ∪ included: a centre dragged re-derives them, the ticks by hand stay
  const [seed] = useState(() => {
    const members = new Set(kit?.gmbs.map((g) => g.id) ?? []);
    const inside = own.filter((g) => area !== null && reviewable(g) && g.location !== null && distanceM(area.center, g.location) <= area.radius_m).map((g) => g.id);
    return { excluded: new Set(inside.filter((id) => !members.has(id))), included: new Set([...members].filter((id) => !inside.includes(id))) };
  });
  const [excluded, setExcluded] = useState<ReadonlySet<number>>(seed.excluded);
  const [included, setIncluded] = useState<ReadonlySet<number>>(seed.included);
  const [saved, setSaved] = useState(kit === null ? null : { area: kit.area, gmbs: keyOf(kit.gmbs.map((g) => g.id)) });
  return (
    <AreaDraw map={map} left={beside()} initial={area}>
      {(drawn, reread) => {
        const near = (g: Gmb) => (drawn === null || g.location === null ? null : distanceM(drawn.center, g.location));
        const isInside = (g: Gmb) => {
          const d = near(g);
          return drawn !== null && d !== null && d <= drawn.radius_m;
        };
        const members = new Set([...own.filter((g) => reviewable(g) && isInside(g) && !excluded.has(g.id)).map((g) => g.id), ...included]);
        const toggle = (g: Gmb) => {
          const auto = reviewable(g) && isInside(g);
          if (members.has(g.id)) {
            setIncluded((s) => new Set([...s].filter((id) => id !== g.id)));
            if (auto) setExcluded((s) => new Set([...s, g.id]));
          } else {
            setExcluded((s) => new Set([...s].filter((id) => id !== g.id)));
            if (!auto) setIncluded((s) => new Set([...s, g.id]));
          }
        };
        return (
          <>
            <Rows drawn={drawn} own={own} near={near} members={members} toggle={toggle} selectAll={(ids) => setExcluded((s) => new Set([...s].filter((id) => !ids.includes(id))))} />
            <Footer drawn={drawn} kit={kit} members={members} saved={saved} onSaved={setSaved} reread={reread} />
          </>
        );
      }}
    </AreaDraw>
  );
}

function Rows({ drawn, own, near, members, toggle, selectAll }: { drawn: Drawn | null; own: readonly Gmb[]; near: (g: Gmb) => number | null; members: ReadonlySet<number>; toggle: (g: Gmb) => void; selectAll: (ids: number[]) => void }) {
  const t = useT();
  const placed = own.flatMap((g) => {
    const d = near(g);
    return d === null ? [] : [{ g, d }];
  });
  placed.sort((a, b) => a.d - b.d);
  const r = drawn?.radius_m ?? 0;
  const inside = placed.filter((p) => p.d <= r);
  const outside = placed.filter((p) => p.d > r && p.d <= 2 * r && (reviewable(p.g) || members.has(p.g.id)));
  const listed = new Set([...inside, ...outside].map((p) => p.g.id));
  const elsewhere = own.filter((g) => members.has(g.id) && !listed.has(g.id));
  const unticked = inside.filter((p) => reviewable(p.g) && !members.has(p.g.id)).map((p) => p.g.id);
  if (drawn === null && elsewhere.length === 0) return null;
  return (
    <div className="flex flex-col gap-1 border-t border-border px-4 py-3">
      {drawn !== null && (
        <Group title={drawn.city !== null ? t(kitKeys.edit.inCity, "In {city}", { city: drawn.city.name }) : t(kitKeys.edit.inCircle, "In the circle")} n={inside.length} aside={unticked.length > 0 && <button type="button" onClick={() => selectAll(unticked)} className="text-xs text-ink-soft hover:text-ink">{t(kitKeys.edit.selectAll, "select all")}</button>}>
          {inside.length === 0 && <p className="py-2 text-sm text-ink-soft">{t(kitKeys.edit.noneInside, "None of your GBPs is in it.")}</p>}
          {inside.map((p) => <Row key={p.g.id} gmb={p.g} d={p.d} on={members.has(p.g.id)} toggle={toggle} />)}
        </Group>
      )}
      {outside.length > 0 && (
        <Group title={t(kitKeys.edit.justOutside, "Just outside")} n={outside.length} aside={<span className="text-xs text-ink-soft">{t(kitKeys.edit.addByTicking, "add by ticking")}</span>}>
          {outside.map((p) => <Row key={p.g.id} gmb={p.g} d={p.d} on={members.has(p.g.id)} toggle={toggle} />)}
        </Group>
      )}
      {elsewhere.length > 0 && (
        <Group title={t(kitKeys.edit.elsewhere, "Elsewhere · in the kit")} n={elsewhere.length}>
          {elsewhere.map((g) => <Row key={g.id} gmb={g} d={null} on toggle={toggle} />)}
        </Group>
      )}
    </div>
  );
}

function Group({ title, n, aside, children }: { title: string; n: number; aside?: ReactNode; children: ReactNode }) {
  return (
    <section className="flex flex-col">
      <h3 className="flex items-center gap-2 py-1.5 font-mono text-xs tracking-wider text-ink-soft uppercase">
        <span>{title}</span>
        <span className="text-ink-faint tabular-nums">{n}</span>
        <span className="ml-auto font-sans normal-case tracking-normal">{aside}</span>
      </h3>
      {children}
    </section>
  );
}

function Row({ gmb, d, on, toggle }: { gmb: Gmb; d: number | null; on: boolean; toggle: (g: Gmb) => void }) {
  const t = useT();
  const locale = useLocale();
  const can = on || reviewable(gmb);
  return (
    <button type="button" aria-pressed={on} disabled={!can} onClick={() => toggle(gmb)} className={cn("flex items-center gap-3 rounded-md py-1.5 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring", can ? "hover:bg-hover" : "cursor-not-allowed opacity-60")}>
      {can ? <span className={cn("flex size-4 shrink-0 items-center justify-center rounded border", on ? "border-ink bg-ink text-background" : "border-border")}>{on && <Check aria-hidden className="size-3" />}</span> : <CircleSlash aria-hidden className="size-4 shrink-0 text-ink-soft" />}
      <span className="flex min-w-0 flex-1 flex-col">
        <span className="truncate text-sm text-ink">{gmb.name}</span>
        <span className="truncate text-xs text-ink-soft">{reviewable(gmb) ? <What gmb={gmb} /> : whyNot(gmb, t)}</span>
      </span>
      {d !== null && <span className="shrink-0 font-mono text-xs text-ink-soft tabular-nums">{formatDistance(d, locale)}</span>}
    </button>
  );
}

/** Its category, its stars, and what its QR does. */
function What({ gmb }: { gmb: Gmb }) {
  const t = useT();
  const locale = useLocale();
  const { tags } = useGmbTags();
  const kit = useResource(api, "/gmbs/{id}/kit", { params: { path: { id: gmb.id }, query: { since: SINCE } } });
  const trade = useTrade(gmb.trade, gmb.country?.code ?? null);
  const category = trade === null ? null : gmb.country === null ? googleName(trade, locale) : categoryName(trade, gmb.country.code, locale);
  const archive = tags.archive?.get(gmb.id);
  const stars = archive === undefined || archive.rating === null ? null : `★ ${archive.rating.toFixed(1)}${archive.reviews === null ? "" : ` (${archive.reviews})`}`;
  const qr =
    kit.status === "loading" ? null : kit.status === "error" ? (
      <span className="text-accent-error">{failureText(kit.failure, t)}</span>
    ) : kit.data.url === null ? (
      <span className="text-accent-warn">{t(kitKeys.edit.off, "kit off for it: no QR")}</span>
    ) : kit.data.page === null ? (
      <span className="text-accent-warn">{t(kitKeys.edit.noPage, "no landing page: QR opens Google's form")}</span>
    ) : kit.data.target === "google" ? (
      t(kitKeys.edit.toGoogle, "QR opens Google's form")
    ) : kit.data.drafts.length === 0 ? (
      <span className="text-accent-warn">{t(kitKeys.edit.noDrafts, "no drafts: QR opens Google's form")}</span>
    ) : (
      t(kitKeys.edit.drafts, "{n} drafts", { n: kit.data.drafts.length })
    );
  const parts = [category, stars].filter((p) => p !== null);
  return (
    <>
      {parts.join(" · ")}
      {parts.length > 0 && qr !== null && " · "}
      {qr}
    </>
  );
}

function Footer({ drawn, kit, members, saved, onSaved, reread }: { drawn: Drawn | null; kit: Kit | null; members: ReadonlySet<number>; saved: { area: string | null; gmbs: string } | null; onSaved: (s: { area: string | null; gmbs: string }) => void; reread: () => void }) {
  const t = useT();
  const button = useButtonSize();
  const kits = useResource(api, "/kits");
  const [busy, setBusy] = useState(false);
  const [refused, setRefused] = useState<string | null>(null);
  const area = drawn === null ? null : drawn.area;
  const slug = kit?.slug ?? area?.slug ?? null;
  const name = area?.name ?? kit?.name ?? null;
  const now = { area: area?.slug ?? null, gmbs: keyOf(members) };
  const clean = saved !== null && saved.area === now.area && saved.gmbs === now.gmbs;
  const taken = kit === null && slug !== null && kits.status === "ok" && kits.data.kits.some((k) => k.slug === slug);
  if (clean && kit !== null) return <Saved slug={kit.slug} n={members.size} />;
  const save = async () => {
    if (slug === null || name === null) throw new Error("a kit saved with no link");
    setBusy(true);
    setRefused(null);
    try {
      const at = drawn !== null ? await saveArea(drawn) : null;
      reread();
      const gmbs = [...members];
      await ok(api.PUT("/kits/{slug}", { params: { path: { slug } }, body: { name, area: at, gmbs } }));
      if (kit === null) editKit(slug);
      else onSaved({ area: at, gmbs: keyOf(gmbs) });
    } catch (e) {
      if (!(e instanceof Refused)) throw e;
      if (e.failure.code !== "unauthenticated") setRefused(failureText(e.failure, t));
    } finally {
      setBusy(false);
    }
  };
  return (
    <footer className="sticky bottom-0 flex flex-col gap-2 border-t border-border bg-background px-4 py-3">
      <div className="flex items-center gap-3">
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="text-sm font-semibold text-ink">{members.size === 1 ? t(kitKeys.edit.place, "Review kit · 1 place") : t(kitKeys.edit.places, "Review kit · {n} places", { n: members.size })}</span>
          <span className="truncate font-mono text-xs text-ink-soft">{slug === null ? t(kitKeys.edit.unnamed, "named once its area is") : `/kit/${slug}`}</span>
        </span>
        {kit !== null && <Delete slug={kit.slug} name={kit.name} />}
        <Button size={button("sm")} disabled={busy || taken || slug === null || (drawn !== null && area === null) || members.size === 0} onClick={() => void save()}>
          {kit === null ? t(kitKeys.edit.create, "Save") : t(kitKeys.edit.save, "Save changes")}
        </Button>
      </div>
      {taken && (
        <p className="text-xs text-accent-warn">
          {t(kitKeys.edit.taken, "You have a kit at /kit/{slug} already: ", { slug })}
          <button type="button" className="underline" onClick={() => editKit(slug)}>
            {t(kitKeys.edit.openTaken, "open it")}
          </button>
        </p>
      )}
      {refused !== null && <p className="text-xs text-accent-error">{refused}</p>}
    </footer>
  );
}

/** A saved kit's hand-off: its page's QR for the operator's own phone, the page, its link, its cards. */
function Saved({ slug, n }: { slug: string; n: number }) {
  const t = useT();
  const button = useButtonSize();
  // the page alone: what this tab carries (its panels) is nothing to the phone
  const url = `${window.location.origin}${new URL(linkTo(pageOf({ kind: "kit", at: { kit: slug } })), window.location.origin).pathname}`;
  return (
    <footer className="sticky bottom-0 flex flex-col gap-3 border-t border-border bg-background px-4 py-3">
      <div className="flex items-center gap-3">
        <Qr text={url} className="size-20 shrink-0 rounded-md" />
        <span className="flex min-w-0 flex-col gap-0.5">
          <span className="text-sm font-semibold text-ink">{n === 1 ? t(kitKeys.edit.place, "Review kit · 1 place") : t(kitKeys.edit.places, "Review kit · {n} places", { n })}</span>
          <span className="truncate font-mono text-xs text-ink-soft">{url.replace(/^https?:\/\//, "")}</span>
          <span className="text-xs text-ink-soft">{t(kitKeys.edit.scan, "scan with your phone to take it out; signed-in only")}</span>
        </span>
      </div>
      <div className="flex gap-2">
        <Button variant="secondary" size={button("sm")} className="flex-1" onClick={() => openWindow({ kind: "kit", at: { kit: slug } })}>
          <Smartphone aria-hidden />
          {t(kitKeys.edit.open, "Open")}
        </Button>
        <Button variant="outline" size={button("sm")} className="flex-1" onClick={() => void navigator.clipboard.writeText(url).then(() => toast(t(kitKeys.edit.copied, "Link copied")))}>
          <Copy aria-hidden />
          {t(kitKeys.edit.copy, "Copy link")}
        </Button>
        <PrintCards slug={slug} />
      </div>
    </footer>
  );
}

/** A card per member with a QR, as the printer gets it: its QR, its name and address for those who search by hand. */
function PrintCards({ slug }: { slug: string }) {
  const t = useT();
  const button = useButtonSize();
  const kit = useKit(slug);
  const members = kit.status === "ok" ? kit.data.gmbs : [];
  const cards = members.filter((g) => g.url !== null);
  const left = members.length - cards.length;
  return (
    <>
      <Button variant="outline" size={button("sm")} className="flex-1" disabled={cards.length === 0} title={left > 0 ? t(kitKeys.edit.noQr, "{n} without a QR left out", { n: left }) : undefined} onClick={() => window.print()}>
        <Printer aria-hidden />
        {t(kitKeys.edit.print, "Print cards")}
        {left > 0 && <span className="text-accent-warn tabular-nums">−{left}</span>}
      </Button>
      {createPortal(
        <div className="print-sheet hidden grid-cols-2 gap-6 p-6 text-black print:grid">
          {cards.map((g) => (
            <article key={g.id} className="flex break-inside-avoid flex-col items-center gap-2 rounded-lg border border-black/30 p-4 text-center">
              {/* filtered to members with a url above */}
              <Qr text={g.url ?? ""} className="size-48" />
              <span className="text-base font-semibold">{g.name}</span>
              {g.address !== null && <span className="text-xs">{g.address}</span>}
            </article>
          ))}
        </div>,
        document.body,
      )}
    </>
  );
}

/** Only the selection goes: no draft, photo or use. */
function Delete({ slug, name }: { slug: string; name: string }) {
  const t = useT();
  const button = useButtonSize();
  return (
    <ConfirmDialog
      trigger={
        <Button variant="ghost" size={button("sm")}>
          {t(kitKeys.edit.delete, "Delete")}
        </Button>
      }
      tone="error"
      title={t(kitKeys.edit.deleteTitle, "Delete {name}?", { name })}
      description={t(kitKeys.edit.deleteBody, "Only the selection goes: drafts, photos and what was collected stay.")}
      confirmLabel={t(kitKeys.edit.delete, "Delete")}
      labels={{ cancel: t(move.cancel, "Cancel") }}
      onConfirm={async () => {
        await ok(api.DELETE("/kits/{slug}", { params: { path: { slug } } }));
        closeWindow({ kind: "kit", at: { kit: slug } });
      }}
    />
  );
}
