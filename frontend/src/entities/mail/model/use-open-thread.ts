"use client";

import { toast } from "@evinvest/uikit";
import { useState } from "react";

import { api, ok } from "@/shared/api";
import { useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";

import { mail } from "./i18n";

/** Opens a Gmail thread in its gmail's running browser; `opening`: the one being opened. */
export function useOpenThread(): { opening: string | null; open: (gmail: number, thread: string) => Promise<void> } {
  const t = useT();
  const [opening, setOpening] = useState<string | null>(null);
  const open = async (gmail: number, thread: string) => {
    setOpening(thread);
    try {
      await ok(api.POST("/gmails/{id}/browser/thread", { params: { path: { id: gmail } }, body: { thread } }));
      toast(t(mail.thread.opened, "Opened in the gmail's browser"));
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setOpening(null);
    }
  };
  return { opening, open };
}
