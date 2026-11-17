"use client";

import { Button, Skeleton, buttonVariants, toast } from "@evinvest/uikit";
import { href } from "@evinvest/views";
import { Copy, ExternalLink, Plus, QrCode, X } from "lucide-react";

import { AreaDraw, TARGET_NAME, kit, open, openKit, outlineOf, useGmails, useOutline, windowKeys } from "@/entities/gmb";
import { api } from "@/shared/api";
import type { KitNearby } from "@/shared/api/schema";
import { VIEWS } from "@/shared/api/views";
import { useLocale, useT } from "@/shared/i18n";
import { formatDistance } from "@/shared/lib/format";
import { useResource } from "@evinvest/api/react";
import { ErrorState } from "@/shared/ui/error-state";
import { notifyFailure } from "@/shared/ui/notify";
import { Qr } from "@/shared/ui/qr";
import { useButtonSize } from "@/shared/ui/touch";

const SECTION_HEAD = "flex items-baseline gap-2 text-xs font-semibold tracking-wide text-ink-soft uppercase";
// the window's width beside it from md up (left-4 + w-104)
const beside = () => (window.matchMedia("(min-width: 48rem)").matches ? 16 + 416 : 0);

/** The review kit's areas (`?kit`), over the map's left so the area stays in sight: `""` draws one, a slug shows that one. */
export function KitWindow({ map, area }: { map: google.maps.Map; area: string }) {
  const t = useT();
  const button = useButtonSize();
  return (
    // the map stays clickable beside it and below it
    <div className="pointer-events-none absolute inset-4 flex items-start">
      <section aria-label={t(kit.window, "Review kit")} className="pointer-events-auto flex max-h-full w-full flex-col overflow-y-auto rounded-lg border border-border bg-background shadow-overlay md:w-104">
        <header className="flex items-center gap-2 border-b border-border px-4 py-3">
          <QrCode aria-hidden className="size-4 text-ink-soft" />
          <h2 className="flex-1 text-sm font-semibold text-ink">{area === "" ? t(kit.area.new, "Review kit · a new area") : t(kit.window, "Review kit")}</h2>
          <Button variant="ghost" size={button("sm")} icon aria-label={t(windowKeys.close, "Close the window")} title={t(windowKeys.close, "Close the window")} onClick={() => openKit(null)}>
            <X aria-hidden />
          </Button>
        </header>
        {area === "" ? <Draw map={map} /> : <Area key={area} map={map} slug={area} />}
      </section>
    </div>
  );
}

/** A new area, and every one drawn below it. */
function Draw({ map }: { map: google.maps.Map }) {
  return (
    <>
      <AreaDraw map={map} left={beside()} onSaved={(a) => openKit(a.slug)} />
      <div className="px-4 pb-4">
        <Areas />
      </div>
    </>
  );
}

/** Every area drawn, anyone's: areas are geography. */
function Areas() {
  const t = useT();
  const areas = useResource(api, "/areas");
  if (areas.status !== "ok" || areas.data.areas.length === 0) return null;
  return (
    <section className="flex flex-col gap-2 border-t border-border pt-3">
      <h3 className={SECTION_HEAD}>{t(kit.areas, "Areas")}</h3>
      <span className="flex flex-wrap gap-1.5">
        {areas.data.areas.map((a) => (
          <button key={a.slug} type="button" onClick={() => openKit(a.slug)} className="flex items-center gap-1.5 rounded-md bg-hover px-2 py-1 text-xs text-ink outline-none hover:bg-hover/70 focus-visible:ring-2 focus-visible:ring-ring">
            {a.name}
            <span className="font-mono text-ink-soft">/{a.slug}/</span>
          </button>
        ))}
      </span>
    </section>
  );
}

function Area({ map, slug }: { map: google.maps.Map; slug: string }) {
  const t = useT();
  const locale = useLocale();
  const button = useButtonSize();
  const res = useResource(api, "/kit/areas/{slug}/gmbs", { params: { path: { slug } } });
  const area = res.status === "ok" ? res.data.area : null;
  useOutline(map, area === null ? null : outlineOf(area), beside());
  if (res.status === "loading") return <Skeleton className="m-4 h-64" />;
  if (res.status === "error")
    return (
      <div className="flex flex-col gap-3 p-4">
        {res.failure.code === "not_found" ? <p className="text-sm text-ink-soft">{t(kit.area.notFound, "No area is at /{slug}/.", { slug })}</p> : <ErrorState failure={res.failure} onRetry={res.reload} />}
        <NewArea />
      </div>
    );
  const { gmbs } = res.data;
  const a = res.data.area;
  const page = href(VIEWS, { view: "area", params: { area: a.slug } });
  const link = `${window.location.origin}${page}`;
  const inside = gmbs.filter((g) => g.inside);
  const near = gmbs.filter((g) => !g.inside);
  const shape = a.shape.kind === "city" ? t(kit.area.isCity, "the city") : t(kit.area.isCircle, "a circle of {radius}", { radius: formatDistance(a.shape.radius_m, locale) });
  return (
    <div className="flex flex-col gap-4 p-4">
      <div className="flex flex-col gap-0.5">
        <p className="text-lg font-semibold text-ink">{a.name}</p>
        <p className="text-sm text-ink-soft">{a.margin_m > 0 ? t(kit.area.summary, "{shape}, and {margin} around it", { shape, margin: formatDistance(a.margin_m, locale) }) : shape}</p>
      </div>
      <Group title={t(kit.area.inIt, "In it")} gmbs={inside} locale={locale} />
      <Group title={t(kit.area.nextToIt, "Next to it")} gmbs={near} locale={locale} />
      {gmbs.length === 0 && <p className="text-sm text-ink-soft">{t(kit.area.noGbps, "None of your GBPs with a point is in it or next to it.")}</p>}
      <div className="flex items-center gap-3 rounded-lg border border-border p-3">
        <Qr text={link} className="size-24 shrink-0 rounded p-1" />
        <div className="flex min-w-0 flex-col gap-1">
          <p className="text-sm font-medium text-ink">{t(kit.area.kit, "Review kit · {n} places", { n: inside.length + near.length })}</p>
          <p className="truncate font-mono text-xs text-ink-soft">{link}</p>
          <p className="text-xs text-ink-soft">{t(kit.area.scan, "Scan with your phone to take it out; signed-in only.")}</p>
        </div>
      </div>
      <div className="flex flex-wrap gap-2">
        <a href={page} target="_blank" rel="noreferrer" className={buttonVariants({ size: button("sm") })}>
          <ExternalLink aria-hidden />
          {t(kit.area.open, "Open {page}", { page })}
        </a>
        <Button
          variant="outline"
          size={button("sm")}
          onClick={() =>
            void navigator.clipboard.writeText(link).then(
              () => toast.positive(t(kit.area.copied, "Link copied")),
              (e: unknown) => notifyFailure(e, t),
            )
          }
        >
          <Copy aria-hidden />
          {t(kit.area.copy, "Copy link")}
        </Button>
        <span className="flex-1" />
        <NewArea />
      </div>
    </div>
  );
}

function NewArea() {
  const t = useT();
  const button = useButtonSize();
  return (
    <Button variant="ghost" size={button("sm")} onClick={() => openKit("")}>
      <Plus aria-hidden />
      {t(kit.area.another, "Another area")}
    </Button>
  );
}


function Group({ title, gmbs, locale }: { title: string; gmbs: readonly KitNearby[]; locale: string }) {
  const t = useT();
  const gmails = useGmails();
  if (gmbs.length === 0) return null;
  // its window's tabs are the caller's GMB's, as the gmails read has it
  const own = (id: number) => (gmails.status === "ok" ? gmails.data.gmbs.find((b) => b.id === id) : undefined);
  return (
    <section className="flex flex-col gap-1">
      <h3 className={SECTION_HEAD}>
        {title} <span className="font-normal tabular-nums">{gmbs.length}</span>
      </h3>
      <ul className="flex flex-col">
        {gmbs.map((g) => (
          <li key={g.id}>
            <button type="button" disabled={own(g.id) === undefined} onClick={() => open(own(g.id)!)} className="flex w-full items-center gap-3 rounded-md px-2 py-1.5 text-left outline-none hover:bg-hover focus-visible:ring-2 focus-visible:ring-ring">
              <span className="flex min-w-0 flex-1 flex-col">
                <span className="truncate text-sm text-ink">{g.name}</span>
                <span className="truncate text-xs text-ink-soft">{[g.address, g.target === null ? t(kit.off, "kit off") : TARGET_NAME[g.target](t)].filter(Boolean).join(" · ")}</span>
              </span>
              <span className="font-mono text-xs text-ink-soft tabular-nums">{formatDistance(g.distance_m, locale)}</span>
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}
