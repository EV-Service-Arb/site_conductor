"use client";

import { Button, Command, CommandEmpty, CommandInput, CommandItem, CommandList, Popover, PopoverContent, PopoverTrigger, cn } from "@evinvest/uikit";
import { ChevronLeft, ChevronRight, GripVertical, Plus, X } from "lucide-react";
import { type ReactNode, useMemo, useState } from "react";

import { googleName, rankGoogle, trades, useGoogleList } from "@/entities/gmb";
import type { Country, GoogleCategory } from "@/shared/api/schema";
import { useLocale, useT, state } from "@/shared/i18n";
import { failureText } from "@/shared/ui/failure-text";
import { useButtonSize } from "@/shared/ui/touch";

/** Google's own cap on additional categories. */
const MAX_ADDITIONAL = 9;
/** Each `CommandItem` registers itself, so the list mounts this many at most. */
const SHOWN = 50;

/** An additional Google category and its competitor tier's weight; `null` until a person gives it one. */
export type Tier = { google: GoogleCategory; weight: number | null };

/** A category's additional Google categories, off Google's list: its trade is the primary. */
export function GoogleCategories({ country, lang, trade, additional, setAdditional }: { country: Country; lang: string; trade: GoogleCategory; additional: readonly Tier[]; setAdditional: (next: Tier[]) => void }) {
  const t = useT();
  const locale = useLocale();
  const button = useButtonSize();
  const [dragged, setDragged] = useState<number | null>(null);
  const [over, setOver] = useState<number | null>(null);
  const move = (from: number, to: number) => {
    const next = [...additional];
    next.splice(to, 0, ...next.splice(from, 1));
    setAdditional(next);
  };
  const full = additional.length >= MAX_ADDITIONAL;
  const taken = new Set([trade.id, ...additional.map((a) => a.google.id)]);

  return (
    <>
      <span className="flex items-start gap-2">
      <span className="flex h-7 w-20 shrink-0 items-center text-sm text-ink-soft">{t(trades.google.additional, "Additional")}</span>
      <ol className="flex min-w-0 flex-1 flex-wrap items-center gap-1.5">
        {additional.map(({ google: g }, i) => (
          <li
            key={g.id}
            draggable
            onDragStart={(e) => {
              e.dataTransfer.setData("text/plain", googleName(g, lang));
              e.dataTransfer.effectAllowed = "move";
              setDragged(i);
            }}
            onDragOver={(e) => {
              if (dragged === null) return;
              e.preventDefault();
              setOver(i);
            }}
            onDragLeave={() => setOver((o) => (o === i ? null : o))}
            onDrop={(e) => {
              e.preventDefault();
              if (dragged !== null && dragged !== i) move(dragged, i);
              setDragged(null);
              setOver(null);
            }}
            onDragEnd={() => {
              setDragged(null);
              setOver(null);
            }}
            className={cn(
              "flex cursor-grab items-center gap-1 rounded-full border border-border py-0.5 pr-1 pl-1 text-xs text-ink active:cursor-grabbing",
              dragged === i && "opacity-40",
              over === i && dragged !== i && "border-ring ring-1 ring-ring",
            )}
          >
            <GripVertical aria-hidden className="size-3 text-ink-faint" />
            <span className="font-mono text-ink-soft tabular-nums">{i + 1}</span>
            <span className="px-0.5">{googleName(g, locale)}</span>
            <ChipButton label={t(trades.google.additional.earlier, "Move {category} earlier", { category: googleName(g, locale) })} disabled={i === 0} onClick={() => move(i, i - 1)}>
              <ChevronLeft aria-hidden className="size-3" />
            </ChipButton>
            <ChipButton label={t(trades.google.additional.later, "Move {category} later", { category: googleName(g, locale) })} disabled={i === additional.length - 1} onClick={() => move(i, i + 1)}>
              <ChevronRight aria-hidden className="size-3" />
            </ChipButton>
            <ChipButton label={t(trades.google.additional.remove, "Remove {category}", { category: googleName(g, locale) })} onClick={() => setAdditional(additional.filter((a) => a.google.id !== g.id))}>
              <X aria-hidden className="size-3" />
            </ChipButton>
          </li>
        ))}
        <li>
          {full ? (
            <Button variant="ghost" size={button("sm")} disabled title={t(trades.google.additional.full, "Google takes 9 at most")}>
              <Plus aria-hidden />
              {t(trades.google.additional.add, "Add")}
            </Button>
          ) : (
            <Picker country={country} lang={lang} exclude={taken} onPick={(g) => setAdditional([...additional, { google: g, weight: null }])}>
              <Button variant="ghost" size={button("sm")}>
                <Plus aria-hidden />
                {t(trades.google.additional.add, "Add")}
              </Button>
            </Picker>
          )}
        </li>
      </ol>
      <span className="flex h-7 items-center font-mono text-xs text-ink-soft tabular-nums">
        {additional.length} / {MAX_ADDITIONAL}
      </span>
      </span>
      <p className="text-xs text-ink-soft">{full ? t(trades.google.additional.full, "Google takes 9 at most") : t(trades.google.additional.order, "Typed into Google's form in this order after the primary. Each is one more query in every area's sweep: adding one makes the next pull buy it. Drag a chip, or move it with ‹ ›.")}</p>
    </>
  );
}

function ChipButton({ label, disabled, onClick, children }: { label: string; disabled?: boolean; onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      disabled={disabled ?? false}
      onClick={onClick}
      className="rounded-full p-0.5 text-ink-soft outline-none enabled:hover:bg-hover enabled:hover:text-ink disabled:opacity-30 focus-visible:ring-2 focus-visible:ring-ring"
    >
      {children}
    </button>
  );
}

function Picker({ country, lang, exclude, onPick, children }: { country: Country; lang: string; exclude: ReadonlySet<string>; onPick: (g: GoogleCategory) => void; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>{children}</PopoverTrigger>
      <PopoverContent align="start" className="w-md max-w-[calc(100vw-2rem)] p-0">
        {open && (
          <Search
            country={country}
            lang={lang}
            exclude={exclude}
            onPick={(g) => {
              setOpen(false);
              onPick(g);
            }}
          />
        )}
      </PopoverContent>
    </Popover>
  );
}

/** Every term must match the reader's name, the country's or the gcid's slug; prefix, then word-start, then the rest. */
function Search({ country, lang, exclude, onPick }: { country: Country; lang: string; exclude: ReadonlySet<string>; onPick: (g: GoogleCategory) => void }) {
  const t = useT();
  const locale = useLocale();
  const list = useGoogleList(country);
  const [search, setSearch] = useState("");
  const data = list.status === "ok" ? list.data : null;
  const ranked = useMemo(() => rankGoogle(data ?? [], search, locale, lang), [data, search, locale, lang]).filter((x) => !exclude.has(x.g.id));

  return (
    <Command shouldFilter={false} search={search} onSearchChange={setSearch}>
      <CommandInput placeholder={t(trades.google.search, "Search Google's categories")} />
      <CommandList>
        {list.status === "loading" && <p className="px-3 py-2 text-sm text-ink-soft">{t(state.loading, "Loading…")}</p>}
        {list.status === "error" && <p className="px-3 py-2 text-sm text-accent-error">{failureText(list.failure, t)}</p>}
        {ranked.slice(0, SHOWN).map(({ g, shown }) => (
          <CommandItem key={g.id} value={g.id} onSelect={() => onPick(g)}>
            <span className="min-w-0 flex-1 truncate" title={shown}>
              {shown}
            </span>
            <span className="max-w-2/5 truncate font-mono text-xs text-ink-faint">{g.id.replace(/^gcid:/, "")}</span>
          </CommandItem>
        ))}
        {ranked.length > SHOWN && <p className="px-3 py-2 text-xs text-ink-soft">{t(trades.google.more, "{n} more: keep typing", { n: ranked.length - SHOWN })}</p>}
        <CommandEmpty>{t(trades.google.nothing, "Google lists no such category")}</CommandEmpty>
      </CommandList>
    </Command>
  );
}
