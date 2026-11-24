"use client";

import { Button, cn } from "@evinvest/uikit";
import { MailX } from "lucide-react";

import { openWindow, useGmails, useScope } from "@/entities/gmb";
import { mail } from "@/entities/mail";
import { useT } from "@/shared/i18n";
import { useButtonSize } from "@/shared/ui/touch";

/** The gmails in scope whose mail is not read: one that fails goes first in the flex column it is in, one with no inbox set last. */
export function Inboxes() {
  const t = useT();
  const button = useButtonSize();
  const gmails = useGmails();
  const scope = useScope();
  if (gmails.status !== "ok") return null;
  return gmails.data.gmails
    .filter((g) => (scope.gmails === null || scope.gmails.has(g.id)) && (g.imap === null || g.imap.error !== null))
    .map((g) => (
      <div key={g.id} className={cn("flex items-center gap-3 border-border px-3 py-2 text-sm", g.imap === null ? "order-last border-t" : "order-first border-b")}>
        <MailX aria-hidden className={cn("size-4 shrink-0", g.imap === null ? "text-ink-soft" : "text-accent-error")} />
        <p className="flex min-w-0 flex-1 flex-col">
          <span className="truncate text-ink">{g.imap === null ? t(mail.inbox.unset, "Not read: {gmail}", { gmail: g.gmail }) : t(mail.inbox.failing, "Cannot read {gmail}", { gmail: g.gmail })}</span>
          {g.imap !== null && (
            <span className="truncate text-xs text-accent-error" title={g.imap.error ?? undefined}>
              {g.imap.error}
            </span>
          )}
        </p>
        <Button variant="outline" size={button("sm")} onClick={() => openWindow({ kind: "account", gmail: g.id, tab: "connections" })}>
          {g.imap === null ? t(mail.inbox.set, "Set its IMAP") : t(mail.inbox.reconnect, "Reconnect")}
        </Button>
      </div>
    ));
}
