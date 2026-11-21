"use client";

import { useResource } from "@evinvest/api/react";
import { cn } from "@evinvest/uikit";
import type { ReactNode } from "react";

import { api } from "@/shared/api";
import type { CategoryVersion } from "@/shared/api/schema";
import { type T, state, useLocale, useT } from "@/shared/i18n";
import { failureText } from "@/shared/ui/failure-text";

import { type Piece, diffBodies } from "../model/diff";
import { googleName } from "../model/gmb";
import { trades as tradesKeys } from "../model/i18n";

const PIECE_NAME = {
  keyword: (t) => t(tradesKeys.diff.keyword, "Keyword"),
  additional: (t) => t(tradesKeys.diff.additional, "Additional category"),
  order: (t) => t(tradesKeys.diff.order, "Order of the additional ones"),
  word: (t) => t(tradesKeys.diff.word, "Excluded word"),
  type: (t) => t(tradesKeys.diff.type, "Excluded type"),
  demand: (t) => t(tradesKeys.diff.demand, "Demand"),
} satisfies Record<Piece, (t: T) => string>;

/** Category version `id`, read; what to draw of it once read. */
export function VersionRead({ id, children }: { id: number; children: (v: CategoryVersion) => ReactNode }) {
  const t = useT();
  const read = useResource(api, "/category-versions/{id}", { params: { path: { id } } });
  if (read.status === "loading") return <p className="text-sm text-ink-soft">{t(state.loading, "Loading…")}</p>;
  if (read.status === "error") return <p className="text-sm text-accent-error">{failureText(read.failure, t)}</p>;
  return children(read.data);
}

/** How `to` differs from `from`, a line a piece. */
export function VersionDiff({ from, to }: { from: CategoryVersion; to: CategoryVersion }) {
  const t = useT();
  const locale = useLocale();
  const changes = diffBodies(from, to);
  const googles = new Map([...from.additional, ...to.additional].map((a) => [a.google.id, a.google]));
  const names = (ids: string | null) => (ids === null || ids === "" ? "—" : ids.split(" ").map((id) => googleName(googles.get(id)!, locale)).join(" › "));
  if (changes.length === 0) return <p className="text-sm text-ink-soft">{t(tradesKeys.diff.same, "Both say the same.")}</p>;
  return (
    <ul className="flex flex-col divide-y divide-border rounded-md border border-border text-sm">
      {changes.map((c) => {
        const mark = c.from === null ? "+" : c.to === null ? "−" : "~";
        const what = c.piece === "additional" ? googleName(c.google!, locale) : c.key;
        const values = c.piece === "order" ? `${names(c.from)} → ${names(c.to)}` : c.piece === "demand" ? `${c.from ?? "—"} → ${c.to ?? "—"}` : c.from !== null && c.to !== null ? `${c.from} → ${c.to}` : c.piece === "word" || c.piece === "type" ? "" : (c.from ?? c.to);
        return (
          <li key={`${c.piece}:${c.key}`} className="flex items-baseline gap-2 px-2 py-1">
            <span className={cn("w-3 shrink-0 font-mono", mark === "+" ? "text-positive" : mark === "−" ? "text-accent-error" : "text-accent-warn")}>{mark}</span>
            <span className="w-36 shrink-0 text-xs text-ink-soft">{PIECE_NAME[c.piece](t)}</span>
            <span className="min-w-0 flex-1 truncate text-ink">{what}</span>
            {values !== "" && <span className="max-w-1/2 truncate font-mono text-xs text-ink-soft">{values}</span>}
          </li>
        );
      })}
    </ul>
  );
}
