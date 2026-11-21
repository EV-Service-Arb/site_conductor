"use client";

import { useResource } from "@evinvest/api/react";
import { Badge, Button, Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList, Popover, PopoverContent, PopoverTrigger, cn } from "@evinvest/uikit";
import { ChevronsUpDown, Library } from "lucide-react";
import { type ReactNode, useState } from "react";

import { api } from "@/shared/api";
import { type Category, type Country, type GoogleCategory, type Listing, countryValues } from "@/shared/api/schema";
import { state, useLocale, useT } from "@/shared/i18n";
import { failureText } from "@/shared/ui/failure-text";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize } from "@/shared/ui/touch";

import { setUpCategory } from "../api/categories";
import { LANG } from "../model/category";
import { useCategories } from "../model/gmails-context";
import { googleName } from "../model/gmb";
import { matches, rankGoogle, useGoogleList, useGoogleNames, useTrade } from "../model/google";
import { trades as tradesKeys } from "../model/i18n";
import { openWindow } from "../model/windows";
import { useAdopt } from "./adopt";

/** Each `CommandItem` registers itself, so Google's section mounts this many at most. */
const SHOWN = 50;

type Choice = { kind: "none" } | { kind: "yours"; trade: string } | { kind: "listing"; listing: Listing } | { kind: "google"; g: GoogleCategory };

/**
 * A trade, picked off the caller's categories, the listings they used, the standard library and Google's list, in that
 * order; `none` labels the choice of no trade, `prompt` the button while none is held. A listing not held is adopted, one over a category held after its diff;
 * a Google trade not held in `country` is set up there with its name alone.
 */
export function TradePicker({ value, onPick, none, prompt, label, country, className }: { value: string | null; onPick: (trade: string | null) => void; none?: string; prompt?: string; label: string; country: Country | null; className?: string }) {
  const t = useT();
  const button = useButtonSize();
  const categories = useCategories();
  const adopting = useAdopt();
  const [open, setOpen] = useState(false);
  const choose = (c: Choice) => {
    setOpen(false);
    switch (c.kind) {
      case "none":
        return onPick(null);
      case "yours":
        return onPick(c.trade);
      case "listing":
        return adopting.adopt(c.listing, c.listing.latest, () => onPick(c.listing.trade.id));
      case "google": {
        const held = categories.status === "ok" && categories.data.some((k) => k.trade.id === c.g.id && k.country === country);
        if (country === null || held) return onPick(c.g.id);
        void setUpCategory(c.g, country)
          .then(() => {
            categories.reload();
            onPick(c.g.id);
          })
          .catch((e: unknown) => notifyFailure(e, t));
      }
    }
  };
  return (
    <>
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <Button variant="outline" size={button("sm")} aria-label={label} title={label} disabled={categories.status !== "ok"} className={cn("min-w-0 justify-between", className ?? "w-56 max-w-full")}>
            <span className="truncate">{value === null ? (prompt ?? none ?? t(tradesKeys.pick, "Pick a trade")) : <TradeName gcid={value} country={country} />}</span>
            <ChevronsUpDown aria-hidden className="text-ink-soft" />
          </Button>
        </PopoverTrigger>
        <PopoverContent align="start" className="w-md max-w-[calc(100vw-2rem)] p-0">
          {open && categories.status === "ok" && <Choices categories={categories.data} country={country} none={none} onChoose={choose} onLibrary={() => setOpen(false)} />}
        </PopoverContent>
      </Popover>
      {adopting.dialog}
    </>
  );
}

/** Trade `gcid` in the reader's language; `…` until read. */
export function TradeName({ gcid, country }: { gcid: string; country: Country | null }) {
  const locale = useLocale();
  const trade = useTrade(gcid, country);
  return <>{trade === null ? "…" : googleName(trade, locale)}</>;
}

/** Where a category's head stands: its listing's release, edited since or not, and a newer release waiting. */
export function HeadBadges({ category }: { category: Category }) {
  const t = useT();
  const { lineage } = category;
  return (
    <>
      <Badge tone="neutral">{lineage === null ? t(tradesKeys.head.own, "your own") : lineage.edited ? t(tradesKeys.head.edited, "{listing} v{n} · edited", { listing: lineage.title, n: lineage.from.n }) : t(tradesKeys.head.listing, "{listing} v{n}", { listing: lineage.title, n: lineage.from.n })}</Badge>
      {lineage?.upstream != null && <Badge tone="warn">{t(tradesKeys.head.upstream, "v{n} out", { n: lineage.upstream.n })}</Badge>}
      {lineage?.withdrawn === true && <Badge tone="error">{t(tradesKeys.head.withdrawn, "withdrawn")}</Badge>}
    </>
  );
}

function Choices({ categories, country, none, onChoose, onLibrary }: { categories: readonly Category[]; country: Country | null; none: string | undefined; onChoose: (c: Choice) => void; onLibrary: () => void }) {
  const t = useT();
  const locale = useLocale();
  const [search, setSearch] = useState("");
  const where = country === null ? {} : { country };
  const used = useResource(api, "/library", { params: { query: { kind: "used", sort: "used", ...where } } });
  const standard = useResource(api, "/library", { params: { query: { kind: "standard", sort: "rank", ...where } } });
  const yours = categories.filter((c) => (country === null || c.country === country) && matches(googleName(c.trade, locale), search));
  const listed = (l: Listing) => matches(`${l.title} ${googleName(l.trade, locale)}`, search);
  const listing = (l: Listing) => (
    <CommandItem key={l.id} value={`listing:${l.id}`} onSelect={() => onChoose({ kind: "listing", listing: l })}>
      <span className="flex min-w-0 flex-1 flex-col">
        <span className="truncate">{l.title}</span>
        <span className="truncate text-xs text-ink-soft">
          {googleName(l.trade, locale)} · {l.country} · {l.author}
        </span>
      </span>
      <Badge tone="neutral">v{l.latest.n}</Badge>
    </CommandItem>
  );
  const read = (r: typeof used, shown: (ls: Listing[]) => ReactNode) =>
    r.status === "loading" ? <p className="px-3 py-1 text-xs text-ink-soft">{t(state.loading, "Loading…")}</p> : r.status === "error" ? <p className="px-3 py-1 text-xs text-accent-error">{failureText(r.failure, t)}</p> : shown(r.data.listings.filter(listed));
  return (
    <Command shouldFilter={false} search={search} onSearchChange={setSearch}>
      <CommandInput placeholder={t(tradesKeys.picker.search, "Search trades and listings")} />
      <CommandList>
        {none !== undefined && search === "" && (
          <CommandItem value="none" onSelect={() => onChoose({ kind: "none" })}>
            {none}
          </CommandItem>
        )}
        {yours.length > 0 && (
          <CommandGroup heading={t(tradesKeys.picker.yours, "Yours")}>
            {yours.map((c) => (
              <CommandItem key={`${c.trade.id}.${c.country}`} value={`yours:${c.trade.id}.${c.country}`} onSelect={() => onChoose({ kind: "yours", trade: c.trade.id })}>
                <span className="min-w-0 flex-1 truncate">
                  {googleName(c.trade, locale)} · {c.country}
                </span>
                <HeadBadges category={c} />
              </CommandItem>
            ))}
          </CommandGroup>
        )}
        <CommandGroup heading={t(tradesKeys.picker.used, "Used")}>{read(used, (ls) => ls.map(listing))}</CommandGroup>
        <CommandGroup heading={country === null ? t(tradesKeys.picker.standardAll, "Standard library") : t(tradesKeys.picker.standard, "Standard library · {country}", { country })}>{read(standard, (ls) => ls.map(listing))}</CommandGroup>
        <CommandGroup heading={t(tradesKeys.picker.google, "Google's trades")}>
          {country === null ? <EveryGoogle search={search} onChoose={onChoose} /> : <CountryGoogle country={country} held={categories} search={search} onChoose={onChoose} />}
        </CommandGroup>
        <CommandEmpty>{t(tradesKeys.picker.nothing, "No trade nor listing matches")}</CommandEmpty>
      </CommandList>
      <button
        type="button"
        onClick={() => {
          onLibrary();
          openWindow({ kind: "library", listing: null });
        }}
        className="flex items-center gap-2 border-t border-border px-3 py-2 text-left text-sm text-ink-soft outline-none hover:bg-hover hover:text-ink focus-visible:ring-2 focus-visible:ring-ring"
      >
        <Library aria-hidden className="size-4" />
        {t(tradesKeys.picker.library, "Browse the library…")}
      </button>
    </Command>
  );
}

function CountryGoogle({ country, held, search, onChoose }: { country: Country; held: readonly Category[]; search: string; onChoose: (c: Choice) => void }) {
  const list = useGoogleList(country);
  const mine = new Set(held.filter((c) => c.country === country).map((c) => c.trade.id));
  return <Google read={list.status === "ok" ? { status: "ok", data: list.data.filter((g) => !mine.has(g.id)) } : list} lang={LANG[country]} search={search} onChoose={onChoose} />;
}

function EveryGoogle({ search, onChoose }: { search: string; onChoose: (c: Choice) => void }) {
  const locale = useLocale();
  const names = useGoogleNames(countryValues);
  return <Google read={names.status === "ok" ? { status: "ok", data: [...names.data.values()] } : names} lang={locale} search={search} onChoose={onChoose} />;
}

function Google({ read, lang, search, onChoose }: { read: { status: "loading" } | { status: "ok"; data: readonly GoogleCategory[] } | { status: "error"; failure: Parameters<typeof failureText>[0] }; lang: string; search: string; onChoose: (c: Choice) => void }) {
  const t = useT();
  const locale = useLocale();
  if (read.status === "loading") return <p className="px-3 py-1 text-xs text-ink-soft">{t(state.loading, "Loading…")}</p>;
  if (read.status === "error") return <p className="px-3 py-1 text-xs text-accent-error">{failureText(read.failure, t)}</p>;
  const ranked = rankGoogle(read.data, search, locale, lang);
  return (
    <>
      {ranked.slice(0, SHOWN).map(({ g, shown }) => (
        <CommandItem key={g.id} value={g.id} onSelect={() => onChoose({ kind: "google", g })}>
          <span className="min-w-0 flex-1 truncate" title={shown}>
            {shown}
          </span>
          <span className="max-w-2/5 truncate font-mono text-xs text-ink-faint">{g.id.replace(/^gcid:/, "")}</span>
        </CommandItem>
      ))}
      {ranked.length > SHOWN && <p className="px-3 py-1 text-xs text-ink-soft">{t(tradesKeys.google.more, "{n} more: keep typing", { n: ranked.length - SHOWN })}</p>}
    </>
  );
}
