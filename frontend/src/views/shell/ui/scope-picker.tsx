"use client";

import { InfoTip, InfoTipContent, InfoTipTrigger, Input, NativeSelect, NativeSelectOption, Popover, PopoverContent, PopoverTrigger, Switch, cn } from "@evinvest/uikit";
import { Check, ChevronDown, ChevronRight, ListFilter, Maximize2, Plus } from "lucide-react";
import { usePathname, useRouter } from "next/navigation";
import { type MouseEvent, useEffect, useState } from "react";

import {
  COUNTRIES,
  type Country,
  GMAIL_ROLES,
  type Gmail,
  type GmailRole,
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
  openTrade as openTradeHere,
  removeGmail,
  select,
  setGmailRole,
  under,
  useScope,
  useTrades,
} from "@/entities/gmb";
import { AddGmailButton } from "@/features/add-gmail";
import { AddGmbButton } from "@/features/add-gmb";
import { useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";

import { HEADING, ON_HOVER, ROW, Remove } from "./outline-parts";

const same = (a: Pattern, b: Pattern) => a.trade === b.trade && a.country === b.country;
/** Shift, Ctrl or Cmd: a click adds to what is picked rather than replacing it. */
const adding = (e: MouseEvent) => e.shiftKey || e.ctrlKey || e.metaKey;
const PICKED = "bg-hover font-medium text-ink ring-1 ring-accent-info ring-inset";
const COVERED = "bg-hover/40 ring-1 ring-accent-info/40 ring-inset";

/**
 * What the outline is scoped to, one picker over three facets that narrow each other's counts: the categories as a matrix of
 * trades by country (a cell a category, a row header the trade everywhere, a column header the country across trades), then the
 * gmails as they are. A gmail toggles among the filters (or, after a move, all its GBPs), a GBP opened under it toggles alone.
 */
export function ScopePicker({ data, selection, onFlip, onChanged }: { data: Gmails; selection: Selection; onFlip: (ids: number[], enabled: boolean) => Promise<void>; onChanged: () => void }) {
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
  const summary =
    [
      scope.categories === null ? null : scope.categories.length === 1 ? name(scope.categories[0]!) : t("scope.categories.many", { n: scope.categories.length }),
      scoped.length === 0 ? null : scoped.length === 1 ? scoped[0]!.gmail : t("gmbs.scope.many", { n: scoped.length }),
    ]
      .filter(Boolean)
      .join(" · ") || t("gmbs.scope.all");

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger className="mx-0.5 flex items-center gap-2 rounded-md border border-border px-2.5 py-2 text-sm text-ink outline-none hover:bg-hover/50 focus-visible:ring-2 focus-visible:ring-ring">
        <ListFilter aria-hidden className="size-4 text-ink-soft" />
        <span className="min-w-0 flex-1 truncate text-left">{summary}</span>
        <span className="text-xs text-ink-soft tabular-nums">{selection.selected.length}</span>
        <ChevronDown aria-hidden className="size-4 text-ink-soft" />
      </PopoverTrigger>
      <PopoverContent align="start" className="max-h-[70svh] w-96 overflow-y-auto p-1">
        <button type="button" onClick={() => select(null)} aria-pressed={chosen === null} className={cn(ROW, "w-full py-1.5 pl-2", chosen === null && PICKED)}>
          <span className="flex-1 text-left">{t("gmbs.scope.all")}</span>
          <span className="text-xs text-ink-soft tabular-nums">{data.gmbs.length}</span>
        </button>
        <div className={HEADING}>
          {t("scope.categories")}
          <InfoTip>
            <InfoTipTrigger label={t("gmbs.what")} />
            <InfoTipContent className="font-normal tracking-normal normal-case">{t("scope.categories.hint")}</InfoTipContent>
          </InfoTip>
        </div>
        {known === null ? null : <Matrix gmbs={data.gmbs.filter(byGmail)} trades={known} picked={scope.categories} />}
        <NewTrade onAdded={trades.reload} />
        <div className={cn(HEADING, "border-t border-border pt-2")}>
          {t("scope.gmails")}
          <InfoTip>
            <InfoTipTrigger label={t("gmbs.what")} />
            <InfoTipContent className="font-normal tracking-normal normal-case">{t("gmbs.gmail.hint")}</InfoTipContent>
          </InfoTip>
          <span className="flex-1" />
          <span className="tabular-nums">{t("gmbs.scope.counts", { gmails: gmails.length, gmbs: data.gmbs.length })}</span>
        </div>
        <ul className="flex flex-col">
          {gmails.map((g) => {
            const gmbs = under(data, g.id);
            const inScope = gmbs.filter(byCategory);
            const on = gmbs.length > 0 && gmbs.every((b) => b.enabled);
            const k = chosen === null ? 0 : covered(gmbs);
            const full = k > 0 && k === gmbs.length;
            const unfolded = opened.has(g.id);
            return (
              <li key={g.id} className={cn(scope.categories !== null && inScope.length === 0 && "opacity-50")}>
                <div className={cn(ROW, full && PICKED, k > 0 && !full && COVERED)}>
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
                    aria-pressed={full ? true : k > 0 ? "mixed" : false}
                    className={cn("min-w-0 flex-1 truncate py-1.5 text-left", on ? "text-ink" : "text-ink-soft", k > 0 && "font-medium")}
                    title={g.gmail}
                  >
                    {g.gmail}
                  </button>
                  <span className={cn("flex items-center", ON_HOVER)}>
                    <AddGmbButton gmailId={g.id} trade={null} onAdded={onChanged} />
                    <Remove title={t("gmbs.gmail.remove.title", { gmail: g.gmail })} body={t("gmbs.gmail.remove.body")} remove={() => removeGmail(g.id)} onRemoved={onChanged} />
                  </span>
                  {scope.categories !== null && <span className="text-xs text-ink-soft tabular-nums" title={t("scope.gmail.inScope")}>{`${inScope.length} ·`}</span>}
                  {k > 0 && !full ? <span className="text-xs text-ink-soft tabular-nums">{`${k}/${gmbs.length}`}</span> : <Room gmail={g} n={gmbs.length} />}
                  <Switch aria-label={t("gmbs.switch", { name: g.gmail })} checked={on} disabled={gmbs.length === 0} onCheckedChange={(v) => void onFlip(gmbs.map((b) => b.id), v)} />
                </div>
                {unfolded && (
                  <ul className="ml-4 flex flex-col border-l border-border pl-1">
                    <li className="flex items-center gap-2 px-2 py-1 text-xs text-ink-soft">
                      <RoleSelect gmail={g} onChanged={onChanged} />
                      {t("gmbs.gmail.role.hint")}
                    </li>
                    {gmbs.length === 0 && <li className="px-2 py-1.5 text-xs text-ink-soft">{t("gmbs.gmail.empty")}</li>}
                    {gmbs.map((b) => {
                      const picked = chosen !== null && isSelected(b.id);
                      return (
                        <li key={b.id}>
                          <button
                            type="button"
                            onClick={() => move([b.id], !picked)}
                            aria-pressed={picked}
                            title={b.address ? `${b.name}\n${b.address}` : b.name}
                            className={cn(
                              "flex w-full items-center gap-1 rounded-md py-1 pr-1 pl-2 text-left text-sm outline-none hover:bg-hover/50 focus-visible:ring-2 focus-visible:ring-ring",
                              picked && PICKED,
                              b.enabled ? "text-ink" : "text-ink-soft",
                              !byCategory(b) && "opacity-50",
                            )}
                          >
                            <span className="min-w-0 flex-1 truncate">{b.name}</span>
                            {picked && <Check aria-hidden className="size-3.5 shrink-0" />}
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
        <div className="border-t border-border pt-1">
          <AddGmailButton onAdded={onChanged} />
        </div>
      </PopoverContent>
    </Popover>
  );
}

/**
 * Trades by the countries that have GBPs or categories: a cell counts the GBPs of that category under the gmails picked. Blank:
 * not set up and holding none; `0`: set up and holding none; a count in the warning tone: GBPs there, the trade not set up.
 */
function Matrix({ gmbs, trades, picked }: { gmbs: Gmb[]; trades: readonly Trade[]; picked: readonly Pattern[] | null }) {
  const t = useT();
  const openTrade = useOpenTrade();
  const countries = COUNTRIES.filter((c) => gmbs.some((b) => b.trade_id !== null && b.country?.code === c) || trades.some((tr) => tr.categories.some((k) => k.country === c)));
  if (trades.length === 0) return <p className="px-2 py-1.5 text-xs text-ink-soft">{t("scope.categories.none")}</p>;
  const pick = (p: Pattern) => (e: MouseEvent) => {
    const now = picked ?? [];
    const has = now.some((q) => same(q, p));
    if (adding(e)) chooseCategoryScope(has ? now.filter((q) => !same(q, p)) : [...now, p]);
    else chooseCategoryScope(has && now.length === 1 ? null : [p]);
  };
  const state = (p: Pattern) => (picked?.some((q) => same(q, p)) ? PICKED : null);
  const count = (trade: number | "*", country: Country | "*") => gmbs.filter((b) => inCategories(b, [{ trade, country }])).length;
  const head = "rounded px-1.5 py-1 text-xs tabular-nums outline-none hover:bg-hover/50 focus-visible:ring-2 focus-visible:ring-ring";
  return (
    <table className="mx-1 mb-1 w-[calc(100%-0.5rem)] border-separate border-spacing-0.5 text-sm">
      <thead>
        <tr>
          <th className="text-left text-xs font-normal text-ink-soft">{t("scope.matrix.corner")}</th>
          {countries.map((c) => {
            const p: Pattern = { trade: "*", country: c };
            return (
              <th key={c} className="w-12 font-normal">
                <button type="button" aria-pressed={state(p) !== null} title={t("scope.matrix.country", { country: c })} onClick={pick(p)} className={cn(head, "w-full text-ink-soft", state(p))}>
                  {c} <span className="text-ink-soft/70">{count("*", c)}</span>
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
                <span className="flex items-center">
                  <button type="button" aria-pressed={state(row) !== null} title={t("scope.matrix.trade", { trade: tr.name })} onClick={pick(row)} className={cn(head, "min-w-0 flex-1 truncate text-left text-sm text-ink", state(row))}>
                    {tr.name} <span className="text-xs text-ink-soft">{count(tr.id, "*")}</span>
                  </button>
                  <OpenTrade trade={tr} className="invisible group-hover/head:visible group-focus-within/head:visible" />
                </span>
              </th>
              {countries.map((c) => {
                const p: Pattern = { trade: tr.id, country: c };
                const setUp = tr.categories.some((k) => k.country === c);
                const n = count(tr.id, c);
                const covering = state(p) === null && picked !== null && picked.some((q) => covers(q, tr.id, c));
                if (!setUp && n === 0)
                  return (
                    <td key={c} className="group/cell text-center">
                      <button
                        type="button"
                        onClick={() => openTrade(tr.id, c)}
                        aria-label={t("scope.matrix.setUp", { category: `${tr.name} · ${c}` })}
                        title={t("scope.matrix.setUp", { category: `${tr.name} · ${c}` })}
                        className={cn(head, "invisible w-full text-ink-soft group-hover/cell:visible focus-visible:visible")}
                      >
                        <Plus aria-hidden className="mx-auto size-3.5" />
                      </button>
                    </td>
                  );
                return (
                  <td key={c} className="group/cell relative text-center">
                    <button
                      type="button"
                      aria-pressed={state(p) !== null}
                      title={setUp ? `${tr.name} · ${c}` : t("scope.matrix.notSetUp", { category: `${tr.name} · ${c}` })}
                      onClick={pick(p)}
                      className={cn(head, "w-full", n === 0 ? "text-ink-soft/60" : setUp ? "text-ink" : "text-accent-warn", covering && COVERED, state(p))}
                    >
                      {n}
                    </button>
                    <OpenTrade trade={tr} country={c} className="absolute -top-1 -right-1 invisible group-hover/cell:visible group-focus-within/cell:visible" />
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
function useOpenTrade(): (trade: number, country: Country | undefined) => void {
  const router = useRouter();
  const pathname = usePathname();
  return (trade, country) => (pathname === "/" ? openTradeHere(trade, country) : router.push(`/?trade=${trade}${country === undefined ? "" : `&tab=${country}`}`));
}

function OpenTrade({ trade, country, className }: { trade: Trade; country?: Country; className: string }) {
  const t = useT();
  const openTrade = useOpenTrade();
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

function NewTrade({ onAdded }: { onAdded: () => void }) {
  const t = useT();
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  return (
    <form
      className="flex items-center gap-1 px-1 pb-1"
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

/** The GBPs on a gmail against what its role holds: "owner · 2/3", "5/6"; the count alone while its role is unknown. */
function Room({ gmail, n }: { gmail: Gmail; n: number }) {
  const t = useT();
  if (gmail.role === null) return <span className="text-xs text-ink-soft tabular-nums">{n}</span>;
  if (gmail.cap === null) throw new Error(`gmail ${gmail.id} has a role and no cap`);
  return (
    <span className={cn("text-xs tabular-nums", n > gmail.cap ? "text-accent-error" : "text-ink-soft")} title={t("gmbs.gmail.room.title")}>
      {t(`gmbs.gmail.room.${gmail.role}`, { n, cap: gmail.cap })}
    </span>
  );
}

function RoleSelect({ gmail, onChanged }: { gmail: Gmail; onChanged: () => void }) {
  const t = useT();
  const pick = async (role: GmailRole) => {
    try {
      await setGmailRole(gmail.id, role);
      onChanged();
    } catch (e) {
      notifyFailure(e, t);
    }
  };
  return (
    <NativeSelect
      size="sm"
      wrapperClassName="w-24"
      aria-label={t("gmbs.gmail.role", { gmail: gmail.gmail })}
      value={gmail.role ?? ""}
      onChange={(e) => e.target.value !== "" && void pick(e.target.value as GmailRole)}
    >
      {gmail.role === null && <NativeSelectOption value="">{t("gmbs.gmail.role.pick")}</NativeSelectOption>}
      {GMAIL_ROLES.map((r) => (
        <NativeSelectOption key={r} value={r}>
          {t(`gmbs.gmail.role.${r}`)}
        </NativeSelectOption>
      ))}
    </NativeSelect>
  );
}

