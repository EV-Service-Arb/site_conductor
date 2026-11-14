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
  const [autocomplete, setAutocomplete] = useState(prices.autocomplete?.toString() ?? "");
  const [busy, setBusy] = useState(false);
  const d = tokens(details);
  const s = tokens(search);
  const a = tokens(autocomplete);
  const changed = d !== prices.details || s !== prices.search || a !== prices.autocomplete;

  return (
    <Card className="gap-3 py-4">
      <CardContent className="flex flex-col gap-3 px-4">
        <h2 className="text-sm font-semibold text-ink">{t("Places on the shared keys")}</h2>
        <p className="text-xs text-ink-soft">{t("Tokens a Google Places request costs when a person has no Maps key of their own that answers. Cached answers and own keys cost nothing.")}</p>
        <form
          className="flex flex-wrap items-end gap-3"
          onSubmit={(e) => {
            e.preventDefault();
            if (d === null || s === null || a === null) return;
            setBusy(true);
            setPrices({ details: d, search: s, autocomplete: a })
              .then(() => {
                toast.positive(t("Prices saved"));
                onSaved();
              })
              .catch((err: unknown) => notifyFailure(err, t))
              .finally(() => setBusy(false));
          }}
        >
          <Field className="w-36">
            <FieldLabel htmlFor={`${id}-details`}>{t("Lookup (details)")}</FieldLabel>
            <Input id={`${id}-details`} type="number" inputMode="numeric" min={0} size={size} value={details} onChange={(e) => setDetails(e.target.value)} placeholder={t("not priced")} />
          </Field>
          <Field className="w-36">
            <FieldLabel htmlFor={`${id}-search`}>{t("Search")}</FieldLabel>
            <Input id={`${id}-search`} type="number" inputMode="numeric" min={0} size={size} value={search} onChange={(e) => setSearch(e.target.value)} placeholder={t("not priced")} />
          </Field>
          <Field className="w-36">
            <FieldLabel htmlFor={`${id}-autocomplete`}>{t("Suggestions (as typed)")}</FieldLabel>
            <Input id={`${id}-autocomplete`} type="number" inputMode="numeric" min={0} size={size} value={autocomplete} onChange={(e) => setAutocomplete(e.target.value)} placeholder={t("not priced")} />
          </Field>
          <Button type="submit" size={button("sm")} disabled={busy || d === null || s === null || a === null || !changed}>
            {t("Save")}
          </Button>
        </form>
        {(prices.details === null || prices.search === null || prices.autocomplete === null) && <p className="text-xs text-accent-warn">{t("Not priced: requests on the shared keys are refused for everyone without a key of their own.")}</p>}
      </CardContent>
    </Card>
  );
}
