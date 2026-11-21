"use client";

import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  Button,
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
  NativeSelect,
  NativeSelectOption,
  buttonVariants,
  cn,
} from "@evinvest/uikit";
import { useResource } from "@evinvest/api/react";
import { Ellipsis, Library, ListFilter, Plus, Tag, Trash2 } from "lucide-react";
import { useState } from "react";

import { TradePicker, categoryName, chooseCategoryScope, googleName, openWindow, setOnly, setUpCategory, useAdopt, useCategories, useTrades, scopeKeys, trades as tradesKeys, gmbs as gmbsKeys, move } from "@/entities/gmb";
import { type Category, type Country, type Gmb, type GoogleCategory, countryValues } from "@/shared/api/schema";
import { api, ok } from "@/shared/api";
import { state, useLocale, useT } from "@/shared/i18n";
import { failureText } from "@/shared/ui/failure-text";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize } from "@/shared/ui/touch";

import { CategoryTab } from "./category-tab";
import { categories as categoriesKeys } from "../model/i18n";

/** A trade's window over the map, shaped as a GBP's: its countries as tabs, side by side, each the caller's category there; the outline stays the one list of its GBPs. */
export function TradeWindow({ trade, country: tab, categories, gmbs, page }: { trade: GoogleCategory; country: Country | null; categories: readonly Category[]; gmbs: readonly Gmb[]; page: boolean }) {
  const t = useT();
  const locale = useLocale();
  const button = useButtonSize();
  const name = googleName(trade, locale);
  const held = gmbs.filter((b) => b.trade === trade.id);
  const here = tab === null ? held : held.filter((b) => b.country?.code === tab);
  const tabs = countryValues.filter((c) => c === tab || categories.some((k) => k.country === c) || held.some((b) => b.country?.code === c));
  const rest = countryValues.filter((c) => !tabs.includes(c));
  const category = tab === null ? undefined : categories.find((c) => c.country === tab);
  const countries = new Set(held.flatMap((b) => (b.country === null ? [] : [b.country.code]))).size;
  const whole = t(tradesKeys.window.trade, "the trade: {gbps} GBPs in {countries} countries", { gbps: held.length, countries });

  return (
    <section aria-label={name} className="flex min-h-0 flex-1 flex-col">
      <header className="flex items-start gap-3 px-6 pt-4">
        <div className="flex min-w-0 flex-1 flex-col gap-0.5">
          <h2 className="flex min-w-0 items-center gap-2 text-lg font-semibold text-ink">
            <Tag aria-hidden className="size-4 shrink-0 text-ink-soft" />
            <button type="button" onClick={() => openWindow({ kind: "categories" })} className="shrink-0 rounded font-medium text-ink-soft outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-ring">
              {t(scopeKeys.categories, "Categories")}
            </button>
            <span aria-hidden className="text-ink-soft/60">
              /
            </span>
            <span className="truncate">{tab === null ? name : categoryName(trade, tab, locale)}</span>
          </h2>
          <p className="flex flex-wrap gap-x-3 text-sm text-ink-soft tabular-nums">
            <span>{t(tradesKeys.window.gbps, "{gbps} GBPs", { gbps: here.length })}</span>
            {tab !== null && <span className="text-ink-soft/70">{whole}</span>}
          </p>
        </div>
        <Button variant="outline" size={button("sm")} onClick={() => chooseCategoryScope([{ trade: trade.id, country: tab ?? "*" }])}>
          <ListFilter aria-hidden />
          {t(tradesKeys.window.scope, "Scope the outline to it")}
        </Button>
        {category !== undefined && <More category={category} holders={here.length} />}
      </header>
      <nav role="tablist" aria-label={t(tradesKeys.window.countries, "Countries")} className="flex items-center gap-1 border-b border-border px-6 pt-3">
        {tabs.map((c) => {
          const setUp = categories.some((k) => k.country === c);
          const n = held.filter((b) => b.country?.code === c).length;
          return (
            <button
              key={c}
              type="button"
              role="tab"
              aria-selected={c === tab}
              onClick={() => openWindow({ kind: "trade", trade: trade.id, country: c })}
              className={cn(
                "-mb-px flex items-center gap-2 border-b-2 px-3 py-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring",
                c === tab ? "border-ink text-ink" : "border-transparent text-ink-soft hover:text-ink",
              )}
            >
              {c}
              <span className={cn("rounded px-1.5 font-mono text-xs tabular-nums", setUp ? "bg-hover" : "bg-accent-warn/15 text-accent-warn")}>
                {setUp ? n : t(tradesKeys.window.tab.notSetUp, "{n} · not set up", { n })}
              </span>
            </button>
          );
        })}
        {rest.length > 0 && (
          <DropdownMenu>
            <DropdownMenuTrigger className="-mb-px flex items-center gap-1 border-b-2 border-transparent px-3 py-2 text-sm text-ink-soft outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-ring">
              <Plus aria-hidden className="size-3.5" />
              {t(tradesKeys.window.country, "Country")}
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start">
              {rest.map((c) => (
                <DropdownMenuItem key={c} onClick={() => openWindow({ kind: "trade", trade: trade.id, country: c })}>
                  {c}
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
        )}
      </nav>
      <div role="tabpanel" className="flex min-h-0 flex-1 flex-col">
        {tab === null ? (
          <p className="p-6 text-sm text-ink-soft">{t(tradesKeys.window.pickCountry, "Not set up anywhere yet: add a country.")}</p>
        ) : category === undefined ? (
          <NotSetUp trade={trade} country={tab} waiting={here.length} />
        ) : (
          <CategoryTab key={`${trade.id}.${tab}`} category={category} here={here} page={page} />
        )}
      </div>
    </section>
  );
}

/** Every trade by country, managed: the level a trade's window sits under, opened from the scope picker, which only selects. */
export function CategoriesWindow({ gmbs }: { gmbs: readonly Gmb[] }) {
  const t = useT();
  const locale = useLocale();
  const button = useButtonSize();
  const categories = useCategories();
  const trades = useTrades(gmbs);
  const [added, setAdded] = useState<readonly Country[]>([]);
  if (trades === null || categories.status !== "ok") return <p className="p-6 text-sm text-ink-soft">{t(state.loading, "Loading…")}</p>;
  const traded = gmbs.filter((b) => b.trade !== null);
  const countries = countryValues.filter((c) => added.includes(c) || gmbs.some((b) => b.country?.code === c) || trades.some((tr) => tr.setUp.includes(c)));
  const rest = countryValues.filter((c) => !countries.includes(c));
  const untraded = gmbs.length - traded.length;
  const cell = "flex h-14 w-full flex-col items-start justify-center gap-0.5 rounded-md px-2 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring";

  return (
    <div className="flex flex-col gap-6 p-6">
        <p className="-mt-2 text-sm text-ink-soft tabular-nums">{t(categoriesKeys.counts, "{trades} trades · {countries} countries · {held} of {of} GBPs hold one", { trades: trades.length, countries: countries.length, held: traded.length, of: gmbs.length })}</p>
        <section className={SECTION}>
          <h3 className={SECTION_HEAD}>
            {t(categoriesKeys.trades, "Trades")}
            <span className="flex-1" />
            <span className="font-normal tracking-normal normal-case">{t(categoriesKeys.how, "a cell opens that country's tab")}</span>
          </h3>
          {trades.length === 0 ? (
            <p className="text-sm text-ink-soft">{t(categoriesKeys.none, "No trades yet: set one up below.")}</p>
          ) : (
            <table className="w-full table-fixed border-separate border-spacing-0 overflow-hidden rounded-lg border border-border text-sm">
              <thead className="bg-hover/50 text-xs">
                <tr>
                  <th className="w-44 px-3 py-2 text-left font-semibold text-ink">{t(tradesKeys.trade, "Trade")}</th>
                  {countries.map((c) => (
                    <th key={c} className="px-3 py-2 text-left font-semibold text-ink">
                      {c} <span className="font-normal text-ink-soft tabular-nums">{traded.filter((b) => b.country?.code === c).length}</span>
                    </th>
                  ))}
                  <th className="w-28 px-2 py-1 text-right font-normal">
                    {rest.length > 0 && (
                      <DropdownMenu>
                        <DropdownMenuTrigger className="flex items-center gap-1 rounded px-1 py-1 whitespace-nowrap text-ink-soft outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-ring">
                          <Plus aria-hidden className="size-3.5" />
                          {t(tradesKeys.window.country, "Country")}
                        </DropdownMenuTrigger>
                        <DropdownMenuContent align="end">
                          {rest.map((c) => (
                            <DropdownMenuItem key={c} onClick={() => setAdded([...added, c])}>
                              {c}
                            </DropdownMenuItem>
                          ))}
                        </DropdownMenuContent>
                      </DropdownMenu>
                    )}
                  </th>
                </tr>
              </thead>
              <tbody>
                {trades.map(({ trade: tr, setUp }) => {
                  const named = googleName(tr, locale);
                  const held = gmbs.filter((b) => b.trade === tr.id);
                  const where = new Set(held.flatMap((b) => (b.country === null ? [] : [b.country.code]))).size;
                  return (
                    <tr key={tr.id} className="hover:bg-hover/30 [&>*]:border-t [&>*]:border-border">
                      <th className="px-3 py-1 text-left font-normal">
                        <button
                          type="button"
                          title={t(categoriesKeys.open, "Open {trade}", { trade: named })}
                          // shortcut: the first country column it is set up in, else the first column; upgrade once a trade has a view of its own
                          onClick={() => openWindow({ kind: "trade", trade: tr.id, country: countries.find((c) => setUp.includes(c)) ?? countries[0] ?? null })}
                          className="-mx-1 flex flex-col rounded px-1 text-left outline-none hover:bg-hover focus-visible:ring-2 focus-visible:ring-ring"
                        >
                          <span className="font-medium text-ink">{named}</span>
                          <span className="text-xs text-ink-soft tabular-nums">{t(categoriesKeys.row, "{gbps} GBPs · {countries} countries", { gbps: held.length, countries: where })}</span>
                        </button>
                      </th>
                      {countries.map((c) => {
                        const category = categories.data.find((k) => k.trade.id === tr.id && k.country === c);
                        const n = held.filter((b) => b.country?.code === c).length;
                        const name = categoryName(tr, c, locale);
                        return (
                          <td key={c} className="p-1">
                            {category !== undefined ? (
                              <button type="button" title={t(scopeKeys.matrix.open, "Open {category}", { category: name })} onClick={() => openWindow({ kind: "trade", trade: tr.id, country: c })} className={cn(cell, "hover:bg-hover")}>
                                <span className={cn("font-semibold tabular-nums", n === 0 ? "text-ink-soft" : "text-ink")}>{n}</span>
                                <span className="max-w-full truncate text-xs text-ink-soft">
                                  {[t(categoriesKeys.keywords, "{n} keywords", { n: category.head.keywords.length }), category.lineage === null ? t(tradesKeys.head.own, "your own") : `${category.lineage.title} v${category.lineage.from.n}`].join(" · ")}
                                </span>
                              </button>
                            ) : n > 0 ? (
                              <button type="button" title={t(scopeKeys.matrix.notSetUp, "{category}: GBPs there, not set up", { category: name })} onClick={() => openWindow({ kind: "trade", trade: tr.id, country: c })} className={cn(cell, "border border-dashed border-accent-warn text-accent-warn hover:bg-accent-warn/10")}>
                                <span className="font-semibold tabular-nums">{t(tradesKeys.window.tab.notSetUp, "{n} · not set up", { n })}</span>
                                <span className="flex items-center gap-1 text-xs">
                                  <Plus aria-hidden className="size-3" />
                                  {t(categoriesKeys.setUp, "Set up")}
                                </span>
                              </button>
                            ) : (
                              <button type="button" aria-label={t(tradesKeys.window.setUp, "Set up {category}", { category: name })} onClick={() => openWindow({ kind: "trade", trade: tr.id, country: c })} className={cn(cell, "group/cell text-ink-soft hover:bg-hover hover:text-ink focus-visible:text-ink")}>
                                <span className="group-hover/cell:hidden group-focus-visible/cell:hidden">—</span>
                                <span className="hidden items-center gap-1.5 text-xs font-medium group-hover/cell:flex group-focus-visible/cell:flex">
                                  <Plus aria-hidden className="size-3.5" />
                                  {t(tradesKeys.window.setUp, "Set up {category}", { category: name })}
                                </span>
                              </button>
                            )}
                          </td>
                        );
                      })}
                      <td />
                    </tr>
                  );
                })}
              </tbody>
            </table>
          )}
          <NewCategory countries={countries} />
        </section>
        {untraded > 0 && (
          <section className={SECTION}>
            <h3 className={SECTION_HEAD}>{t(tradesKeys.none, "No trade")}</h3>
            <p className="flex flex-wrap items-center gap-3 text-sm text-ink">
              <span className="min-w-0 flex-1">{t(categoriesKeys.untraded, "{n} GBPs hold no trade: no category keywords to rank on, no Google category on Launch.", { n: untraded })}</span>
              <Button variant="outline" size={button("sm")} onClick={() => setOnly("no_category")}>
                <Tag aria-hidden />
                {t(categoriesKeys.untraded.show, "Show them · No category {n}", { n: untraded })}
              </Button>
            </p>
          </section>
        )}
    </div>
  );
}

/** A trade set up in a country: off Google's list, or a listing of the library. */
function NewCategory({ countries }: { countries: readonly Country[] }) {
  const t = useT();
  const [country, setCountry] = useState<Country>(countries[0] ?? countryValues[0]!);
  return (
    <span className="flex flex-wrap items-center gap-2">
      <span className="text-sm text-ink-soft">{t(categoriesKeys.new, "Set up a trade in")}</span>
      <NativeSelect size="sm" aria-label={t(tradesKeys.window.country, "Country")} value={country} onChange={(e) => setCountry(e.currentTarget.value as Country)}>
        {countryValues.map((c) => (
          <NativeSelectOption key={c} value={c}>
            {c}
          </NativeSelectOption>
        ))}
      </NativeSelect>
      <TradePicker value={null} country={country} label={t(categoriesKeys.new.pick, "A trade to set up")} onPick={(trade) => trade !== null && openWindow({ kind: "trade", trade, country })} />
    </span>
  );
}

/** No longer set up in its country, once no live GBP, review draft nor brand holds it. */
function More({ category, holders }: { category: Category; holders: number }) {
  const t = useT();
  const locale = useLocale();
  const button = useButtonSize();
  const categories = useCategories();
  const [deleting, setDeleting] = useState(false);
  const name = categoryName(category.trade, category.country, locale);
  const title = t(tradesKeys.window.unset, "No longer set up {category}", { category: name });
  const remove = async () => {
    try {
      await ok(api.DELETE("/categories/{trade}/{country}", { params: { path: { trade: category.trade.id, country: category.country } } }));
      categories.reload();
    } catch (e) {
      notifyFailure(e, t);
    }
  };
  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button variant="ghost" size={button("sm")} icon aria-label={t(tradesKeys.window.more, "More")} title={t(tradesKeys.window.more, "More")}>
            <Ellipsis aria-hidden />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          <DropdownMenuItem variant="destructive" disabled={holders > 0} onClick={() => setDeleting(true)}>
            <Trash2 aria-hidden />
            <span className="flex flex-col">
              {title}
              {holders > 0 && <span className="text-xs text-accent-warn">{t(tradesKeys.window.delete.held, "{n} GBPs hold it: give them another trade first", { n: holders })}</span>}
            </span>
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      <AlertDialog open={deleting} onOpenChange={setDeleting}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{title}</AlertDialogTitle>
            <AlertDialogDescription>{t(tradesKeys.window.unset.body, "Its settings go; its history is kept, and setting it up again starts over.")}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel className={buttonVariants({ variant: "outline", size: button() })}>{t(move.cancel, "Cancel")}</AlertDialogCancel>
            <AlertDialogAction className={buttonVariants({ variant: "destructive", size: button() })} onClick={() => void remove()}>
              {t(gmbsKeys.remove, "Remove")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}

/** Set up with its name alone, or off a listing of the library for it there. */
function NotSetUp({ trade, country, waiting }: { trade: GoogleCategory; country: Country; waiting: number }) {
  const t = useT();
  const locale = useLocale();
  const button = useButtonSize();
  const categories = useCategories();
  const adopting = useAdopt();
  const listings = useResource(api, "/library", { params: { query: { trade: trade.id, country, sort: "rank" } } });
  const name = categoryName(trade, country, locale);
  const setUp = async () => {
    try {
      await setUpCategory(trade, country);
      categories.reload();
    } catch (e) {
      notifyFailure(e, t);
    }
  };
  return (
    <div className="m-6 flex max-w-3xl flex-col items-start gap-3 rounded-lg border border-accent-warn/40 bg-accent-warn/5 px-4 py-3">
      <p className="text-sm text-ink">{waiting > 0 ? t(tradesKeys.window.waiting, "{n} GBPs wait for {category}.", { n: waiting, category: name }) : t(tradesKeys.window.notSetUp, "{category} is not set up.", { category: name })}</p>
      <span className="flex flex-wrap gap-2">
        <Button size={button("sm")} onClick={() => void setUp()}>
          <Plus aria-hidden />
          {t(tradesKeys.window.setUp, "Set up {category}", { category: name })}
        </Button>
        <Button variant="outline" size={button("sm")} onClick={() => openWindow({ kind: "library", listing: null })}>
          <Library aria-hidden />
          {t(tradesKeys.picker.library, "Browse the library…")}
        </Button>
      </span>
      <p className="text-xs text-ink-soft">{t(tradesKeys.window.willAsk, "Set up, it holds its name as its one keyword; its Definition asks the rest: additional categories · keywords, weighed · what is never a competitor · its demand.")}</p>
      {listings.status === "error" && <p className="text-xs text-accent-error">{failureText(listings.failure, t)}</p>}
      {listings.status === "ok" && listings.data.listings.length > 0 && (
        <section className="flex w-full flex-col gap-1">
          <h4 className={SECTION_HEAD}>{t(tradesKeys.window.listings, "Or start off a listing")}</h4>
          <ul className="flex flex-col divide-y divide-border">
            {listings.data.listings.map((l) => (
              <li key={l.id} className="flex items-center gap-3 py-1.5 text-sm">
                <button type="button" onClick={() => openWindow({ kind: "library", listing: l.id })} className="min-w-0 flex-1 truncate rounded text-left text-ink outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring">
                  {l.title} <span className="text-xs text-ink-soft">v{l.latest.n} · {l.author}</span>
                </button>
                {l.standard_rank !== null && <span className="text-xs text-ink-soft">{t(tradesKeys.library.standard, "standard")}</span>}
                <Button variant="outline" size={button("sm")} onClick={() => adopting.adopt(l, l.latest, () => undefined)}>
                  {t(tradesKeys.library.use, "Use")}
                </Button>
              </li>
            ))}
          </ul>
        </section>
      )}
      {adopting.dialog}
    </div>
  );
}

const SECTION = "flex flex-col gap-2";
const SECTION_HEAD = "flex items-baseline gap-2 text-xs font-semibold tracking-wide text-ink-soft uppercase";
