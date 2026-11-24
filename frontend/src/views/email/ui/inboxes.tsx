"use client";

import { Button, cn } from "@evinvest/uikit";
import { ChevronDown, MailX } from "lucide-react";
import { type ReactNode, useState } from "react";

import { pageOf, useGmails, useScope } from "@/entities/gmb";
import { mail } from "@/entities/mail";
import type { Gmail } from "@/shared/api/schema";
import { useT } from "@/shared/i18n";
import { ViewLink } from "@/shared/lib/location";
import { useButtonSize } from "@/shared/ui/touch";

/** The gmails in scope whose mail is not read: those that fail go first in the flex column they are in, those with no inbox set last. */
export function Inboxes() {
  const t = useT();
  const gmails = useGmails();
  const scope = useScope();
  if (gmails.status !== "ok") return null;
  const inScope = gmails.data.gmails.filter((g) => scope.gmails === null || scope.gmails.has(g.id));
  const failing = inScope.filter((g) => g.imap !== null && g.imap.error !== null);
  const unset = inScope.filter((g) => g.imap === null);
  return (
    <>
      <Many of={failing} title={t(mail.inbox.failing.n, "Cannot read {n} inboxes", { n: failing.length })} ink="text-accent-error" className="order-first border-b" />
      <Many of={unset} title={t(mail.inbox.unset.n, "{n} gmails not read: no IMAP set", { n: unset.length })} ink="text-ink-soft" className="order-last border-t" />
    </>
  );
}

const APART = 2;

/** Each inbox in a row; more than `APART` in one, opened. */
function Many({ of, title, ink, className }: { of: readonly Gmail[]; title: string; ink: string; className: string }) {
  if (of.length === 0) return null;
  const rows = of.map((g) => <Inbox key={g.id} g={g} />);
  return <div className={cn("flex shrink-0 flex-col divide-y divide-border border-border", className)}>{of.length <= APART ? rows : <Fold title={title} icon={cn("size-4 shrink-0", ink)}>{rows}</Fold>}</div>;
}

function Fold({ title, icon, children }: { title: string; icon: string; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button type="button" aria-expanded={open} onClick={() => setOpen(!open)} className="flex min-h-11 items-center gap-3 px-3 py-2 text-left text-sm text-ink outline-none hover:bg-hover/50 focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset">
        <MailX aria-hidden className={icon} />
        <span className="min-w-0 flex-1 break-words">{title}</span>
        <ChevronDown aria-hidden className={cn("size-4 shrink-0 text-ink-soft", open && "rotate-180")} />
      </button>
      {open && children}
    </>
  );
}

function Inbox({ g }: { g: Gmail }) {
  const t = useT();
  const button = useButtonSize();
  return (
    <div className="flex items-center gap-3 px-3 py-2 text-sm">
      <MailX aria-hidden className={cn("size-4 shrink-0", g.imap === null ? "text-ink-soft" : "text-accent-error")} />
      <p className="flex min-w-0 flex-1 flex-col break-words">
        <span className="text-ink">{g.imap === null ? t(mail.inbox.unset, "Not read: {gmail}", { gmail: g.gmail }) : t(mail.inbox.failing, "Cannot read {gmail}", { gmail: g.gmail })}</span>
        {g.imap !== null && <span className="text-xs text-accent-error">{g.imap.error}</span>}
      </p>
      <Button asChild variant="outline" size={button("sm")}>
        <ViewLink to={pageOf({ kind: "account", gmail: g.id, tab: "connections" })}>{g.imap === null ? t(mail.inbox.set, "Set its IMAP") : t(mail.inbox.reconnect, "Reconnect")}</ViewLink>
      </Button>
    </div>
  );
}
