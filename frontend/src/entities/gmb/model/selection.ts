"use client";

import { useEffect } from "react";

import type { Gmail } from "@/shared/api/schema";
import { setCarried, useCarried } from "@/shared/lib/location";

import { useGmails, useGmbTags } from "./gmails-context";
import { ISSUES, type Issue, issueOf, issuesOf } from "./issues";
import { type Listed, narrowed } from "./narrow";
import { type Pattern, useScope } from "./scope";

// CONTRACT §9: every app narrows to the selected GMBs, and may select them itself.
const GMBS_EVENT = "sa:gmbs";
const GMBS_REQUEST_EVENT = "sa:gmbs-request";
const GMBS_SELECT_EVENT = "sa:gmbs-select";

export interface Selection {
  /** The gmails the filters select by; none: every gmail. */
  scoped: Gmail[];
  /** The category patterns the filters select by; `null`: every GBP, those with no trade too. */
  categories: readonly Pattern[] | null;
  filter: string;
  only: Issue | null;
  /** Under the scope and the filter: what the issue chips count. */
  found: Listed[];
  /** Every GMB, the most pressing first: the outline's rows. */
  rows: Listed[];
  /** The selected ids; `null`: every GMB, those added later too. */
  chosen: ReadonlySet<number> | null;
  /** What the apps narrow to. */
  selected: Listed[];
  /** Whether rows were moved since the filters last changed. */
  edited: boolean;
  isSelected: (gmb: number) => boolean;
  /** Moves `gmbs` into the selection, or out of it: an edit of what the filters select. Into a selection of every GMB, it becomes them alone. */
  move: (gmbs: readonly number[], into: boolean) => void;
}

/** `?gmbs`, an edit of what the filters select: absent, none made; empty, nothing selected. */
const editOf = (gmbs: readonly number[] | undefined): ReadonlySet<number> | null => (gmbs === undefined ? null : new Set(gmbs));

const sorted = (gmbs: Iterable<number>) => [...gmbs].sort((a, b) => a - b);

/** Selects exactly `gmbs` (`null`: every GMB); the filters no longer say what is selected, so they go. */
export function select(gmbs: Iterable<number> | null): void {
  setCarried({ gmbs: gmbs === null ? undefined : sorted(gmbs), gmail: undefined, category: undefined, brands: undefined, find: undefined, issue: undefined });
}

/** The selection, and the outline's rows beside it; `null` while the gmails load. */
export function useSelection(): Selection | null {
  const gmails = useGmails();
  const scope = useScope();
  const { tags } = useGmbTags();
  const carried = useCarried();
  const filter = carried.find ?? "";
  const only = carried.issue ?? null;
  const edit = editOf(carried.gmbs);
  if (gmails.status !== "ok") return null;
  const scoped = gmails.data.gmails.filter((g) => scope.gmails?.has(g.id));
  const found = narrowed(gmails.data, scope, filter);
  const rank = (l: Listed) => {
    const issue = issueOf(l.gmb, tags);
    return issue === null ? ISSUES.length : ISSUES.indexOf(issue);
  };
  const rows = narrowed(gmails.data, null, "").sort((a, b) => rank(a) - rank(b));
  const filtered = scope.gmails !== null || scope.categories !== null || scope.brands !== null || filter.trim() !== "" || only !== null;
  const chosen = edit ?? (filtered ? new Set(found.filter((l) => only === null || issuesOf(l.gmb, tags).includes(only as Issue)).map((l) => l.gmb.id)) : null);
  const isSelected = (gmb: number) => chosen === null || chosen.has(gmb);
  const move = (gmbs: readonly number[], into: boolean) => {
    if (chosen === null && into) return setCarried({ gmbs: sorted(gmbs) });
    const next = new Set(chosen ?? rows.map((l) => l.gmb.id));
    for (const id of gmbs) {
      if (into) next.add(id);
      else next.delete(id);
    }
    if (rows.every((l) => next.has(l.gmb.id))) select(null);
    else setCarried({ gmbs: sorted(next) });
  };
  return { scoped, categories: scope.categories, filter, only, found, rows, chosen, selected: rows.filter((l) => isSelected(l.gmb.id)), edited: edit !== null, isSelected, move };
}

export function setFilter(filter: string): void {
  setCarried({ find: filter === "" ? undefined : filter, gmbs: undefined });
}

export function setOnly(issue: Issue | null): void {
  setCarried({ issue: issue ?? undefined, gmbs: undefined });
}

/** Tells the elements the selected ids (`null`: every GMB) on each change and to one that asks, and takes the selection an element makes. */
export function PublishSelection() {
  const selection = useSelection();
  const gmails = useGmails();
  const chosen = selection?.chosen;
  const edit = editOf(useCarried().gmbs);
  const known = gmails.status === "ok" ? new Set(gmails.data.gmbs.map((b) => b.id)) : undefined;
  const told = chosen === undefined ? undefined : JSON.stringify(chosen === null ? null : [...chosen].sort((a, b) => a - b));

  // a selected GMB removed, here or elsewhere: out of the selection
  const stale = edit !== null && known !== undefined && [...edit].some((id) => !known.has(id));
  useEffect(() => {
    if (stale && edit !== null && known !== undefined) setCarried({ gmbs: sorted([...edit].filter((id) => known.has(id))) });
  });

  useEffect(() => {
    if (told === undefined || stale) return;
    const tell = () => window.dispatchEvent(new CustomEvent(GMBS_EVENT, { detail: { ids: JSON.parse(told) as number[] | null } }));
    tell();
    window.addEventListener(GMBS_REQUEST_EVENT, tell);
    return () => window.removeEventListener(GMBS_REQUEST_EVENT, tell);
  }, [told, stale]);

  useEffect(() => {
    const take = (e: Event) => {
      const ids: unknown = (e as CustomEvent<{ ids?: unknown }>).detail?.ids;
      if (ids !== null && !(Array.isArray(ids) && ids.every((id) => Number.isSafeInteger(id)))) throw new Error(`${GMBS_SELECT_EVENT}: not {ids: number[] | null}: ${JSON.stringify(ids)}`);
      select(ids as number[] | null);
    };
    window.addEventListener(GMBS_SELECT_EVENT, take);
    return () => window.removeEventListener(GMBS_SELECT_EVENT, take);
  }, []);
  return null;
}
