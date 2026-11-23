"use client";

import { Badge, Card, CardContent, cn } from "@evinvest/uikit";
import { useState } from "react";

import { tokens } from "@/entities/tokens";
import type { MeterAccount, Meters } from "@/shared/api/schema";
import { useT } from "@/shared/i18n";

/** A reading older than this is stale: the sync runs daily. */
const STALE_MS = 36 * 3600 * 1000;

const day = (iso: string) => new Date(iso).toLocaleString(undefined, { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });

const tone = (share: number) => (share >= 1 ? "bg-accent-error" : share >= 0.8 ? "bg-accent-warn" : "bg-ink-soft");

function Bar({ share, thin }: { share: number; thin?: boolean }) {
  return (
    <div className={cn("overflow-hidden rounded-full bg-muted", thin === true ? "h-1" : "h-1.5")} aria-hidden>
      <div className={cn("h-full rounded-full", tone(share))} style={{ width: `${Math.min(share, 1) * 100}%` }} />
    </div>
  );
}

const money = (x: number) => x.toLocaleString(undefined, { maximumFractionDigits: 2 });

/** An account as the guard last held it: each slice's real money against its day and month, and the levers holding it; what spends with no lever, and what it disabled. */
function Guarded({ account: a }: { account: MeterAccount }) {
  const t = useT();
  if (a.guard === null) {
    return <p className="text-xs text-ink-soft">{t(tokens.meters.unguardedYet, "{name}: the guard never ticked here", { name: a.name })}</p>;
  }
  const g = a.guard;
  return (
    <section className="flex flex-col gap-1.5 text-xs">
      <div className="flex flex-wrap items-baseline gap-2">
        <span className="text-ink">{a.name}</span>
        <span className="text-ink-soft">{t(tokens.meters.tick, "guard's tick {at}", { at: day(g.tick) })}</span>
        {!g.holds_this_list && <Badge tone="warn">{t(tokens.meters.otherList, "holds another nix/google.nix")}</Badge>}
      </div>
      <ul className="flex flex-col gap-1 pl-3">
        {a.allowances.map((s) => (
          <li key={s.slice} className="flex flex-col gap-0.5">
            <div className="flex flex-wrap items-baseline gap-2">
              <span className="text-ink">{s.slice}</span>
              {s.state === "over" && <Badge tone="error">{t(tokens.meters.over, "over")}</Badge>}
              <span className="ml-auto tabular-nums text-ink-soft">
                {t(tokens.meters.allowance, "today {today} of {day} · this month {month} of {perMonth}", { today: money(s.real_today), day: money(s.day), month: money(s.real_month), perMonth: money(s.month) })}
              </span>
            </div>
            {s.levers.length > 0 && (
              <ul className="flex flex-col pl-3 text-ink-soft">
                {s.levers.map((l) => (
                  <li key={`${l.project ?? ""} ${l.lever}`} className="tabular-nums">
                    {l.project !== null && `${l.project} `}
                    {l.lever}
                    {l.setting !== null && ` = ${money(l.setting)}`}
                    {l.today !== null && ` · ${t(tokens.meters.requests, "{n} today", { n: l.today.toLocaleString() })}`}
                  </li>
                ))}
              </ul>
            )}
          </li>
        ))}
      </ul>
      {g.unguarded.length > 0 && (
        <p className="pl-3 text-ink-soft">
          <Badge tone="warn">{t(tokens.meters.noLever, "no lever")}</Badge> {g.unguarded.map((u) => `${u.api} (${u.project})`).join(", ")}
        </p>
      )}
      {g.disabled.length > 0 && (
        <p className="pl-3 text-ink-soft">
          <Badge tone="error">{t(tokens.meters.disabled, "disabled")}</Badge> {g.disabled.map((u) => `${u.api} (${u.project})`).join(", ")}
        </p>
      )}
    </section>
  );
}

/** Each vendor meter this period, over every billing account and folded out per account: the vendor's own count plus ours since it, against its free units; and each account's spend against its allowance, as the guard holds it. */
export function MetersCard({ meters: { accounts, meters } }: { meters: Meters }) {
  const t = useT();
  const [now] = useState(Date.now);
  return (
    <Card className="gap-3 py-4">
      <CardContent className="flex flex-col gap-3 px-4">
        <header className="flex flex-col gap-1">
          <h2 className="text-sm font-semibold text-ink">{t(tokens.meters.title, "Vendors this month")}</h2>
          <p className="text-xs text-ink-soft">
            {t(
              tokens.meters.why,
              "Each account's count is the vendor's own (every project on that billing account, hours behind), plus ours on its keys since. A buy goes to an account with free units left, then one within its allowance. Street View stops at each account's free ones.",
            )}
          </p>
        </header>
        <ul className="flex flex-wrap gap-2">
          {accounts.map((a) => (
            <li key={a.name} className="flex items-baseline gap-2 rounded-md border border-border px-2 py-1 text-xs">
              <span className="text-ink">{a.name}</span>
              <span className="tabular-nums text-ink-soft">
                {a.spend === null
                  ? t(tokens.meters.unspent, "no reading this month · allows {allow}", { allow: a.allow.toLocaleString() })
                  : t(tokens.meters.spent, "{net} of {allow} {currency} net", { net: a.spend.net.toLocaleString(undefined, { maximumFractionDigits: 2 }), allow: a.allow.toLocaleString(), currency: a.spend.currency })}
              </span>
            </li>
          ))}
        </ul>
        <div className="flex flex-col gap-2">
          {accounts.map((a) => (
            <Guarded key={a.name} account={a} />
          ))}
        </div>
        <ul className="flex flex-col divide-y divide-border">
          {meters.map((m) => {
            const share = m.total.free === 0 ? 1 : m.total.used / m.total.free;
            const unread = m.accounts.filter((a) => a.reading === null || now - Date.parse(a.reading.synced_at) > STALE_MS);
            return (
              <li key={m.meter} className="py-2">
                <details className="group flex flex-col gap-1.5">
                  <summary className="flex cursor-pointer list-none flex-col gap-1.5">
                    <div className="flex flex-wrap items-baseline gap-2">
                      <span className="text-sm text-ink">{m.name}</span>
                      {m.stop === "at_free" && <Badge tone="neutral">{t(tokens.meters.stops, "stops at free")}</Badge>}
                      {share >= 0.8 && <Badge tone={share >= 1 ? "error" : "warn"}>{t(tokens.meters.near, "{p}% of free", { p: Math.round(share * 100) })}</Badge>}
                      {unread.length > 0 && <Badge tone="warn">{t(tokens.meters.unread, "{n} not read lately", { n: unread.length })}</Badge>}
                      <span className="ml-auto text-sm tabular-nums text-ink">
                        {m.total.used.toLocaleString()} / {m.total.free.toLocaleString()}
                      </span>
                    </div>
                    <Bar share={share} />
                  </summary>
                  <ul className="mt-1 flex flex-col gap-1.5 pl-3">
                    {m.accounts.map((a) => {
                      const mine = a.used / m.free;
                      const stale = a.reading === null || now - Date.parse(a.reading.synced_at) > STALE_MS;
                      return (
                        <li key={a.account} className="flex flex-col gap-1">
                          <div className="flex flex-wrap items-baseline gap-2 text-xs">
                            <span className="text-ink">{a.account}</span>
                            {stale && (
                              <Badge tone="warn">
                                {a.reading === null ? t(tokens.meters.never, "never read") : t(tokens.meters.stale, "read {at}", { at: day(a.reading.synced_at) })}
                              </Badge>
                            )}
                            <span className="ml-auto tabular-nums text-ink-soft">
                              {t(tokens.meters.split, "{vendor} {baseline} as of {at}, ours {delta} since", { vendor: "Google", baseline: a.baseline.toLocaleString(), at: day(a.since), delta: a.delta.toLocaleString() })}
                              {" · "}
                              {a.used.toLocaleString()} / {m.free.toLocaleString()}
                            </span>
                          </div>
                          <Bar share={mine} thin />
                        </li>
                      );
                    })}
                  </ul>
                </details>
              </li>
            );
          })}
        </ul>
      </CardContent>
    </Card>
  );
}