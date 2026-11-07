"use client";

import { useEffect } from "react";

import { setQuery, useQuery } from "@/shared/lib/query";

import { useGmails, useGmbTags } from "./gmails-context";
import type { Gmail } from "./gmb";
import { ISSUES, type Issue, issueOf } from "./issues";
import { type Listed, narrowed } from "./narrow";
import { useGmailScope } from "./scope";

// CONTRACT §9: every app narrows to the selected GMBs, and may select them itself.
const GMBS_EVENT = "sa:gmbs";
const GMBS_REQUEST_EVENT = "sa:gmbs-request";
const GMBS_SELECT_EVENT = "sa:gmbs-select";

export interface Selection {
  scoped: Gmail | null;
  filter: string;
  only: Issue | null;
  /** Under the scope and the filter: what the issue chips count. */
  found: Listed[];
  /** `found` under the chip, the most pressing first: the outline's rows. */
  shown: Listed[];
  /** The selected ids; `null`: every GMB, those added later too. */
  chosen: ReadonlySet<number> | null;
  /** Every selected GMB, whatever the outline shows: what the apps narrow to. */
  selected: Listed[];
  isSelected: (gmb: number) => boolean;
  /** Moves `gmbs` into the selection, or out of it. */
  move: (gmbs: readonly number[], into: boolean) => void;
}

/** `?gmbs`: absent, every GMB; empty, none. */
function chosenOf(raw: string | null): ReadonlySet<number> | null {
  if (raw === null) return null;
  return new Set(
    raw === ""
      ? []
      : raw.split(",").map((s) => {
          const id = Number(s);
          if (!Number.isSafeInteger(id)) throw new Error(`?gmbs=${raw}: not GMB ids`);
          return id;
        }),
  );
}

export function select(gmbs: Iterable<number> | null): void {
  setQuery({ gmbs: gmbs === null ? null : [...gmbs].sort((a, b) => a - b).join(",") });
}

/** The selection, and the outline's rows beside it; `null` while the gmails load. */
export function useSelection(): Selection | null {
  const gmails = useGmails();
  const scope = useGmailScope();
  const { tags } = useGmbTags();
  const filter = useQuery("find") ?? "";
  const only = useQuery("issue");
  const chosen = chosenOf(useQuery("gmbs"));
  if (only !== null && !(ISSUES as readonly string[]).includes(only)) throw new Error(`?issue=${only}: not one of ${ISSUES.join(", ")}`);
  if (gmails.status !== "ok") return null;
  const scoped = gmails.data.find((g) => g.id === scope) ?? null;
  const found = narrowed(gmails.data, scoped?.id ?? null, filter);
  const rank = (l: Listed) => {
    const issue = issueOf(l.gmb, tags);
    return issue === null ? ISSUES.length : ISSUES.indexOf(issue);
  };
  const shown = found.filter((l) => only === null || issueOf(l.gmb, tags) === only).sort((a, b) => rank(a) - rank(b));
  const all = narrowed(gmails.data, null, "");
  const isSelected = (gmb: number) => chosen === null || chosen.has(gmb);
  const move = (gmbs: readonly number[], into: boolean) => {
    const next = new Set(chosen ?? all.map((l) => l.gmb.id));
    for (const id of gmbs) {
      if (into) next.add(id);
      else next.delete(id);
    }
    select(all.every((l) => next.has(l.gmb.id)) ? null : next);
  };
  return { scoped, filter, only: only as Issue | null, found, shown, chosen, selected: all.filter((l) => isSelected(l.gmb.id)), isSelected, move };
}

export function setFilter(filter: string): void {
  setQuery({ find: filter === "" ? null : filter });
}

export function setOnly(issue: Issue | null): void {
  setQuery({ issue });
}

/** Tells the elements the selected ids (`null`: every GMB) on each change and to one that asks, and takes the selection an element makes. */
export function PublishSelection() {
  const selection = useSelection();
  const gmails = useGmails();
  const chosen = selection?.chosen;
  const known = gmails.status === "ok" ? new Set(gmails.data.flatMap((g) => g.gmbs.map((b) => b.id))) : undefined;
  const told = chosen === undefined ? undefined : JSON.stringify(chosen === null ? null : [...chosen].sort((a, b) => a - b));

  // a selected GMB removed, here or elsewhere: out of the selection
  const stale = chosen != null && known !== undefined && [...chosen].some((id) => !known.has(id));
  useEffect(() => {
    if (stale && chosen != null && known !== undefined) select([...chosen].filter((id) => known.has(id)));
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
