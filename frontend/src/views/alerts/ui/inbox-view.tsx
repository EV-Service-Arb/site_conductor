"use client";

import { Button, SectionLabel, Skeleton, Switch, ToggleGroup, ToggleGroupItem, cn } from "@evinvest/uikit";
import { useResource } from "@evinvest/api/react";
import { ArrowLeft, CheckCheck, ChevronRight, Inbox, Settings, SlidersHorizontal } from "lucide-react";
import { type KeyboardEvent, useState } from "react";

import { AlertRow, FAMILIES, FAMILY_ICON, FAMILY_NAME, TIERS, TIER_LOOK, TIER_NAME, alerts as keys, readAll, useUnread } from "@/entities/alerts";
import { type AlertsAt, inScope, openWindow, useGmails, useScope } from "@/entities/gmb";
import { api, ok } from "@/shared/api";
import type { Alert, AlertInbox, Family, Tier } from "@/shared/api/schema";
import { useLocale, useT } from "@/shared/i18n";
import { ErrorState } from "@/shared/ui/error-state";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize } from "@/shared/ui/touch";

import { AlertDetail } from "./alert-detail";

type At = Exclude<AlertsAt, { settings: true }>;
type Sought = At & { unread: boolean };

/**
 * The caller's alerts: what to look among, the list by day, and the one opened — three panes, two or one by the room there is
 * (the outline and the assistant take theirs); in one, the list or the one opened.
 */
export function Alerts({ at, page }: { at: At; page: boolean }) {
  const sought = { ...at, unread: at.unread ?? false };
  const opened = sought.open;
  return (
    <div className={cn("@container flex flex-col", page ? "min-h-0 flex-1" : "h-[min(40rem,75svh)]")}>
      <div className="flex min-h-0 flex-1">
        <Filters sought={sought} />
        <div className={cn("flex min-h-0 min-w-0 flex-1 flex-col @xl:max-w-72 @xl:border-r @xl:border-border @3xl:max-w-96 @5xl:max-w-110", opened !== undefined && "@max-xl:hidden")}>
          <Chips sought={sought} />
          <Narrowed sought={sought} />
        </div>
        <div className={cn("flex min-h-0 min-w-0 flex-1 flex-col", opened === undefined && "@max-xl:hidden")}>
          {opened === undefined ? (
            <Pick />
          ) : (
            <>
              <Back sought={sought} />
              <AlertDetail key={opened} id={opened} />
            </>
          )}
        </div>
      </div>
    </div>
  );
}

/** The window's name, and what is done from anywhere in it. */
export function AlertsTitle({ at }: { at: AlertsAt }) {
  const t = useT();
  const size = useButtonSize();
  const unread = useUnread();
  const [busy, setBusy] = useState(false);
  const newest = unread.status === "ok" ? unread.data.alerts[0]?.id : undefined;
  const all = async (up: number) => {
    setBusy(true);
    try {
      await readAll(up);
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
      unread.reload();
    }
  };
  if (at.settings === true)
    return (
      <span className="flex min-w-0 flex-1 items-center gap-1">
        <Button type="button" variant="ghost" size={size("sm")} icon aria-label={t(keys.inbox, "Inbox")} onClick={() => openWindow({ kind: "alerts", at: {} })}>
          <ArrowLeft aria-hidden />
        </Button>
        <span className="truncate">{t(keys.settings.title, "Alert settings")}</span>
      </span>
    );
  return (
    <span className="flex min-w-0 flex-1 items-center gap-1">
      <span className="flex-1 truncate">{t(keys.title, "Alerts")}</span>
      <Button type="button" variant="ghost" size={size("sm")} icon disabled={busy || newest === undefined} aria-label={t(keys.readAll, "Mark all read")} title={t(keys.readAll, "Mark all read")} onClick={() => newest !== undefined && void all(newest)}>
        <CheckCheck aria-hidden />
      </Button>
      <Button type="button" variant="ghost" size={size("sm")} icon aria-label={t(keys.settings.title, "Alert settings")} title={t(keys.settings.title, "Alert settings")} onClick={() => openWindow({ kind: "alerts", at: { settings: true } })}>
        <Settings aria-hidden />
      </Button>
    </span>
  );
}

/** The inbox narrowed otherwise: what is left out by the change is as it was. */
function seek(sought: Sought, change: { [K in keyof Sought]?: Sought[K] | undefined }): void {
  const next = Object.fromEntries(Object.entries({ ...sought, open: undefined, ...change }).filter(([, v]) => v !== undefined)) as At;
  openWindow({ kind: "alerts", at: next }, "replace");
}

function Back({ sought }: { sought: Sought }) {
  const t = useT();
  return (
    <div className="flex items-center gap-1 border-b border-border px-2 py-1.5 @xl:hidden">
      <Button type="button" variant="ghost" size="touch" icon aria-label={t(keys.back, "Back to the list")} onClick={() => seek(sought, {})}>
        <ArrowLeft aria-hidden className="size-5" />
      </Button>
      <p className="text-sm text-ink-soft">{t(keys.back, "Back to the list")}</p>
    </div>
  );
}

const ITEM = "flex w-full items-center gap-2.5 rounded-md px-2 py-1.5 text-left text-sm text-ink outline-none hover:bg-hover/50 focus-visible:ring-2 focus-visible:ring-ring aria-pressed:bg-hover";

function useCounts(): { inbox: AlertInbox | null; of: (family: Family) => number } {
  const unread = useUnread();
  const inbox = unread.status === "ok" ? unread.data : null;
  return { inbox, of: (family) => inbox?.alerts.filter((a) => a.family === family).length ?? 0 };
}

function Filters({ sought }: { sought: Sought }) {
  const t = useT();
  const { inbox, of } = useCounts();
  const n = inbox?.unread;
  const total = n === undefined ? 0 : n.urgent + n.important + n.info;
  return (
    <nav aria-label={t(keys.filters, "Filters")} className="hidden w-62 shrink-0 flex-col gap-1 overflow-y-auto border-r border-border p-3 @5xl:flex">
      <button type="button" aria-pressed={sought.tier === undefined} className={ITEM} onClick={() => seek(sought, { tier: undefined })}>
        <Inbox aria-hidden className="size-4 text-ink-soft" />
        <span className="flex-1">{t(keys.filter.all, "All")}</span>
        <Count n={total} />
      </button>
      {TIERS.map((tier) => (
        <button key={tier} type="button" aria-pressed={sought.tier === tier} className={ITEM} onClick={() => seek(sought, { tier: sought.tier === tier ? undefined : tier })}>
          <span aria-hidden className={cn("mx-1 size-2 rounded-full", TIER_LOOK[tier].dot)} />
          <span className="flex-1">{TIER_NAME[tier](t)}</span>
          <Count n={n?.[tier] ?? 0} className={tier === "info" ? undefined : TIER_LOOK[tier].ink} />
        </button>
      ))}
      <label className="flex items-center justify-between px-2 py-2 text-sm text-ink">
        {t(keys.unreadOnly, "Unread only")}
        <Switch checked={sought.unread} onCheckedChange={(unread) => seek(sought, { unread })} />
      </label>
      <SectionLabel className="px-2 pt-3">{t(keys.about, "About")}</SectionLabel>
      {FAMILIES.map((family) => {
        const Icon = FAMILY_ICON[family];
        return (
          <button key={family} type="button" aria-pressed={sought.family === family} className={ITEM} onClick={() => seek(sought, { family: sought.family === family ? undefined : family })}>
            <Icon aria-hidden className="size-4 text-ink-soft" />
            <span className="flex-1">{FAMILY_NAME[family](t)}</span>
            <Count n={of(family)} />
          </button>
        );
      })}
      <p className="px-2 pt-3 text-xs text-ink-soft">{t(keys.scope, "Of the GBPs in the outline's scope. What names no GBP always shows.")}</p>
    </nav>
  );
}

function Count({ n, className }: { n: number; className?: string | undefined }) {
  const t = useT();
  if (n === 0) return null;
  return (
    <span className={cn("text-xs text-ink-soft tabular-nums", className)}>
      {n} <span className="sr-only">{t(keys.unread, "unread")}</span>
    </span>
  );
}

const CHIP = "inline-flex min-h-9 shrink-0 items-center gap-2 rounded-full border border-border px-3 text-sm text-ink outline-none focus-visible:ring-2 focus-visible:ring-ring aria-pressed:bg-hover";

function Chips({ sought }: { sought: Sought }) {
  const t = useT();
  const { inbox } = useCounts();
  const [more, setMore] = useState(sought.family !== undefined);
  const n = inbox?.unread;
  return (
    <div className="flex flex-col gap-2 border-b border-border px-3 py-2.5 @5xl:hidden">
      <div className="flex gap-2 overflow-x-auto">
        <button type="button" aria-pressed={sought.tier === undefined} className={CHIP} onClick={() => seek(sought, { tier: undefined })}>
          {t(keys.filter.all, "All")}
        </button>
        {TIERS.map((tier) => (
          <button key={tier} type="button" aria-pressed={sought.tier === tier} className={CHIP} onClick={() => seek(sought, { tier: sought.tier === tier ? undefined : tier })}>
            <span aria-hidden className={cn("size-2 rounded-full", TIER_LOOK[tier].dot)} />
            {TIER_NAME[tier](t)}
            <Count n={n?.[tier] ?? 0} />
          </button>
        ))}
      </div>
      <div className="flex items-center gap-3">
        <button type="button" aria-expanded={more} className={CHIP} onClick={() => setMore(!more)}>
          <SlidersHorizontal aria-hidden className="size-4" />
          {t(keys.filters, "Filters")}
        </button>
        <label className="flex items-center gap-2 text-sm text-ink-soft">
          {t(keys.unreadOnly, "Unread only")}
          <Switch checked={sought.unread} onCheckedChange={(unread) => seek(sought, { unread })} />
        </label>
      </div>
      {more && (
        <ToggleGroup type="single" size="sm" variant="outline" value={sought.family ?? ""} onValueChange={(v) => seek(sought, { family: v === "" ? undefined : (v as Family) })} aria-label={t(keys.about, "About")} className="flex-wrap justify-start">
          {FAMILIES.map((family) => (
            <ToggleGroupItem key={family} value={family}>
              {FAMILY_NAME[family](t)}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
      )}
    </div>
  );
}

interface Query {
  unread: boolean;
  tier?: Tier;
  family?: Family;
  gmbs?: string;
}

/** The list, anew for each way it is narrowed; the outline's scope narrows it by the GBPs in it. */
function Narrowed({ sought }: { sought: Sought }) {
  const scope = useScope();
  const gmails = useGmails();
  const scoped = scope.gmails !== null || scope.categories !== null || scope.brands !== null;
  if (scoped && gmails.status !== "ok") return <Skeleton className="m-3 h-60" />;
  const gmbs = scoped && gmails.status === "ok" ? gmails.data.gmbs.filter((b) => inScope(b, scope)).map((b) => b.id) : null;
  const query: Query = { unread: sought.unread, ...(sought.tier !== undefined && { tier: sought.tier }), ...(sought.family !== undefined && { family: sought.family }), ...(gmbs !== null && { gmbs: gmbs.join(",") }) };
  return <List key={JSON.stringify(query)} sought={sought} query={query} />;
}

/** The server's page, the older ones after it. */
function List({ sought, query }: { sought: Sought; query: Query }) {
  const t = useT();
  const locale = useLocale();
  const first = useResource(api, "/alerts/inbox", { params: { query } });
  const [older, setOlder] = useState<readonly Alert[]>([]);
  const [over, setOver] = useState(false);
  const [busy, setBusy] = useState(false);
  if (first.status === "loading") return <Skeleton className="m-3 h-60" />;
  if (first.status === "error")
    return (
      <div className="p-4">
        <ErrorState failure={first.failure} onRetry={first.reload} />
      </div>
    );
  const head = first.data.alerts;
  const all = [...head, ...older.filter((o) => !head.some((h) => h.id === o.id))];
  const last = all.at(-1);
  const load = async () => {
    if (last === undefined) return;
    setBusy(true);
    try {
      const page = await ok(api.GET("/alerts/inbox", { params: { query: { ...query, before: last.id } } }));
      setOlder((was) => [...was, ...page.alerts]);
      if (page.alerts.length < PAGE) setOver(true);
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
    }
  };
  const days = byDay(all, locale, t(keys.today, "Today"), t(keys.yesterday, "Yesterday"));
  return (
    <div className="min-h-0 flex-1 overflow-y-auto p-1.5" onKeyDown={step}>
      {all.length === 0 && <p className="px-3 py-10 text-center text-sm text-ink-soft">{sought.unread ? t(keys.nothing, "Nothing unread") : t(keys.none, "No alerts here")}</p>}
      {days.map(({ day, alerts }) => (
        <section key={day}>
          <SectionLabel as="h2" className="px-2.5 pt-3 pb-1.5">
            {day}
          </SectionLabel>
          <div className="flex flex-col gap-0.5">
            {alerts.map((a) => (
              <AlertRow key={a.id} alert={a} within={sought} current={a.id === sought.open} />
            ))}
          </div>
        </section>
      ))}
      {!over && head.length === PAGE && (
        <div className="flex justify-center py-3">
          <Button type="button" variant="ghost" size="sm" disabled={busy} onClick={() => void load()}>
            {t(keys.older, "Older")}
            <ChevronRight aria-hidden className="size-4 rotate-90" />
          </Button>
        </div>
      )}
    </div>
  );
}

/** The server's page size: a page shorter than it is the last. */
const PAGE = 50;

function byDay(alerts: readonly Alert[], locale: string, today: string, yesterday: string): { day: string; alerts: Alert[] }[] {
  const now = new Date();
  const before = new Date(now.getFullYear(), now.getMonth(), now.getDate() - 1).toDateString();
  const name = (d: Date) => (d.toDateString() === now.toDateString() ? today : d.toDateString() === before ? yesterday : d.toLocaleDateString(locale, { weekday: "long", month: "short", day: "numeric" }));
  const days: { day: string; alerts: Alert[] }[] = [];
  for (const a of alerts) {
    const day = name(new Date(a.at));
    const into = days.find((d) => d.day === day);
    if (into === undefined) days.push({ day, alerts: [a] });
    else into.alerts.push(a);
  }
  return days;
}

/** Arrow keys go from row to row. */
function step(e: KeyboardEvent<HTMLElement>) {
  if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
  const rows = [...e.currentTarget.querySelectorAll<HTMLElement>("[data-alert] a")];
  const here = rows.indexOf(document.activeElement as HTMLElement);
  if (here === -1) return;
  e.preventDefault();
  rows[here + (e.key === "ArrowDown" ? 1 : -1)]?.focus();
}

function Pick() {
  const t = useT();
  return <p className="px-4 py-16 text-center text-sm text-ink-soft">{t(keys.pick, "Pick an alert to read it here.")}</p>;
}
