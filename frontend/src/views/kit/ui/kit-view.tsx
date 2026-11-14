"use client";

import { Button, Empty, EmptyDescription, EmptyHeader, EmptyTitle, Input, Skeleton, cn } from "@evinvest/uikit";
import { ChevronLeft, ChevronRight, QrCode, Search } from "lucide-react";
import { useEffect, useRef, useState, useSyncExternalStore } from "react";

import { type KitNearby, KitTargetSegment, TARGET_NAME, fetchAreaGmbs, fetchGmbKit, kit, kitShown } from "@/entities/gmb";
import { useLocale, useT } from "@/shared/i18n";
import { formatDistance } from "@/shared/lib/format";
import { useResource } from "@/shared/lib/use-resource";
import { ErrorState } from "@/shared/ui/error-state";
import { notifyFailure } from "@/shared/ui/notify";
import { Qr } from "@/shared/ui/qr";

const SWIPE_PX = 60;

const subscribeHash = (onChange: () => void) => {
  window.addEventListener("hashchange", onChange);
  return () => window.removeEventListener("hashchange", onChange);
};

/** `/<slug>/` is the page the server answers for an area's slug; `/kit/` itself names none. */
const slugOfPath = () => {
  const [first, ...rest] = window.location.pathname.split("/").filter(Boolean);
  return first === undefined || first === "kit" || rest.length > 0 ? null : first;
};

/** The GBP shown full screen, `#<id>`: the phone's back returns to the list. */
const shownOfHash = () => {
  const raw = window.location.hash.slice(1);
  if (raw === "") return null;
  const id = Number(raw);
  if (!Number.isSafeInteger(id)) throw new Error(`#${raw}: not a GBP id`);
  return id;
};

/** An area's kit on the operator's phone (13b): its GBPs nearest first; one tapped fills the screen with its QR (13c). */
export function KitView() {
  // `undefined` while prerendered: the export has no path to read
  const slug = useSyncExternalStore(subscribeHash, slugOfPath, () => undefined);
  if (slug === undefined) return <Skeleton className="min-h-svh rounded-none" />;
  if (slug === null) return <NoArea />;
  return <AreaKit slug={slug} />;
}

function NoArea() {
  const t = useT();
  return (
    <Empty className="min-h-svh">
      <EmptyHeader>
        <EmptyTitle>{t(kit.page.noArea, "No area named")}</EmptyTitle>
        <EmptyDescription>{t(kit.page.noArea.why, "An area's kit is at its own link, /<area>/: draw one from the map's Review kit.")}</EmptyDescription>
      </EmptyHeader>
    </Empty>
  );
}

function AreaKit({ slug }: { slug: string }) {
  const res = useResource(`kit.area.${slug}`, () => fetchAreaGmbs(slug), `kit.area.${slug}`, { focus: true });
  const shown = useSyncExternalStore(subscribeHash, shownOfHash, () => null);
  const [find, setFind] = useState("");
  const pushed = useRef(false);
  if (res.status === "loading") return <Skeleton className="min-h-svh rounded-none" />;
  if (res.status === "error")
    return res.failure.kind === "not_found" ? (
      <NoArea />
    ) : (
      <div className="p-4">
        <ErrorState failure={res.failure} onRetry={res.reload} />
      </div>
    );
  const q = find.trim().toLowerCase();
  const listed = res.data.gmbs.filter((g) => q === "" || `${g.name} ${g.address ?? ""}`.toLowerCase().includes(q));
  const at = shown === null ? -1 : listed.findIndex((g) => g.id === shown);
  const show = (id: number) => {
    pushed.current = true;
    window.location.hash = String(id);
  };
  const back = () => {
    if (pushed.current) window.history.back();
    else window.location.replace("#");
    pushed.current = false;
  };
  return (
    <div className="flex min-h-svh flex-col bg-background">
      {at >= 0 ? (
        <Shown gmbs={listed} at={at} area={res.data.area.name} onBack={back} onReturn={res.reload} />
      ) : (
        <List area={res.data.area} gmbs={listed} total={res.data.gmbs.length} find={find} onFind={setFind} onShow={show} />
      )}
    </div>
  );
}

function List({ area, gmbs, total, find, onFind, onShow }: { area: { name: string; margin_m: number }; gmbs: readonly KitNearby[]; total: number; find: string; onFind: (s: string) => void; onShow: (id: number) => void }) {
  const t = useT();
  const locale = useLocale();
  const today = new Date().toDateString();
  return (
    <>
      <header className="flex items-baseline gap-2 border-b border-border px-4 py-3">
        <h1 className="text-xl font-semibold text-ink">{area.name}</h1>
        {area.margin_m > 0 && <span className="text-sm text-ink-soft">+ {formatDistance(area.margin_m, locale)}</span>}
      </header>
      <main className="flex flex-1 flex-col gap-3 p-3">
        <label className="relative">
          <Search aria-hidden className="absolute top-1/2 left-3 size-4 -translate-y-1/2 text-ink-soft" />
          <Input size="lg" value={find} onChange={(e) => onFind(e.target.value)} placeholder={t(kit.page.find, "A place, or what it does")} aria-label={t(kit.page.find, "A place, or what it does")} className="pl-9" />
        </label>
        <h2 className="flex items-baseline gap-2 px-1 text-xs font-semibold tracking-wide text-ink-soft uppercase">
          {t(kit.page.nearest, "Nearest first")} <span className="font-normal tabular-nums">{gmbs.length}</span>
        </h2>
        {total === 0 ? (
          <p className="px-1 text-sm text-ink-soft">{t(kit.area.noGbps, "None of your GBPs with a point is in it or next to it.")}</p>
        ) : (
          <ul className="flex flex-col divide-y divide-border overflow-hidden rounded-xl border border-border">
            {gmbs.map((g) => (
              <li key={g.id}>
                <button type="button" onClick={() => onShow(g.id)} className="flex min-h-16 w-full items-center gap-3 px-4 py-3 text-left outline-none active:bg-hover focus-visible:ring-2 focus-visible:ring-ring">
                  <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                    <span className="truncate text-base font-medium text-ink">{g.name}</span>
                    <span className="truncate text-sm text-ink-soft">
                      <span className={cn(g.target === null && "text-accent-warn")}>{g.target === null ? t(kit.off, "kit off") : TARGET_NAME[g.target](t)}</span>
                      {g.address !== null && ` · ${g.address}`}
                    </span>
                  </span>
                  <span className="flex shrink-0 flex-col items-end gap-0.5">
                    <span className="font-mono text-sm text-ink tabular-nums">{formatDistance(g.distance_m, locale)}</span>
                    {g.last_shown !== null && new Date(g.last_shown).toDateString() === today && <span className="text-xs text-ink-soft">{t(kit.page.shownToday, "shown today")}</span>}
                  </span>
                  <ChevronRight aria-hidden className="size-4 shrink-0 text-ink-soft" />
                </button>
              </li>
            ))}
          </ul>
        )}
      </main>
      <footer className="flex items-center gap-2 px-4 py-3 text-xs text-ink-soft">
        <QrCode aria-hidden className="size-3.5" />
        {t(kit.page.tap, "Tap a place: its QR fills the screen, for them to scan")}
      </footer>
    </>
  );
}

/** One GBP's QR for the reviewer's camera: light, whatever the theme; a swipe or the arrows to the next. */
function Shown({ gmbs, at, area, onBack, onReturn }: { gmbs: readonly KitNearby[]; at: number; area: string; onBack: () => void; onReturn: () => void }) {
  const t = useT();
  const gmb = gmbs[at]!;
  const res = useResource(`kit.gmb.${gmb.id}`, () => fetchGmbKit(gmb.id));
  const url = res.status === "ok" ? res.data.url : null;
  const start = useRef<number | null>(null);
  const step = (by: number) => {
    const next = gmbs[(at + by + gmbs.length) % gmbs.length];
    if (next !== undefined && next.id !== gmb.id) window.location.replace(`#${next.id}`);
  };

  useEffect(() => {
    if (url === null) return;
    kitShown(gmb.id).catch((e: unknown) => notifyFailure(e, t));
  }, [gmb.id, url, t]);
  // the list's "shown today" and targets, as they are when it is back
  useEffect(() => onReturn, [onReturn]);

  const lead = res.status !== "ok" || res.data.target === null ? null : res.data.target === "google" ? t(kit.page.lead.google, "Google's review form") : res.data.target === "one_tap" ? t(kit.page.lead.oneTap, "one tap copies a drafted review, then Google") : t(kit.page.lead.drafts, "a drafted review to start from, then Google");

  return (
    <div
      className="flex min-h-svh touch-pan-y flex-col"
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
        <Button variant="ghost" size="touch" onClick={onBack}>
          <ChevronLeft aria-hidden />
          {area}
        </Button>
        <span className="flex-1" />
        {gmbs.length > 1 && (
          <>
            <Button variant="ghost" size="touch" icon aria-label={t(kit.page.prev, "Previous")} onClick={() => step(-1)}>
              <ChevronLeft aria-hidden />
            </Button>
            <span className="font-mono tabular-nums">
              {at + 1} / {gmbs.length}
            </span>
            <Button variant="ghost" size="touch" icon aria-label={t(kit.page.next, "Next")} onClick={() => step(1)}>
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
          res.status === "ok" ? <p className="py-24 text-base text-neutral-600">{t(kit.page.off, "Its kit is off: pick where its QR leads below.")}</p> : <Skeleton className="aspect-square w-full max-w-sm" />
        ) : (
          <>
            <Qr text={url} className="aspect-square w-full max-w-sm" />
            <p className="text-lg font-semibold">{t(kit.page.scan, "Scan to leave a review")}</p>
            {lead !== null && <p className="text-sm text-neutral-600">{lead}</p>}
          </>
        )}
      </main>
      <footer className="flex flex-col gap-2 px-3 pt-3 pb-4">
        {res.status === "ok" && <KitTargetSegment gmb={gmb.id} kit={res.data} onSet={res.reload} />}
        {url !== null && <p className="truncate px-1 font-mono text-xs text-ink-soft">{url}</p>}
      </footer>
    </div>
  );
}
