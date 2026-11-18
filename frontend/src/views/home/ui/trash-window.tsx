"use client";

import { Badge, Button, ConfirmDialog, FloatingPanel, Select, SelectContent, SelectItem, SelectTrigger, SelectValue, Switch, type Tone } from "@evinvest/uikit";
import { ChevronDown, ExternalLink, RotateCcw, Trash2 } from "lucide-react";
import { type ReactNode, useState } from "react";

import { move, openTrash, STATE_NAME, STATE_TONE, useGmails, trash as trashKeys, windowKeys } from "@/entities/gmb";
import { may, useMe } from "@/entities/session";
import { api, ok } from "@/shared/api";
import type { Trash, TrashedGbp } from "@/shared/api/schema";
import { useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize, useControlSize } from "@/shared/ui/touch";

const DAY_MS = 86_400_000;
/** Where the home's floating windows first open: over the map, beside the outline. */
export const FLOAT_AT = { x: 500, y: 140 };
const KEEP_DAYS = [1, 7, 30, 90] as const; // as the server accepts
const SECTION = "flex flex-col gap-2";
const SECTION_HEAD = "flex items-baseline gap-2 text-xs font-semibold tracking-wide text-ink-soft uppercase";

function days(from: string, to: string): number {
  const d = (Date.parse(to) - Date.parse(from)) / DAY_MS;
  if (Number.isNaN(d)) throw new Error(`${from} → ${to}: not RFC 3339`);
  return Math.max(0, Math.round(d));
}

/** What tells a removed GBP apart, to weigh restoring it against deleting it for good. */
function Facts({ gbp }: { gbp: Pick<TrashedGbp, "place_id" | "address" | "phone"> }) {
  const t = useT();
  const facts: [string, ReactNode][] = [
    [
      t(trashKeys.place, "Place"),
      gbp.place_id === null ? null : (
        <a className="inline-flex items-center gap-1 break-all text-accent hover:underline" href={`https://www.google.com/maps/place/?q=place_id:${encodeURIComponent(gbp.place_id)}`} target="_blank" rel="noreferrer">
          {gbp.place_id}
          <ExternalLink aria-hidden className="size-3 shrink-0" />
        </a>
      ),
    ],
    [t(trashKeys.address, "Address"), gbp.address],
    [t(trashKeys.phone, "Phone"), gbp.phone],
  ];
  return (
    <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 text-xs">
      {facts.map(([k, v]) => (
        <div key={k} className="contents">
          <dt className="text-ink-soft">{k}</dt>
          <dd className={v === null ? "text-ink-soft" : "text-ink"}>{v ?? "—"}</dd>
        </div>
      ))}
    </dl>
  );
}

/** What the caller removed (`?trash`), floating over the map as the Categories window: restorable until purged. */
export function TrashWindow({ trash }: { trash: Trash }) {
  const t = useT();
  const button = useButtonSize();
  const { reload } = useGmails();
  const [busy, setBusy] = useState(false);
  const [open, setOpen] = useState<string | null>(null); // one row at a time
  const now = new Date().toISOString();
  const meta = (removed: string, purge: string | null) =>
    [days(removed, now) === 0 ? t(trashKeys.removed.today, "removed today") : t(trashKeys.removed, "removed {days} d ago", { days: days(removed, now) }), purge === null ? t(trashKeys.kept, "kept until restored") : t(trashKeys.gone, "gone for good in {days} d", { days: days(now, purge) })].join(" · ");
  const act = async (run: () => Promise<void>) => {
    setBusy(true);
    try {
      await run();
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
      reload();
    }
  };
  const row = (key: string, primary: string, secondary: string | null, badge: { name: string; tone: Tone } | null, removed: string, purge: string | null, blocked: string | null, details: ReactNode, restore: () => Promise<void>, erase: () => Promise<void>) => (
    <li key={key} className="flex flex-col gap-2 rounded-md px-2 py-2 hover:bg-hover/50">
      <div className="flex items-start gap-3">
      <button type="button" className="flex min-w-0 flex-1 cursor-pointer flex-col gap-0.5 text-left" aria-expanded={open === key} onClick={() => setOpen(open === key ? null : key)}>
        <p className="flex items-center gap-2 text-sm">
          <ChevronDown aria-hidden className={`size-4 shrink-0 text-ink-soft transition-transform ${open === key ? "" : "-rotate-90"}`} />
          <span className="truncate font-medium text-ink">{primary}</span>
          {secondary !== null && <span className="truncate text-ink-soft">{secondary}</span>}
          {badge !== null && <Badge tone={badge.tone}>{badge.name}</Badge>}
        </p>
        <p className="text-xs text-ink-soft tabular-nums">{meta(removed, purge)}</p>
        {blocked !== null && <p className="text-xs text-accent-warn">{blocked}</p>}
      </button>
      <Button variant="outline" size={button("sm")} disabled={busy || blocked !== null} title={blocked ?? undefined} onClick={() => act(restore)}>
        <RotateCcw aria-hidden />
        {t(trashKeys.restore, "Restore")}
      </Button>
      <ConfirmDialog
        trigger={
          <Button variant="ghost" size={button("sm")} icon disabled={busy} aria-label={t(trashKeys.erase, "Delete for good")} title={t(trashKeys.erase, "Delete for good")}>
            <Trash2 aria-hidden />
          </Button>
        }
        tone="error"
        title={t(trashKeys.erase.title, "Delete {name} for good?", { name: primary })}
        description={t(trashKeys.erase.body, "It cannot be restored after this.")}
        confirmLabel={t(trashKeys.erase, "Delete for good")}
        labels={{ cancel: t(move.cancel, "Cancel") }}
        onConfirm={() => act(erase)}
      />
      </div>
      {open === key && <div className="ml-6 flex flex-col gap-3 border-l border-border pl-3">{details}</div>}
    </li>
  );

  return (
    <FloatingPanel
      title={
        <span className="flex items-center gap-2">
          <Trash2 aria-hidden className="size-4 shrink-0 text-ink-soft" />
          {t(trashKeys.title, "Trash")}
        </span>
      }
      at={FLOAT_AT}
      closeLabel={t(windowKeys.close, "Close the window")}
      onClose={() => openTrash(false)}
    >
      <div className="flex w-3xl max-w-full flex-col gap-6 p-6">
        <p className="-mt-2 text-sm text-ink-soft">{t(trashKeys.what, "What you removed: every service stops working on it until it is restored.")}</p>
        <Settings trash={trash} onSet={reload} />
        {trash.gmails.length === 0 && trash.gmbs.length === 0 ? (
          <p className="text-sm text-ink-soft">{t(trashKeys.empty, "Nothing removed.")}</p>
        ) : (
          <>
            {trash.gmails.length > 0 && (
              <section className={SECTION}>
                <h3 className={SECTION_HEAD}>
                  {t(trashKeys.gmails, "Gmails")} <span className="font-normal tabular-nums">{trash.gmails.length}</span>
                </h3>
                <ul className="flex flex-col">
                  {trash.gmails.map((g) => row(`g${g.id}`, g.gmail, t(trashKeys.with, "{n} GBPs with it", { n: g.gmbs.length }), null, g.removed_at, g.purge_at, g.blocked, g.gmbs.length === 0 ? <p className="text-xs text-ink-soft">{t(trashKeys.with.none, "No GBPs went with it.")}</p> : g.gmbs.map((b) => (
                    <div key={b.id} className="flex flex-col gap-1">
                      <p className="flex items-center gap-2 text-sm">
                        <span className="truncate font-medium text-ink">{b.name}</span>
                        <Badge tone={STATE_TONE[b.state]}>{STATE_NAME[b.state](t)}</Badge>
                      </p>
                      <Facts gbp={b} />
                    </div>
                  )), () => ok(api.POST("/trash/gmails/{id}/restore", { params: { path: { id: g.id } } })), () => ok(api.DELETE("/trash/gmails/{id}", { params: { path: { id: g.id } } }))))}
                </ul>
              </section>
            )}
            {trash.gmbs.length > 0 && (
              <section className={SECTION}>
                <h3 className={SECTION_HEAD}>
                  {t(trashKeys.gmbs, "GBPs removed on their own")} <span className="font-normal tabular-nums">{trash.gmbs.length}</span>
                </h3>
                <ul className="flex flex-col">
                  {trash.gmbs.map((b) => row(`b${b.id}`, b.name, b.gmail, { name: STATE_NAME[b.state](t), tone: STATE_TONE[b.state] }, b.removed_at, b.purge_at, b.blocked, <Facts gbp={b} />, () => ok(api.POST("/trash/gmbs/{id}/restore", { params: { path: { id: b.id } } })), () => ok(api.DELETE("/trash/gmbs/{id}", { params: { path: { id: b.id } } }))))}
                </ul>
              </section>
            )}
          </>
        )}
      </div>
    </FloatingPanel>
  );
}

/** How long the caller's trash keeps what they removed; an admin may keep it forever. */
function Settings({ trash, onSet }: { trash: Trash; onSet: () => void }) {
  const t = useT();
  const size = useControlSize();
  const me = useMe();
  const { keep_days, never_purge } = trash.settings;
  const save = async (keep: number, never?: boolean) => {
    try {
      await ok(api.PUT("/trash/settings", { body: never === undefined ? { keep_days: keep } : { keep_days: keep, never_purge: never } }));
    } catch (e) {
      notifyFailure(e, t);
    }
    onSet();
  };
  return (
    <section className="flex flex-col gap-2 rounded-lg border border-border p-3">
      <div className="flex items-center gap-2 text-sm text-ink">
        {t(trashKeys.keep.before, "Keep for")}
        <Select value={String(keep_days)} onValueChange={(v) => void save(Number(v))}>
          <SelectTrigger size={size} className="w-24 tabular-nums" disabled={never_purge}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {KEEP_DAYS.map((d) => (
              <SelectItem key={d} value={String(d)}>
                {t(trashKeys.keep.days, "{days} d", { days: d })}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        {t(trashKeys.keep.after, "then delete for good")}
      </div>
      {may(me, "sa:admin:trash:keep") && (
        <label className="flex items-center gap-2 text-sm text-ink" title={t(trashKeys.never.hint, "Turned off, the kept days count from then: nothing here is deleted for good at that moment.")}>
          <Switch checked={never_purge} onCheckedChange={(on) => void save(keep_days, on)} />
          {t(trashKeys.never, "Never delete for good")}
        </label>
      )}
      {never_purge && <p className="text-xs text-ink-soft">{t(trashKeys.never.on, "Nothing here is deleted for good.")}</p>}
    </section>
  );
}