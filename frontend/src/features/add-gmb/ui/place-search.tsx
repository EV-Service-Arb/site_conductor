"use client";

import { Button, Command, CommandInput, CommandItem, CommandList } from "@evinvest/uikit";
import { MapPin, X } from "lucide-react";
import { useEffect, useState } from "react";

import { type Candidate, type Suggestion, gmbs, resolvePlace, suggestPlaces } from "@/entities/gmb";
import { state as stateKeys, useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize } from "@/shared/ui/touch";

import { add as addKeys } from "../model/i18n";

/** Each distinct input is a billed request: asked only once typing pauses. */
const PAUSE_MS = 350;
const MIN_INPUT = 3;

/** Google's suggestions as one types, as Maps' own box: a pick is resolved to its place (point, address, country); Enter on nothing picked resolves what was typed, so a Maps link or a place id works too. */
export function PlaceSearch({ gmail, placeholder, picked, onPick }: { gmail: number | null; placeholder: string; picked: Candidate | null; onPick: (c: Candidate | null) => void }) {
  const t = useT();
  const button = useButtonSize();
  const [input, setInput] = useState("");
  const [answered, setAnswered] = useState<{ q: string; found: Suggestion[] } | null>(null);
  const [resolving, setResolving] = useState(false);
  const q = input.trim();
  const found = answered !== null && answered.q === q ? answered.found : null;

  useEffect(() => {
    if (q.length < MIN_INPUT) return;
    let live = true;
    const timer = setTimeout(() => {
      suggestPlaces(q, gmail).then(
        (found) => live && setAnswered({ q, found }),
        (e: unknown) => live && notifyFailure(e, t),
      );
    }, PAUSE_MS);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [q, gmail, t]);

  const resolve = async (place: string) => {
    setResolving(true);
    try {
      const [first, ...rest] = await resolvePlace(place, gmail);
      if (first === undefined || rest.length > 0) {
        setAnswered({ q, found: [first, ...rest].filter((c) => c !== undefined).map((c) => ({ place_id: c.place_id, main: c.name ?? c.place_id, secondary: c.address })) });
        return;
      }
      onPick(first);
      setInput("");
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setResolving(false);
    }
  };

  if (picked !== null)
    return (
      <div className="flex items-center gap-3 rounded-md border border-border bg-hover/40 px-3 py-2">
        <MapPin aria-hidden className="size-4 shrink-0 text-ink-soft" />
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="truncate text-sm text-ink">{picked.name ?? picked.address ?? picked.place_id}</span>
          {picked.name !== null && picked.address !== null && <span className="truncate text-xs text-ink-soft">{picked.address}</span>}
          <span className="text-xs text-ink-faint">
            {[picked.location === null ? t(addKeys.picked.noPoint, "no point from Google") : `${picked.location.lat.toFixed(4)}, ${picked.location.lng.toFixed(4)}`, picked.country].filter(Boolean).join(" · ")}
          </span>
        </span>
        <Button type="button" variant="ghost" size={button("sm")} icon aria-label={t(addKeys.picked.clear, "Pick another")} title={t(addKeys.picked.clear, "Pick another")} onClick={() => onPick(null)}>
          <X aria-hidden />
        </Button>
      </div>
    );

  return (
    <Command shouldFilter={false} search={input} onSearchChange={setInput} className="h-auto overflow-visible rounded-md border border-border">
      <CommandInput
        placeholder={placeholder}
        autoCapitalize="none"
        onKeyDown={(e) => {
          // cmdk takes Enter for the highlighted item; with none listed, the typed text is resolved as it is
          if (e.key === "Enter" && (found === null || found.length === 0) && q !== "") {
            e.preventDefault();
            void resolve(q);
          }
        }}
      />
      {(found !== null || resolving) && (
        <CommandList>
          {resolving && <p className="px-3 py-2 text-sm text-ink-soft">{t(stateKeys.loading, "Loading…")}</p>}
          {found?.length === 0 && <p className="px-3 py-2 text-sm text-ink-soft">{t(gmbs.gmb.none, "No place found.")}</p>}
          {found?.map((s) => (
            <CommandItem key={s.place_id} value={s.place_id} onSelect={() => void resolve(s.place_id)}>
              <MapPin aria-hidden className="text-ink-soft" />
              <span className="flex min-w-0 flex-col">
                <span className="truncate text-sm text-ink">{s.main}</span>
                {s.secondary !== null && <span className="truncate text-xs text-ink-soft">{s.secondary}</span>}
              </span>
            </CommandItem>
          ))}
          {found !== null && found.length > 0 && <p className="border-t border-border px-3 py-1.5 text-xs text-ink-faint">{t(addKeys.suggestions, "Google's suggestions · one request per pause in typing · paid once, then kept")}</p>}
        </CommandList>
      )}
    </Command>
  );
}
