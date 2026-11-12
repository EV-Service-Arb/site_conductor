"use client";

import { Button, Input, NativeSelect, NativeSelectOption } from "@evinvest/uikit";
import { useState } from "react";

import { addTrade } from "../api/gmbs";
import { useTrades } from "../model/gmails-context";
import type { Country } from "../model/gmb";
import { useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize, useControlSize } from "@/shared/ui/touch";

const NEW = "new";

/**
 * The caller's trades, then "New trade…", which names one and picks it; `none` labels the choice of no trade, absent while one is held.
 * With `country`, each trade says what it resolves to there: Google's primary category, or not set up.
 */
export function TradeSelect({ value, onPick, none, label, country, className }: { value: number | null; onPick: (trade: number | null) => void; none?: string; label: string; country?: Country | null; className?: string }) {
  const t = useT();
  const size = useControlSize();
  const button = useButtonSize();
  const trades = useTrades();
  const [naming, setNaming] = useState(false);
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);

  if (naming)
    return (
      <form
        className="flex gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          setBusy(true);
          void addTrade(name.trim())
            .then((made) => {
              trades.reload();
              setNaming(false);
              setName("");
              onPick(made.id);
            })
            .catch((err: unknown) => notifyFailure(err, t))
            .finally(() => setBusy(false));
        }}
      >
        <Input size={size} autoFocus aria-label={t("trades.new.name")} placeholder={t("trades.new.name")} value={name} onChange={(e) => setName(e.target.value)} className="min-w-0 flex-1" />
        <Button type="submit" size={button("sm")} disabled={busy || name.trim() === ""}>
          {t("trades.new.add")}
        </Button>
        <Button type="button" variant="ghost" size={button("sm")} onClick={() => setNaming(false)}>
          {t("move.cancel")}
        </Button>
      </form>
    );
  return (
    <NativeSelect
      size="sm"
      wrapperClassName={className ?? "w-56 max-w-full"}
      aria-label={label}
      disabled={trades.status !== "ok"}
      value={value === null ? "" : String(value)}
      onChange={(e) => {
        const v = e.target.value;
        if (v === NEW) setNaming(true);
        else onPick(v === "" ? null : Number(v));
      }}
    >
      {(value === null || none !== undefined) && <NativeSelectOption value="">{none ?? t("trades.pick")}</NativeSelectOption>}
      {trades.status === "ok" &&
        trades.data.map((tr) => {
          const there = country == null ? undefined : (tr.categories.find((c) => c.country === country) ?? null);
          return (
            <NativeSelectOption key={tr.id} value={String(tr.id)}>
              {there === undefined ? tr.name : there === null ? t("trades.option.notSetUp", { trade: tr.name, country: country! }) : `${tr.name} — ${there.google_primary?.name ?? "…"}`}
            </NativeSelectOption>
          );
        })}
      <NativeSelectOption value={NEW}>{t("trades.new")}</NativeSelectOption>
    </NativeSelect>
  );
}
