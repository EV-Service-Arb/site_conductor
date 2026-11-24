"use client";

import { Button, type HotbarProps } from "@evinvest/uikit";
import { ExternalLink, Flame, Phone, X } from "lucide-react";
import { useEffect, useState } from "react";

import { AlertButtons, alerts as keys, lines, phoneOf, readAlert, useNamedGmbs, useNow, useUnread } from "@/entities/alerts";
import type { Alert } from "@/shared/api/schema";
import { useT } from "@/shared/i18n";
import { go } from "@/shared/lib/location";
import { notifyFailure } from "@/shared/ui/notify";

type Takeover = NonNullable<HotbarProps["takeover"]>;

/** The oldest urgent alert unread, in the hotbar's place on every screen: read anywhere it leaves, folded it waits in the bell until the next urgent one. */
export function useTakeover(): Takeover | undefined {
  const t = useT();
  const unread = useUnread();
  const gmbs = useNamedGmbs();
  // those up to this one are folded into the bell
  const [folded, setFolded] = useState(0);
  const urgent = unread.status === "ok" ? unread.data.alerts.filter((a) => a.tier === "urgent") : [];
  const showing = urgent.filter((a) => a.id > folded);
  const oldest = showing.at(-1);
  const newest = showing[0]?.id ?? 0;
  useEffect(() => {
    if (newest === 0) return;
    const onKey = (e: KeyboardEvent) => {
      // a layer, a window or a field has the key first
      if (e.key !== "Escape" || e.defaultPrevented || e.target !== document.body) return;
      setFolded(newest);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [newest]);
  if (oldest === undefined) return undefined;
  const { title, sub } = lines(oldest, gmbs);
  const of = showing.length > 1 ? t(keys.takeover.of, "1 of {n} urgent", { n: showing.length }) : "";
  return {
    tone: "error",
    message: (
      <span className="inline-flex items-center gap-2">
        <Flame aria-hidden className="size-4 shrink-0" />
        <Waiting key={oldest.id} at={oldest.at} />
        <span className="font-semibold text-ink">{title}</span>
        <span className="text-ink-mid">{[sub, of].filter((s) => s !== "").join(" · ")}</span>
      </span>
    ),
    action: <Actions alert={oldest} onFold={() => setFolded(newest)} />,
  };
}

/** How long it has waited: to the second while seconds count. */
function Waiting({ at }: { at: string }) {
  const t = useT();
  const now = useNow(1_000);
  const seconds = Math.max(0, Math.floor((now.getTime() - Date.parse(at)) / 1_000));
  const minutes = Math.floor(seconds / 60);
  return (
    // re-announcing every second would drown the alert it stands in
    <span aria-hidden className="shrink-0 rounded-sm bg-accent-error px-1.5 py-0.5 font-mono text-xs font-semibold text-on-accent-error tabular-nums">
      {minutes < 10 ? `${minutes}:${String(seconds % 60).padStart(2, "0")}` : minutes < 120 ? t(keys.takeover.minutes, "{n} min", { n: minutes }) : t(keys.takeover.hours, "{n} h", { n: Math.floor(minutes / 60) })}
    </span>
  );
}

function Actions({ alert, onFold }: { alert: Alert; onFold: () => void }) {
  const t = useT();
  const unread = useUnread();
  const phone = phoneOf(alert.text);
  const open = () => {
    void readAlert(alert.id).then(unread.reload, (e: unknown) => notifyFailure(e, t));
    go(alert.link ?? { view: "alerts", params: { open: alert.id } });
  };
  return (
    <span className="flex shrink-0 items-center gap-2 pl-1">
      {phone !== null && (
        <Button asChild variant="secondary" size="sm">
          <a href={`tel:${phone.replace(/[^\d+]/g, "")}`}>
            <Phone aria-hidden className="size-4" />
            {phone}
          </a>
        </Button>
      )}
      <AlertButtons key={alert.id} alert={alert} />
      {alert.buttons.length === 0 && (
        <Button type="button" size="sm" onClick={open}>
          <ExternalLink aria-hidden className="size-4" />
          {t(keys.open, "Open")}
        </Button>
      )}
      <Button type="button" variant="ghost" size="sm" icon onClick={onFold} aria-label={t(keys.takeover.fold, "Fold into the bell")} title={t(keys.takeover.fold, "Fold into the bell")}>
        <X aria-hidden className="size-4" />
      </Button>
    </span>
  );
}
