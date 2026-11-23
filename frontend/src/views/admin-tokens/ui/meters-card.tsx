"use client";

import { Badge, Button, Card, CardContent, Input, Textarea, cn, toast } from "@evinvest/uikit";
import { useState } from "react";

import { tokens } from "@/entities/tokens";
import { api, ok } from "@/shared/api";
import type { Hold, MeterAccount, Meters } from "@/shared/api/schema";
import { useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize, useControlSize } from "@/shared/ui/touch";

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

/** What the conductor may buy past free units a month: the meters' slice's month. */
const allowed = (a: MeterAccount) => (a.hold === null ? "-" : (a.hold.allow.maps?.month ?? 0).toLocaleString());

const money = (x: number) => x.toLocaleString(undefined, { maximumFractionDigits: 2 });

/** What the guard holds an account to: each slice's real money a day and a month, and the APIs it leaves alone. Published beside the export within a minute, held from the guard's next tick. */
function HoldEditor({ account: a, slices, onSaved }: { account: MeterAccount; slices: string[]; onSaved: () => void }) {
  const t = useT();
  const size = useControlSize();
  const button = useButtonSize();
  const start = (): Record<string, string> =>
    Object.fromEntries(slices.flatMap((s) => (["day", "month"] as const).map((f) => [`${s}.${f}`, a.hold === null ? "" : String(a.hold.allow[s]?.[f] ?? "")])));
  const [drafts, setDrafts] = useState(start);
  const [unguarded, setUnguarded] = useState(a.hold === null ? "" : a.hold.unguarded.join("\n"));
  const [busy, setBusy] = useState(false);
  const num = (k: string) => (drafts[k] === "" ? undefined : Number(drafts[k]));
  const invalid = slices.some((s) => {
    const [d, m] = [num(`${s}.day`), num(`${s}.month`)];
    return d === undefined || m === undefined || !Number.isFinite(d) || !Number.isFinite(m) || d < 0 || d > m;
  });
  const save = () => {
    const body: Hold = {
      allow: Object.fromEntries(slices.map((s) => [s, { day: num(`${s}.day`)!, month: num(`${s}.month`)! }])),
      unguarded: unguarded
        .split(/\s+/)
        .map((u) => u.trim())
        .filter((u) => u !== ""),
    };
    setBusy(true);
    ok(api.PUT("/admin/meters/accounts/{name}/hold", { params: { path: { name: a.name } }, body }))
      .then(() => {
        toast.positive(t(tokens.meters.holdSaved, "{name}: held from the guard's next tick", { name: a.name }));
        onSaved();
      })
      .catch((err: unknown) => notifyFailure(err, t))
      .finally(() => setBusy(false));
  };
  return (
    <div className="flex flex-col gap-1.5 pl-3">
      {a.hold === null && <Badge tone="error">{t(tokens.meters.noHold, "no hold set: nothing of ours uses it, and the guard holds nothing of it")}</Badge>}
      <ul className="flex flex-col gap-1">
        {slices.map((s) => (
          <li key={s} className="flex flex-wrap items-center gap-2">
            <span className="w-32 text-ink">{s}</span>
            {(["day", "month"] as const).map((f) => (
              <label key={f} className="flex items-center gap-1 text-ink-soft">
                <Input
                  type="number"
                  inputMode="decimal"
                  min={0}
                  step="any"
                  size={size}
                  className="w-20 tabular-nums"
                  aria-invalid={num(`${s}.${f}`) === undefined || undefined}
                  value={drafts[`${s}.${f}`]}
                  onChange={(e) => setDrafts((d) => ({ ...d, [`${s}.${f}`]: e.target.value }))}
                />
                {f === "day" ? t(tokens.meters.perDay, "a day") : t(tokens.meters.perMonth, "a month")}
              </label>
            ))}
          </li>
        ))}
      </ul>
      <label className="flex flex-col gap-1 text-ink-soft">
        {t(tokens.meters.unguardedLabel, "Left alone by the guard, one API a line (no lever but the backstop)")}
        <Textarea rows={3} className="font-mono text-xs" value={unguarded} onChange={(e) => setUnguarded(e.target.value)} />
      </label>
      <div>
        <Button type="button" size={button("sm")} disabled={busy || invalid} onClick={save}>
          {t(tokens.meters.holdSave, "Hold to this")}
        </Button>
      </div>
    </div>
  );
}

/** An account as the guard last held it: each slice's real money against its day and month, and the levers holding it; what spends with no lever, and what it disabled. */
function Guarded({ account: a, slices, onSaved }: { account: MeterAccount; slices: string[]; onSaved: () => void }) {
  const t = useT();
  if (!a.exported) {
    return <p className="text-xs text-ink-soft">{t(tokens.meters.unexported, "{name}: no billing export, so nothing of ours uses it", { name: a.name })}</p>;
  }
  const g = a.guard;
  return (
    <section className="flex flex-col gap-1.5 text-xs">
      <div className="flex flex-wrap items-baseline gap-2">
        <span className="text-ink">{a.name}</span>
        <span className="text-ink-soft">{g === null ? t(tokens.meters.unguardedYet, "the guard never ticked here") : t(tokens.meters.tick, "guard's tick {at}", { at: day(g.tick) })}</span>
        {g !== null && !g.holds_this_list && <Badge tone="warn">{t(tokens.meters.otherList, "holds another definition than this one")}</Badge>}
      </div>
      <HoldEditor key={JSON.stringify(a.hold)} account={a} slices={slices} onSaved={onSaved} />
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
      {g !== null && g.unguarded.length > 0 && (
        <p className="pl-3 text-ink-soft">
          <Badge tone="warn">{t(tokens.meters.noLever, "no lever")}</Badge> {g.unguarded.map((u) => `${u.api} (${u.project})`).join(", ")}
        </p>
      )}
      {g !== null && g.disabled.length > 0 && (
        <p className="pl-3 text-ink-soft">
          <Badge tone="error">{t(tokens.meters.disabled, "disabled")}</Badge> {g.disabled.map((u) => `${u.api} (${u.project})`).join(", ")}
        </p>
      )}
    </section>
  );
}

/** Each vendor meter this period, over every billing account and folded out per account: the vendor's own count plus ours since it, against its free units; and each account's spend against its allowance, as the guard holds it. */
export function MetersCard({ meters: { accounts, slices, meters }, onSaved }: { meters: Meters; onSaved: () => void }) {
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
                  ? t(tokens.meters.unspent, "no reading this month · allows {allow}", { allow: allowed(a) })
                  : t(tokens.meters.spent, "{net} of {allow} {currency} net", { net: a.spend.net.toLocaleString(undefined, { maximumFractionDigits: 2 }), allow: allowed(a), currency: a.spend.currency })}
              </span>
            </li>
          ))}
        </ul>
        <div className="flex flex-col gap-2">
          {accounts.map((a) => (
            <Guarded key={a.name} account={a} slices={slices} onSaved={onSaved} />
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