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
  DropdownMenuSeparator,
  DropdownMenuTrigger,
  Input,
  buttonVariants,
  cn,
} from "@evinvest/uikit";
import { Ellipsis, ListFilter, Pencil, Plus, Tag, Trash2, X } from "lucide-react";
import { useState } from "react";

import { chooseCategoryScope, closeWindow, googleName, openWindow, setOnly, useTrades, scopeKeys, trades as tradesKeys, gmbs as gmbsKeys, move } from "@/entities/gmb";
import { type Country, type Gmb, type Trade, countryValues } from "@/shared/api/schema";
import { api, ok } from "@/shared/api";
import { useLocale, useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize, useControlSize } from "@/shared/ui/touch";

import { CategoryTab } from "./category-tab";
import { categories } from "../model/i18n";

/** A trade's window over the map, shaped as a GBP's: its countries as tabs, side by side; the outline stays the one list of its GBPs. */
export function TradeWindow({ trade, country: tab, gmbs, page }: { trade: Trade; country: Country | null; gmbs: readonly Gmb[]; page: boolean }) {
  const t = useT();
  const button = useButtonSize();
  const [renaming, setRenaming] = useState(false);
  const held = gmbs.filter((b) => b.trade_id === trade.id);
  const here = tab === null ? held : held.filter((b) => b.country?.code === tab);
  const tabs = countryValues.filter((c) => c === tab || trade.categories.some((k) => k.country === c) || held.some((b) => b.country?.code === c));
  const rest = countryValues.filter((c) => !tabs.includes(c));
  const category = tab === null ? undefined : trade.categories.find((c) => c.country === tab);
  const countries = new Set(held.flatMap((b) => (b.country === null ? [] : [b.country.code]))).size;
  const whole = t(tradesKeys.window.trade, "the trade: {gbps} GBPs in {countries} countries", { gbps: held.length, countries });

  return (
    <section aria-label={trade.name} className="flex min-h-0 flex-1 flex-col">
      <header className="flex items-start gap-3 px-6 pt-4">
        <div className="flex min-w-0 flex-1 flex-col gap-0.5">
          {renaming ? (
            <Rename trade={trade} onDone={() => setRenaming(false)} />
          ) : (
            <h2 className="flex min-w-0 items-center gap-2 text-lg font-semibold text-ink">
              <Tag aria-hidden className="size-4 shrink-0 text-ink-soft" />
              <button type="button" onClick={() => openWindow({ kind: "categories" })} className="shrink-0 rounded font-medium text-ink-soft outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-ring">
                {t(scopeKeys.categories, "Categories")}
              </button>
              <span aria-hidden className="text-ink-soft/60">
                /
              </span>
              <span className="truncate">{tab === null ? trade.name : `${trade.name} · ${tab}`}</span>
            </h2>
          )}
          <p className="flex flex-wrap gap-x-3 text-sm text-ink-soft tabular-nums">
            <span>{t(tradesKeys.window.gbps, "{gbps} GBPs", { gbps: here.length })}</span>
            {tab !== null && <span className="text-ink-soft/70">{whole}</span>}
          </p>
        </div>
        <Button variant="outline" size={button("sm")} onClick={() => chooseCategoryScope([{ trade: trade.id, country: tab ?? "*" }])}>
          <ListFilter aria-hidden />
          {t(tradesKeys.window.scope, "Scope the outline to it")}
        </Button>
        <More trade={trade} tab={category === undefined ? null : tab} holders={held.length} onRename={() => setRenaming(true)} />
      </header>
      <nav role="tablist" aria-label={t(tradesKeys.window.countries, "Countries")} className="flex items-center gap-1 border-b border-border px-6 pt-3">
        {tabs.map((c) => {
          const setUp = trade.categories.some((k) => k.country === c);
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
          <CategoryTab key={`${trade.id}.${tab}`} trade={trade} category={category} here={here} page={page} />
        )}
      </div>
    </section>
  );
}

/** Every trade by country, managed: the level a trade's window sits under, opened from the scope picker, which only selects. */
export function CategoriesWindow({ trades, gmbs }: { trades: readonly Trade[]; gmbs: readonly Gmb[] }) {
  const t = useT();
  const locale = useLocale();
  const button = useButtonSize();
  const [added, setAdded] = useState<readonly Country[]>([]);
  const [renaming, setRenaming] = useState<number | null>(null);
  const traded = gmbs.filter((b) => b.trade_id !== null);
  const countries = countryValues.filter((c) => added.includes(c) || gmbs.some((b) => b.country?.code === c) || trades.some((tr) => tr.categories.some((k) => k.country === c)));
  const rest = countryValues.filter((c) => !countries.includes(c));
  const untraded = gmbs.length - traded.length;
  const cell = "flex h-14 w-full flex-col items-start justify-center gap-0.5 rounded-md px-2 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring";

  return (
    <div className="flex flex-col gap-6 p-6">
        <p className="-mt-2 text-sm text-ink-soft tabular-nums">{t(categories.counts, "{trades} trades · {countries} countries · {held} of {of} GBPs hold one", { trades: trades.length, countries: countries.length, held: traded.length, of: gmbs.length })}</p>
        <section className={SECTION}>
          <h3 className={SECTION_HEAD}>
            {t(categories.trades, "Trades")}
            <span className="flex-1" />
            <span className="font-normal tracking-normal normal-case">{t(categories.how, "a cell opens that country's tab · ⋯ renames or deletes the trade")}</span>
          </h3>
          {trades.length === 0 ? (
            <p className="text-sm text-ink-soft">{t(categories.none, "No trades yet: name one below.")}</p>
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
                {trades.map((tr) => {
                  const held = gmbs.filter((b) => b.trade_id === tr.id);
                  const where = new Set(held.flatMap((b) => (b.country === null ? [] : [b.country.code]))).size;
                  return (
                    <tr key={tr.id} className="hover:bg-hover/30 [&>*]:border-t [&>*]:border-border">
                      <th className="px-3 py-1 text-left font-normal">
                        {renaming === tr.id ? (
                          <Rename trade={tr} onDone={() => setRenaming(null)} />
                        ) : (
                          <button
                            type="button"
                            title={t(categories.open, "Open {trade}", { trade: tr.name })}
                            // shortcut: the first country column it is set up in, else the first column; upgrade once a trade has a view of its own
                            onClick={() => openWindow({ kind: "trade", trade: tr.id, country: countries.find((c) => tr.categories.some((k) => k.country === c)) ?? countries[0] ?? null })}
                            className="-mx-1 flex flex-col rounded px-1 text-left outline-none hover:bg-hover focus-visible:ring-2 focus-visible:ring-ring"
                          >
                            <span className="font-medium text-ink">{tr.name}</span>
                            <span className="text-xs text-ink-soft tabular-nums">{t(categories.row, "{gbps} GBPs · {countries} countries", { gbps: held.length, countries: where })}</span>
                          </button>
                        )}
                      </th>
                      {countries.map((c) => {
                        const category = tr.categories.find((k) => k.country === c);
                        const n = held.filter((b) => b.country?.code === c).length;
                        const name = `${tr.name} · ${c}`;
                        return (
                          <td key={c} className="p-1">
                            {category !== undefined ? (
                              <button type="button" title={t(scopeKeys.matrix.open, "Open {category}", { category: name })} onClick={() => openWindow({ kind: "trade", trade: tr.id, country: c })} className={cn(cell, "hover:bg-hover")}>
                                <span className={cn("font-semibold tabular-nums", n === 0 ? "text-ink-soft" : "text-ink")}>{n}</span>
                                <span className="max-w-full truncate text-xs text-ink-soft">
                                  {[t(categories.keywords, "{n} keywords", { n: category.keywords.length }), category.google_primary === null ? t(categories.noGoogle, "no Google category") : googleName(category.google_primary, locale)].join(" · ")}
                                </span>
                              </button>
                            ) : n > 0 ? (
                              <button type="button" title={t(scopeKeys.matrix.notSetUp, "{category}: GBPs there, not set up", { category: name })} onClick={() => openWindow({ kind: "trade", trade: tr.id, country: c })} className={cn(cell, "border border-dashed border-accent-warn text-accent-warn hover:bg-accent-warn/10")}>
                                <span className="font-semibold tabular-nums">{t(tradesKeys.window.tab.notSetUp, "{n} · not set up", { n })}</span>
                                <span className="flex items-center gap-1 text-xs">
                                  <Plus aria-hidden className="size-3" />
                                  {t(categories.setUp, "Set up")}
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
                      <td className="px-2 text-right">
                        <More trade={tr} tab={null} holders={held.length} onRename={() => setRenaming(tr.id)} />
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          )}
          <NewTrade />
          <p className="text-xs text-ink-soft">{t(categories.new.why, "A new trade has no country yet: a cell's Set up gives it one.")}</p>
        </section>
        {untraded > 0 && (
          <section className={SECTION}>
            <h3 className={SECTION_HEAD}>{t(tradesKeys.none, "No trade")}</h3>
            <p className="flex flex-wrap items-center gap-3 text-sm text-ink">
              <span className="min-w-0 flex-1">{t(categories.untraded, "{n} GBPs hold no trade: no category keywords to rank on, no Google category on Launch.", { n: untraded })}</span>
              <Button variant="outline" size={button("sm")} onClick={() => setOnly("no_category")}>
                <Tag aria-hidden />
                {t(categories.untraded.show, "Show them · No category {n}", { n: untraded })}
              </Button>
            </p>
          </section>
        )}
    </div>
  );
}

function NewTrade() {
  const t = useT();
  const size = useControlSize();
  const button = useButtonSize();
  const trades = useTrades();
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  return (
    <form
      className="flex items-center gap-2"
      onSubmit={(e) => {
        e.preventDefault();
        setBusy(true);
        void ok(api.POST("/trades", { body: { name: name.trim() } }))
          .then(() => {
            setName("");
            trades.reload();
          })
          .catch((err: unknown) => notifyFailure(err, t))
          .finally(() => setBusy(false));
      }}
    >
      <Input size={size} value={name} onChange={(e) => setName(e.target.value)} placeholder={t(tradesKeys.new.name, "A trade's name: Plumbing")} aria-label={t(tradesKeys.new.name, "A trade's name: Plumbing")} className="min-w-0 flex-1" />
      <Button type="submit" variant="outline" size={button("sm")} disabled={busy || name.trim() === ""}>
        <Plus aria-hidden />
        {t(tradesKeys.new.add, "Add the trade")}
      </Button>
    </form>
  );
}

/** The trade's name, in place of its title until saved. */
function Rename({ trade, onDone }: { trade: Trade; onDone: () => void }) {
  const t = useT();
  const size = useControlSize();
  const button = useButtonSize();
  const trades = useTrades();
  const [name, setName] = useState(trade.name);
  return (
    <form
      className="flex gap-2"
      onSubmit={(e) => {
        e.preventDefault();
        void ok(api.PATCH("/trades/{id}", { params: { path: { id: trade.id } }, body: { name: name.trim() } }))
          .then(() => {
            onDone();
            trades.reload();
          })
          .catch((err: unknown) => notifyFailure(err, t));
      }}
    >
      <Input size={size} autoFocus aria-label={t(tradesKeys.window.rename, "Rename")} value={name} onChange={(e) => setName(e.target.value)} />
      <Button type="submit" size={button("sm")} disabled={name.trim() === "" || name.trim() === trade.name}>
        {t(gmbsKeys.gbp.save, "Save")}
      </Button>
      <Button type="button" variant="ghost" size={button("sm")} onClick={onDone}>
        {t(move.cancel, "Cancel")}
      </Button>
    </form>
  );
}

/** Rename; no longer set up in the open country; delete, with its categories, only once no live GBP holds it. */
function More({ trade, tab, holders, onRename }: { trade: Trade; tab: Country | null; holders: number; onRename: () => void }) {
  const t = useT();
  const button = useButtonSize();
  const trades = useTrades();
  const [deleting, setDeleting] = useState(false);
  const act = (action: () => Promise<void>, after: () => void) => async () => {
    try {
      await action();
      after();
      trades.reload();
    } catch (e) {
      notifyFailure(e, t);
    }
  };
  const title = t(tradesKeys.window.delete, "Delete {trade}", { trade: trade.name });
  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button variant="ghost" size={button("sm")} icon aria-label={t(tradesKeys.window.more, "More")} title={t(tradesKeys.window.more, "More")}>
            <Ellipsis aria-hidden />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          <DropdownMenuItem onClick={onRename}>
            <Pencil aria-hidden />
            {t(tradesKeys.window.rename, "Rename")}
          </DropdownMenuItem>
          {tab !== null && (
            <DropdownMenuItem onClick={() => void act(() => ok(api.DELETE("/trades/{id}/{country}", { params: { path: { id: trade.id, country: tab } } })), () => undefined)()}>
              <X aria-hidden />
              {t(tradesKeys.window.unset, "No longer set up {category}", { category: `${trade.name} · ${tab}` })}
            </DropdownMenuItem>
          )}
          <DropdownMenuSeparator />
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
            <AlertDialogDescription>{t(tradesKeys.window.delete.body, "Its categories go with it. No GBP holds it.")}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel className={buttonVariants({ variant: "outline", size: button() })}>{t(move.cancel, "Cancel")}</AlertDialogCancel>
            <AlertDialogAction
              className={buttonVariants({ variant: "destructive", size: button() })}
              onClick={() => void act(() => ok(api.DELETE("/trades/{id}", { params: { path: { id: trade.id } } })), () => closeWindow({ kind: "trade", trade: trade.id, country: null }))()}
            >
              {t(gmbsKeys.remove, "Remove")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}

function NotSetUp({ trade, country, waiting }: { trade: Trade; country: Country; waiting: number }) {
  const t = useT();
  const button = useButtonSize();
  const trades = useTrades();
  const name = `${trade.name} · ${country}`;
  const setUp = async () => {
    try {
      await ok(api.PUT("/trades/{id}/{country}", { params: { path: { id: trade.id, country } }, body: { keywords: [], google_primary: null, google_additional: [], excluded: { words: [], types: [] }, demand: null } }));
      trades.reload();
    } catch (e) {
      notifyFailure(e, t);
    }
  };
  return (
    <div className="m-6 flex max-w-3xl flex-col items-start gap-3 rounded-lg border border-accent-warn/40 bg-accent-warn/5 px-4 py-3">
      <p className="text-sm text-ink">{waiting > 0 ? t(tradesKeys.window.waiting, "{n} GBPs wait for {category}.", { n: waiting, category: name }) : t(tradesKeys.window.notSetUp, "{category} is not set up.", { category: name })}</p>
      <Button size={button("sm")} onClick={() => void setUp()}>
        <Plus aria-hidden />
        {t(tradesKeys.window.setUp, "Set up {category}", { category: name })}
      </Button>
      <p className="text-xs text-ink-soft">{t(tradesKeys.window.willAsk, "It will ask: Google's categories · keywords in its language, weighed · what is never a competitor · its demand.")}</p>
    </div>
  );
}

const SECTION = "flex flex-col gap-2";
const SECTION_HEAD = "flex items-baseline gap-2 text-xs font-semibold tracking-wide text-ink-soft uppercase";
