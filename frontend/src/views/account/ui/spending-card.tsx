"use client";

import { Card, CardContent, CardHeader, CardTitle, Input, Skeleton } from "@evinvest/uikit";
import { useState } from "react";

import { tokens as keys, useTokens } from "@/entities/tokens";
import { api, ok } from "@/shared/api";
import { useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";
import { useControlSize } from "@/shared/ui/touch";

/** Above how many tokens an action asks first. */
export function SpendingCard() {
  const t = useT();
  const held = useTokens();
  return (
    <Card className="gap-3 py-4">
      <CardHeader className="px-4">
        <CardTitle className="text-xs font-medium uppercase tracking-wide text-ink-soft">{t(keys.spending, "Spending")}</CardTitle>
      </CardHeader>
      <CardContent className="px-4">{held.status === "ok" ? <ConfirmAbove set={held.data.confirm_above} onSet={held.reload} /> : <Skeleton className="h-9 w-full" />}</CardContent>
    </Card>
  );
}

function ConfirmAbove({ set, onSet }: { set: number; onSet: () => void }) {
  const t = useT();
  const size = useControlSize();
  const [typed, setTyped] = useState(String(set));
  const save = async () => {
    const n = Number(typed);
    if (typed.trim() === "" || !Number.isInteger(n) || n === set) return setTyped(String(set));
    try {
      await ok(api.PUT("/tokens/settings", { body: { confirm_above: n } }));
    } catch (e) {
      notifyFailure(e, t);
      setTyped(String(set));
    }
    onSet();
  };
  return (
    <label className="flex flex-wrap items-center gap-2 text-sm text-ink">
      {t(keys.confirmAbove.before, "Ask before an action costing more than")}
      <Input size={size} type="number" min={0} step={1} inputMode="numeric" value={typed} onChange={(e) => setTyped(e.target.value)} onBlur={() => void save()} onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()} className="w-24 tabular-nums" />
      {t(keys.confirmAbove.after, "tokens")}
    </label>
  );
}
