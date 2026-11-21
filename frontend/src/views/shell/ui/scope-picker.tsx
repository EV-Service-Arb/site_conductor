"use client";

import { Popover, PopoverContent, PopoverTrigger, Switch, cn } from "@evinvest/uikit";
import { ArrowUpRight, Check, ChevronDown, ChevronRight, ListFilter, Plug, Plus, Settings } from "lucide-react";
import { type MouseEvent, useEffect, useState } from "react";

import { BrandMark, NewBrand, brandKeys, chooseBrandScope, chooseCategoryScope, chooseGmailScope, covers, inCategories, openWindow, type Pattern, type Selection, under, useBrands, useScope, useTrades, type Traded, categoryName, googleName, scopeKeys, gmbs as gmbsKeys } from "@/entities/gmb";
import { AddGmailButton } from "@/features/add-gmail";
import { AddGmbButton } from "@/features/add-gmb";
import { api, ok } from "@/shared/api";
import { type Country, type Gmail, type Gmails, type Gmb, countryValues } from "@/shared/api/schema";
import { useLocale, useT, account } from "@/shared/i18n";

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
 * managing gmails, then the brands under both. A gmail toggles among the filters (or, after a move, all its GBPs), a GBP opened under it toggles alone.
 */
export function ScopePicker({ data, selection, onFlip, onChanged }: { data: Gmails; selection: Selection; onFlip: (gmbs: Gmb[], enabled: boolean) => Promise<void>; onChanged: () => void }) {
  const t = useT();
  const locale = useLocale();
  const trades = useTrades(data.gmbs);
  const scope = useScope();
  const [open, setOpen] = useState(false);
  const [opened, setOpened] = useState<ReadonlySet<number>>(new Set());
  const openSettings = (trade: string, country: Country | undefined) => {
    setOpen(false);
    openWindow({ kind: "trade", trade, country: country ?? null });
  };
  const { scoped, chosen, edited, isSelected, move } = selection;
  const { gmails } = data;
  const known = trades;

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
    const named = known?.find((k) => k.trade.id === p.trade)?.trade;
    const trade = p.trade === "*" ? null : named === undefined ? "…" : googleName(named, locale);
    return [trade, p.country === "*" ? null : p.country].filter(Boolean).join(" · ");
  };
  const holding = gmails.filter((g) => under(data, g.id).some(byCategory));
  const brands = useBrands();
  const brandName = (id: number) => (brands.status === "ok" ? brands.data.find((b) => b.id === id)?.name : undefined) ?? "…";
  const summary = [
    scope.categories === null ? t(scopeKeys.categories.all, "All categories") : scope.categories.length === 1 ? name(scope.categories[0]!) : t(scopeKeys.categories.many, "{n} categories", { n: scope.categories.length }),
    scoped.length === 1 ? scoped[0]!.gmail : scoped.length === 0 && holding.length === 1 ? holding[0]!.gmail : t(gmbsKeys.scope.many, "{n} gmails", { n: scoped.length === 0 ? holding.length : scoped.length }),
    scope.brands === null ? null : scope.brands.size === 1 ? brandName([...scope.brands][0]!) : t(brandKeys.scope.many, "{n} brands", { n: scope.brands.size }),
  ]
    .filter(Boolean)
    .join(" · ");

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
          <ManageCategories onOpened={() => setOpen(false)} />
          <span className="flex-1" />
          {known !== null && (
            <span className={META}>
              {t(scopeKeys.categories.counts, "{trades} trades · {countries} countries", { trades: known.length, countries: new Set(known.flatMap((tr) => tr.setUp)).size })}
            </span>
          )}
        </div>
        {known !== null && <Matrix all={data.gmbs} gmbs={data.gmbs.filter(byGmail)} trades={known} picked={scope.categories} onSettings={openSettings} />}
        <p className="px-2 pt-1 text-xs text-ink-soft">{t(scopeKeys.matrix.how, "Click scopes · Shift-click adds")}</p>
        {data.gmbs.some((b) => b.trade === null) && <p className="px-2 text-xs text-ink-soft">{t(scopeKeys.matrix.untraded, "{n} GBPs have no trade → the outline's No category chip", { n: data.gmbs.filter((b) => b.trade === null).length })}</p>}
        <div className={cn(HEADING, "mt-1 border-t border-border pt-2")}>
          {t(scopeKeys.gmails, "Managing gmails")}
          <span className="flex-1" />
          <span className={cn(META, "tabular-nums")}>
            {scope.categories === null ? t(gmbsKeys.scope.counts, "{gmails} · {gmbs} GMBs", { gmails: gmails.length, gmbs: data.gmbs.length }) : t(scopeKeys.gmails.holding, "{n} of {of} hold it", { n: holding.length, of: gmails.length })}
          </span>
        </div>
        <button
          type="button"
          onClick={() => chooseGmailScope(null)}
          aria-pressed={scoped.length === 0}
          className={cn(ROW, "w-full py-1.5 pl-2", scoped.length === 0 && PICKED)}
        >
          <span className="flex-1 text-left">{t(scopeKeys.gmails.all, "All gmails")}</span>
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
                    aria-label={(unfolded ? t(gmbsKeys.collapse, "Hide {gmail}'s GMBs", { gmail: g.gmail }) : t(gmbsKeys.expand, "Show {gmail}'s GMBs", { gmail: g.gmail }))}
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
                    <Room n={gmbs.length} />
                  </button>
                  <span className={cn("flex items-center", ON_HOVER)}>
                    <OpenAccount gmail={g} onOpened={() => setOpen(false)} />
                    <AddGmbButton gmail={g.id} trade={null} />
                    <Remove title={t(gmbsKeys.gmail.remove.title, "Remove {gmail}", { gmail: g.gmail })} remove={() => ok(api.DELETE("/gmails/{id}", { params: { path: { id: g.id } } }))} onRemoved={onChanged} />
                  </span>
                  <span className="text-xs text-ink-soft tabular-nums" title={t(scopeKeys.gmail.count, "Its GBPs in the categories picked")}>
                    {edited && k > 0 && !full ? `${k}/${gmbs.length}` : inScope}
                  </span>
                  <Switch aria-label={t(gmbsKeys.switch, "{name} on or off", { name: g.gmail })} checked={on} disabled={gmbs.length === 0} onCheckedChange={(v) => void onFlip(gmbs, v)} />
                </div>
                {unfolded && (
                  <ul className="ml-4 flex flex-col border-l border-border pl-1">
                    {gmbs.length === 0 && <li className="px-2 py-1.5 text-xs text-ink-soft">{t(gmbsKeys.gmail.empty, "No GMBs under this gmail yet.")}</li>}
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
        <Brands data={data} onOpened={() => setOpen(false)} />
        <div className="flex items-center gap-1 border-t border-border pt-1">
          <AddGmailButton onAdded={onChanged} />
          <span className="text-xs text-ink-soft">{t(scopeKeys.everyApp, "scope applies in every app")}</span>
        </div>
      </PopoverContent>
    </Popover>
  );
}

/**
 * Trades by the countries that have GBPs or categories: a cell counts the GBPs of that category under the gmails picked. Blank:
 * not set up and holding none; `0`: set up and holding none; dashed, in the warning tone: GBPs there, the trade not set up.
 */
function Matrix({ all, gmbs, trades, picked, onSettings }: { all: readonly Gmb[]; gmbs: Gmb[]; trades: readonly Traded[]; picked: readonly Pattern[] | null; onSettings: (trade: string, country: Country | undefined) => void }) {
  const t = useT();
  const locale = useLocale();
  const countries = countryValues.filter((c) => all.some((b) => b.country?.code === c) || trades.some((tr) => tr.setUp.includes(c)));
  if (trades.length === 0) return <p className="px-2 py-1.5 text-xs text-ink-soft">{t(scopeKeys.categories.none, "No trades yet: Categories ↗ sets one up.")}</p>;
  const pick = (p: Pattern) => (e: MouseEvent) => {
    const now = picked ?? [];
    const has = now.some((q) => same(q, p));
    if (adding(e)) chooseCategoryScope(has ? now.filter((q) => !same(q, p)) : [...now, p]);
    else chooseCategoryScope(has && now.length === 1 ? null : [p]);
  };
  const state = (p: Pattern) => (picked?.some((q) => same(q, p)) ? PICKED : null);
  const count = (trade: string, country: Country | "*") => gmbs.filter((b) => inCategories(b, [{ trade, country }])).length;
  const box = "h-7 rounded-md text-xs tabular-nums outline-none hover:bg-hover/60 focus-visible:ring-2 focus-visible:ring-ring";
  const settings = (label: string, open: () => void, className: string) => (
    <button type="button" aria-label={label} title={label} onClick={open} className={cn("items-center justify-center rounded text-ink-soft outline-none hover:bg-hover hover:text-ink focus-visible:ring-2 focus-visible:ring-ring", className)}>
      <ArrowUpRight aria-hidden className="size-3" />
    </button>
  );
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
              <span className="flex-1 text-left">{t(scopeKeys.matrix.all, "All")}</span>
              <span>{gmbs.filter((b) => b.trade !== null).length}</span>
            </button>
          </th>
          {countries.map((c) => {
            const p: Pattern = { trade: "*", country: c };
            return (
              <th key={c} className="w-14 font-normal">
                <button type="button" aria-pressed={state(p) !== null} title={t(scopeKeys.matrix.country, "Every trade in {country}", { country: c })} onClick={pick(p)} className={cn(box, "flex h-9 w-full flex-col items-center justify-center leading-tight", state(p))}>
                  <span className="font-semibold text-ink">{c}</span>
                  <span className="text-ink-soft">{count("*", c)}</span>
                </button>
              </th>
            );
          })}
        </tr>
      </thead>
      <tbody>
        {trades.map(({ trade, setUp: there }) => {
          const tr = { id: trade.id, name: googleName(trade, locale) };
          const row: Pattern = { trade: tr.id, country: "*" };
          return (
            <tr key={tr.id}>
              <th className="font-normal">
                <span className="group flex items-center">
                  <button
                    type="button"
                    aria-pressed={state(row) !== null}
                    title={t(scopeKeys.matrix.trade, "{trade} in every country", { trade: tr.name })}
                    onClick={pick(row)}
                    className={cn(box, "flex min-w-0 flex-1 items-center gap-2 px-2 text-sm", there.length === 0 ? "text-ink-soft" : "text-ink", state(row))}
                  >
                    <span className="min-w-0 flex-1 truncate text-left">{tr.name}</span>
                    <span className="text-xs text-ink-soft">{count(tr.id, "*")}</span>
                  </button>
                  {settings(t(scopeKeys.matrix.open, "Open {category}", { category: tr.name }), () => onSettings(tr.id, countries.find((c) => there.includes(c))), cn("size-5 shrink-0", ON_HOVER))}
                </span>
              </th>
              {countries.map((c) => {
                const p: Pattern = { trade: tr.id, country: c };
                const setUp = there.includes(c);
                const n = count(tr.id, c);
                const covering = state(p) === null && picked !== null && picked.some((q) => covers(q, tr.id, c));
                if (!setUp && n === 0) return <td key={c} />;
                return (
                  <td key={c} className="group/cell relative">
                    <button
                      type="button"
                      aria-pressed={state(p) !== null}
                      title={setUp ? categoryName(trade, c, locale) : t(scopeKeys.matrix.notSetUp, "{category}: GBPs there, not set up", { category: categoryName(trade, c, locale) })}
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
                    {settings(t(scopeKeys.matrix.open, "Open {category}", { category: categoryName(trade, c, locale) }), () => onSettings(tr.id, c), "absolute top-0 right-0 hidden size-4 bg-popover group-hover/cell:flex group-focus-within/cell:flex")}
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

/** The categories' own window, where they are managed, opened by the heading; the picker only selects. */
function ManageCategories({ onOpened }: { onOpened: () => void }) {
  const t = useT();
  return (
    <button
      type="button"
      onClick={() => {
        onOpened();
        openWindow({ kind: "categories" });
      }}
      title={t(scopeKeys.categories.manage, "Manage categories")}
      className="flex items-center gap-1 rounded p-0.5 tracking-wide uppercase outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-ring"
    >
      {t(scopeKeys.categories, "Categories")}
      <Settings aria-hidden className="size-3.5" />
    </button>
  );
}

/** A gmail's own window, its connections, over the map as a trade's is. */
function OpenAccount({ gmail, onOpened }: { gmail: Gmail; onOpened: () => void }) {
  const t = useT();
  const label = t(account.open, "Connections of {gmail}", { gmail: gmail.gmail });
  return (
    <button
      type="button"
      onClick={() => {
        onOpened();
        openWindow({ kind: "account", gmail: gmail.id, tab: "connections" });
      }}
      aria-label={label}
      title={label}
      className="rounded p-1 text-ink-soft outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-ring"
    >
      <Plug aria-hidden className="size-3.5" />
    </button>
  );
}

function Room({ n }: { n: number }) {
  const t = useT();
  return <span className="block truncate text-xs text-ink-soft tabular-nums">{t(account.gbps, "{n} GBP", { n })}</span>;
}

/**
 * The brands under the gmails and categories picked: a brand is one gmail's in one category, so a new one needs exactly one of
 * each picked. A brand toggles in the scope; its arrow opens it.
 */
function Brands({ data, onOpened }: { data: Gmails; onOpened: () => void }) {
  const t = useT();
  const scope = useScope();
  const locale = useLocale();
  const brands = useBrands();
  const trades = useTrades(data.gmbs);
  const [adding, setAdding] = useState(false);
  const known = brands.status === "ok" ? brands.data : null;

  // a scoped brand removed, here or in another tab: out of the scope
  useEffect(() => {
    if (known !== null && scope.brands !== null && [...scope.brands].some((id) => !known.some((b) => b.id === id))) chooseBrandScope([...scope.brands].filter((id) => known.some((b) => b.id === id)));
  }, [known, scope.brands]);

  if (known === null) return null;
  const shown = known.filter((b) => (scope.gmails === null || scope.gmails.has(b.gmail_id)) && (scope.categories === null || scope.categories.some((p) => covers(p, b.trade, b.country))));
  const gmail = scope.gmails?.size === 1 ? [...scope.gmails][0]! : null;
  const one = scope.categories?.length === 1 ? scope.categories[0]! : null;
  const category = one !== null && one.trade !== "*" && one.country !== "*" ? { trade: one.trade, country: one.country } : null;
  const tradeName = (id: string) => {
    const named = trades?.find((tr) => tr.trade.id === id)?.trade;
    return named === undefined ? "…" : googleName(named, locale);
  };
  const gmailName = (id: number) => data.gmails.find((g) => g.id === id)?.gmail ?? "…";
  const toggle = (id: number) => {
    const next = new Set(scope.brands ?? []);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    chooseBrandScope(next);
  };
  return (
    <>
      <div className={cn(HEADING, "mt-1 border-t border-border pt-2")}>
        {t(brandKeys.scope, "Brands")}
        <span className="flex-1" />
        <span className={META}>{t(brandKeys.scope.what, "one gmail's, in one category")}</span>
      </div>
      {shown.length > 0 && (
        <button type="button" onClick={() => chooseBrandScope(null)} aria-pressed={scope.brands === null} className={cn(ROW, "w-full py-1.5 pl-2", scope.brands === null && PICKED)}>
          <span className="flex-1 text-left">{t(brandKeys.scope.all, "Any brand, or none")}</span>
        </button>
      )}
      <ul className="flex flex-col">
        {shown.map((b) => {
          const picked = scope.brands?.has(b.id) === true;
          return (
            <li key={b.id} className={cn(ROW, picked && PICKED)}>
              <button type="button" onClick={() => toggle(b.id)} aria-pressed={picked} className="flex min-w-0 flex-1 items-center gap-2 py-1 pl-1 text-left">
                <BrandMark brand={b} className="size-5 text-xs" />
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-ink">{b.name}</span>
                  <span className="block truncate text-xs text-ink-soft">{[gmailName(b.gmail_id), `${tradeName(b.trade)} · ${b.country}`].join(" · ")}</span>
                </span>
              </button>
              <span className={cn("flex items-center", ON_HOVER)}>
                <button
                  type="button"
                  aria-label={t(brandKeys.open, "Open {brand}", { brand: b.name })}
                  title={t(brandKeys.open, "Open {brand}", { brand: b.name })}
                  onClick={() => {
                    onOpened();
                    openWindow({ kind: "brand", brand: b.id });
                  }}
                  className="rounded p-1 text-ink-soft outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-ring"
                >
                  <ArrowUpRight aria-hidden className="size-3.5" />
                </button>
              </span>
              <span className="text-xs text-ink-soft tabular-nums">{b.gmbs}</span>
            </li>
          );
        })}
      </ul>
      {gmail !== null && category !== null ? (
        adding ? (
          <div className="px-1 py-1">
            <NewBrand
              gmail={gmail}
              trade={category.trade}
              country={category.country}
              onDone={(added) => {
                setAdding(false);
                if (added !== null) {
                  onOpened();
                  openWindow({ kind: "brand", brand: added.id });
                }
              }}
            />
          </div>
        ) : (
          <button type="button" onClick={() => setAdding(true)} className={cn(ROW, "w-full gap-1.5 py-1.5 pl-2 text-ink-soft hover:text-ink")}>
            <Plus aria-hidden className="size-3.5" />
            {t(brandKeys.add.under, "Brand under {gmail} · {category}", { gmail: gmailName(gmail), category: `${tradeName(category.trade)} · ${category.country}` })}
          </button>
        )
      ) : (
        <p className="px-2 py-1 text-xs text-ink-soft">{t(brandKeys.add.how, "Pick one gmail and one category to add a brand under them")}</p>
      )}
    </>
  );
}
