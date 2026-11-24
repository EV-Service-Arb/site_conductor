"use client";

import { Button, Select, SelectContent, SelectItem, SelectTrigger, SelectValue, Skeleton, Switch, cn, toast } from "@evinvest/uikit";
import { ArrowLeft, BellOff, CalendarClock, ChevronRight, Globe, Info, Moon, Undo2 } from "lucide-react";
import { type ReactNode, useEffect, useId, useRef, useState } from "react";

import { FAMILIES, FAMILY_ICON, FAMILY_NAME, RULE_HINT, RULE_NAME, TIERS, TIER_LOOK, TIER_NAME, VARIES, alerts as keys, clock, late, nextAt, quietOf, setHours, unreached, useAlerts, useNow } from "@/entities/alerts";
import { ConnectTelegram, PushHere } from "@/features/add-destination";
import { api, ok } from "@/shared/api";
import type { AlertHours, AlertRule, Alerts, Tier } from "@/shared/api/schema";
import { useLocale, useT } from "@/shared/i18n";
import { go, useAt } from "@/shared/lib/location";
import { DESKTOP_QUERY, useMediaQuery } from "@/shared/lib/use-media-query";
import { ErrorState } from "@/shared/ui/error-state";
import { notifyFailure } from "@/shared/ui/notify";
import { useControlSize } from "@/shared/ui/touch";

import { Matrix } from "./matrix";

export type Act = (write: () => Promise<unknown>) => Promise<void>;

/** Where the caller's alerts go, how soon, and which count as what. */
export function AlertSettings({ page }: { page: boolean }) {
  const t = useT();
  const alerts = useAlerts();
  const [busy, setBusy] = useState(false);
  const act: Act = async (write) => {
    setBusy(true);
    try {
      await write();
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
      alerts.reload();
    }
  };
  useConfirm(alerts.reload);
  const phone = !useMediaQuery(DESKTOP_QUERY);
  // a phone shows the matrix, the rest a press away
  const [open, setPart] = useState<"hours" | "rules" | null>(null);
  const part = phone ? open : null;
  const hours = alerts.status === "ok" ? alerts.data.hours : null;
  const quiet = hours === null ? null : quietOf(hours);
  const changed = alerts.status === "ok" ? alerts.data.rules.filter((r) => r.tier !== null).length : 0;
  return (
    <div className={cn(page && "mx-auto w-full max-w-300")}>
      <div className="@container">
        <div className={cn("flex w-full flex-col gap-6 px-4", page ? "py-6 md:px-8" : "py-4")}>
          {alerts.status === "loading" ? (
            <Skeleton className="h-60 w-full" />
          ) : alerts.status === "error" ? (
            <ErrorState failure={alerts.failure} onRetry={alerts.reload} />
          ) : (
            <>
              {part === null && <Reached alerts={alerts.data} />}
              {part !== null && (
                <Button type="button" variant="ghost" size="touch" className="-mx-2 justify-self-start self-start" onClick={() => setPart(null)}>
                  <ArrowLeft aria-hidden className="size-5" />
                  {t(keys.settings.all, "All settings")}
                </Button>
              )}
              {!phone || part === "hours" ? <HoursBar hours={alerts.data.hours} busy={busy} act={act} /> : part === null && <Into title={quiet === null ? t(keys.quiet.none, "No quiet hours") : t(keys.quiet.range, "Quiet {from} to {to}", { from: clock(quiet[0]), to: clock(quiet[1]) })} what={t(keys.quiet.daily, "daily at {time} · {zone}", { time: clock(hours?.daily ?? 9 * 60), zone: hours?.tz ?? "UTC" })} onClick={() => setPart("hours")} />}
              {(!phone || part === null) && (
                <section className="space-y-3">
                  <Heading title={t(keys.matrix, "Where and when")} what={t(keys.matrix.what, "Everything is always in the panel. Pick what each place tells you, and how soon.")} />
                  <Matrix alerts={alerts.data} busy={busy} act={act} />
                </section>
              )}
              {!phone || part === "rules" ? (
                <section className="space-y-3">
                  <Heading title={t(keys.rules, "What counts as what")} what={t(keys.rules.what, "Change an alert's tier; it moves column everywhere at once.")} />
                  <Rules rules={alerts.data.rules} busy={busy} act={act} />
                </section>
              ) : (
                part === null && <Into title={t(keys.rules, "What counts as what")} what={changed === 0 ? t(keys.rules.usual, "each at its usual tier") : t(keys.rules.changed, "{n} changed", { n: changed })} onClick={() => setPart("rules")} />
              )}
            </>
          )}
        </div>
      </div>
    </div>
  );
}

/** A mailed link's token proves its address, once; the token leaves the location either way. */
function useConfirm(reload: () => void) {
  const t = useT();
  const at = useAt();
  const token = at.view === "alert_settings" ? at.params.confirm : undefined;
  // a token is good once: an effect run again must not spend it again
  const spent = useRef<string | null>(null);
  useEffect(() => {
    if (token === undefined || spent.current === token) return;
    spent.current = token;
    go({ view: "alert_settings", params: {} }, "replace");
    ok(api.POST("/alerts/email/confirm", { body: { token } })).then(
      () => {
        toast.positive(t(keys.add.email.confirmed, "The address is confirmed: alerts go there now."));
        reload();
      },
      (e: unknown) => notifyFailure(e, t),
    );
  }, [token, reload, t]);
}

function Into({ title, what, onClick }: { title: string; what: string; onClick: () => void }) {
  return (
    <button type="button" onClick={onClick} className="flex w-full items-center gap-3 rounded-lg border border-border bg-card px-4 py-3 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring">
      <span className="min-w-0 flex-1">
        <span className="block text-sm font-medium text-ink">{title}</span>
        <span className="block text-xs text-ink-soft">{what}</span>
      </span>
      <ChevronRight aria-hidden className="size-4 text-ink-soft" />
    </button>
  );
}

function Heading({ title, what }: { title: string; what: string }) {
  return (
    <div className="flex flex-wrap items-baseline gap-x-3">
      <h2 className="text-base font-semibold text-ink">{title}</h2>
      <p className="text-sm text-ink-soft">{what}</p>
    </div>
  );
}

/** Until an urgent one reaches the caller at once: how it would, and the two presses that fix it. */
function Reached({ alerts }: { alerts: Alerts }) {
  const t = useT();
  const nowhere = unreached(alerts);
  if (!nowhere && !late(alerts)) return null;
  return (
    <div className="flex flex-wrap items-center gap-3 rounded-lg border border-tint-warn-edge bg-tint-warn-wash px-4 py-3">
      <Info aria-hidden className="size-4 shrink-0 text-accent-warn" />
      <div className="min-w-0 flex-1 basis-60">
        <p className="text-sm font-medium text-ink">{nowhere ? t(keys.nowhere, "Nothing reaches you when the panel is closed") : t(keys.late, "An urgent alert would reach you late")}</p>
        <p className="text-xs text-ink-soft">{nowhere ? t(keys.nowhere.what, "An urgent lead would wait until you open it.") : t(keys.late.what, "Telegram or push tells you at once.")}</p>
      </div>
      <div className="flex flex-wrap gap-2">
        <ConnectTelegram />
        <PushHere />
      </div>
    </div>
  );
}

/** Every half hour of a day. */
const HALF_HOURS = Array.from({ length: 48 }, (_, i) => i * 30);

function TimeSelect({ value, label, disabled, onChange }: { value: number; label: string; disabled: boolean; onChange: (minutes: number) => void }) {
  const size = useControlSize();
  // one set elsewhere, off the half hours
  const at = HALF_HOURS.includes(value) ? HALF_HOURS : [...HALF_HOURS, value].sort((a, b) => a - b);
  return (
    <Select value={String(value)} onValueChange={(v) => onChange(Number(v))}>
      <SelectTrigger size={size} className="w-30 font-mono tabular-nums" disabled={disabled} aria-label={label}>
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {at.map((m) => (
          <SelectItem key={m} value={String(m)}>
            {clock(m)}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}

function Hour({ icon: Icon, title, control, children, hint }: { icon: typeof Moon; title: string; control?: ReactNode; children: ReactNode; hint: string }) {
  return (
    <div className="flex flex-col gap-2 p-4">
      <div className="flex min-h-6 items-center gap-2 text-sm font-medium text-ink">
        <Icon aria-hidden className="size-4 text-ink-soft" />
        <span className="flex-1">{title}</span>
        {control}
      </div>
      <div className="flex flex-wrap items-center gap-2">{children}</div>
      <p className="text-xs text-ink-soft">{hint}</p>
    </div>
  );
}

const NIGHT: [number, number] = [22 * 60, 7 * 60];
const EVENING = 18 * 60;
const MORNING = 7 * 60;

function HoursBar({ hours: set, busy, act }: { hours: AlertHours | null; busy: boolean; act: Act }) {
  const t = useT();
  const locale = useLocale();
  const now = useNow(30_000);
  const size = useControlSize();
  const zone = useId();
  // until this page's first visit sets them: what goes out by then
  const hours = set ?? { tz: "UTC", daily: 9 * 60, quiet: null, snoozed_until: null };
  const change = (c: Partial<AlertHours>) => void act(() => setHours(set, c));
  const quiet = quietOf(hours);
  const here = Intl.DateTimeFormat().resolvedOptions().timeZone;
  const zones = [...new Set([hours.tz, here, ...Intl.supportedValuesOf("timeZone")])];
  const until = hours.snoozed_until === null || new Date(hours.snoozed_until) <= now ? null : new Date(hours.snoozed_until);
  const time = (d: Date) => d.toLocaleString(locale, { timeZone: hours.tz, hour: "2-digit", minute: "2-digit", ...(d.getTime() - now.getTime() > 12 * 3_600_000 && { weekday: "short" }) });
  const snoozes: { id: string; label: string; at: Date | null }[] = [
    { id: "off", label: t(keys.snooze.off, "Off"), at: null },
    { id: "hour", label: t(keys.snooze.oneHour, "1 h"), at: new Date(now.getTime() + 3_600_000) },
    { id: "evening", label: t(keys.snooze.evening, "Until this evening"), at: nextAt(hours.tz, EVENING, now) },
    { id: "tomorrow", label: t(keys.snooze.tomorrow, "Until tomorrow"), at: nextAt(hours.tz, quiet?.[1] ?? MORNING, now) },
  ];
  return (
    <section aria-label={t(keys.hours, "Your hours")} className="grid divide-border rounded-lg border border-border bg-card @max-4xl:divide-y @2xl:grid-cols-2 @4xl:grid-cols-4 @4xl:divide-x">
      <Hour
        icon={Moon}
        title={t(keys.quiet, "Quiet hours")}
        control={<Switch aria-label={t(keys.quiet, "Quiet hours")} checked={quiet !== null} disabled={busy} onCheckedChange={(on) => change({ quiet: on ? NIGHT : null })} />}
        hint={quiet === null ? t(keys.quiet.off, "Off: alerts come at any hour.") : t(keys.quiet.hint, "Urgent still comes. The rest arrives at {time} as one message.", { time: clock(quiet[1]) })}
      >
        {quiet !== null && (
          <>
            <TimeSelect value={quiet[0]} label={t(keys.quiet.from, "Quiet from")} disabled={busy} onChange={(from) => from !== quiet[1] && change({ quiet: [from, quiet[1]] })} />
            <span className="text-sm text-ink-soft">{t(keys.quiet.to, "to")}</span>
            <TimeSelect value={quiet[1]} label={t(keys.quiet.till, "Quiet until")} disabled={busy} onChange={(to) => to !== quiet[0] && change({ quiet: [quiet[0], to] })} />
          </>
        )}
      </Hour>
      <Hour icon={BellOff} title={t(keys.snooze, "Snooze")} hint={until === null ? t(keys.snooze.hint, "Urgent still comes.") : t(keys.snooze.until, "Until {time}. Urgent still comes.", { time: time(until) })}>
        <Select
          value={until === null ? "off" : "on"}
          onValueChange={(v) => {
            const at = snoozes.find((s) => s.id === v);
            if (at !== undefined) change({ snoozed_until: at.at?.toISOString() ?? null });
          }}
        >
          <SelectTrigger size={size} className="w-full" disabled={busy} aria-label={t(keys.snooze, "Snooze")}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {until !== null && <SelectItem value="on">{t(keys.snooze.on, "Until {time}", { time: time(until) })}</SelectItem>}
            {snoozes.map((s) => (
              <SelectItem key={s.id} value={s.id} textValue={s.label}>
                {s.label}
                {s.at !== null && <span className="ml-2 text-xs text-ink-soft">{time(s.at)}</span>}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </Hour>
      <Hour icon={CalendarClock} title={t(keys.daily, "Daily message at")} hint={t(keys.daily.hint, "What every Daily cell sends, once.")}>
        <TimeSelect value={hours.daily} label={t(keys.daily, "Daily message at")} disabled={busy} onChange={(daily) => change({ daily })} />
      </Hour>
      <Hour icon={Globe} title={t(keys.tz, "Time zone")} hint={t(keys.tz.now, "{time} there now.", { time: now.toLocaleTimeString(locale, { timeZone: hours.tz, hour: "2-digit", minute: "2-digit" }) })}>
        <input
          list={zone}
          defaultValue={hours.tz}
          key={hours.tz}
          disabled={busy}
          aria-label={t(keys.tz, "Time zone")}
          onChange={(e) => zones.includes(e.target.value) && e.target.value !== hours.tz && change({ tz: e.target.value })}
          className="h-9 w-full rounded-md border border-border bg-secondary px-3 text-sm text-ink outline-none focus-visible:ring-2 focus-visible:ring-ring max-md:h-12"
        />
        <datalist id={zone}>
          {zones.map((z) => (
            <option key={z} value={z} />
          ))}
        </datalist>
      </Hour>
    </section>
  );
}

const AS_IT_COMES = "as_it_comes";

function Rules({ rules, busy, act }: { rules: readonly AlertRule[]; busy: boolean; act: Act }) {
  const t = useT();
  if (rules.length === 0) return <p className="text-sm text-ink-soft">{t(keys.rules.none, "Your access has nothing to alert you of.")}</p>;
  return (
    <>
      <div className="gap-4 @3xl:columns-2">
        {FAMILIES.map((family) => {
          const of = rules.filter((r) => r.family === family);
          if (of.length === 0) return null;
          const Icon = FAMILY_ICON[family];
          return (
            <section key={family} className="mb-4 break-inside-avoid rounded-lg border border-border bg-card">
              <h3 className="flex items-center gap-2 px-4 py-2.5 text-sm font-medium text-ink">
                <Icon aria-hidden className="size-4 text-ink-soft" />
                {FAMILY_NAME[family](t)}
              </h3>
              {of.map((r) => (
                <RuleRow key={r.rule} rule={r} busy={busy} act={act} />
              ))}
            </section>
          );
        })}
      </div>
      <p className="text-xs text-ink-soft">{t(keys.rules.access, "Only what your access can hear of is listed.")}</p>
    </>
  );
}

function RuleRow({ rule: r, busy, act }: { rule: AlertRule; busy: boolean; act: Act }) {
  const t = useT();
  const id = useId();
  const size = useControlSize();
  const varies = VARIES[r.rule];
  const hint = varies ?? RULE_HINT[r.rule];
  const changed = r.tier !== null;
  const usual = varies === undefined ? TIER_NAME[r.default](t) : t(keys.asItComes, "As it comes");
  const set = (tier: Tier | null) => void act(() => ok(api.PUT("/alerts/rules/{rule}", { params: { path: { rule: r.rule } }, body: { tier } })));
  const value = r.tier ?? (varies === undefined ? r.default : AS_IT_COMES);
  return (
    <div className="flex items-center gap-3 border-t border-border px-4 py-2">
      <label htmlFor={id} className="min-w-0 flex-1">
        <span className="block text-sm text-ink">{RULE_NAME[r.rule](t)}</span>
        {hint !== undefined && <span className="block text-xs text-ink-soft">{hint(t)}</span>}
      </label>
      {changed ? (
        <button type="button" disabled={busy} onClick={() => set(null)} className="inline-flex items-center gap-1 rounded-sm text-xs text-ink-soft outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-ring">
          <Undo2 aria-hidden className="size-3" />
          <span className="sr-only">{t(keys.rules.reset, "Back to")}</span>
          {usual}
        </button>
      ) : (
        <span className="text-xs text-ink-faint">{t(keys.rules.default, "default")}</span>
      )}
      <Select value={value} onValueChange={(v) => set(v === AS_IT_COMES || (varies === undefined && v === r.default) ? null : (v as Tier))}>
        <SelectTrigger id={id} size={size} className="w-36" disabled={busy}>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {varies !== undefined && <SelectItem value={AS_IT_COMES}>{t(keys.asItComes, "As it comes")}</SelectItem>}
          {TIERS.map((tier) => (
            <SelectItem key={tier} value={tier} textValue={TIER_NAME[tier](t)}>
              <span className="inline-flex items-center gap-2">
                <span aria-hidden className={cn("size-2 rounded-full", TIER_LOOK[tier].dot)} />
                {TIER_NAME[tier](t)}
              </span>
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  );
}
