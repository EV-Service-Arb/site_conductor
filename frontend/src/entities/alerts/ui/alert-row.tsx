"use client";

import { Button, Kbd, cn, toast } from "@evinvest/uikit";
import { Phone } from "lucide-react";
import { useState } from "react";

import { type AlertsAt, type WinKey, openWindow, pageOf } from "@/entities/gmb";
import type { Alert } from "@/shared/api/schema";
import { useLocale, useT } from "@/shared/i18n";
import { ViewLink } from "@/shared/lib/location";
import { age } from "@/shared/lib/time";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize } from "@/shared/ui/touch";

import { pressAlert } from "../api/alerts";
import { useAlerts, useNamedGmbs, useNow, useUnread } from "../model/context";
import { alerts as keys } from "../model/i18n";
import { FAMILY_ICON, FAMILY_NAME, TIER_LOOK, TIER_NAME } from "../model/names";
import { lines } from "../model/text";

/** The family's icon in the tier's colour: what it is and how much it matters, in one mark. */
export function AlertMark({ alert, className }: { alert: Alert; className?: string }) {
  const t = useT();
  const Icon = FAMILY_ICON[alert.family];
  return (
    <span className={cn("flex size-7 shrink-0 items-center justify-center rounded-full", TIER_LOOK[alert.tier].wash, className)}>
      <Icon className="size-3.5" aria-hidden />
      <span className="sr-only">
        {TIER_NAME[alert.tier](t)}, {FAMILY_NAME[alert.family](t)}
      </span>
    </span>
  );
}

/** "now", "14:06", "Nov 12": as far as it is told apart by. */
export function When({ at }: { at: string }) {
  const locale = useLocale();
  const now = useNow(60_000);
  const then = new Date(at);
  const minutes = Math.floor((now.getTime() - then.getTime()) / 60_000);
  const today = then.toDateString() === now.toDateString();
  return (
    <time dateTime={at} title={then.toLocaleString(locale)} className="shrink-0 font-mono text-xs text-ink-soft tabular-nums">
      {minutes < 60 ? age(at, locale) : today ? then.toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit" }) : then.toLocaleDateString(locale, { month: "short", day: "numeric" })}
    </time>
  );
}

/** Presses an alert's buttons, one at a time. */
export function usePress(alert: Alert, onPressed?: () => void): { busy: boolean; press: (button: string) => Promise<void> } {
  const t = useT();
  const unread = useUnread();
  const [busy, setBusy] = useState(false);
  const press = async (button: string) => {
    setBusy(true);
    try {
      await pressAlert(alert.id, button);
      toast.positive(t(keys.pressed, "Recorded"));
      onPressed?.();
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
      unread.reload();
    }
  };
  return { busy, press };
}

/** What an alert asks to be done, as its message's buttons: the first is the one to press. `keycaps`: what presses each, where the page binds them. */
export function AlertButtons({ alert, className, onPressed, keycaps = [] }: { alert: Alert; className?: string; onPressed?: () => void; keycaps?: readonly (string | undefined)[] }) {
  const size = useButtonSize();
  const { busy, press } = usePress(alert, onPressed);
  if (alert.buttons.length === 0) return null;
  return (
    <div className={cn("flex flex-wrap gap-2", className)}>
      {alert.buttons.map((b, i) => (
        <Button key={b.id} type="button" size={size("sm")} variant={i === 0 ? (alert.tier === "urgent" ? "destructive" : "primary") : "outline"} disabled={busy} onClick={() => void press(b.id)}>
          {i === 0 && alert.family === "leads" && <Phone aria-hidden className="size-4" />}
          {b.label}
          {keycaps[i] !== undefined && <Kbd className="max-md:hidden">{keycaps[i]}</Kbd>}
        </Button>
      ))}
    </div>
  );
}

/** One alert in a list, `within` the inbox as it is narrowed: a link that opens it, its buttons beside. */
export function AlertRow({ alert, within = {}, current = false, buttons = true, onOpen }: { alert: Alert; within?: Exclude<AlertsAt, { settings: true }>; current?: boolean; buttons?: boolean; onOpen?: () => void }) {
  const t = useT();
  const settings = useAlerts();
  const { title, sub } = lines(alert, useNamedGmbs());
  const read = alert.read_at !== null;
  const opened: WinKey = { kind: "alerts", at: { ...within, open: alert.id } };
  return (
    <div data-alert={alert.id} className={cn("relative rounded-md px-2.5 py-2 hover:bg-hover/50", alert.tier === "urgent" && !read && "bg-tint-error-wash hover:bg-tint-error-fill", current && "bg-hover")}>
      <div className="flex gap-2.5">
        <AlertMark alert={alert} />
        <div className="min-w-0 flex-1">
          <ViewLink
            to={pageOf(opened)}
            open={() => openWindow(opened, current || within.open !== undefined ? "replace" : "push")}
            {...(onOpen && { onClick: onOpen })}
            aria-current={current || undefined}
            className={cn("block truncate text-sm outline-none after:absolute after:inset-0 after:rounded-md focus-visible:after:ring-2 focus-visible:after:ring-ring", read ? "text-ink-mid" : "font-medium text-ink")}
          >
            {title}
          </ViewLink>
          {sub !== "" && <p className="truncate text-xs text-ink-soft">{sub}</p>}
        </div>
        <span className="flex shrink-0 flex-col items-end gap-1.5">
          <When at={alert.at} />
          {!read && (
            <span className={cn("size-1.5 rounded-full", TIER_LOOK[alert.tier].dot)}>
              <span className="sr-only">{t(keys.unread, "unread")}</span>
            </span>
          )}
        </span>
      </div>
      {buttons && !read && alert.buttons.length > 0 && (
        // above the row's stretched link
        <div className="relative z-10 mt-2 pl-9.5">
          <AlertButtons alert={alert} onPressed={settings.reload} />
        </div>
      )}
    </div>
  );
}

/** A tier's dot and name. */
export function TierLabel({ tier, className }: { tier: Alert["tier"]; className?: string }) {
  const t = useT();
  return (
    <span className={cn("inline-flex items-center gap-2", TIER_LOOK[tier].ink, className)}>
      <span aria-hidden className={cn("size-2 rounded-full", TIER_LOOK[tier].dot)} />
      {TIER_NAME[tier](t)}
    </span>
  );
}
