"use client";

import { toast } from "@evinvest/uikit";
import { useEffect, useRef } from "react";

import { alerts as keys, lines, resubscribe, useAlerts, useNamedGmbs, useUnread } from "@/entities/alerts";
import { useMe } from "@/entities/session";
import { safeLocalPath } from "@/shared/api";
import type { Alert } from "@/shared/api/schema";
import { useT } from "@/shared/i18n";
import { ViewLink, go } from "@/shared/lib/location";
import { notifyFailure } from "@/shared/ui/notify";

const TOAST_MS = 8_000;

/** What the shell does of alerts whatever its page: a toast for an important one that comes, and this browser's pushes kept and opened here. */
export function useAlertLive(): void {
  useToasts();
  usePushes();
}

function useToasts() {
  const t = useT();
  const unread = useUnread();
  const gmbs = useNamedGmbs();
  // the newest there was at load, then the newest told of
  const seen = useRef<number | null>(null);
  const shown = useRef<number | null>(null);
  const inbox = unread.status === "ok" ? unread.data : null;
  useEffect(() => {
    if (inbox === null) return;
    const newest = inbox.alerts[0]?.id ?? 0;
    const was = seen.current;
    seen.current = Math.max(was ?? 0, newest);
    if (was === null) return;
    const came = inbox.alerts.filter((a) => a.id > was && a.tier === "important");
    const first = came[0];
    if (first === undefined) return;
    if (shown.current !== null) toast.dismiss(shown.current);
    const more = inbox.unread.important - 1;
    shown.current = toast.warn(<ToastLink alert={first} title={lines(first, gmbs).title} />, {
      duration: TOAST_MS,
      description: [lines(first, gmbs).sub, more > 0 ? t(keys.toast.more, "{n} more important waiting in the bell", { n: more }) : ""].filter((l) => l !== "").join(" · "),
    });
  }, [inbox, gmbs, t]);
}

function ToastLink({ alert, title }: { alert: Alert; title: string }) {
  return (
    <ViewLink to={{ view: "alerts", params: { open: alert.id } }} className="hover:underline">
      {title}
    </ViewLink>
  );
}

function usePushes() {
  const t = useT();
  const me = useMe();
  const alerts = useAlerts();
  const { reload } = alerts;
  const settings = alerts.status === "ok" ? alerts.data : null;
  const tried = useRef(false);
  useEffect(() => {
    if (settings === null || me.acting !== null || tried.current) return;
    tried.current = true;
    resubscribe(me.user_id, settings).then(
      (anew) => anew && reload(),
      (e: unknown) => notifyFailure(e, t),
    );
  }, [settings, me, reload, t]);
  useEffect(() => {
    if (!("serviceWorker" in navigator)) return;
    const onMessage = (e: MessageEvent) => {
      const data: unknown = e.data;
      if (typeof data !== "object" || data === null || !("type" in data) || data.type !== "sa:open") return;
      const path = "path" in data && typeof data.path === "string" ? safeLocalPath(data.path) : null;
      if (path === null) throw new Error("sa:open: not a path on this origin");
      go(path);
    };
    navigator.serviceWorker.addEventListener("message", onMessage);
    return () => navigator.serviceWorker.removeEventListener("message", onMessage);
  }, []);
}
