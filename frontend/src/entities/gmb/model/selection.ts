"use client";

import { useEffect } from "react";

import { setQuery, useQuery } from "@/shared/lib/query";

import { useGmails, useGmbTags } from "./gmails-context";
import type { Gmail } from "./gmb";
import { ISSUES, type Issue, issueOf } from "./issues";
import { type Listed, narrowed } from "./narrow";
import { useGmailScope } from "./scope";

// CONTRACT §9: the elements narrow to the outline's GMBs.
const GMBS_EVENT = "sa:gmbs";
const GMBS_REQUEST_EVENT = "sa:gmbs-request";

export interface Selection {
  scoped: Gmail | null;
  filter: string;
  only: Issue | null;
  /** Under the scope and the filter: what the issue chips count. */
  found: Listed[];
  /** `found` under the chip, the most pressing first. */
  selected: Listed[];
  /** Whether anything is narrowed: `false` selects every GMB the caller has. */
  narrows: boolean;
}

/** The GMBs the outline lists, which every app narrows to; `null` while the gmails load. */
export function useSelection(): Selection | null {
  const gmails = useGmails();
  const scope = useGmailScope();
  const { tags } = useGmbTags();
  const filter = useQuery("find") ?? "";
  const only = useQuery("issue");
  if (only !== null && !(ISSUES as readonly string[]).includes(only)) throw new Error(`?issue=${only}: not one of ${ISSUES.join(", ")}`);
  if (gmails.status !== "ok") return null;
  const scoped = gmails.data.find((g) => g.id === scope) ?? null;
  const found = narrowed(gmails.data, scoped?.id ?? null, filter);
  const rank = (l: Listed) => {
    const issue = issueOf(l.gmb, tags);
    return issue === null ? ISSUES.length : ISSUES.indexOf(issue);
  };
  const selected = found.filter((l) => only === null || issueOf(l.gmb, tags) === only).sort((a, b) => rank(a) - rank(b));
  return { scoped, filter, only: only as Issue | null, found, selected, narrows: scoped !== null || filter.trim() !== "" || only !== null };
}

export function setFilter(filter: string): void {
  setQuery({ find: filter === "" ? null : filter });
}

export function setOnly(issue: Issue | null): void {
  setQuery({ issue });
}

/** Tells the elements the selected GMB ids (`null`: nothing narrowed) on each change, and again to one that asks. */
export function PublishSelection() {
  const selection = useSelection();
  const ids = selection === null ? undefined : selection.narrows ? selection.selected.map((l) => l.gmb.id).sort((a, b) => a - b) : null;
  const told = ids === undefined ? undefined : JSON.stringify(ids);
  useEffect(() => {
    if (told === undefined) return;
    const tell = () => window.dispatchEvent(new CustomEvent(GMBS_EVENT, { detail: { ids: JSON.parse(told) as number[] | null } }));
    tell();
    window.addEventListener(GMBS_REQUEST_EVENT, tell);
    return () => window.removeEventListener(GMBS_REQUEST_EVENT, tell);
  }, [told]);
  return null;
}
