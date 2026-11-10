"use client";

import { Input, Popover, PopoverContent, PopoverTrigger, Switch, cn } from "@evinvest/uikit";
import { Check, ChevronDown, ChevronRight, ListFilter, Maximize2, Plug, Plus } from "lucide-react";
import { usePathname, useRouter } from "next/navigation";
import { type MouseEvent, useEffect, useState } from "react";

import {
  COUNTRIES,
  type Country,
  type Gmail,
  type Gmails,
  type Gmb,
  type Pattern,
  type Selection,
  type Trade,
  addTrade,
  chooseCategoryScope,
  chooseGmailScope,
  covers,
  inCategories,
  openAccount,
  openTrade as openTradeHere,
  removeGmail,
  under,
  useScope,
  useTrades,
} from "@/entities/gmb";
import { AddGmailButton } from "@/features/add-gmail";
import { AddGmbButton } from "@/features/add-gmb";
import { useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";

import { HEADING, ON_HOVER, ROW, Remove, ResetSelection } from "./outline-parts";

const same = (a: Pattern, b: Pattern) => a.trade === b.trade && a.country === b.country;
/** Shift, Ctrl or Cmd: a click adds to what is picked rather than replacing it. */
const adding = (e: MouseEvent) => e.shiftKey || e.ctrlKey || e.metaKey;
const PICKED = "bg-hover font-medium text-ink ring-1 ring-ink ring-inset";
const COVERED = "bg-accent-info/15";
const META = "font-normal tracking-normal normal-case";

/**
 * What the outline is scoped to, one picker over three facets that narrow each other's counts: the categories as a matrix of
 * trades by country (a cell a category, a row header the trade everywhere, a column header the country across trades), then the
 * managing gmails. A gmail toggles among the filters (or, after a move, all its GBPs), a GBP opened under it toggles alone.
 */
export function ScopePicker({ data, selection, onFlip, onChanged }: { data: Gmails; selection: Selection; onFlip: (gmbs: Gmb[], enabled: boolean) => Promise<void>; onChanged: () => void }) {
  const t = useT();
  const trades = useTrades();
  const scope = useScope();
  const [open, setOpen] = useState(false);
  const [opened, setOpened] = useState<ReadonlySet<number>>(new Set());
  const { scoped, chosen, edited, isSelected, move } = selection;
  const { gmails } = data;
  const known = trades.status === "ok" ? trades.data : null;

  // a scoped trade deleted, here or in another tab: out of the scope
  useEffect(() => {
    if (known !== null && scope.categories !== null && scope.categories.some((p) => p.trade !== "*" && !known.some((k) => k.id === p.trade)))
      chooseCategoryScope(scope.categories.filter((p) => p.trade === "*" || known.some((k) => k.id === p.trade)));
  }, [known, scope.categories]);

  const byGmail = (b: Gmb) => scope.gmails === null || (b.gmail_id !== null && scope.gmails.has(b.gmail_id));
  const byCategory = (b: Gmb) => scope.categories === null || inCategories(b, scope.categories);
  const covered = (gmbs: Gmb[]) => gmbs.filter((b) => isSelected(b.id)).length;
  const pickGmail = (g: Gmail) => {
    const gmbs = under(data, g.id);
    if (edited) return move(gmbs.map((b) => b.id), covered(gmbs) < gmbs.length);
    const next = new Set(scoped.map((s) => s.id));
    if (next.has(g.id)) next.delete(g.id);
    else next.add(g.id);
    chooseGmailScope(next);
  };
  const unfold = (id: number) => {
    const next = new Set(opened);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    setOpened(next);
  };
  const name = (p: Pattern) => {
    const trade = p.trade === "*" ? null : (known?.find((k) => k.id === p.trade)?.name ?? "…");
    return [trade, p.country === "*" ? null : p.country].filter(Boolean).join(" · ");
  };
  const holding = gmails.filter((g) => under(data, g.id).some(byCategory));
  const summary = [
    scope.categories === null ? t("scope.categories.all") : scope.categories.length === 1 ? name(scope.categories[0]!) : t("scope.categories.many", { n: scope.categories.length }),
    scoped.length === 1 ? scoped[0]!.gmail : scoped.length === 0 && holding.length === 1 ? holding[0]!.gmail : t("gmbs.scope.many", { n: scoped.length === 0 ? holding.length : scoped.length }),
  ].join(" · ");

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <div className="mx-0.5 flex items-center rounded-md border border-border pr-1 hover:bg-hover/50">
        <PopoverTrigger className="flex min-w-0 flex-1 items-center gap-2 rounded-md px-2.5 py-2 text-sm text-ink outline-none focus-visible:ring-2 focus-visible:ring-ring">
          <ListFilter aria-hidden className="size-4 text-ink-soft" />
          <span className="min-w-0 flex-1 truncate text-left">{summary}</span>
          <span className="text-xs text-ink-soft tabular-nums">{selection.selected.length}</span>
          <ChevronDown aria-hidden className="size-4 text-ink-soft" />
        </PopoverTrigger>
        {chosen !== null && <ResetSelection />}
      </div>
      <PopoverContent align="start" className="max-h-[80svh] w-96 overflow-y-auto p-1.5">
        <div className={HEADING}>
          {t("scope.categories")}
          <span className="flex-1" />
          {known !== null && (
            <span className={META}>
              {t("scope.categories.counts", { trades: known.length, countries: new Set(known.flatMap((tr) => tr.categories.map((c) => c.country))).size })}
            </span>
          )}
        </div>
        {known !== null && <Matrix all={data.gmbs} gmbs={data.gmbs.filter(byGmail)} trades={known} picked={scope.categories} onOpened={() => setOpen(false)} />}
        <p className="px-2 pt-1 text-xs text-ink-soft">{t("scope.matrix.how")}</p>
        {data.gmbs.some((b) => b.trade_id === null) && <p className="px-2 text-xs text-ink-soft">{t("scope.matrix.untraded", { n: data.gmbs.filter((b) => b.trade_id === null).length })}</p>}
        <NewTrade onAdded={trades.reload} />
        <div className={cn(HEADING, "mt-1 border-t border-border pt-2")}>
          {t("scope.gmails")}
          <span className="flex-1" />
          <span className={cn(META, "tabular-nums")}>
            {scope.categories === null ? t("gmbs.scope.counts", { gmails: gmails.length, gmbs: data.gmbs.length }) : t("scope.gmails.holding", { n: holding.length, of: gmails.length })}
          </span>
        </div>
        <button
          type="button"
          onClick={() => chooseGmailScope(null)}
          aria-pressed={scoped.length === 0}
          className={cn(ROW, "w-full py-1.5 pl-2", scoped.length === 0 && PICKED)}
        >
          <span className="flex-1 text-left">{t("scope.gmails.all")}</span>
          <span className="text-xs text-ink-soft tabular-nums">{data.gmbs.filter(byCategory).length}</span>
        </button>
        <ul className="flex flex-col">
          {gmails.map((g) => {
            const gmbs = under(data, g.id);
            const inScope = gmbs.filter(byCategory).length;
            const on = gmbs.length > 0 && gmbs.every((b) => b.enabled);
            const k = chosen === null ? 0 : covered(gmbs);
            const full = k > 0 && k === gmbs.length;
            const unfolded = opened.has(g.id);
            const picked = scoped.some((s) => s.id === g.id);
            return (
              <li key={g.id} className={cn(scope.categories !== null && inScope === 0 && !picked && "opacity-40")}>
                <div className={cn(ROW, (picked || (edited && full)) && PICKED, edited && k > 0 && !full && COVERED)}>
                  <button
                    type="button"
                    onClick={() => unfold(g.id)}
                    aria-expanded={unfolded}
                    aria-label={t(unfolded ? "gmbs.collapse" : "gmbs.expand", { gmail: g.gmail })}
                    className="rounded p-1 text-ink-soft outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-ring"
                  >
                    <ChevronRight aria-hidden className={cn("size-3.5 transition-transform", unfolded && "rotate-90")} />
                  </button>
                  <button
                    type="button"
                    onClick={() => pickGmail(g)}
                    aria-pressed={edited ? (full ? true : k > 0 ? "mixed" : false) : picked}
                    className={cn("min-w-0 flex-1 py-1 text-left", on ? "text-ink" : "text-ink-soft")}
                    title={g.gmail}
                  >
                    <span className="block truncate">{g.gmail}</span>
                    <Room gmail={g} n={gmbs.length} />
                  </button>
                  <span className={cn("flex items-center", ON_HOVER)}>
                    <OpenAccount gmail={g} onOpened={() => setOpen(false)} />
                    <AddGmbButton gmailId={g.id} trade={null} onAdded={onChanged} />
                    <Remove title={t("gmbs.gmail.remove.title", { gmail: g.gmail })} body={t("gmbs.gmail.remove.body")} remove={() => removeGmail(g.id)} onRemoved={onChanged} />
                  </span>
                  <span className="text-xs text-ink-soft tabular-nums" title={t("scope.gmail.count")}>
                    {edited && k > 0 && !full ? `${k}/${gmbs.length}` : inScope}
                  </span>
                  <Switch aria-label={t("gmbs.switch", { name: g.gmail })} checked={on} disabled={gmbs.length === 0} onCheckedChange={(v) => void onFlip(gmbs, v)} />
                </div>
                {unfolded && (
                  <ul className="ml-4 flex flex-col border-l border-border pl-1">
                    {gmbs.length === 0 && <li className="px-2 py-1.5 text-xs text-ink-soft">{t("gmbs.gmail.empty")}</li>}
                    {gmbs.map((b) => {
                      const one = chosen !== null && isSelected(b.id);
                      return (
                        <li key={b.id}>
                          <button
                            type="button"
                            onClick={() => move([b.id], !one)}
                            aria-pressed={one}
                            title={b.address ? `${b.name}\n${b.address}` : b.name}
                            className={cn(
                              "flex w-full items-center gap-1 rounded-md py-1 pr-1 pl-2 text-left text-sm outline-none hover:bg-hover/50 focus-visible:ring-2 focus-visible:ring-ring",
                              one && PICKED,
                              b.enabled ? "text-ink" : "text-ink-soft",
                              !byCategory(b) && "opacity-40",
                            )}
                          >
                            <span className="min-w-0 flex-1 truncate">{b.name}</span>
                            {one && <Check aria-hidden className="size-3.5 shrink-0" />}
                          </button>
                        </li>
                      );
                    })}
                  </ul>
                )}
              </li>
            );
          })}
        </ul>
        <div className="flex items-center gap-1 border-t border-border pt-1">
          <AddGmailButton onAdded={onChanged} />
          <span className="text-xs text-ink-soft">{t("scope.everyApp")}</span>
        </div>
      </PopoverContent>
    </Popover>
  );
}

/**
 * Trades by the countries that have GBPs or categories: a cell counts the GBPs of that category under the gmails picked. Blank:
 * not set up and holding none; `0`: set up and holding none; dashed, in the warning tone: GBPs there, the trade not set up.
 */
function Matrix({ all, gmbs, trades, picked, onOpened }: { all: readonly Gmb[]; gmbs: Gmb[]; trades: readonly Trade[]; picked: readonly Pattern[] | null; onOpened: () => void }) {
  const t = useT();
  const openTrade = useOpenTrade(onOpened);
  const countries = COUNTRIES.filter((c) => all.some((b) => b.country?.code === c) || trades.some((tr) => tr.categories.some((k) => k.country === c)));
  if (trades.length === 0) return <p className="px-2 py-1.5 text-xs text-ink-soft">{t("scope.categories.none")}</p>;
  const pick = (p: Pattern) => (e: MouseEvent) => {
    const now = picked ?? [];
    const has = now.some((q) => same(q, p));
    if (adding(e)) chooseCategoryScope(has ? now.filter((q) => !same(q, p)) : [...now, p]);
    else chooseCategoryScope(has && now.length === 1 ? null : [p]);
  };
  const state = (p: Pattern) => (picked?.some((q) => same(q, p)) ? PICKED : null);
  const count = (trade: number | "*", country: Country | "*") => gmbs.filter((b) => inCategories(b, [{ trade, country }])).length;
  const box = "h-7 rounded-md text-xs tabular-nums outline-none hover:bg-hover/60 focus-visible:ring-2 focus-visible:ring-ring";
  return (
    <table className="w-full border-separate border-spacing-0.5 text-sm">
      <thead>
        <tr>
          <th className="font-normal">
            <button
              type="button"
              onClick={() => chooseCategoryScope(null)}
              aria-pressed={picked === null}
              className={cn(box, "flex w-full items-center px-2 text-ink-soft", picked === null && PICKED)}
            >
              <span className="flex-1 text-left">{t("scope.matrix.all")}</span>
              <span>{gmbs.filter((b) => b.trade_id !== null).length}</span>
            </button>
          </th>
          {countries.map((c) => {
            const p: Pattern = { trade: "*", country: c };
            return (
              <th key={c} className="w-14 font-normal">
                <button type="button" aria-pressed={state(p) !== null} title={t("scope.matrix.country", { country: c })} onClick={pick(p)} className={cn(box, "flex h-9 w-full flex-col items-center justify-center leading-tight", state(p))}>
                  <span className="font-semibold text-ink">{c}</span>
                  <span className="text-ink-soft">{count("*", c)}</span>
                </button>
              </th>
            );
          })}
        </tr>
      </thead>
      <tbody>
        {trades.map((tr) => {
          const row: Pattern = { trade: tr.id, country: "*" };
          return (
            <tr key={tr.id}>
              <th className="group/head font-normal">
                <span className="relative flex items-center">
                  <button
                    type="button"
                    aria-pressed={state(row) !== null}
                    title={t("scope.matrix.trade", { trade: tr.name })}
                    onClick={pick(row)}
                    className={cn(box, "flex min-w-0 flex-1 items-center gap-2 px-2 text-sm", tr.categories.length === 0 ? "text-ink-soft" : "text-ink", state(row))}
                  >
                    <span className="min-w-0 flex-1 truncate text-left">{tr.name}</span>
                    <span className="text-xs text-ink-soft">{count(tr.id, "*")}</span>
                  </button>
                  <OpenTrade trade={tr} onOpened={onOpened} className="absolute right-0.5 opacity-0 group-hover/head:opacity-100 focus-visible:opacity-100" />
                </span>
              </th>
              {countries.map((c) => {
                const p: Pattern = { trade: tr.id, country: c };
                const setUp = tr.categories.some((k) => k.country === c);
                const n = count(tr.id, c);
                const covering = state(p) === null && picked !== null && picked.some((q) => covers(q, tr.id, c));
                if (!setUp && n === 0)
                  return (
                    <td key={c} className="group/cell">
                      <button
                        type="button"
                        onClick={() => openTrade(tr.id, c)}
                        aria-label={t("scope.matrix.setUp", { category: `${tr.name} · ${c}` })}
                        title={t("scope.matrix.setUp", { category: `${tr.name} · ${c}` })}
                        className={cn(box, "w-full text-ink-soft opacity-0 group-hover/cell:opacity-100 focus-visible:opacity-100")}
                      >
                        <Plus aria-hidden className="mx-auto size-3.5" />
                      </button>
                    </td>
                  );
                return (
                  <td key={c} className="group/cell relative">
                    <button
                      type="button"
                      aria-pressed={state(p) !== null}
                      title={setUp ? `${tr.name} · ${c}` : t("scope.matrix.notSetUp", { category: `${tr.name} · ${c}` })}
                      onClick={pick(p)}
                      className={cn(
                        box,
                        "w-full",
                        n === 0 ? "text-ink-soft/60" : setUp ? "text-ink" : "border border-dashed border-accent-warn text-accent-warn",
                        covering && COVERED,
                        state(p),
                      )}
                    >
                      {n}
                    </button>
                    <OpenTrade trade={tr} country={c} onOpened={onOpened} className="absolute top-1/2 right-0 -translate-y-1/2 opacity-0 group-hover/cell:opacity-100 focus-visible:opacity-100" />
                  </td>
                );
              })}
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

/** The trade's window lives over the map: from another page, the map opens with it. */
function useOpenTrade(onOpened: () => void): (trade: number, country: Country | undefined) => void {
  const router = useRouter();
  const pathname = usePathname();
  return (trade, country) => {
    onOpened();
    if (pathname === "/") openTradeHere(trade, country);
    else router.push(`/?trade=${trade}${country === undefined ? "" : `&tab=${country}`}`);
  };
}

function OpenTrade({ trade, country, className, onOpened }: { trade: Trade; country?: Country; className: string; onOpened: () => void }) {
  const t = useT();
  const openTrade = useOpenTrade(onOpened);
  const label = t("scope.matrix.open", { category: country === undefined ? trade.name : `${trade.name} · ${country}` });
  return (
    <button
      type="button"
      onClick={() => openTrade(trade.id, country ?? trade.categories[0]?.country)}
      aria-label={label}
      title={label}
      className={cn("rounded bg-background p-0.5 text-ink-soft outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-ring", className)}
    >
      <Maximize2 aria-hidden className="size-3" />
    </button>
  );
}

/** A gmail's own window, its connections, over the map as a trade's is. */
function OpenAccount({ gmail, onOpened }: { gmail: Gmail; onOpened: () => void }) {
  const t = useT();
  const router = useRouter();
  const pathname = usePathname();
  const label = t("account.open", { gmail: gmail.gmail });
  return (
    <button
      type="button"
      onClick={() => {
        onOpened();
        if (pathname === "/") openAccount({ gmail: gmail.id, tab: "connections" });
        else router.push(`/?account=${gmail.id}&tab=connections`);
      }}
      aria-label={label}
      title={label}
      className="rounded p-1 text-ink-soft outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-ring"
    >
      <Plug aria-hidden className="size-3.5" />
    </button>
  );
}

function NewTrade({ onAdded }: { onAdded: () => void }) {
  const t = useT();
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  return (
    <form
      className="flex items-center gap-1 px-1 pt-1"
      onSubmit={(e) => {
        e.preventDefault();
        setBusy(true);
        void addTrade(name.trim())
          .then(() => {
            setName("");
            onAdded();
          })
          .catch((err: unknown) => notifyFailure(err, t))
          .finally(() => setBusy(false));
      }}
    >
      <Input size="sm" value={name} onChange={(e) => setName(e.target.value)} placeholder={t("trades.new.name")} aria-label={t("trades.new.name")} className="min-w-0 flex-1" />
      <button type="submit" disabled={busy || name.trim() === ""} aria-label={t("trades.new.add")} title={t("trades.new.add")} className="rounded p-1 text-ink-soft outline-none enabled:hover:text-ink disabled:opacity-40 focus-visible:ring-2 focus-visible:ring-ring">
        <Plus aria-hidden className="size-4" />
      </button>
    </form>
  );
}

function Room({ gmail, n }: { gmail: Gmail; n: number }) {
  const t = useT();
  return <span className="block truncate text-xs text-ink-soft tabular-nums">{[gmail.role && t(`gmbs.gmail.role.${gmail.role}`), t("account.gbps", { n })].filter(Boolean).join(" · ")}</span>;
}
