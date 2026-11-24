"use client";

import { type Resource, useResource } from "@evinvest/api/react";
import { type ReactNode, createContext, useContext, useEffect, useMemo, useRef, useState } from "react";

import { useGmails } from "@/entities/gmb";
import { useMe } from "@/entities/session";
import { type Failure, api, ok } from "@/shared/api";
import type { AlertHours, AlertInbox, Alerts, Gmb } from "@/shared/api/schema";

import { quietNow } from "./hours";

type Read<T> = Resource<T, Failure> & { reload: () => void };

interface Context {
  alerts: Read<Alerts>;
  /** The unread, newest first. */
  unread: Read<AlertInbox>;
}

const AlertsContext = createContext<Context | null>(null);

/** One read each of how the caller's alerts are set and of what is unread, for the bell, the strip, the toast and the pages: a change made in one is seen by the others. */
export function AlertsProvider({ children }: { children: ReactNode }) {
  const me = useMe();
  // openapi-fetch reads a tuple (`hours.quiet`) as an array
  const alerts = useResource(api, "/alerts") as Read<Alerts>;
  const unread = useResource(api, "/alerts/inbox", { params: { query: { unread: true } }, keep: true });
  const { reload } = alerts;
  const unset = alerts.status === "ok" && alerts.data.hours === null && me.acting === null;
  const told = useRef(false);
  useEffect(() => {
    if (!unset || told.current) return;
    told.current = true;
    // the daily message goes by where its person is; a refusal leaves it at UTC, which the settings show
    void ok(api.PUT("/alerts/hours", { body: startingHours() })).then(reload, () => {});
  }, [unset, reload]);
  return <AlertsContext value={{ alerts, unread }}>{children}</AlertsContext>;
}

export const startingHours = (): AlertHours => ({ tz: Intl.DateTimeFormat().resolvedOptions().timeZone, daily: 9 * 60, quiet: null, snoozed_until: null });

function useContextValue(): Context {
  const value = useContext(AlertsContext);
  if (value === null) throw new Error("used outside AlertsProvider");
  return value;
}

export const useAlerts = (): Read<Alerts> => useContextValue().alerts;
export const useUnread = (): Read<AlertInbox> => useContextValue().unread;

const NO_GMBS: readonly Gmb[] = [];

/** The GBPs an alert may name; none until read. */
export function useNamedGmbs(): readonly Gmb[] {
  const gmails = useGmails();
  return gmails.status === "ok" ? gmails.data.gmbs : NO_GMBS;
}

/** The time, anew every `ms`. */
export function useNow(ms: number): Date {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const timer = setInterval(() => setNow(new Date()), ms);
    return () => clearInterval(timer);
  }, [ms]);
  return now;
}

/** What holds everything but urgent now. */
export function useQuiet() {
  const alerts = useAlerts();
  const now = useNow(30_000);
  const hours = alerts.status === "ok" ? alerts.data.hours : null;
  return useMemo(() => quietNow(hours, now), [hours, now]);
}
