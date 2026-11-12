"use client";

import { Button, Command, CommandEmpty, CommandInput, CommandItem, CommandList, Popover, PopoverContent, PopoverTrigger, cn } from "@evinvest/uikit";
import { ChevronLeft, ChevronRight, ChevronsUpDown, Copy, GripVertical, Plus, X } from "lucide-react";
import { type ReactNode, useMemo, useState } from "react";

import { type Category, type Country, type GoogleCategory, fetchGoogleCategories } from "@/entities/gmb";
import { useT } from "@/shared/i18n";
import { useResource } from "@/shared/lib/use-resource";
import { failureText } from "@/shared/ui/failure-text";
import { useButtonSize } from "@/shared/ui/touch";

/** Google's own cap on additional categories. */
const MAX_ADDITIONAL = 9;
/** Each `CommandItem` registers itself, so the list mounts this many at most. */
const SHOWN = 50;

type Google = Pick<Category, "google_primary" | "google_additional">;

/** A category's primary and additional Google categories, off Google's list; `write` sends both whole. */
export function GoogleCategories({ category, write }: { category: Category; write: (change: Google) => void }) {
  const t = useT();
  const button = useButtonSize();
  const { google_primary: primary, google_additional: additional } = category;
  const [dragged, setDragged] = useState<number | null>(null);
  const [over, setOver] = useState<number | null>(null);
  const move = (from: number, to: number) => {
    const next = [...additional];
    next.splice(to, 0, ...next.splice(from, 1));
    write({ google_primary: primary, google_additional: next });
  };
  const full = additional.length >= MAX_ADDITIONAL;
  const taken = new Set([...(primary === null ? [] : [primary.id]), ...additional.map((g) => g.id)]);

  return (
    <>
      <span className="flex items-center gap-2">
        <Picker country={category.country} lang={category.lang} exclude={primary === null ? new Set() : new Set([primary.id])} onPick={(g) => write({ google_primary: g, google_additional: additional.filter((a) => a.id !== g.id) })}>
          <Button variant="outline" size={button("sm")} className={cn("max-w-md min-w-0 flex-1 justify-between", primary === null && "border-accent-warn/60 text-accent-warn")}>
            <span className="truncate">{primary?.name ?? t("trades.google.pick")}</span>
            <ChevronsUpDown aria-hidden className="text-ink-soft" />
          </Button>
        </Picker>
        {primary !== null && (
          <Button type="button" variant="ghost" size={button("sm")} onClick={() => void navigator.clipboard.writeText(primary.name)}>
            <Copy aria-hidden />
            {t("trades.google.copy")}
          </Button>
        )}
      </span>

      <h4 className="mt-2 flex items-baseline gap-2 text-xs font-medium text-ink-soft">
        {t("trades.google.additional")}
        <span className="flex-1" />
        <span className="font-mono tabular-nums">
          {additional.length} / {MAX_ADDITIONAL}
        </span>
      </h4>
      <ol className="flex flex-wrap items-center gap-1.5">
        {additional.map((g, i) => (
          <li
            key={g.id}
            draggable
            onDragStart={(e) => {
              e.dataTransfer.setData("text/plain", g.name);
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
            <span className="px-0.5">{g.name}</span>
            <ChipButton label={t("trades.google.additional.earlier", { category: g.name })} disabled={i === 0} onClick={() => move(i, i - 1)}>
              <ChevronLeft aria-hidden className="size-3" />
            </ChipButton>
            <ChipButton label={t("trades.google.additional.later", { category: g.name })} disabled={i === additional.length - 1} onClick={() => move(i, i + 1)}>
              <ChevronRight aria-hidden className="size-3" />
            </ChipButton>
            <ChipButton label={t("trades.google.additional.remove", { category: g.name })} onClick={() => write({ google_primary: primary, google_additional: additional.filter((a) => a.id !== g.id) })}>
              <X aria-hidden className="size-3" />
            </ChipButton>
          </li>
        ))}
        <li>
          {primary === null || full ? (
            <Button variant="ghost" size={button("sm")} disabled title={t(full ? "trades.google.additional.full" : "trades.google.additional.needPrimary")}>
              <Plus aria-hidden />
              {t("trades.google.additional.add")}
            </Button>
          ) : (
            <Picker country={category.country} lang={category.lang} exclude={taken} onPick={(g) => write({ google_primary: primary, google_additional: [...additional, g] })}>
              <Button variant="ghost" size={button("sm")}>
                <Plus aria-hidden />
                {t("trades.google.additional.add")}
              </Button>
            </Picker>
          )}
        </li>
      </ol>
      <p className="text-xs text-ink-soft">{t(primary === null ? "trades.google.additional.needPrimary" : full ? "trades.google.additional.full" : "trades.google.additional.order")}</p>
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
      <PopoverContent align="start" className="w-[28rem] max-w-[calc(100vw-2rem)] p-0">
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

const fold = (s: string) => s.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
const WORD = /[^\p{L}\p{N}]+/u;

/** Every term must match the name or the gcid's slug, so English terms find French names; prefix, then word-start, then the rest. */
function Search({ country, lang, exclude, onPick }: { country: Country; lang: string; exclude: ReadonlySet<string>; onPick: (g: GoogleCategory) => void }) {
  const t = useT();
  const list = useResource(`google-categories.${country}`, () => fetchGoogleCategories(country));
  const [search, setSearch] = useState("");
  const data = list.status === "ok" ? list.data : null;
  const indexed = useMemo(() => {
    const collator = new Intl.Collator(lang);
    return (data ?? [])
      .map((g) => {
        const name = fold(g.name);
        const slug = g.id.replace(/^gcid:/, "").replaceAll("_", " ");
        return { g, name, slug, nameWords: name.split(WORD), words: [...name.split(WORD), ...slug.split(" ")] };
      })
      .sort((a, b) => collator.compare(a.g.name, b.g.name));
  }, [data, lang]);
  const terms = fold(search).split(/\s+/).filter((x) => x !== "");
  const whole = terms.join(" ");
  const ranked = indexed
    .filter((x) => !exclude.has(x.g.id) && terms.every((term) => x.name.includes(term) || x.slug.includes(term)))
    .map((x) => {
      const starts = (words: string[]) => terms.every((term) => words.some((w) => w.startsWith(term)));
      // the name, shown and typed, before the slug
      return { x, rank: [x.name.startsWith(whole), x.slug.startsWith(whole), starts(x.nameWords), starts(x.words), true].indexOf(true) };
    })
    // stable: the collator's order within a rank
    .sort((a, b) => a.rank - b.rank)
    .map(({ x }) => x.g);

  return (
    <Command shouldFilter={false} search={search} onSearchChange={setSearch}>
      <CommandInput placeholder={t("trades.google.search")} />
      <CommandList>
        {list.status === "loading" && <p className="px-3 py-2 text-sm text-ink-soft">{t("state.loading")}</p>}
        {list.status === "error" && <p className="px-3 py-2 text-sm text-accent-error">{failureText(list.failure, t)}</p>}
        {ranked.slice(0, SHOWN).map((g) => (
          <CommandItem key={g.id} value={g.id} onSelect={() => onPick(g)}>
            <span className="min-w-0 flex-1 truncate" title={g.name}>
              {g.name}
            </span>
            <span className="max-w-[40%] truncate font-mono text-xs text-ink-faint">{g.id.replace(/^gcid:/, "")}</span>
          </CommandItem>
        ))}
        {ranked.length > SHOWN && <p className="px-3 py-2 text-xs text-ink-soft">{t("trades.google.more", { n: ranked.length - SHOWN })}</p>}
        <CommandEmpty>{t("trades.google.nothing")}</CommandEmpty>
      </CommandList>
    </Command>
  );
}
