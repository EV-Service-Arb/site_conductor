"use client";

import { ConfirmDialog } from "@evinvest/uikit";
import { type ReactNode, useState } from "react";

import { useT } from "@/shared/i18n";

import { tokens as keys } from "./i18n";
import { useTokens } from "./use-tokens";

type Act = () => Promise<void>;

/**
 * `ask(cost, act)` runs `act`: at once, or past the caller's `confirm_above` once they confirm in
 * `dialog`; settled as `act` settles, or with nothing done when they cancel. An unpriced `cost` is
 * refused by the conductor, saying why.
 */
export function useSpendConfirm(): { ask: (cost: number | undefined, act: Act) => Promise<void>; dialog: ReactNode } {
  const t = useT();
  const held = useTokens();
  const [asked, setAsked] = useState<{ cost: number; answer: (go: boolean) => void } | null>(null);
  const ask = async (cost: number | undefined, act: Act) => {
    if (held.status !== "ok") throw new Error("a cost asked of before the caller's tokens are read");
    const go = cost === undefined || cost <= held.data.confirm_above || (await new Promise<boolean>((answer) => setAsked({ cost, answer })));
    if (go) await act();
  };
  const answer = (go: boolean) => {
    asked?.answer(go);
    setAsked(null);
  };
  const dialog = asked !== null && held.status === "ok" && (
    <ConfirmDialog
      open
      onOpenChange={(open) => !open && answer(false)}
      title={t(keys.confirm, "Spend ≈ {cost} tokens?", { cost: asked.cost })}
      description={t(keys.confirm.body, "This costs ≈ {cost} tokens of your {balance}.", { cost: asked.cost, balance: held.data.balance })}
      confirmLabel={t(keys.confirm.spend, "Spend")}
      labels={{ cancel: t(keys.confirm.cancel, "Cancel") }}
      onConfirm={() => answer(true)}
    />
  );
  return { ask, dialog };
}
