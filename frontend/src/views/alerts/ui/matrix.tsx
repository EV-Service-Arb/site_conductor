"use client";

import { Button, Checkbox, ConfirmDialog, Input, Popover, PopoverContent, PopoverTrigger, Select, SelectContent, SelectItem, SelectTrigger, SelectValue, cn, toast } from "@evinvest/uikit";
import { Bell, ChevronDown, ChevronRight, Plus, Send, Trash2, X } from "lucide-react";
import { type ComponentType, type ReactNode, useId, useState } from "react";

import { CADENCES, CADENCE_NAME, METHOD_ICON, METHOD_NAME, RULE_NAME, type Set as Change, TIERS, TIER_LOOK, TIER_MEANS, TIER_NAME, alerts as keys, clock, keptPush, setDestination, unsubscribePush } from "@/entities/alerts";
import { googleName, useBrands, useGmails, useTrades } from "@/entities/gmb";
import { AddDestination } from "@/features/add-destination";
import { api, ok } from "@/shared/api";
import { type Alerts, type Cadence, type Destination, type Filter, type Gmails, type Rule, type Tier, ruleValues } from "@/shared/api/schema";
import { type T, useLocale, useT } from "@/shared/i18n";
import { useControlSize } from "@/shared/ui/touch";

import type { Act } from "./settings-view";

const ROW = "grid grid-cols-(--grid-alert-place) items-center gap-x-3 gap-y-3 px-4 py-3 md:grid-cols-(--grid-alert-matrix)";
/** The three cells: a phone's own line under the name. */
const CELLS = "col-span-3 grid grid-cols-3 gap-2 md:contents";

/** Rows are places, columns tiers, a cell how soon that place is told of that tier. */
export function Matrix({ alerts, busy, act }: { alerts: Alerts; busy: boolean; act: Act }) {
  const t = useT();
  const daily = clock(alerts.hours?.daily ?? 9 * 60); // unset, it goes out at 9:00
  return (
    <div className="divide-y divide-border rounded-lg border border-border bg-card">
      <div className={ROW}>
        <span className="col-span-3 max-md:hidden" />
        <div className={CELLS}>
          {TIERS.map((tier) => (
            <div key={tier} className="min-w-0">
              <p className={cn("flex items-center gap-2 text-sm font-medium", TIER_LOOK[tier].ink)}>
                <span aria-hidden className={cn("size-2 rounded-full", TIER_LOOK[tier].dot)} />
                {TIER_NAME[tier](t)}
              </p>
              <p className="text-xs text-ink-soft">{TIER_MEANS[tier](t)}</p>
            </div>
          ))}
        </div>
      </div>
      <div className={ROW}>
        <span className="max-md:hidden" />
        <Icon of={Bell} />
        <Name title={t(keys.panel, "This panel")} what={t(keys.panel.what, "the bell and the inbox")} className="max-md:col-span-2" />
        <div className={cn(CELLS, "text-sm text-ink-soft")}>
          <span>{t(keys.panel.urgent, "Takes the hotbar")}</span>
          <span>{t(keys.panel.important, "Toast")}</span>
          <span>{t(keys.panel.info, "Counted")}</span>
        </div>
      </div>
      {alerts.destinations.map((d) => (
        <Place key={d.id} destination={d} alerts={alerts} daily={daily} busy={busy} act={act} />
      ))}
      <div className="px-3 py-2">
        <AddDestination />
      </div>
    </div>
  );
}

function Icon({ of: Of, dead = false }: { of: ComponentType<{ className?: string }>; dead?: boolean }) {
  return (
    <span className={cn("flex size-9 items-center justify-center rounded-md bg-secondary", dead ? "text-accent-error" : "text-ink")}>
      <Of className="size-4" />
    </span>
  );
}

function Name({ title, what, dead, className }: { title: string; what: string; dead?: string | undefined; className?: string }) {
  return (
    <div className={cn("min-w-0", className)}>
      <p className="truncate text-sm font-medium text-ink">{title}</p>
      {dead !== undefined ? <p className="text-xs text-accent-error">{dead}</p> : <p className="truncate text-xs text-ink-soft">{what}</p>}
    </div>
  );
}

function reads(d: Destination, t: T): string {
  if (d.kind === "private") return t(keys.reads.chat, "your chat");
  return d.private ? t(keys.reads.own, "only you read it") : t(keys.reads.channel, "no names, phones or buttons");
}

const FACETS = ["gmails", "trades", "brands", "gmbs"] as const satisfies readonly (keyof Filter)[];

/** What differs from a new row: its filter and its rules, in a few words. */
function summary(d: Destination, t: T): string {
  const filtered = FACETS.some((f) => d.filter[f].length > 0);
  const rules = Object.keys(d.rules).length;
  return [reads(d, t), filtered ? t(keys.filtered, "some GBPs only") : null, rules > 0 ? t(keys.differs, "{n} alerts differ", { n: rules }) : null].filter((s) => s !== null).join(" · ");
}

function Place({ destination: d, alerts, daily, busy, act }: { destination: Destination; alerts: Alerts; daily: string; busy: boolean; act: Act }) {
  const t = useT();
  const body = useId();
  const [open, setOpen] = useState(false);
  const name = [METHOD_NAME[d.method](t), d.title, d.topic_id === null ? null : t(keys.topic, "topic {topic}", { topic: d.topic_id })].filter((s) => s !== null).join(" · ");
  const change = (c: Change) => void act(() => setDestination(d, c));
  const test = () =>
    void act(async () => {
      await ok(api.POST("/alerts/destinations/{id}/test", { params: { path: { id: d.id } } }));
      toast.positive(t(keys.test.sent, "A test is on its way"));
    });
  return (
    <div>
      <div className={ROW}>
        <button
          type="button"
          aria-expanded={open}
          aria-controls={body}
          aria-label={t(keys.more, "{name}: more", { name })}
          onClick={() => setOpen(!open)}
          className="flex size-6 items-center justify-center rounded-sm text-ink-soft outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-ring"
        >
          <ChevronRight aria-hidden className={cn("size-4 transition-transform", open && "rotate-90")} />
        </button>
        <Icon of={METHOD_ICON[d.method]} dead={d.dead !== null} />
        <div className="flex min-w-0 items-center gap-2">
          <Name title={name} what={summary(d, t)} dead={d.dead === null ? undefined : t(keys.dead, "Nothing reaches it: {why}. Fix it, then Test.", { why: d.dead })} className="flex-1" />
          {d.dead !== null && (
            <Button type="button" variant="outline" size="sm" disabled={busy} onClick={test}>
              {t(keys.test, "Test")}
            </Button>
          )}
        </div>
        <div className={CELLS}>
          {TIERS.map((tier) => (
            <CadenceSelect key={tier} value={d.cadences[tier]} daily={daily} label={`${name}, ${TIER_NAME[tier](t)}`} disabled={busy} onChange={(c) => change({ cadences: { ...d.cadences, [tier]: c } })} />
          ))}
        </div>
      </div>
      {open && (
        <div id={body} className="grid gap-6 bg-background/60 px-4 py-4 md:grid-cols-2 md:pl-18">
          <OnlyFor destination={d} busy={busy} change={change} />
          <Different destination={d} alerts={alerts} daily={daily} busy={busy} change={change} />
          <div className="flex items-center gap-3 md:col-span-2">
            <Button type="button" variant="outline" size="sm" disabled={busy} onClick={test}>
              <Send aria-hidden className="size-4" />
              {t(keys.test.send, "Send a test")}
            </Button>
            <span className="flex-1" />
            <ConfirmDialog
              tone="error"
              trigger={
                <Button type="button" variant="ghost" size="sm" disabled={busy} className="text-accent-error">
                  <Trash2 aria-hidden className="size-4" />
                  {t(keys.remove, "Remove")}
                </Button>
              }
              title={t(keys.remove.title, "Remove {name}?", { name })}
              description={t(keys.remove.what, "Nothing more goes there, and what was waiting to is dropped. The alerts stay in the panel.")}
              confirmLabel={t(keys.remove, "Remove")}
              onConfirm={() =>
                act(async () => {
                  await ok(api.DELETE("/alerts/destinations/{id}", { params: { path: { id: d.id } } }));
                  if (keptPush()?.destination === d.id) await unsubscribePush();
                })
              }
            />
          </div>
        </div>
      )}
    </div>
  );
}

function cadenceMeans(c: Cadence, daily: string, t: T): string {
  switch (c) {
    case "now":
      return t(keys.cadence.now.means, "at once");
    case "unread":
      return t(keys.cadence.unread.means, "10 min later, unless you read it in the panel");
    case "hourly":
      return t(keys.cadence.hourly.means, "one message an hour");
    case "daily":
      return t(keys.cadence.daily.means, "one message at {time}", { time: daily });
    case "off":
      return t(keys.cadence.off.means, "never; still in the panel");
  }
}

function CadenceSelect({ value, daily, label, disabled, onChange }: { value: Cadence; daily: string; label: string; disabled: boolean; onChange: (c: Cadence) => void }) {
  const t = useT();
  const size = useControlSize();
  return (
    <Select value={value} onValueChange={(v) => onChange(v as Cadence)}>
      <SelectTrigger size={size} className={cn("w-full min-w-0", value === "off" && "text-ink-soft")} disabled={disabled} aria-label={label}>
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {CADENCES.map((c) => (
          <SelectItem key={c} value={c} textValue={CADENCE_NAME[c](t)}>
            <span className="flex flex-col">
              <span>{CADENCE_NAME[c](t)}</span>
              <span className="text-xs text-ink-soft">{cadenceMeans(c, daily, t)}</span>
            </span>
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}

// ── only for ────────────────────────────────────────────────────────────────────────────

function OnlyFor({ destination: d, busy, change }: { destination: Destination; busy: boolean; change: (c: Change) => void }) {
  const t = useT();
  const gmails = useGmails();
  return (
    <section className="min-w-0 space-y-3">
      <h3 className="text-sm font-medium text-ink">{t(keys.onlyFor, "Only for")}</h3>
      {gmails.status === "ok" && <Facets data={gmails.data} filter={d.filter} busy={busy} onChange={(filter) => change({ filter })} />}
      <p className="text-xs text-ink-soft">{t(keys.onlyFor.how, "An alert passes when one of its GBPs fits every line. One that names no GBP always passes.")}</p>
    </section>
  );
}

function Facets({ data, filter, busy, onChange }: { data: Gmails; filter: Filter; busy: boolean; onChange: (f: Filter) => void }) {
  const t = useT();
  const locale = useLocale();
  const trades = useTrades(data.gmbs);
  const brands = useBrands();
  const passing = data.gmbs.filter(
    (b) =>
      (filter.gmails.length === 0 || (b.gmail_id !== null && filter.gmails.includes(b.gmail_id))) &&
      (filter.trades.length === 0 || (b.trade !== null && filter.trades.includes(b.trade))) &&
      (filter.brands.length === 0 || (b.brand_id !== null && filter.brands.includes(b.brand_id))) &&
      (filter.gmbs.length === 0 || filter.gmbs.includes(b.id)),
  ).length;
  return (
    <div className="grid grid-cols-(--grid-alert-facet) items-center gap-x-3 gap-y-2">
      <Facet label={t(keys.facet.gmails, "Gmails")} options={data.gmails.map((g) => ({ id: g.id, name: g.gmail }))} picked={filter.gmails} busy={busy} onChange={(gmails) => onChange({ ...filter, gmails })} />
      <Facet label={t(keys.facet.trades, "Categories")} options={(trades ?? []).map((tr) => ({ id: tr.trade.id, name: googleName(tr.trade, locale) }))} picked={filter.trades} busy={busy} onChange={(to) => onChange({ ...filter, trades: to })} />
      <Facet label={t(keys.facet.brands, "Brands")} options={brands.status === "ok" ? brands.data.map((b) => ({ id: b.id, name: b.name })) : []} picked={filter.brands} busy={busy} onChange={(to) => onChange({ ...filter, brands: to })} />
      <Facet label={t(keys.facet.gmbs, "GBPs")} options={data.gmbs.map((b) => ({ id: b.id, name: b.name }))} picked={filter.gmbs} busy={busy} onChange={(gmbs) => onChange({ ...filter, gmbs })} />
      <p className="col-span-2 text-xs text-ink-soft">{t(keys.onlyFor.count, "{n} of {of} GBPs", { n: passing, of: data.gmbs.length })}</p>
    </div>
  );
}

function Facet<Id extends string | number>({ label, options, picked, busy, onChange }: { label: string; options: readonly { id: Id; name: string }[]; picked: readonly Id[]; busy: boolean; onChange: (ids: Id[]) => void }) {
  const t = useT();
  const search = useId();
  const [open, setOpen] = useState(false);
  const [find, setFind] = useState("");
  // sent once, as the list closes: a write per tick would race itself
  const [draft, setDraft] = useState<readonly Id[]>(picked);
  const names = picked.map((id) => options.find((o) => o.id === id)?.name ?? "…");
  const toggle = (next: boolean) => {
    setOpen(next);
    if (next) {
      setDraft(picked);
      setFind("");
      return;
    }
    if (draft.length !== picked.length || draft.some((id) => !picked.includes(id))) onChange([...draft]);
  };
  const shown = options.filter((o) => o.name.toLowerCase().includes(find.trim().toLowerCase()));
  return (
    <>
      <span className="text-sm text-ink-soft">{label}</span>
      <Popover open={open} onOpenChange={toggle}>
        <PopoverTrigger
          type="button"
          disabled={busy || options.length === 0}
          aria-label={label}
          className="flex h-8 min-w-0 items-center gap-2 rounded-md border border-border bg-secondary px-2.5 text-sm text-ink outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50 max-md:h-11"
        >
          <span className="min-w-0 flex-1 truncate text-left">{picked.length === 0 ? t(keys.facet.any, "Any of {n}", { n: options.length }) : names.join(", ")}</span>
          <ChevronDown aria-hidden className="size-4 shrink-0 text-ink-soft" />
        </PopoverTrigger>
        <PopoverContent align="start" className="flex max-h-80 w-72 flex-col gap-1 p-1.5">
          {options.length > 8 && <Input id={search} size="sm" value={find} placeholder={t(keys.facet.find, "Find")} aria-label={t(keys.facet.find, "Find")} onChange={(e) => setFind(e.target.value)} />}
          <div className="min-h-0 overflow-y-auto">
            {shown.map((o) => (
              <label key={o.id} className="flex cursor-pointer items-center gap-2 rounded-md px-2 py-1.5 text-sm text-ink hover:bg-hover">
                <Checkbox checked={draft.includes(o.id)} onCheckedChange={(on) => setDraft(on ? [...draft, o.id] : draft.filter((id) => id !== o.id))} />
                <span className="min-w-0 truncate">{o.name}</span>
              </label>
            ))}
          </div>
          <div className="flex justify-between border-t border-border pt-1">
            <Button type="button" variant="ghost" size="sm" disabled={draft.length === 0} onClick={() => setDraft([])}>
              {t(keys.facet.clear, "Any")}
            </Button>
            <Button type="button" size="sm" onClick={() => toggle(false)}>
              {t(keys.done, "Done")}
            </Button>
          </div>
        </PopoverContent>
      </Popover>
    </>
  );
}

// ── different here ──────────────────────────────────────────────────────────────────────

function Different({ destination: d, alerts, daily, busy, change }: { destination: Destination; alerts: Alerts; daily: string; busy: boolean; change: (c: Change) => void }) {
  const t = useT();
  const size = useControlSize();
  const set = ruleValues.filter((r) => d.rules[r] !== undefined);
  const open = alerts.rules.filter((r) => d.rules[r.rule] === undefined);
  const tierOf = (rule: Rule): Tier | undefined => {
    const r = alerts.rules.find((x) => x.rule === rule);
    return r === undefined ? undefined : (r.tier ?? r.default);
  };
  const put = (rule: Rule, c: Cadence | null) => {
    const rules = Object.fromEntries(Object.entries(d.rules).filter(([r]) => r !== rule));
    change({ rules: c === null ? rules : { ...rules, [rule]: c } });
  };
  return (
    <section className="min-w-0 space-y-3">
      <h3 className="text-sm font-medium text-ink">{t(keys.different, "Different here")}</h3>
      {set.map((rule) => {
        const tier = tierOf(rule);
        return (
          <Line key={rule} name={RULE_NAME[rule](t)} tier={tier === undefined ? null : TIER_NAME[tier](t)}>
            <div className="w-32 shrink-0">
              <CadenceSelect value={d.rules[rule]!} daily={daily} label={RULE_NAME[rule](t)} disabled={busy} onChange={(c) => put(rule, c)} />
            </div>
            <Button type="button" variant="ghost" size="sm" icon disabled={busy} aria-label={t(keys.different.remove, "{rule}: as its tier", { rule: RULE_NAME[rule](t) })} onClick={() => put(rule, null)}>
              <X aria-hidden className="size-4" />
            </Button>
          </Line>
        );
      })}
      {open.length > 0 && (
        // its value stays empty: a pick adds the alert, at what its tier has here, to be changed
        <Select key={set.length} value="" onValueChange={(v) => put(v as Rule, d.cadences[tierOf(v as Rule)!] === "off" ? "now" : "off")}>
          <SelectTrigger size={size} className="w-48" disabled={busy} aria-label={t(keys.different.add, "Set an alert apart here")}>
            <span className="inline-flex items-center gap-2 text-ink-soft">
              <Plus aria-hidden className="size-4" />
              {t(keys.different.alert, "Alert")}
            </span>
          </SelectTrigger>
          <SelectContent>
            {open.map((r) => (
              <SelectItem key={r.rule} value={r.rule}>
                {RULE_NAME[r.rule](t)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      )}
    </section>
  );
}

function Line({ name, tier, children }: { name: string; tier: string | null; children: ReactNode }) {
  return (
    <div className="flex items-center gap-2">
      <span className="min-w-0 flex-1 truncate text-sm text-ink">
        {name} {tier !== null && <span className="text-xs text-ink-soft">{tier}</span>}
      </span>
      {children}
    </div>
  );
}
