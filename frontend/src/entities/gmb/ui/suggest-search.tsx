"use client";

import { Command, CommandInput, CommandItem, CommandList, cn } from "@evinvest/uikit";
import { MapPin } from "lucide-react";
import { useEffect, useState } from "react";

import type { Suggestion } from "@/shared/api/schema";
import { useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";

/** Each distinct input is a billed request: asked only once typing pauses. */
const PAUSE_MS = 350;
const MIN_INPUT = 2;

/** Google's suggestions as one types; `ask` keeps its identity across renders. */
export function SuggestSearch({
  placeholder,
  none,
  ask,
  onPick,
  autoFocus = false,
  className,
}: {
  placeholder: string;
  none: string;
  ask: (input: string) => Promise<Suggestion[]>;
  onPick: (s: Suggestion) => void;
  autoFocus?: boolean;
  className?: string;
}) {
  const t = useT();
  const [input, setInput] = useState("");
  const [answered, setAnswered] = useState<{ q: string; found: Suggestion[] } | null>(null);
  const q = input.trim();
  const found = answered !== null && answered.q === q ? answered.found : null;
  useEffect(() => {
    if (q.length < MIN_INPUT) return;
    let live = true;
    const timer = setTimeout(() => {
      ask(q).then(
        (found) => live && setAnswered({ q, found }),
        (e: unknown) => live && notifyFailure(e, t),
      );
    }, PAUSE_MS);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [q, ask, t]);
  return (
    <Command shouldFilter={false} search={input} onSearchChange={setInput} className={cn("h-auto overflow-visible rounded-md border border-border", className)}>
      <CommandInput placeholder={placeholder} autoCapitalize="none" autoFocus={autoFocus} />
      {found !== null && (
        <CommandList className="max-h-60">
          {found.length === 0 && <p className="px-3 py-2 text-sm text-ink-soft">{none}</p>}
          {found.map((s) => (
            <CommandItem
              key={s.place_id}
              value={s.place_id}
              onSelect={() => {
                onPick(s);
                setInput("");
              }}
            >
              <MapPin aria-hidden className="text-ink-soft" />
              <span className="flex min-w-0 flex-col">
                <span className="truncate text-sm text-ink">{s.main}</span>
                {s.secondary !== null && <span className="truncate text-xs text-ink-soft">{s.secondary}</span>}
              </span>
            </CommandItem>
          ))}
        </CommandList>
      )}
    </Command>
  );
}
