"use client";

import { Button, Card, CardContent, CardHeader, CardTitle, PageFrame, Skeleton } from "@evinvest/uikit";
import Link from "next/link";
import { useState } from "react";

import { may, useMe } from "@/entities/session";
import { type LedgerEntry, UsageFigures, fetchLedger, fetchUsage, useTokens, tokens as tokensKeys } from "@/entities/tokens";
import { ClaimButton } from "@/features/claim-kickbacks";
import { ROUTES } from "@/shared/config/routes";
import { type T, useLocale, useT, account } from "@/shared/i18n";
import { formatDateTime } from "@/shared/lib/format";
import { useResource } from "@/shared/lib/use-resource";
import { ErrorState } from "@/shared/ui/error-state";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize } from "@/shared/ui/touch";

const PAGE = 50;

const KIND_NAME = {
  accrual: (t) => t(tokensKeys.kind.accrual, "Daily renewal"),
  add: (t) => t(tokensKeys.kind.add, "Added"),
  charge: (t) => t(tokensKeys.kind.charge, "Charge"),
  kickback: (t) => t(tokensKeys.kind.kickback, "Given back"),
  set: (t) => t(tokensKeys.kind.set, "Set"),
} satisfies Record<LedgerEntry["kind"], (t: T) => string>;

export function TokensView() {
  const t = useT();
  const me = useMe();
  const tokens = useTokens();
  const usage = useResource("tokens.usage", fetchUsage, "tokens.usage", { focus: true });
  const [ledgerVersion, setLedgerVersion] = useState(0);
  const refresh = () => {
    tokens.reload();
    usage.reload();
    setLedgerVersion((v) => v + 1);
  };

  return (
    <PageFrame
      breakpoint="md"
      title={t(tokensKeys.title, "Tokens")}
      description={t(tokensKeys.description, "What your walks may spend, what is owed back to you, and every change.")}
      width="content"
      actions={
        may(me, "sa:admin:tokens:grant") && (
          <Link className="text-sm text-ink hover:underline" href={ROUTES.adminTokens}>
            {t(tokensKeys.admin, "All balances")}
          </Link>
        )
      }
    >
      <Section title={t(tokensKeys.balance, "Now")}>
        {tokens.status === "loading" && <Skeleton className="h-10 w-48" />}
        {tokens.status === "error" && <ErrorState failure={tokens.failure} onRetry={tokens.reload} />}
        {tokens.status === "ok" && (
          <>
            <span className="text-2xl font-semibold tabular-nums">{t(tokensKeys.count, "{n} tokens", { n: tokens.data.balance })}</span>
            <p className="text-sm text-ink-soft">{t(account.tokens.renewal, "+{daily} a day, up to {cap}", { daily: tokens.data.daily, cap: tokens.data.cap })}</p>
          </>
        )}
      </Section>
      {tokens.status === "ok" && tokens.data.kickbacks.length > 0 && (
        <Section title={t(tokensKeys.kickbacks, "Owed back")}>
          <ul className="flex flex-col gap-1 text-sm">
            {tokens.data.kickbacks.map((k) => (
              <li key={k.id} className="flex items-baseline gap-3">
                <span className="tabular-nums text-ink">+{k.amount}</span>
                <span className="min-w-0 flex-1 truncate text-ink-mid">{k.note ?? k.label ?? k.key}</span>
              </li>
            ))}
          </ul>
          <div>
            <ClaimButton total={tokens.data.kickbacks.reduce((n, k) => n + k.amount, 0)} onClaimed={refresh} />
          </div>
        </Section>
      )}
      <Section title={t(account.usage, "Usage")}>
        {usage.status === "loading" && <Skeleton className="h-24 w-full" />}
        {usage.status === "error" && <ErrorState failure={usage.failure} onRetry={usage.reload} />}
        {usage.status === "ok" && <UsageFigures days={usage.data} />}
      </Section>
      <Section title={t(tokensKeys.ledger, "History")}>
        <Ledger key={ledgerVersion} />
      </Section>
    </PageFrame>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <Card className="gap-3 py-4">
      <CardHeader className="px-4">
        <CardTitle className="text-xs font-medium uppercase tracking-wide text-ink-soft">{title}</CardTitle>
      </CardHeader>
      <CardContent className="flex flex-col gap-2 px-4">{children}</CardContent>
    </Card>
  );
}

/** Newest first, a page at a time by the API's cursor. */
function Ledger() {
  const t = useT();
  const locale = useLocale();
  const button = useButtonSize();
  const first = useResource("tokens.ledger", () => fetchLedger(null, PAGE));
  const [more, setMore] = useState<{ entries: LedgerEntry[]; next: number | null } | null>(null);
  const [busy, setBusy] = useState(false);

  if (first.status === "loading") return <Skeleton className="h-24 w-full" />;
  if (first.status === "error") return <ErrorState failure={first.failure} onRetry={first.reload} />;
  const entries = [...first.data.entries, ...(more?.entries ?? [])];
  const next = more ? more.next : first.data.next;

  const loadMore = async (before: number) => {
    setBusy(true);
    try {
      const page = await fetchLedger(before, PAGE);
      setMore((m) => ({ entries: [...(m?.entries ?? []), ...page.entries], next: page.next }));
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
    }
  };

  if (entries.length === 0) return <p className="text-sm text-ink-soft">{t(tokensKeys.ledger.empty, "Nothing yet.")}</p>;
  return (
    <>
      <ul className="flex flex-col divide-y divide-border text-sm">
        {entries.map((e) => (
          <li key={e.id} className="grid grid-cols-(--grid-ledger-row) items-baseline gap-3 py-1.5">
            <span className={e.delta < 0 ? "tabular-nums text-ink-mid" : "tabular-nums text-positive"}>
              {e.delta > 0 ? `+${e.delta}` : e.delta}
            </span>
            <span className="min-w-0 truncate text-ink">
              {KIND_NAME[e.kind](t)}
              {(e.label ?? e.note) && <span className="text-ink-soft"> · {e.label ?? e.note}</span>}
              {e.by && <span className="text-ink-soft"> · {e.by}</span>}
            </span>
            <span className="text-xs tabular-nums text-ink-soft">{formatDateTime(e.at, locale)}</span>
          </li>
        ))}
      </ul>
      {next !== null && (
        <div>
          <Button variant="outline" size={button("sm")} disabled={busy} onClick={() => void loadMore(next)}>
            {t(tokensKeys.ledger.more, "More")}
          </Button>
        </div>
      )}
    </>
  );
}
