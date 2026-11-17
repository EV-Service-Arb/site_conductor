"use client";

// shortcut: sites' document is not vendored yet; these go to createApi over it when it is (Phase 2, C6)
import { useCallback, useEffect, useState } from "react";

import { readCsrf } from "@/shared/api";
import { type Parser, parse } from "@/shared/lib/parse";

import { type SiteTag, type SitesPlace, sitesParser, sitesPlacesParser } from "../model/gmb";

/** sites' answer, checked; `null` when it refused or did not answer: its tags are absent and the page stands. */
async function read<T>(path: string, parser: Parser<T>): Promise<T | null> {
  let answer: Response;
  try {
    answer = await fetch(`/api/sites${path}`, { credentials: "same-origin", cache: "no-store", headers: { accept: "application/json" } });
  } catch {
    // no answer: as a refusal, the tags absent
    return null;
  }
  if (!answer.ok) return null;
  return parse(parser, await answer.json());
}

export type SitesRead<T> = { status: "loading" } | { status: "ok"; data: T | null; reload: () => void };

/** sites' read at `path`, again when the window regains focus (its changes are no write of the panel's); `null` path: none. */
function useSites<T>(path: string | null, parser: Parser<T>): SitesRead<T> & { reload: () => void } {
  const [answer, setAnswer] = useState<{ path: string | null; tick: number; data: T | null } | null>(null);
  const [tick, setTick] = useState(0);
  const reload = useCallback(() => setTick((n) => n + 1), []);
  useEffect(() => {
    window.addEventListener("focus", reload);
    return () => window.removeEventListener("focus", reload);
  }, [reload]);
  useEffect(() => {
    let current = true;
    void (path === null ? Promise.resolve(null) : read(path, parser)).then((data) => current && setAnswer({ path, tick, data }));
    return () => {
      current = false;
    };
    // `parser` is a module constant
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [path, tick]);
  if (answer === null || answer.path !== path) return { status: "loading", reload };
  return { status: "ok", data: answer.data, reload };
}

export const useSiteTags = (sees: boolean): SitesRead<Map<number, SiteTag>> & { reload: () => void } => useSites(sees ? "/gmbs" : null, sitesParser);

export const useSitesPlaces = (): SitesRead<{ places: SitesPlace[] }> & { reload: () => void } => useSites("/places", sitesPlacesParser);

/** sites' place answer is not read: the page re-reads its tags. */
export async function linkPlace(brand: string, location: string, gmbId: number | null): Promise<void> {
  const csrf = readCsrf(document.cookie);
  if (csrf === null) throw new Error("a write with no CSRF cookie beside the session");
  const answer = await fetch(`/api/sites/places/${encodeURIComponent(brand)}/${encodeURIComponent(location)}/gmb`, {
    method: "PUT",
    credentials: "same-origin",
    headers: { accept: "application/json", "content-type": "application/json", "x-sa-csrf": csrf },
    body: JSON.stringify({ gmb_id: gmbId }),
  });
  if (!answer.ok) throw new Error(`sites refused the link: ${answer.status}`);
}
