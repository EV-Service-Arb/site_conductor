"use client";

import { Button, toast } from "@evinvest/uikit";
import { Smartphone } from "lucide-react";
import { useState } from "react";

import { TelegramIcon, alerts as keys, pushable, subscribePush, useAlerts } from "@/entities/alerts";
import { useMe } from "@/entities/session";
import { api, ok } from "@/shared/api";
import { type T, useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize } from "@/shared/ui/touch";

/** Opened within the click, or a popup blocker takes it: the link only exists after the POST. */
export async function openBot(t: T): Promise<void> {
  const tab = window.open("about:blank", "_blank");
  try {
    const { url } = await ok(api.POST("/alerts/telegram/link"));
    if (tab === null) window.location.assign(url);
    else tab.location.href = url;
  } catch (e) {
    tab?.close();
    notifyFailure(e, t);
  }
}

/** The caller's own chat with the bot. */
export function ConnectTelegram({ label, onDone }: { label?: string; onDone?: () => void }) {
  const t = useT();
  const me = useMe();
  const size = useButtonSize();
  const alerts = useAlerts();
  const [busy, setBusy] = useState(false);
  if (alerts.status !== "ok" || !alerts.data.methods.includes("telegram") || !api.mayCall("post", "/alerts/telegram/link", me.permissions)) return null;
  const connect = () => {
    setBusy(true);
    void openBot(t).finally(() => {
      setBusy(false);
      onDone?.();
    });
  };
  return (
    <Button type="button" size={size("sm")} disabled={busy} onClick={connect}>
      <TelegramIcon className="size-4" />
      {label ?? t(keys.connect, "Connect Telegram")}
    </Button>
  );
}

/** This browser's pushes: never while acting in another's account, whose alerts they would be. */
export function PushHere({ label, variant = "outline", onDone }: { label?: string; variant?: "outline" | "primary"; onDone?: () => void }) {
  const t = useT();
  const me = useMe();
  const size = useButtonSize();
  const alerts = useAlerts();
  const [busy, setBusy] = useState(false);
  const key = alerts.status === "ok" ? alerts.data.push_key : null;
  if (key === null || me.acting !== null || pushable() !== "yes" || !api.mayCall("post", "/alerts/push", me.permissions)) return null;
  const allow = async () => {
    setBusy(true);
    try {
      if ((await subscribePush(me.user_id, key)) === null) return void toast.warn(t(keys.push.refused, "This browser blocks notifications here: allow them in its site settings, then try again."));
      onDone?.();
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
      alerts.reload();
    }
  };
  return (
    <Button type="button" variant={variant} size={size("sm")} disabled={busy} onClick={() => void allow()}>
      <Smartphone aria-hidden className="size-4" />
      {label ?? t(keys.push.here, "Push on this browser")}
    </Button>
  );
}
