"use client";

import { Button, NavDot, Popover, PopoverContent, PopoverTrigger, Sheet, SheetContent, SheetHeader, SheetTitle, SheetTrigger, cn } from "@evinvest/uikit";
import { Bell, BellOff, Check, CheckCheck, ChevronRight, Inbox, Moon, Settings, Unplug } from "lucide-react";
import { type ReactNode, useState } from "react";

import { AlertRow, TIERS, TIER_LOOK, TIER_NAME, alerts as keys, readAll, setHours, unreached, useAlerts, useQuiet, useUnread } from "@/entities/alerts";
import { ConnectTelegram, PushHere } from "@/features/add-destination";
import type { AlertInbox, Tier } from "@/shared/api/schema";
import { useT } from "@/shared/i18n";
import { ViewLink } from "@/shared/lib/location";
import { DESKTOP_QUERY, useMediaQuery } from "@/shared/lib/use-media-query";
import { ErrorState } from "@/shared/ui/error-state";
import { notifyFailure } from "@/shared/ui/notify";

/** Rows a tier shows here before its heading only counts them. */
const SHOWN: Record<Tier, number> = { urgent: 5, important: 4, info: 3 };

/** The bell: what is unread, most pressing first; its badge counts what asks something of the caller. */
export function AlertBell() {
  const t = useT();
  const desktop = useMediaQuery(DESKTOP_QUERY);
  const [open, setOpen] = useState(false);
  const alerts = useAlerts();
  const unread = useUnread();
  const quiet = useQuiet();
  const n = unread.status === "ok" ? unread.data.unread : { urgent: 0, important: 0, info: 0 };
  const badge = quiet === null ? n.urgent + n.important : n.urgent;
  const nowhere = alerts.status === "ok" && unreached(alerts.data);
  const label = badge > 0 ? t(keys.bell.count, "Alerts, {n} waiting", { n: badge }) : t(keys.title, "Alerts");
  const face = (
    <Button type="button" variant="ghost" size="sm" icon className="relative" aria-label={label}>
      <Bell aria-hidden />
      {badge > 0 ? (
        <span aria-hidden className={cn("absolute -top-0.5 -right-1 flex h-4 min-w-4 items-center justify-center rounded-full px-1 text-xs leading-none font-semibold tabular-nums", n.urgent > 0 ? "bg-accent-error text-on-accent-error" : "bg-accent-warn text-on-accent-warn")}>
          {badge > 99 ? "99+" : badge}
        </span>
      ) : quiet !== null ? (
        <Moon aria-hidden className="absolute -right-0.5 bottom-0.5 size-3! fill-ink-soft text-ink-soft" />
      ) : (
        nowhere && <NavDot corner tone="warn" label={t(keys.nowhere, "Nothing reaches you when the panel is closed")} />
      )}
    </Button>
  );
  const close = () => setOpen(false);
  if (desktop)
    return (
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>{face}</PopoverTrigger>
        <PopoverContent align="end" initialFocus="container" className="flex max-h-[min(44rem,85svh)] w-110 flex-col p-0">
          <Body onClose={close} title={<h2 className="text-base font-semibold text-ink">{t(keys.title, "Alerts")}</h2>} />
        </PopoverContent>
      </Popover>
    );
  return (
    <Sheet open={open} onOpenChange={setOpen}>
      <SheetTrigger asChild>{face}</SheetTrigger>
      <SheetContent side="bottom" className="max-h-[85svh] gap-0 rounded-t-xl p-0">
        <Body
          onClose={close}
          title={
            <SheetHeader className="p-0">
              <SheetTitle>{t(keys.title, "Alerts")}</SheetTitle>
            </SheetHeader>
          }
        />
      </SheetContent>
    </Sheet>
  );
}

function Body({ title, onClose }: { title: ReactNode; onClose: () => void }) {
  const t = useT();
  const unread = useUnread();
  const [busy, setBusy] = useState(false);
  const act = async (write: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await write();
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
      unread.reload();
    }
  };
  const inbox = unread.status === "ok" ? unread.data : null;
  const total = inbox === null ? 0 : inbox.unread.urgent + inbox.unread.important + inbox.unread.info;
  const newest = inbox?.alerts[0]?.id;
  return (
    <>
      <div className="flex items-center gap-2 border-b border-border px-4 py-3 max-md:pr-12">
        {title}
        {total > 0 && <span className="text-sm text-ink-soft">{t(keys.unreadCount, "{n} unread", { n: total })}</span>}
        <span className="flex-1" />
        {newest !== undefined && (
          <Button type="button" variant="ghost" size="sm" disabled={busy} onClick={() => void act(() => readAll(newest))}>
            <CheckCheck aria-hidden className="size-4" />
            {t(keys.readAll, "Mark all read")}
          </Button>
        )}
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto">
        <Standing busy={busy} act={act} onClose={onClose} />
        {unread.status === "error" ? (
          <div className="p-4">
            <ErrorState failure={unread.failure} onRetry={unread.reload} />
          </div>
        ) : inbox !== null && (total === 0 ? <Nothing /> : <Groups inbox={inbox} onOpen={onClose} />)}
      </div>
      <Foot busy={busy} act={act} onClose={onClose} />
    </>
  );
}

type Act = (write: () => Promise<unknown>) => Promise<void>;

/** Quiet, or reached nowhere: what stands between an alert and its person. */
function Standing({ busy, act, onClose }: { busy: boolean; act: Act; onClose: () => void }) {
  const t = useT();
  const alerts = useAlerts();
  const quiet = useQuiet();
  if (alerts.status !== "ok") return null;
  if (quiet !== null)
    return (
      <div className="flex gap-3 bg-secondary px-4 py-3">
        <Moon aria-hidden className="mt-0.5 size-4 shrink-0 text-ink-soft" />
        <div className="min-w-0 space-y-1.5">
          <p className="text-sm font-medium text-ink">{t(keys.quiet.until, "Quiet until {time}", { time: quiet.until })}</p>
          <p className="text-xs text-ink-soft">{t(keys.quiet.what, "Urgent still comes through. The rest is sent as one message when it ends.")}</p>
          {quiet.by === "snooze" && (
            <Button type="button" variant="outline" size="sm" disabled={busy} onClick={() => void act(() => setHours(alerts.data.hours, { snoozed_until: null }).then(alerts.reload))}>
              {t(keys.quiet.end, "End now")}
            </Button>
          )}
        </div>
      </div>
    );
  if (!unreached(alerts.data)) return null;
  return (
    <div className="flex gap-3 bg-tint-warn-wash px-4 py-3">
      <Unplug aria-hidden className="mt-0.5 size-4 shrink-0 text-accent-warn" />
      <div className="min-w-0 space-y-1.5">
        <p className="text-sm font-medium text-ink">{t(keys.nowhere, "Nothing reaches you when the panel is closed")}</p>
        <p className="text-xs text-ink-soft">{t(keys.nowhere.what, "An urgent lead would wait until you open it.")}</p>
        <div className="flex flex-wrap gap-2">
          <ConnectTelegram onDone={onClose} />
          <PushHere onDone={onClose} />
        </div>
      </div>
    </div>
  );
}

function Nothing() {
  const t = useT();
  return (
    <div className="flex flex-col items-center gap-2 px-4 py-12 text-center">
      <Check aria-hidden className="size-5 text-positive" />
      <p className="text-sm font-medium text-ink">{t(keys.nothing, "Nothing unread")}</p>
    </div>
  );
}

function Groups({ inbox, onOpen }: { inbox: AlertInbox; onOpen: () => void }) {
  const t = useT();
  return (
    <div className="flex flex-col gap-1 p-1.5">
      {TIERS.map((tier) => {
        const of = inbox.alerts.filter((a) => a.tier === tier);
        const n = inbox.unread[tier];
        if (n === 0) return null;
        return (
          <section key={tier} aria-label={TIER_NAME[tier](t)}>
            <ViewLink
              to={{ view: "alerts", params: { tier, unread: true } }}
              onClick={onOpen}
              className="flex items-center gap-2 rounded-md px-2.5 py-1.5 text-sm outline-none hover:bg-hover/50 focus-visible:ring-2 focus-visible:ring-ring"
            >
              <span aria-hidden className={cn("size-2 rounded-full", TIER_LOOK[tier].dot)} />
              <span className={TIER_LOOK[tier].ink}>{TIER_NAME[tier](t)}</span>
              <span className="flex-1 text-ink-soft tabular-nums">{n}</span>
              {n > SHOWN[tier] && <span className="text-xs text-ink-soft">{t(keys.all.n, "All {n}", { n })}</span>}
              <ChevronRight aria-hidden className="size-4 text-ink-soft" />
            </ViewLink>
            <div className="flex flex-col gap-0.5">
              {of.slice(0, SHOWN[tier]).map((a) => (
                <AlertRow key={a.id} alert={a} onOpen={onOpen} />
              ))}
            </div>
          </section>
        );
      })}
    </div>
  );
}

const FOOT = "inline-flex items-center gap-2 rounded-md px-2 py-1.5 text-sm text-ink outline-none hover:bg-hover focus-visible:ring-2 focus-visible:ring-ring max-md:min-h-11";

function Foot({ busy, act, onClose }: { busy: boolean; act: Act; onClose: () => void }) {
  const t = useT();
  const alerts = useAlerts();
  const quiet = useQuiet();
  const hours = alerts.status === "ok" ? alerts.data.hours : null;
  const snooze = () => void act(() => setHours(hours, { snoozed_until: new Date(Date.now() + 3_600_000).toISOString() }).then(alerts.reload));
  return (
    <div className="flex items-center gap-1 border-t border-border px-2 py-2">
      <ViewLink to={{ view: "alerts", params: {} }} onClick={onClose} className={FOOT}>
        <Inbox aria-hidden className="size-4" />
        {t(keys.all, "All alerts")}
      </ViewLink>
      <ViewLink to={{ view: "alert_settings", params: {} }} onClick={onClose} className={FOOT}>
        <Settings aria-hidden className="size-4" />
        {t(keys.settings, "Settings")}
      </ViewLink>
      <span className="flex-1" />
      {alerts.status === "ok" && quiet === null && (
        <button type="button" disabled={busy} onClick={snooze} className={FOOT}>
          <BellOff aria-hidden className="size-4" />
          {t(keys.snooze.hour, "Snooze 1 h")}
        </button>
      )}
    </div>
  );
}
