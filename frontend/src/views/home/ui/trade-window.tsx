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
  AlertDialogTrigger,
  Button,
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
  Input,
  NativeSelect,
  NativeSelectOption,
  buttonVariants,
  cn,
} from "@evinvest/uikit";
import { ListFilter, Pencil, Plus, Trash2, X } from "lucide-react";
import { useState } from "react";

import {
  COUNTRIES,
  type Category,
  type Country,
  type Gmb,
  type Trade,
  chooseCategoryScope,
  deleteTrade,
  openTrade,
  removeCategory,
  renameTrade,
  setUpCategory,
  useTrades,
} from "@/entities/gmb";
import { usePlacement } from "@/features/placement-overlay";
import { ApiError } from "@/shared/api";
import { useT } from "@/shared/i18n";
import { setQuery } from "@/shared/lib/query";
import { failureText } from "@/shared/ui/failure-text";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize, useControlSize } from "@/shared/ui/touch";

const MAX_KEYWORDS = 50;

/** A trade's window over the map, shaped as a GBP's: its countries as tabs, side by side; the outline stays the one list of its GBPs. */
export function TradeWindow({ trade, tab, gmbs }: { trade: Trade; tab: Country | null; gmbs: readonly Gmb[] }) {
  const t = useT();
  const button = useButtonSize();
  const held = gmbs.filter((b) => b.trade_id === trade.id);
  const here = tab === null ? held : held.filter((b) => b.country?.code === tab);
  const gmails = new Set(here.flatMap((b) => (b.gmail_id === null ? [] : [b.gmail_id]))).size;
  const tabs = COUNTRIES.filter((c) => trade.categories.some((k) => k.country === c) || held.some((b) => b.country?.code === c));
  const rest = COUNTRIES.filter((c) => !tabs.includes(c));
  const category = tab === null ? undefined : trade.categories.find((c) => c.country === tab);

  return (
    <section aria-label={trade.name} className="absolute inset-y-0 right-0 left-0 flex flex-col overflow-y-auto border-l border-border bg-background md:left-36">
      <header className="flex flex-col gap-1 px-6 pt-4">
        <div className="flex items-center gap-3">
          <Name trade={trade} />
          {tab !== null && <span className="text-lg text-ink-soft">· {tab}</span>}
          <span className="text-sm text-ink-soft tabular-nums">{t("trades.window.counts", { gbps: here.length, gmails })}</span>
          <span className="flex-1" />
          <Button variant="outline" size={button("sm")} onClick={() => chooseCategoryScope([{ trade: trade.id, country: tab ?? "*" }])}>
            <ListFilter aria-hidden />
            {t("trades.window.scope")}
          </Button>
          <Delete trade={trade} holders={held.length} />
          <Button variant="ghost" size={button("sm")} icon aria-label={t("window.close")} title={t("window.close")} onClick={() => setQuery({ trade: null, tab: null })}>
            <X aria-hidden />
          </Button>
        </div>
      </header>
      <nav role="tablist" aria-label={t("trades.window.countries")} className="flex items-center gap-1 border-b border-border px-6 pt-3">
        {tabs.map((c) => {
          const setUp = trade.categories.some((k) => k.country === c);
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
                !setUp && "text-accent-warn",
              )}
            >
              {c}
              <span className="text-xs tabular-nums opacity-70">{held.filter((b) => b.country?.code === c).length}</span>
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
                <DropdownMenuItem key={c} onSelect={() => openTrade(trade.id, c)}>
                  {c}
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
        )}
      </nav>
      <div role="tabpanel" className="p-6">
        {tab === null ? (
          <p className="text-sm text-ink-soft">{t("trades.window.pickCountry")}</p>
        ) : category === undefined ? (
          <NotSetUp trade={trade} country={tab} waiting={here.length} />
        ) : (
          <CategoryEditor key={`${trade.id}.${tab}`} trade={trade} category={category} />
        )}
      </div>
    </section>
  );
}

/** The trade's name, renamed in place. */
function Name({ trade }: { trade: Trade }) {
  const t = useT();
  const size = useControlSize();
  const button = useButtonSize();
  const trades = useTrades();
  const [naming, setNaming] = useState<string | null>(null);
  if (naming === null)
    return (
      <span className="flex min-w-0 items-center gap-1">
        <h2 className="min-w-0 truncate text-lg font-semibold text-ink">{trade.name}</h2>
        <Button variant="ghost" size={button("sm")} icon aria-label={t("trades.window.rename")} title={t("trades.window.rename")} onClick={() => setNaming(trade.name)}>
          <Pencil aria-hidden />
        </Button>
      </span>
    );
  return (
    <form
      className="flex gap-2"
      onSubmit={(e) => {
        e.preventDefault();
        void renameTrade(trade.id, naming.trim())
          .then(() => {
            setNaming(null);
            trades.reload();
          })
          .catch((err: unknown) => notifyFailure(err, t));
      }}
    >
      <Input size={size} autoFocus aria-label={t("trades.window.rename")} value={naming} onChange={(e) => setNaming(e.target.value)} />
      <Button type="submit" size={button("sm")} disabled={naming.trim() === "" || naming.trim() === trade.name}>
        {t("gmbs.gbp.save")}
      </Button>
      <Button type="button" variant="ghost" size={button("sm")} onClick={() => setNaming(null)}>
        {t("move.cancel")}
      </Button>
    </form>
  );
}

/** Deleted with its categories, only once no live GBP holds it. */
function Delete({ trade, holders }: { trade: Trade; holders: number }) {
  const t = useT();
  const button = useButtonSize();
  const trades = useTrades();
  const title = t("trades.window.delete", { trade: trade.name });
  const act = async () => {
    try {
      await deleteTrade(trade.id);
      setQuery({ trade: null, tab: null });
      trades.reload();
    } catch (e) {
      notifyFailure(e, t);
    }
  };
  return (
    <AlertDialog>
      <AlertDialogTrigger asChild>
        <Button variant="ghost" size={button("sm")} icon disabled={holders > 0} aria-label={title} title={holders > 0 ? t("trades.window.delete.held", { n: holders }) : title}>
          <Trash2 aria-hidden />
        </Button>
      </AlertDialogTrigger>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{title}</AlertDialogTitle>
          <AlertDialogDescription>{t("trades.window.delete.body")}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel className={buttonVariants({ variant: "outline", size: button() })}>{t("move.cancel")}</AlertDialogCancel>
          <AlertDialogAction className={buttonVariants({ variant: "destructive", size: button() })} onClick={() => void act()}>
            {t("gmbs.remove")}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
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
        {t("trades.window.setUp", { category: name })}
      </Button>
    </div>
  );
}

/** A category's keywords, Google's label and placement's study; each change writes the whole category. */
function CategoryEditor({ trade, category }: { trade: Trade; category: Category }) {
  const t = useT();
  const size = useControlSize();
  const button = useButtonSize();
  const trades = useTrades();
  const placement = usePlacement();
  const [typed, setTyped] = useState("");
  const [label, setLabel] = useState(category.google_category ?? "");
  const [failed, setFailed] = useState<string | null>(null);
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
  const remove = async () => {
    try {
      await removeCategory(trade.id, category.country);
      trades.reload();
    } catch (e) {
      notifyFailure(e, t);
    }
  };

  return (
    <div className="flex max-w-3xl flex-col gap-4">
      <section className="flex flex-col gap-2 rounded-lg border border-border px-4 py-3">
        <h3 className="flex items-baseline gap-2 font-medium text-ink">
          {t("trades.keywords")}
          <span className="text-xs font-normal text-ink-soft">{t("trades.keywords.hint", { lang: category.country })}</span>
          <span className="flex-1" />
          <span className="font-mono text-xs font-normal text-ink-soft tabular-nums">
            {category.keywords.length}/{MAX_KEYWORDS}
          </span>
        </h3>
        {category.keywords.length > 0 && (
          <span className="flex flex-wrap gap-1">
            {category.keywords.map((k) => (
              <span key={k} className="flex items-center gap-1 rounded-full border border-border py-0.5 pr-1 pl-2 text-xs text-ink">
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
          <Input size={size} value={typed} onChange={(e) => setTyped(e.target.value)} placeholder={t("trades.keywords.add")} aria-label={t("trades.keywords.add")} className="min-w-0 flex-1" />
        </form>
        {failed !== null && <p className="text-xs text-accent-error">{failed}</p>}
        <p className="text-xs text-ink-soft">{t("trades.keywords.why")}</p>
      </section>

      <section className="flex flex-col gap-3 rounded-lg border border-border px-4 py-3 text-sm">
        <div className="flex items-center gap-3">
          <span className="w-36 shrink-0 text-ink-soft">{t("trades.google")}</span>
          <form
            className="flex min-w-0 flex-1 gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              void write({ google_category: label.trim() === "" ? null : label.trim() });
            }}
          >
            <Input size={size} value={label} onChange={(e) => setLabel(e.target.value)} aria-label={t("trades.google")} className="min-w-0 flex-1" />
            <Button type="submit" variant="outline" size={button("sm")} disabled={(label.trim() === "" ? null : label.trim()) === category.google_category}>
              {t("gmbs.gbp.save")}
            </Button>
          </form>
        </div>
        <div className="flex items-center gap-3">
          <span className="w-36 shrink-0 text-ink-soft">{t("trades.study")}</span>
          <NativeSelect
            size="sm"
            wrapperClassName="w-56"
            aria-label={t("trades.study")}
            disabled={studies === null}
            value={category.study ?? ""}
            onChange={(e) => void write({ study: e.target.value === "" ? null : e.target.value })}
          >
            <NativeSelectOption value="">{t("trades.study.none")}</NativeSelectOption>
            {category.study !== null && studies !== null && !studies.includes(category.study) && <NativeSelectOption value={category.study}>{category.study}</NativeSelectOption>}
            {(studies ?? (category.study === null ? [] : [category.study])).map((s) => (
              <NativeSelectOption key={s} value={s}>
                {s}
              </NativeSelectOption>
            ))}
          </NativeSelect>
          <span className="text-xs text-ink-soft">{studies === null ? t("placement.unconnected") : t("trades.study.why")}</span>
        </div>
      </section>

      <Button variant="ghost" size={button("sm")} className="self-start text-ink-soft" onClick={() => void remove()}>
        {t("trades.window.unset", { category: `${trade.name} · ${category.country}` })}
      </Button>
    </div>
  );
}
