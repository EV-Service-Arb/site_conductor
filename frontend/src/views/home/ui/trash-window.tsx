"use client";

import { Badge, Button, Input, Switch } from "@evinvest/uikit";
import { RotateCcw, Trash2, X } from "lucide-react";
import { useState } from "react";

import { openTrash, restoreGmail, restoreGmb, setTrashSettings, STATE_LABEL, type State, type Trash, useGmails } from "@/entities/gmb";
import { may, useMe } from "@/entities/session";
import { useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize, useControlSize } from "@/shared/ui/touch";

const DAY_MS = 86_400_000;
/** As the outline colours the states. */
const TONE: Partial<Record<State, string>> = { verifying: "text-accent-warn", suspended: "text-accent-error" };
const SECTION = "flex flex-col gap-2";
const SECTION_HEAD = "flex items-baseline gap-2 text-xs font-semibold tracking-wide text-ink-soft uppercase";

function days(from: string, to: string): number {
  const d = (Date.parse(to) - Date.parse(from)) / DAY_MS;
  if (Number.isNaN(d)) throw new Error(`${from} → ${to}: not RFC 3339`);
  return Math.max(0, Math.round(d));
}

/** What the caller removed (`?trash`), over the map as the Categories window: restorable until purged. */
export function TrashWindow({ trash }: { trash: Trash }) {
  const t = useT();
  const button = useButtonSize();
  const { reload } = useGmails();
  const [busy, setBusy] = useState(false);
  const now = new Date().toISOString();
  const meta = (removed: string, purge: string | null) =>
    [days(removed, now) === 0 ? t("removed today") : t("removed {days} d ago", { days: days(removed, now) }), purge === null ? t("kept until restored") : t("gone for good in {days} d", { days: days(now, purge) })].join(" · ");
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
  const row = (key: string, primary: string, secondary: string | null, badge: string | null, badgeTone: string | undefined, removed: string, purge: string | null, blocked: string | null, restore: () => Promise<void>) => (
    <li key={key} className="flex items-start gap-3 rounded-md px-2 py-2 hover:bg-hover/50">
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <p className="flex items-center gap-2 text-sm">
          <span className="truncate font-medium text-ink">{primary}</span>
          {secondary !== null && <span className="truncate text-ink-soft">{secondary}</span>}
          {badge !== null && <Badge className={badgeTone}>{badge}</Badge>}
        </p>
        <p className="text-xs text-ink-soft tabular-nums">{meta(removed, purge)}</p>
        {blocked !== null && <p className="text-xs text-accent-warn">{blocked}</p>}
      </div>
      <Button variant="outline" size={button("sm")} disabled={busy || blocked !== null} title={blocked ?? undefined} onClick={() => act(restore)}>
        <RotateCcw aria-hidden />
        {t("Restore")}
      </Button>
    </li>
  );

  return (
    <section aria-label={t("Trash")} className="absolute inset-y-0 right-0 left-0 flex flex-col overflow-y-auto border-l border-border bg-background md:left-36">
      <header className="flex items-start gap-3 border-b border-border px-6 pt-4 pb-3">
        <div className="flex min-w-0 flex-1 flex-col gap-0.5">
          <h2 className="flex items-center gap-2 text-lg font-semibold text-ink">
            <Trash2 aria-hidden className="size-4 shrink-0 text-ink-soft" />
            {t("Trash")}
          </h2>
          <p className="text-sm text-ink-soft">{t("What you removed: every service stops working on it until it is restored.")}</p>
        </div>
        <Button variant="ghost" size={button("sm")} icon aria-label={t("Close the window")} title={t("Close the window")} onClick={() => openTrash(false)}>
          <X aria-hidden />
        </Button>
      </header>
      <div className="flex max-w-3xl flex-col gap-6 p-6">
        <Settings trash={trash} onSet={reload} />
        {trash.gmails.length === 0 && trash.gmbs.length === 0 ? (
          <p className="text-sm text-ink-soft">{t("Nothing removed.")}</p>
        ) : (
          <>
            {trash.gmails.length > 0 && (
              <section className={SECTION}>
                <h3 className={SECTION_HEAD}>
                  {t("Gmails")} <span className="font-normal tabular-nums">{trash.gmails.length}</span>
                </h3>
                <ul className="flex flex-col">
                  {trash.gmails.map((g) => row(`g${g.id}`, g.gmail, t("{n} GBPs with it", { n: g.gmbs }), null, undefined, g.removed_at, g.purge_at, g.blocked, () => restoreGmail(g.id)))}
                </ul>
              </section>
            )}
            {trash.gmbs.length > 0 && (
              <section className={SECTION}>
                <h3 className={SECTION_HEAD}>
                  {t("GBPs removed on their own")} <span className="font-normal tabular-nums">{trash.gmbs.length}</span>
                </h3>
                <ul className="flex flex-col">
                  {trash.gmbs.map((b) => row(`b${b.id}`, b.name, b.gmail, STATE_LABEL[b.state](t), TONE[b.state], b.removed_at, b.purge_at, b.blocked, () => restoreGmb(b.id)))}
                </ul>
              </section>
            )}
          </>
        )}
      </div>
    </section>
  );
}

/** How long the caller's trash keeps what they removed; an admin may keep it forever. */
function Settings({ trash, onSet }: { trash: Trash; onSet: () => void }) {
  const t = useT();
  const size = useControlSize();
  const me = useMe();
  const { keep_days, never_purge } = trash.settings;
  const [typed, setTyped] = useState(String(keep_days));
  const save = async (keep: number, never?: boolean) => {
    try {
      await setTrashSettings(keep, never);
    } catch (e) {
      notifyFailure(e, t);
    }
    onSet();
  };
  const commit = () => {
    const n = Number(typed);
    if (!Number.isInteger(n) || n < 1 || n > 90) return setTyped(String(keep_days));
    if (n !== keep_days) void save(n);
  };
  return (
    <section className="flex flex-col gap-2 rounded-lg border border-border p-3">
      <label className="flex items-center gap-2 text-sm text-ink">
        {t("Keep for")}
        <Input
          type="number"
          min={1}
          max={90}
          size={size}
          className="w-20 tabular-nums"
          disabled={never_purge}
          value={typed}
          onChange={(e) => setTyped(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => e.key === "Enter" && commit()}
        />
        {t("days, then delete for good")}
        <span className="text-xs text-ink-soft tabular-nums">1–90</span>
      </label>
      {may(me, "sa:admin:trash:keep") && (
        <label className="flex items-center gap-2 text-sm text-ink">
          <Switch checked={never_purge} onCheckedChange={(on) => void save(keep_days, on)} />
          {t("Never delete for good")}
        </label>
      )}
      {never_purge && <p className="text-xs text-ink-soft">{t("Nothing here is deleted for good.")}</p>}
    </section>
  );
}
