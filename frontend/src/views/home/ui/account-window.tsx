"use client";

import { Button, NativeSelect, NativeSelectOption, Settled, Skeleton, cn } from "@evinvest/uikit";
import { Mail, X } from "lucide-react";

import { GMAIL_ROLE_NAME, openAccount, useGmails, windowKeys, gmbs } from "@/entities/gmb";
import { BrowserSession } from "@/features/browser-session";
import { ImapCard } from "@/features/connect-imap";
import { BrowserCard } from "@/features/set-browser";
import { MapsKeyCard } from "@/features/set-maps-key";
import { api, ok } from "@/shared/api";
import { type Gmail, type GmailRole, type Tab, gmailRoleValues } from "@/shared/api/schema";
import { useT, account } from "@/shared/i18n";
import { useResource } from "@evinvest/api/react";
import { ErrorState } from "@/shared/ui/error-state";
import { TAB_NAME } from "./window";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize } from "@/shared/ui/touch";

/** A managing gmail's own window over the map, shaped as a trade's: what it is connected to, and its browser. */
export function AccountWindow({ gmail, tab, gbps }: { gmail: Gmail; tab: Tab; gbps: number }) {
  const t = useT();
  if (!gmail.tabs.includes(tab)) throw new Error(`?tab=${tab}: not a tab of gmail ${gmail.id}'s window`);
  const button = useButtonSize();

  return (
    <section aria-label={gmail.gmail} className="absolute inset-y-0 right-0 left-0 flex flex-col overflow-y-auto border-l border-border bg-background md:left-36">
      <header className="flex items-start gap-3 px-6 pt-4">
        <div className="flex min-w-0 flex-1 flex-col gap-0.5">
          <h2 className="flex min-w-0 items-center gap-2 text-lg font-semibold text-ink">
            <Mail aria-hidden className="size-4 shrink-0 text-ink-soft" />
            <span className="truncate">{gmail.gmail}</span>
          </h2>
          <p className="flex items-center gap-2 text-sm text-ink-soft tabular-nums">
            {t(account.gbps, "{n} GBP", { n: gbps })}
            <RoleSelect gmail={gmail} />
          </p>
        </div>
        <Button variant="ghost" size={button("sm")} icon aria-label={t(windowKeys.close, "Close the window")} title={t(windowKeys.close, "Close the window")} onClick={() => openAccount(null)}>
          <X aria-hidden />
        </Button>
      </header>
      <nav role="tablist" aria-label={gmail.gmail} className="flex items-center gap-1 border-b border-border px-6 pt-3">
        {gmail.tabs.map((k) => (
          <button
            key={k}
            type="button"
            role="tab"
            aria-selected={k === tab}
            onClick={() => openAccount({ gmail: gmail.id, tab: k })}
            className={cn("-mb-px border-b-2 px-3 py-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring", k === tab ? "border-ink text-ink" : "border-transparent text-ink-soft hover:text-ink")}
          >
            {TAB_NAME[k](t)}
          </button>
        ))}
      </nav>
      <div role="tabpanel" className="p-6">
        {tab === "connections" && <Connections gmail={gmail} />}
        {tab === "browser" && <BrowserSession gmail={gmail} gmb={null} />}
      </div>
    </section>
  );
}

function Connections({ gmail }: { gmail: Gmail }) {
  const connections = useResource(api, "/gmails/{id}/connections", { params: { path: { id: gmail.id } }, keep: true });
  return (
    <Settled loading={connections.status === "loading"} skeleton={<Skeleton className="h-64 w-full" />}>
      {connections.status === "error" && <ErrorState failure={connections.failure} onRetry={connections.reload} />}
      {connections.status === "ok" && (
        <div className="grid gap-4 lg:grid-cols-2">
          <ImapCard gmail={gmail} imap={connections.data.imap} onChanged={connections.reload} />
          <MapsKeyCard gmail={gmail.id} mapsKey={connections.data.maps_key} prices={connections.data.prices} onChanged={connections.reload} />
          <BrowserCard gmail={gmail.id} browser={connections.data.browser} onChanged={connections.reload} />
        </div>
      )}
    </Settled>
  );
}

function RoleSelect({ gmail }: { gmail: Gmail }) {
  const t = useT();
  const { reload } = useGmails();
  const pick = async (role: GmailRole | null) => {
    try {
      await ok(api.PATCH("/gmails/{id}", { params: { path: { id: gmail.id } }, body: { role } }));
      reload();
    } catch (e) {
      notifyFailure(e, t);
    }
  };
  return (
    <NativeSelect
      size="sm"
      wrapperClassName="w-28 shrink-0"
      aria-label={t(gmbs.gmail.role, "Role of {gmail} on its GBPs", { gmail: gmail.gmail })}
      title={t(gmbs.gmail.role.hint, "optional · owners usually hold up to 3 GBPs, managers 6")}
      value={gmail.role ?? ""}
      onChange={(e) => void pick(e.target.value === "" ? null : (e.target.value as GmailRole))}
    >
      <NativeSelectOption value="">{t(gmbs.gmail.role.pick, "No role")}</NativeSelectOption>
      {gmailRoleValues.map((r) => (
        <NativeSelectOption key={r} value={r}>
          {GMAIL_ROLE_NAME[r](t)}
        </NativeSelectOption>
      ))}
    </NativeSelect>
  );
}
