"use client";

import { Button, Card, CardContent, Field, FieldLabel, Input, toast } from "@evinvest/uikit";
import { useId, useState } from "react";

import { type Prices, setPrices } from "@/entities/gmb";
import { useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize, useControlSize } from "@/shared/ui/touch";

const tokens = (raw: string) => (raw.trim() !== "" && Number.isSafeInteger(Number(raw)) && Number(raw) >= 0 ? Number(raw) : null);

/** What a Places request on the shared keys costs a person, per kind; a person's own key costs nothing. */
export function PricesCard({ prices, onSaved }: { prices: Prices; onSaved: () => void }) {
  const t = useT();
  const id = useId();
  const button = useButtonSize();
  const size = useControlSize();
  const [details, setDetails] = useState(prices.details?.toString() ?? "");
  const [search, setSearch] = useState(prices.search?.toString() ?? "");
  const [busy, setBusy] = useState(false);
  const d = tokens(details);
  const s = tokens(search);
  const changed = d !== prices.details || s !== prices.search;

  return (
    <Card className="gap-3 py-4">
      <CardContent className="flex flex-col gap-3 px-4">
        <h2 className="text-sm font-semibold text-ink">{t("prices.title")}</h2>
        <p className="text-xs text-ink-soft">{t("prices.why")}</p>
        <form
          className="flex flex-wrap items-end gap-3"
          onSubmit={(e) => {
            e.preventDefault();
            if (d === null || s === null) return;
            setBusy(true);
            setPrices({ details: d, search: s })
              .then(() => {
                toast.positive(t("prices.saved"));
                onSaved();
              })
              .catch((err: unknown) => notifyFailure(err, t))
              .finally(() => setBusy(false));
          }}
        >
          <Field className="w-36">
            <FieldLabel htmlFor={`${id}-details`}>{t("prices.details")}</FieldLabel>
            <Input id={`${id}-details`} type="number" inputMode="numeric" min={0} size={size} value={details} onChange={(e) => setDetails(e.target.value)} placeholder={t("prices.unset")} />
          </Field>
          <Field className="w-36">
            <FieldLabel htmlFor={`${id}-search`}>{t("prices.search")}</FieldLabel>
            <Input id={`${id}-search`} type="number" inputMode="numeric" min={0} size={size} value={search} onChange={(e) => setSearch(e.target.value)} placeholder={t("prices.unset")} />
          </Field>
          <Button type="submit" size={button("sm")} disabled={busy || d === null || s === null || !changed}>
            {t("prices.save")}
          </Button>
        </form>
        {(prices.details === null || prices.search === null) && <p className="text-xs text-accent-warn">{t("prices.unpriced")}</p>}
      </CardContent>
    </Card>
  );
}
