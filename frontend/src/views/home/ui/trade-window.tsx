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
  NativeSelect,
  NativeSelectOption,
  buttonVariants,
  cn,
} from "@evinvest/uikit";
import { Copy, Ellipsis, ListFilter, Mail, Pencil, Plus, Tag, Trash2, X } from "lucide-react";
import { useState } from "react";

import {
  COUNTRIES,
  type Category,
  type Country,
  type Gmail,
  type Gmb,
  type Trade,
  chooseCategoryScope,
  chooseGmailScope,
  deleteTrade,
  openTrade,
  removeCategory,
  renameTrade,
  setUpCategory,
  useTrades,
} from "@/entities/gmb";
import { usePlacement } from "@/features/placement-overlay";
import { ApiError } from "@/shared/api";
import { useLocale, useT } from "@/shared/i18n";
import { setQuery } from "@/shared/lib/query";
import { failureText } from "@/shared/ui/failure-text";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize, useControlSize } from "@/shared/ui/touch";

const MAX_KEYWORDS = 50;

/** A trade's window over the map, shaped as a GBP's: its countries as tabs, side by side; the outline stays the one list of its GBPs. */
export function TradeWindow({ trade, tab, gmbs, gmails }: { trade: Trade; tab: Country | null; gmbs: readonly Gmb[]; gmails: readonly Gmail[] }) {
  const t = useT();
  const button = useButtonSize();
  const [renaming, setRenaming] = useState(false);
  const held = gmbs.filter((b) => b.trade_id === trade.id);
  const here = tab === null ? held : held.filter((b) => b.country?.code === tab);
  const holders = new Set(here.map((b) => b.gmail_id));
  const tabs = COUNTRIES.filter((c) => c === tab || trade.categories.some((k) => k.country === c) || held.some((b) => b.country?.code === c));
  const rest = COUNTRIES.filter((c) => !tabs.includes(c));
  const category = tab === null ? undefined : trade.categories.find((c) => c.country === tab);
  const countries = new Set(held.flatMap((b) => (b.country === null ? [] : [b.country.code]))).size;

  return (
    <section aria-label={trade.name} className="absolute inset-y-0 right-0 left-0 flex flex-col overflow-y-auto border-l border-border bg-background md:left-36">
      <header className="flex items-start gap-3 px-6 pt-4">
        <div className="flex min-w-0 flex-1 flex-col gap-0.5">
          {renaming ? (
            <Rename trade={trade} onDone={() => setRenaming(false)} />
          ) : (
            <h2 className="flex min-w-0 items-center gap-2 text-lg font-semibold text-ink">
              <Tag aria-hidden className="size-4 shrink-0 text-ink-soft" />
              <span className="truncate">{tab === null ? trade.name : `${trade.name} · ${tab}`}</span>
            </h2>
          )}
          <p className="flex flex-wrap gap-x-3 text-sm text-ink-soft tabular-nums">
            <span>{t("trades.window.counts", { gbps: here.length, gmails: [...holders].filter((g) => g !== null).length })}</span>
            {tab !== null && <span className="text-ink-soft/70">{t("trades.window.trade", { gbps: held.length, countries })}</span>}
          </p>
        </div>
        <Button variant="outline" size={button("sm")} onClick={() => chooseCategoryScope([{ trade: trade.id, country: tab ?? "*" }])}>
          <ListFilter aria-hidden />
          {t("trades.window.scope")}
        </Button>
        <More trade={trade} tab={category === undefined ? null : tab} holders={held.length} onRename={() => setRenaming(true)} />
        <Button variant="ghost" size={button("sm")} icon aria-label={t("window.close")} title={t("window.close")} onClick={() => setQuery({ trade: null, tab: null })}>
          <X aria-hidden />
        </Button>
      </header>
      <nav role="tablist" aria-label={t("trades.window.countries")} className="flex items-center gap-1 border-b border-border px-6 pt-3">
        {tabs.map((c) => {
          const setUp = trade.categories.some((k) => k.country === c);
          const n = held.filter((b) => b.country?.code === c).length;
          return (
            <button
              key={c}
              type="button"
              role="tab"
              aria-selected={c === tab}
              onClick={() => openTrade(trade.id, c)}
              className={cn(
                "-mb-px flex items-center gap-2 border-b-2 px-3 py-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring",
                c === tab ? "border-ink text-ink" : "border-transparent text-ink-soft hover:text-ink",
              )}
            >
              {c}
              <span className={cn("rounded px-1.5 font-mono text-xs tabular-nums", setUp ? "bg-hover" : "bg-accent-warn/15 text-accent-warn")}>
                {setUp ? n : t("trades.window.tab.notSetUp", { n })}
              </span>
            </button>
          );
        })}
        {rest.length > 0 && (
          <DropdownMenu>
            <DropdownMenuTrigger className="-mb-px flex items-center gap-1 border-b-2 border-transparent px-3 py-2 text-sm text-ink-soft outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-ring">
              <Plus aria-hidden className="size-3.5" />
              {t("trades.window.country")}
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start">
              {rest.map((c) => (
                <DropdownMenuItem key={c} onClick={() => openTrade(trade.id, c)}>
                  {c}
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
        )}
      </nav>
      <div role="tabpanel" className="max-w-3xl p-6">
        {tab === null ? (
          <p className="text-sm text-ink-soft">{t("trades.window.pickCountry")}</p>
        ) : category === undefined ? (
          <NotSetUp trade={trade} country={tab} waiting={here.length} />
        ) : (
          <CategoryEditor key={`${trade.id}.${tab}`} trade={trade} category={category} here={here} gmails={gmails} />
        )}
      </div>
    </section>
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
        void renameTrade(trade.id, name.trim())
          .then(() => {
            onDone();
            trades.reload();
          })
          .catch((err: unknown) => notifyFailure(err, t));
      }}
    >
      <Input size={size} autoFocus aria-label={t("trades.window.rename")} value={name} onChange={(e) => setName(e.target.value)} />
      <Button type="submit" size={button("sm")} disabled={name.trim() === "" || name.trim() === trade.name}>
        {t("gmbs.gbp.save")}
      </Button>
      <Button type="button" variant="ghost" size={button("sm")} onClick={onDone}>
        {t("move.cancel")}
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
  const title = t("trades.window.delete", { trade: trade.name });
  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button variant="ghost" size={button("sm")} icon aria-label={t("trades.window.more")} title={t("trades.window.more")}>
            <Ellipsis aria-hidden />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          <DropdownMenuItem onClick={onRename}>
            <Pencil aria-hidden />
            {t("trades.window.rename")}
          </DropdownMenuItem>
          {tab !== null && (
            <DropdownMenuItem onClick={() => void act(() => removeCategory(trade.id, tab), () => undefined)()}>
              <X aria-hidden />
              {t("trades.window.unset", { category: `${trade.name} · ${tab}` })}
            </DropdownMenuItem>
          )}
          <DropdownMenuSeparator />
          <DropdownMenuItem variant="destructive" disabled={holders > 0} onClick={() => setDeleting(true)}>
            <Trash2 aria-hidden />
            <span className="flex flex-col">
              {title}
              {holders > 0 && <span className="text-xs text-accent-warn">{t("trades.window.delete.held", { n: holders })}</span>}
            </span>
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      <AlertDialog open={deleting} onOpenChange={setDeleting}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{title}</AlertDialogTitle>
            <AlertDialogDescription>{t("trades.window.delete.body")}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel className={buttonVariants({ variant: "outline", size: button() })}>{t("move.cancel")}</AlertDialogCancel>
            <AlertDialogAction
              className={buttonVariants({ variant: "destructive", size: button() })}
              onClick={() => void act(() => deleteTrade(trade.id), () => setQuery({ trade: null, tab: null }))()}
            >
              {t("gmbs.remove")}
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
      await setUpCategory(trade.id, country, { keywords: [], google_category: null, study: null });
      trades.reload();
    } catch (e) {
      notifyFailure(e, t);
    }
  };
  return (
    <div className="flex max-w-3xl flex-col items-start gap-3 rounded-lg border border-accent-warn/40 bg-accent-warn/5 px-4 py-3">
      <p className="text-sm text-ink">{waiting > 0 ? t("trades.window.waiting", { n: waiting, category: name }) : t("trades.window.notSetUp", { category: name })}</p>
      <Button size={button("sm")} onClick={() => void setUp()}>
        <Plus aria-hidden />
        {t("trades.window.setUp", { category: name })}
      </Button>
      <p className="text-xs text-ink-soft">{t("trades.window.willAsk")}</p>
    </div>
  );
}

const SECTION = "flex flex-col gap-2";
const SECTION_HEAD = "flex items-baseline gap-2 text-xs font-semibold tracking-wide text-ink-soft uppercase";

/** A category's keywords, Google's label and placement's study, and the gmails holding it; each change writes the whole category. */
function CategoryEditor({ trade, category, here, gmails }: { trade: Trade; category: Category; here: readonly Gmb[]; gmails: readonly Gmail[] }) {
  const t = useT();
  const locale = useLocale();
  const size = useControlSize();
  const button = useButtonSize();
  const trades = useTrades();
  const placement = usePlacement();
  const [typed, setTyped] = useState("");
  const [label, setLabel] = useState(category.google_category ?? "");
  const [failed, setFailed] = useState<string | null>(null);
  const name = `${trade.name} · ${category.country}`;
  const lang = new Intl.DisplayNames([locale], { type: "language" }).of(category.lang) ?? category.lang;
  const country = new Intl.DisplayNames([locale], { type: "region" }).of(category.country) ?? category.country;
  const write = async (change: Partial<Pick<Category, "keywords" | "google_category" | "study">>) => {
    try {
      await setUpCategory(trade.id, category.country, { keywords: category.keywords, google_category: category.google_category, study: category.study, ...change });
      setFailed(null);
      trades.reload();
      return true;
    } catch (e) {
      if (!(e instanceof ApiError)) throw e;
      if (e.failure.kind !== "unauthenticated") setFailed(failureText(e.failure, t));
      return false;
    }
  };
  const studies = placement.status === "ok" ? placement.trades : null;
  const typedLabel = label.trim() === "" ? null : label.trim();
  const held = [...new Set(here.map((b) => b.gmail_id))].map((id) => ({ id, gmail: id === null ? null : gmails.find((g) => g.id === id), n: here.filter((b) => b.gmail_id === id).length }));

  return (
    <div className="flex flex-col gap-6">
      <section className={SECTION}>
        <h3 className={SECTION_HEAD}>
          {t("trades.keywords")}
          <span className="flex-1" />
          <span className="font-normal tracking-normal normal-case">{lang}</span>
          <span className="font-mono font-normal tabular-nums">
            {category.keywords.length} / {MAX_KEYWORDS}
          </span>
        </h3>
        {category.keywords.length > 0 && (
          <span className="flex flex-wrap gap-1.5">
            {category.keywords.map((k) => (
              <span key={k} className="flex items-center gap-1 rounded-full border border-border py-0.5 pr-1 pl-2.5 text-xs text-ink">
                {k}
                <button type="button" aria-label={t("trades.keywords.remove", { keyword: k })} onClick={() => void write({ keywords: category.keywords.filter((x) => x !== k) })} className="rounded-full text-ink-soft hover:text-ink">
                  <X aria-hidden className="size-3" />
                </button>
              </span>
            ))}
          </span>
        )}
        <form
          className="flex items-center gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            const added = typed.split(",").filter((k) => k.trim() !== "");
            if (added.length > 0) void write({ keywords: [...category.keywords, ...added] }).then((ok) => ok && setTyped(""));
          }}
        >
          <Input size={size} value={typed} onChange={(e) => setTyped(e.target.value)} placeholder={t("trades.keywords.add", { lang })} aria-label={t("trades.keywords.add", { lang })} className="min-w-0 flex-1" />
        </form>
        {failed !== null ? <p className="text-xs text-accent-error">{failed}</p> : <p className="text-xs text-ink-soft">{t("trades.keywords.why")}</p>}
      </section>

      <section className={SECTION}>
        <h3 className={SECTION_HEAD}>
          {t("trades.google")}
          <span className="flex-1" />
          <span className="font-normal tracking-normal normal-case">{t("trades.google.where", { country })}</span>
        </h3>
        <form
          className="flex gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            void write({ google_category: typedLabel });
          }}
        >
          <Input size={size} value={label} onChange={(e) => setLabel(e.target.value)} aria-label={t("trades.google")} className="min-w-0 flex-1" />
          {typedLabel === category.google_category ? (
            label !== "" && (
              <Button type="button" variant="ghost" size={button("sm")} onClick={() => void navigator.clipboard.writeText(label)}>
                <Copy aria-hidden />
                {t("trades.google.copy")}
              </Button>
            )
          ) : (
            <Button type="submit" variant="outline" size={button("sm")}>
              {t("gmbs.gbp.save")}
            </Button>
          )}
        </form>
        <p className="text-xs text-ink-soft">{t("trades.google.why", { category: name })}</p>
      </section>

      <section className={SECTION}>
        <h3 className={SECTION_HEAD}>
          {t("trades.study")}
          <span className="flex-1" />
          <span className="font-normal tracking-normal normal-case">{t("trades.study.what")}</span>
        </h3>
        <NativeSelect
          size="sm"
          wrapperClassName="w-full"
          aria-label={t("trades.study")}
          disabled={studies === null}
          value={category.study ?? ""}
          onChange={(e) => void write({ study: e.target.value === "" ? null : e.target.value })}
        >
          <NativeSelectOption value="">{t("trades.study.none")}</NativeSelectOption>
          {[...new Set([...(studies ?? []), ...(category.study === null ? [] : [category.study])])].map((s) => (
            <NativeSelectOption key={s} value={s}>
              {s}
            </NativeSelectOption>
          ))}
        </NativeSelect>
        <p className={cn("text-xs text-ink-soft", placement.status === "error" && "text-accent-error")}>
          {placement.status === "ok"
            ? t("trades.study.why", { category: name })
            : placement.status === "loading"
              ? t("state.loading")
              : placement.status === "unconnected"
                ? t("placement.unconnected")
                : placement.message}
        </p>
      </section>

      <section className={SECTION}>
        <h3 className={SECTION_HEAD}>
          {t("trades.heldBy")}
          <span className="flex-1" />
          <span className="font-normal tracking-normal normal-case">{t("trades.heldBy.what")}</span>
        </h3>
        {held.length === 0 ? (
          <p className="text-xs text-ink-soft">{t("trades.heldBy.none")}</p>
        ) : (
          <span className="flex flex-wrap gap-1.5">
            {held.map(({ id, gmail, n }) => (
              <button
                key={id ?? "none"}
                type="button"
                disabled={id === null}
                onClick={() => {
                  chooseCategoryScope([{ trade: trade.id, country: category.country }]);
                  if (id !== null) chooseGmailScope([id]);
                }}
                className="flex items-center gap-1.5 rounded-md bg-hover px-2 py-1 text-xs text-ink outline-none enabled:hover:bg-hover/70 disabled:text-ink-soft focus-visible:ring-2 focus-visible:ring-ring"
              >
                <Mail aria-hidden className="size-3.5 text-ink-soft" />
                {gmail?.gmail ?? t("gmbs.gbp.gmail.none")}
                <span className="text-ink-soft tabular-nums">{n}</span>
              </button>
            ))}
          </span>
        )}
      </section>
    </div>
  );
}
