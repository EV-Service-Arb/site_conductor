"use client";

import { Badge, Button, Skeleton, ToggleGroup, ToggleGroupItem, type Tone, cn } from "@evinvest/uikit";
import { useResource } from "@evinvest/api/react";
import { ChevronRight, ExternalLink, Phone } from "lucide-react";
import { useEffect, useState } from "react";

import { AlertButtons, AlertMark, METHOD_ICON, METHOD_NAME, RULE_NAME, TIERS, TIER_LOOK, TIER_NAME, VARIES, alerts as keys, lines, phoneOf, photoSrc, readAlert, useAlerts, useNamedGmbs, useUnread } from "@/entities/alerts";
import { openWindow } from "@/entities/gmb";
import { api, ok } from "@/shared/api";
import type { Alert, Told, Tier } from "@/shared/api/schema";
import { type T, useLocale, useT } from "@/shared/i18n";
import { ViewLink } from "@/shared/lib/location";
import { formatDateTime } from "@/shared/lib/format";
import { ago } from "@/shared/lib/time";
import { ErrorState } from "@/shared/ui/error-state";
import { notifyFailure } from "@/shared/ui/notify";

/** One alert: what it says, what to do about it, where it went. Opening it reads it. */
export function AlertDetail({ id }: { id: number }) {
  const t = useT();
  const read = useResource(api, "/alerts/inbox/{id}", { params: { path: { id } } });
  const { reload } = useUnread();
  const unread = read.status === "ok" && read.data.alert.read_at === null;
  useEffect(() => {
    if (unread) readAlert(id).then(reload, (e: unknown) => notifyFailure(e, t));
  }, [unread, id, reload, t]);
  if (read.status === "loading") return <Skeleton className="m-5 h-60" />;
  if (read.status === "error")
    return (
      <div className="p-5">
        <ErrorState failure={read.failure} onRetry={read.reload} />
      </div>
    );
  const { alert, told } = read.data;
  return <Shown alert={alert} told={told} reload={read.reload} />;
}

function Shown({ alert, told, reload }: { alert: Alert; told: readonly Told[]; reload: () => void }) {
  const t = useT();
  const locale = useLocale();
  const gmbs = useNamedGmbs();
  const { title } = lines(alert, gmbs);
  const rest = alert.text.split("\n").filter((l) => l.trim() !== "").slice(1);
  const phone = phoneOf(alert.text);
  return (
    <article className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto px-4 py-5 @3xl:px-7">
      <header className="flex gap-3">
        <AlertMark alert={alert} className="size-10 @max-3xl:hidden" />
        <div className="min-w-0 space-y-1">
          <h2 className="text-lg font-semibold text-ink">{title}</h2>
          <p className="flex flex-wrap items-center gap-2 text-sm text-ink-soft">
            <Badge tone={TIER_TONE[alert.tier]}>{TIER_NAME[alert.tier](t)}</Badge>
            <time dateTime={alert.at}>{formatDateTime(alert.at, locale)}</time>
            <span>· {ago(alert.at, locale)}</span>
          </p>
        </div>
      </header>
      <div className="flex flex-col gap-4 @4xl:flex-row @4xl:items-start">
        <div className="min-w-0 flex-1 space-y-1.5">
          {rest.map((l, i) => (
            <p key={i} className="text-sm break-words text-ink">
              {l}
            </p>
          ))}
        </div>
        {alert.photo && (
          <a href={photoSrc(alert.id)} target="_blank" rel="noreferrer" className="shrink-0">
            {/* eslint-disable-next-line @next/next/no-img-element -- the session's own, behind the API */}
            <img src={photoSrc(alert.id)} alt={t(keys.photo, "The capture it came with")} className="max-h-80 rounded-md border border-border @4xl:max-w-60" />
          </a>
        )}
      </div>
      <div className="flex flex-wrap items-center gap-2">
        <AlertButtons alert={alert} onPressed={reload} />
        {phone !== null && (
          <Button asChild variant="outline" size="sm">
            <a href={`tel:${phone.replace(/[^\d+]/g, "")}`}>
              <Phone aria-hidden className="size-4" />
              {phone}
            </a>
          </Button>
        )}
        {alert.link !== null && (
          <Button asChild variant="outline" size="sm">
            <ViewLink href={alert.link}>
              <ChevronRight aria-hidden className="size-4" />
              {t(keys.open, "Open")}
            </ViewLink>
          </Button>
        )}
        {alert.links.map((l) => (
          <Button key={l.url} asChild variant="outline" size="sm">
            <a href={l.url} target="_blank" rel="noreferrer">
              <ExternalLink aria-hidden className="size-4" />
              {l.label}
            </a>
          </Button>
        ))}
      </div>
      {alert.gmbs.map((gmb) => {
        const of = gmbs.find((b) => b.id === gmb);
        return (
          of !== undefined && (
            <button key={gmb} type="button" onClick={() => openWindow({ kind: "gbp", gmb, tab: "profile" })} className="flex items-center gap-3 rounded-lg border border-border px-3 py-2.5 text-left outline-none hover:bg-hover/50 focus-visible:ring-2 focus-visible:ring-ring">
              <span className="min-w-0 flex-1">
                <span className="block truncate text-sm font-medium text-ink">{of.name}</span>
                {of.address !== null && <span className="block truncate text-xs text-ink-soft">{of.address}</span>}
              </span>
              <ChevronRight aria-hidden className="size-4 text-ink-soft" />
            </button>
          )
        );
      })}
      <Went told={told} />
      <Retier alert={alert} />
    </article>
  );
}

function outcome(told: Told, t: T, at: string): string {
  switch (told.state) {
    case "sent":
      return at;
    case "pending":
      return t(keys.told.pending, "due {at}", { at });
    case "dead":
      return t(keys.told.dead, "did not go out: {why}", { why: told.error ?? "" });
    case "dropped":
      return told.error === "read" ? t(keys.told.read, "dropped, you read it here") : t(keys.told.dropped, "dropped: {why}", { why: told.error ?? "" });
  }
}

/** Where it went, or will go. */
function Went({ told }: { told: readonly Told[] }) {
  const t = useT();
  const locale = useLocale();
  const alerts = useAlerts();
  if (told.length === 0 || alerts.status !== "ok") return null;
  return (
    <section className="space-y-2 border-t border-border pt-4">
      <h3 className="text-xs font-medium tracking-wide text-ink-soft uppercase">{t(keys.told, "Told")}</h3>
      <ul className="space-y-2">
        {told.map((w) => {
          const d = alerts.data.destinations.find((x) => x.id === w.destination_id);
          if (d === undefined) return null; // removed since
          const Icon = METHOD_ICON[d.method];
          return (
            <li key={w.destination_id} className="flex items-center gap-2.5 text-sm text-ink">
              <Icon className="size-4 shrink-0 text-ink-soft" />
              <span className="min-w-0 flex-1 truncate">{[METHOD_NAME[d.method](t), d.title].filter((s) => s !== null).join(" · ")}</span>
              <span className={cn("max-w-3/5 text-right text-xs", w.state === "dead" ? "text-accent-error" : "text-ink-soft")}>{outcome(w, t, formatDateTime(w.at, locale))}</span>
            </li>
          );
        })}
      </ul>
    </section>
  );
}

const TIER_TONE = { urgent: "error", important: "warn", info: "neutral" } as const satisfies Record<Tier, Tone>;

const AS_IT_COMES = "as_it_comes";

/** The rule's tier, from one of its alerts. */
function Retier({ alert }: { alert: Alert }) {
  const t = useT();
  const alerts = useAlerts();
  const [busy, setBusy] = useState(false);
  const rule = alerts.status === "ok" ? alerts.data.rules.find((r) => r.rule === alert.rule) : undefined;
  if (rule === undefined) return null; // no longer theirs to hear of
  const varies = VARIES[rule.rule];
  const set = async (tier: Tier | null) => {
    setBusy(true);
    try {
      await ok(api.PUT("/alerts/rules/{rule}", { params: { path: { rule: rule.rule } }, body: { tier } }));
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
      alerts.reload();
    }
  };
  const value = rule.tier ?? (varies === undefined ? rule.default : AS_IT_COMES);
  const choose = (v: string) => void set(v === AS_IT_COMES || (varies === undefined && v === rule.default) ? null : (v as Tier));
  return (
    <section className="space-y-2 border-t border-border pt-4">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-medium text-ink">{t(keys.retier, "Change when this reaches me")}</h3>
        <ViewLink to={{ view: "alert_settings", params: {} }} open={() => openWindow({ kind: "alerts", at: { settings: true } })} className="inline-flex items-center gap-1 text-sm text-ink-soft hover:text-ink">
          {t(keys.settings, "Settings")}
          <ChevronRight aria-hidden className="size-4" />
        </ViewLink>
      </div>
      <div className="flex flex-wrap items-center gap-3">
        <span className="text-sm text-ink-soft">{t(keys.retier.are, "{rule} are", { rule: RULE_NAME[rule.rule](t) })}</span>
        <ToggleGroup type="single" variant="outline" size="sm" value={value} onValueChange={(v) => typeof v === "string" && v !== "" && !busy && choose(v)} aria-label={t(keys.retier, "Change when this reaches me")}>
          {varies !== undefined && <ToggleGroupItem value={AS_IT_COMES}>{t(keys.asItComes, "As it comes")}</ToggleGroupItem>}
          {TIERS.map((tier) => (
            <ToggleGroupItem key={tier} value={tier}>
              <span aria-hidden className={cn("size-2 rounded-full", TIER_LOOK[tier].dot)} />
              {TIER_NAME[tier](t)}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
      </div>
      {varies !== undefined && <p className="text-xs text-ink-soft">{t(keys.asItComes.what, "As it comes: {how}.", { how: varies(t) })}</p>}
    </section>
  );
}
