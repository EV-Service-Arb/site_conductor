"use client";

import { Button, NativeSelect, NativeSelectOption, Settled, Skeleton, cn } from "@evinvest/uikit";
import { Mail, X } from "lucide-react";

import { ACCOUNT_TABS, type AccountTab, GMAIL_ROLES, type Gmail, type GmailRole, fetchConnections, openAccount, setGmailRole, useGmails, windowKeys, gmbs, GMAIL_ROLE_NAME } from "@/entities/gmb";
import { BrowserSession } from "@/features/browser-session";
import { ImapCard } from "@/features/connect-imap";
import { BrowserCard } from "@/features/set-browser";
import { MapsKeyCard } from "@/features/set-maps-key";
import { type T, useT, account } from "@/shared/i18n";
import { useResource } from "@/shared/lib/use-resource";
import { ErrorState } from "@/shared/ui/error-state";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize } from "@/shared/ui/touch";

/** A managing gmail's own window over the map, shaped as a trade's: what it is connected to, and its browser. */
const TAB_NAME = {
  connections: (t) => t(account.tab.connections, "Connections"),
  browser: (t) => t(account.tab.browser, "Browser"),
} satisfies Record<AccountTab, (t: T) => string>;

export function AccountWindow({ gmail, tab, gbps }: { gmail: Gmail; tab: AccountTab; gbps: number }) {
  const t = useT();
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
        {ACCOUNT_TABS.map((k) => (
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
  const connections = useResource(`connections.${gmail.id}`, () => fetchConnections(gmail.id), "connections", { live: "browsers" });
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
      await setGmailRole(gmail.id, role);
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
      {GMAIL_ROLES.map((r) => (
        <NativeSelectOption key={r} value={r}>
          {GMAIL_ROLE_NAME[r](t)}
        </NativeSelectOption>
      ))}
    </NativeSelect>
  );
}
