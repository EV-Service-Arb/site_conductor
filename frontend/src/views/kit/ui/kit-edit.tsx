"use client";

import { useResource } from "@evinvest/api/react";
import { Button, ConfirmDialog, Input, ToggleGroup, ToggleGroupItem, cn } from "@evinvest/uikit";
import { Check, Map as MapIcon, Pencil, Search, X } from "lucide-react";
import { useEffect, useState } from "react";

import { AreaDraw, CategoryChip, floatKit, isSlug, kit as kitKeys, move, slugOf, useGmails, useOutline, useSelection, windowKeys } from "@/entities/gmb";
import { Refused, api, ok } from "@/shared/api";
import type { AreaGmbs, Gmb } from "@/shared/api/schema";
import { type T, useLocale, useT } from "@/shared/i18n";
import { formatDistance } from "@/shared/lib/format";
import { failureText } from "@/shared/ui/failure-text";
import { useButtonSize } from "@/shared/ui/touch";

import { sinceOf } from "../model/kit";

type Source = "area" | "outline" | "hand";
// the window's width beside it from md up (right-4 + w-[30rem])
const beside = () => (window.matchMedia("(min-width: 48rem)").matches ? 16 + 480 : 0);

const WAITS: Partial<Record<Gmb["state"], (t: T) => string>> = {
  draft: (t) => t(kitKeys.edit.draft, "not on Google yet: joins once it is"),
  verifying: (t) => t(kitKeys.edit.verifying, "Verifying: joins when Verified"),
  suspended: (t) => t(kitKeys.edit.suspended, "Suspended: no kit while suspended"),
};

/** A kit made or changed (13p): its GBPs from an area drawn on the map, the outline's selection or by hand; the area stays with it as a reference. */
export function KitEdit({ map, slug }: { map: google.maps.Map; slug: string }) {
  const t = useT();
  if (slug === "") return <Editor map={map} slug="" name0="" area0={null} gmbs0={[]} title={t(kitKeys.edit.new, "New kit")} />;
  return <Existing key={slug} map={map} slug={slug} />;
}

function Existing({ map, slug }: { map: google.maps.Map; slug: string }) {
  const t = useT();
  const kit = useResource(api, "/kits/{slug}", { params: { path: { slug }, query: { since: sinceOf("today") } } });
  if (kit.status !== "ok") return null;
  return <Editor map={map} slug={slug} name0={kit.data.name} area0={kit.data.area} gmbs0={kit.data.gmbs.map((g) => g.id)} title={t(kitKeys.edit.title, "Edit {name}", { name: kit.data.name })} />;
}

/** The area's GBPs, told up once read. */
function InArea({ slug, onRead }: { slug: string; onRead: (read: AreaGmbs) => void }) {
  const res = useResource(api, "/areas/{slug}/gmbs", { params: { path: { slug } } });
  const read = res.status === "ok" ? res.data : null;
  useEffect(() => {
    if (read !== null) onRead(read);
  }, [read, onRead]);
  return null;
}

function Editor({ map, slug, name0, area0, gmbs0, title }: { map: google.maps.Map; slug: string; name0: string; area0: string | null; gmbs0: number[]; title: string }) {
  const t = useT();
  const locale = useLocale();
  const button = useButtonSize();
  const gmails = useGmails();
  const selection = useSelection();
  const areas = useResource(api, "/areas");
  const [name, setName] = useState(name0);
  const [slugTyped, setSlugTyped] = useState<string | null>(null);
  const [area, setArea] = useState(area0);
  const [drawing, setDrawing] = useState(false);
  const [source, setSource] = useState<Source>(area0 !== null || slug === "" ? "area" : "hand");
  const [chosen, setChosen] = useState<ReadonlySet<number>>(new Set(gmbs0));
  const [find, setFind] = useState("");
  const [busy, setBusy] = useState(false);
  const [refused, setRefused] = useState<string | null>(null);
  const [inArea, setInArea] = useState<AreaGmbs | null>(null);
  const read = inArea !== null && inArea.area.slug === area ? inArea : null;
  const circle = read?.area ?? null;
  useOutline(map, drawing ? null : circle, 0);
  const own = gmails.status === "ok" ? gmails.data.gmbs : [];
  const byId = new Map(own.map((g) => [g.id, g]));
  const near = new Map(read?.gmbs.map((g) => [g.id, g]) ?? []);
  const offered: Gmb[] =
    source === "area"
      ? [...near.keys()].flatMap((id) => byId.get(id) ?? [])
      : source === "outline"
        ? (selection?.selected ?? []).map((l) => l.gmb)
        : find.trim() === ""
          ? []
          : own.filter((g) => `${g.name} ${g.address ?? ""}`.toLowerCase().includes(find.trim().toLowerCase())).slice(0, 20);
  const listed = [...[...chosen].flatMap((id) => byId.get(id) ?? []), ...offered.filter((g) => !chosen.has(g.id))];
  const finalSlug = slugTyped ?? (slug !== "" ? slug : slugOf(name));
  const toggle = (id: number) => setChosen((s) => (s.has(id) ? new Set([...s].filter((x) => x !== id)) : new Set([...s, id])));
  const save = async () => {
    setBusy(true);
    try {
      await ok(api.PUT("/kits/{slug}", { params: { path: { slug: finalSlug } }, body: { name: name.trim(), area, gmbs: [...chosen] } }));
      floatKit({ kit: finalSlug });
    } catch (e) {
      if (!(e instanceof Refused)) throw e;
      if (e.failure.code !== "unauthenticated") setRefused(failureText(e.failure, t));
    } finally {
      setBusy(false);
    }
  };
  const waiting = [...chosen].flatMap((id) => byId.get(id) ?? []).filter((g) => g.state !== "verified");
  return (
    <div className="pointer-events-none absolute inset-4 flex items-start justify-end">
      <section aria-label={title} className="pointer-events-auto flex max-h-full w-full flex-col overflow-y-auto rounded-lg border border-border bg-background shadow-overlay md:w-120">
        <header className="flex items-center gap-2 border-b border-border px-4 py-3">
          <h2 className="flex-1 text-sm font-semibold text-ink">{title}</h2>
          <Button variant="ghost" size={button("sm")} icon aria-label={t(windowKeys.close, "Close the window")} onClick={() => floatKit(slug === "" ? null : { kit: slug })}>
            <X aria-hidden />
          </Button>
        </header>
        <div className="flex flex-col gap-4 p-4">
          <label className="flex flex-col gap-1">
            <span className="text-sm text-ink">{t(kitKeys.edit.name, "Name")}</span>
            <Input value={name} onChange={(e) => setName(e.target.value)} placeholder={t(kitKeys.edit.namePlaceholder, "Lyon · Saturday")} />
          </label>
          <div className="flex flex-col gap-2">
            <span className="text-sm text-ink">{t(kitKeys.edit.gbps, "GBPs")}</span>
            <ToggleGroup type="single" variant="outline" size="sm" className="w-full" value={source} onValueChange={(v) => (v === "area" || v === "outline" || v === "hand") && setSource(v)}>
              <ToggleGroupItem value="area" className="flex-1 text-xs">
                {t(kitKeys.edit.fromArea, "An area")}
              </ToggleGroupItem>
              <ToggleGroupItem value="outline" className="flex-1 text-xs">
                {t(kitKeys.edit.fromOutline, "The outline's {n} selected", { n: selection?.selected.length ?? 0 })}
              </ToggleGroupItem>
              <ToggleGroupItem value="hand" className="flex-1 text-xs">
                {t(kitKeys.edit.byHand, "By hand")}
              </ToggleGroupItem>
            </ToggleGroup>
            {source === "area" &&
              (drawing ? (
                <div className="rounded-md border border-border">
                  <AreaDraw map={map} left={beside()} onSaved={(a) => (setArea(a.slug), setDrawing(false), areas.reload())} />
                </div>
              ) : (
                <div className="flex flex-wrap items-center gap-1.5">
                  {areas.status === "ok" &&
                    areas.data.areas.map((a) => (
                      <button key={a.slug} type="button" aria-pressed={a.slug === area} onClick={() => setArea(a.slug === area ? null : a.slug)} className={cn("flex items-center gap-1 rounded-md px-2 py-1 text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring", a.slug === area ? "bg-ink text-background" : "bg-hover text-ink hover:bg-hover/70")}>
                        <MapIcon aria-hidden className="size-3" />
                        {a.name} · {formatDistance(a.radius_m, locale)}
                      </button>
                    ))}
                  <Button variant="ghost" size={button("sm")} onClick={() => setDrawing(true)}>
                    <Pencil aria-hidden />
                    {t(kitKeys.edit.draw, "Draw one")}
                  </Button>
                </div>
              ))}
            {area !== null && <InArea slug={area} onRead={setInArea} />}
            {source === "area" && read !== null && <p className="text-xs text-ink-soft">{t(kitKeys.edit.inArea, "{n} of your GBPs in it or next to it", { n: read.gmbs.length })}</p>}
            {source === "hand" && (
              <label className="relative">
                <Search aria-hidden className="absolute top-1/2 left-3 size-4 -translate-y-1/2 text-ink-soft" />
                <Input value={find} onChange={(e) => setFind(e.target.value)} placeholder={t(kitKeys.edit.find, "A GBP's name or address")} className="pl-9" />
              </label>
            )}
            <ul className="flex max-h-80 flex-col divide-y divide-border overflow-y-auto rounded-md border border-border">
              {listed.length === 0 && <li className="px-3 py-2 text-sm text-ink-soft">{t(kitKeys.edit.noneYet, "None picked yet.")}</li>}
              {listed.map((g) => {
                const on = chosen.has(g.id);
                const n = near.get(g.id);
                const waits = WAITS[g.state];
                return (
                  <li key={g.id}>
                    <button type="button" aria-pressed={on} onClick={() => toggle(g.id)} className={cn("flex w-full items-center gap-3 px-3 py-2 text-left outline-none hover:bg-hover focus-visible:ring-2 focus-visible:ring-ring", g.state === "suspended" && !on && "opacity-50")}>
                      <span className={cn("flex size-4 shrink-0 items-center justify-center rounded border", on ? "border-ink bg-ink text-background" : "border-border")}>{on && <Check aria-hidden className="size-3" />}</span>
                      <span className="flex min-w-0 flex-1 flex-col">
                        <span className="truncate text-sm text-ink">{g.name}</span>
                        <span className={cn("truncate text-xs", waits ? "text-accent-warn" : "text-ink-soft")}>{waits ? waits(t) : n ? (n.inside ? t(kitKeys.edit.inIt, "in it") : t(kitKeys.edit.nextTo, "next to it · {d}", { d: formatDistance(n.distance_m, locale) })) : g.address}</span>
                      </span>
                      <CategoryChip gmb={g} />
                    </button>
                  </li>
                );
              })}
            </ul>
          </div>
          <label className="flex flex-col gap-1">
            <span className="text-sm text-ink">{t(kitKeys.edit.link, "Link")}</span>
            <span className="flex items-center gap-1 font-mono text-sm text-ink-soft">
              /kit/
              <Input value={finalSlug} disabled={slug !== ""} onChange={(e) => setSlugTyped(e.target.value.toLowerCase())} aria-invalid={finalSlug !== "" && (!isSlug(finalSlug) || finalSlug === "new")} className="min-w-0 flex-1 font-mono" />
            </span>
            <span className="text-xs text-ink-soft">{t(kitKeys.edit.linkHint, "the operator's page; a reviewer never sees it (their QR goes to the brand's site)")}</span>
          </label>
          {refused !== null && <p className="text-xs text-accent-error">{refused}</p>}
        </div>
        <footer className="flex items-center gap-2 border-t border-border px-4 py-3">
          <span className="flex-1 text-xs text-ink-soft">{waiting.length > 0 && t(kitKeys.edit.waiting, "{n} wait in it, counted once Verified", { n: waiting.length })}</span>
          {slug !== "" && <Delete slug={slug} name={name0} />}
          <Button size={button("sm")} disabled={busy || name.trim() === "" || !isSlug(finalSlug) || finalSlug === "new"} onClick={() => void save()}>
            {slug === "" ? t(kitKeys.edit.create, "Create · {n} GBPs", { n: chosen.size }) : t(kitKeys.edit.save, "Save · {n} GBPs", { n: chosen.size })}
          </Button>
        </footer>
      </section>
    </div>
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
        floatKit({ kit: "" });
      }}
    />
  );
}
