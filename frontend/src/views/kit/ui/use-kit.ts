"use client";

import { useResource } from "@evinvest/api/react";
import { useEffect } from "react";

import { api } from "@/shared/api";

import { sinceOf, usePeriod } from "../model/kit";

const LIVE_MS = 30_000;

/** Kit `slug` over the period, read again every 30 s while shown and whenever the tab comes back. */
export function useKit(slug: string) {
  const period = usePeriod();
  const res = useResource(api, "/kits/{slug}", { params: { path: { slug }, query: { since: sinceOf(period) } } });
  const { reload } = res;
  useEffect(() => {
    const every = setInterval(reload, LIVE_MS);
    window.addEventListener("focus", reload);
    return () => {
      clearInterval(every);
      window.removeEventListener("focus", reload);
    };
  }, [reload]);
  return res;
}
