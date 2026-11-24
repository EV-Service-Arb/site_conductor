"use client";

import { Button, cn } from "@evinvest/uikit";
import { useResource } from "@evinvest/api/react";
import { MailX } from "lucide-react";

import { openWindow, useGmails, useScope } from "@/entities/gmb";
import { mail } from "@/entities/mail";
import { api } from "@/shared/api";
import type { Gmail } from "@/shared/api/schema";
import { useT } from "@/shared/i18n";
import { useButtonSize } from "@/shared/ui/touch";

/** The gmails in scope whose mail is not read: one that fails goes first in the flex column it is in, one with no inbox set last. */
export function Inboxes() {
  const gmails = useGmails();
  const scope = useScope();
  if (gmails.status !== "ok") return null;
  // shortcut: a read a gmail, `GET /gmails` does not carry its inbox's standing; one read once it does
  return gmails.data.gmails.filter((g) => scope.gmails === null || scope.gmails.has(g.id)).map((g) => <Inbox key={g.id} gmail={g} />);
}

function Inbox({ gmail }: { gmail: Gmail }) {
  const t = useT();
  const button = useButtonSize();
  const connections = useResource(api, "/gmails/{id}/connections", { params: { path: { id: gmail.id } } });
  if (connections.status !== "ok") return null;
  const { imap } = connections.data;
  if (imap !== null && imap.checked.error === null) return null;
  return (
    <div className={cn("flex items-center gap-3 border-border px-3 py-2 text-sm", imap === null ? "order-last border-t" : "order-first border-b")}>
      <MailX aria-hidden className={imap === null ? "size-4 shrink-0 text-ink-soft" : "size-4 shrink-0 text-accent-error"} />
      <p className="flex min-w-0 flex-1 flex-col">
        <span className="truncate text-ink">{imap === null ? t(mail.inbox.unset, "Not read: {gmail}", { gmail: gmail.gmail }) : t(mail.inbox.failing, "Cannot read {gmail}", { gmail: gmail.gmail })}</span>
        {imap !== null && <span className="truncate text-xs text-accent-error" title={imap.checked.error ?? undefined}>{imap.checked.error}</span>}
      </p>
      <Button variant="outline" size={button("sm")} onClick={() => openWindow({ kind: "account", gmail: gmail.id, tab: "connections" })}>
        {imap === null ? t(mail.inbox.set, "Set its IMAP") : t(mail.inbox.reconnect, "Reconnect")}
      </Button>
    </div>
  );
}
