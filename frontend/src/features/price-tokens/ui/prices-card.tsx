"use client";

import { Badge, Button, Card, CardContent, Input, toast } from "@evinvest/uikit";
import { type ChangeEvent, useState } from "react";

import { type Price, type PricedItem, type Scope, setPrices } from "@/entities/tokens";
import { type T, nav, useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize, useControlSize } from "@/shared/ui/touch";
import { prices as pricesKeys } from "../model/i18n";

const SCOPE_NAME = {
  conductor: (t) => t(pricesKeys.scope.conductor, "GBPs and the map"),
  review_archive: (t) => t(nav.reviewArchive, "Review Archive"),
  browsers: (t) => t(pricesKeys.scope.browsers, "Antidetect browsers and phones"),
  placement: (t) => t(pricesKeys.scope.placement, "Placement studies"),
} satisfies Record<Scope, (t: T) => string>;

/** In first-seen order. */
function grouped<K>(prices: Price[], by: (p: Price) => K): [K, Price[]][] {
  const groups = new Map<K, Price[]>();
  for (const p of prices) groups.set(by(p), [...(groups.get(by(p)) ?? []), p]);
  return [...groups];
}

/** What is typed, as stored: empty is none, `undefined` not a number this field takes. */
const tokens = (raw: string): number | null | undefined => {
  if (raw.trim() === "") return null;
  const n = Number(raw);
  return Number.isSafeInteger(n) && n >= 0 ? n : undefined;
};
const usdMicros = (raw: string): number | null | undefined => {
  if (raw.trim() === "") return null;
  const n = Number(raw);
  return Number.isFinite(n) && n >= 0 ? Math.round(n * 1e6) : undefined;
};

type Field = "price" | "cost";
const draftKey = (item: PricedItem, field: Field) => `${item}:${field}`;

/**
 * Every costly action, by where it happens and with what: its price to a person in tokens (sold
 * ones only) and what a unit costs us in dollars, set in place; only what changed is sent.
 */
export function PricesCard({ prices, onSaved }: { prices: Price[]; onSaved: (prices: Price[]) => void }) {
  const t = useT();
  const button = useButtonSize();
  const size = useControlSize();
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  const shownPrice = (p: Price) => drafts[draftKey(p.item, "price")] ?? p.price?.tokens.toString() ?? "";
  const shownCost = (p: Price) => drafts[draftKey(p.item, "cost")] ?? (p.cost === null ? "" : (p.cost.usd_micros / 1e6).toString());
  const priceChanged = prices.filter((p) => p.sale === "sold" && tokens(shownPrice(p)) !== (p.price?.tokens ?? null));
  const costChanged = prices.filter((p) => usdMicros(shownCost(p)) !== (p.cost?.usd_micros ?? null));
  const invalid = priceChanged.some((p) => tokens(shownPrice(p)) === undefined) || costChanged.some((p) => usdMicros(shownCost(p)) === undefined);
  const changes = priceChanged.length + costChanged.length;
  const unsold = prices.filter((p) => p.sale === "sold" && p.price === null).length;
  const uncosted = prices.filter((p) => p.cost === null).length;

  const save = () => {
    setBusy(true);
    setPrices(
      Object.fromEntries(priceChanged.map((p) => [p.item, tokens(shownPrice(p))])),
      Object.fromEntries(costChanged.map((p) => [p.item, usdMicros(shownCost(p))])),
    )
      .then((saved) => {
        toast.positive(t(pricesKeys.saved, "Prices saved"));
        setDrafts({});
        onSaved(saved);
      })
      .catch((err: unknown) => notifyFailure(err, t))
      .finally(() => setBusy(false));
  };
  const edit = (item: PricedItem, field: Field) => (e: ChangeEvent<HTMLInputElement>) => setDrafts((d) => ({ ...d, [draftKey(item, field)]: e.target.value }));

  return (
    <Card className="gap-3 py-4">
      <CardContent className="flex flex-col gap-4 px-4">
        <header className="flex flex-wrap items-baseline gap-2">
          <h2 className="text-sm font-semibold text-ink">{t(pricesKeys.title, "Prices")}</h2>
          {unsold > 0 && <Badge tone="warn">{t(pricesKeys.unsold.count, "{n} not sold", { n: unsold })}</Badge>}
          {uncosted > 0 && <Badge>{t(pricesKeys.uncosted.count, "{n} without a cost", { n: uncosted })}</Badge>}
          <p className="basis-full text-xs text-ink-soft">
            {t(
              pricesKeys.why,
              "Tokens: what each thing costs a person; empty is not sold, so a Places request is refused and a walk step not walked. Cached answers and a person's own Maps keys cost nothing. $: what one unit costs us, for the spending below.",
            )}
          </p>
        </header>
        {grouped(prices, (p) => p.scope).map(([scope, ofScope]) => (
          <section key={scope} className="flex flex-col gap-3">
            <h3 className="text-xs font-semibold tracking-wide text-ink-soft uppercase">{SCOPE_NAME[scope](t)}</h3>
            {grouped(ofScope, (p) => p.tool_title).map(([tool, items]) => (
              <div key={tool} className="flex flex-col gap-1">
                <h4 className="text-sm text-ink">{tool}</h4>
                <ul className="flex flex-col divide-y divide-border">
                  {items.map((p) => (
                    <li key={p.item} className="flex flex-wrap items-center gap-x-4 gap-y-1 py-2">
                      <div className="flex min-w-0 flex-1 basis-64 flex-col">
                        <span className="text-sm text-ink">{p.title}</span>
                        <span className="text-xs text-ink-soft">{p.description}</span>
                      </div>
                      <label className="flex items-center gap-1.5 text-xs text-ink-soft">
                        {p.sale === "sold" ? (
                          <Input
                            type="number"
                            inputMode="numeric"
                            min={0}
                            size={size}
                            className="w-24 tabular-nums"
                            aria-label={t(pricesKeys.tokens.label, "{item}: tokens a {unit}", { item: p.title, unit: p.unit })}
                            aria-invalid={tokens(shownPrice(p)) === undefined || undefined}
                            value={shownPrice(p)}
                            placeholder={t(pricesKeys.unsold, "not sold")}
                            onChange={edit(p.item, "price")}
                          />
                        ) : (
                          <span className="w-24 text-center">{t(pricesKeys.metered, "nobody charged")}</span>
                        )}
                        <span className="w-12">{t(pricesKeys.tokens, "tokens")}</span>
                      </label>
                      <label className="flex items-center gap-1.5 text-xs text-ink-soft">
                        <span>$</span>
                        <Input
                          type="number"
                          inputMode="decimal"
                          min={0}
                          step="any"
                          size={size}
                          className="w-28 tabular-nums"
                          aria-label={t(pricesKeys.cost.label, "{item}: dollars a {unit}", { item: p.title, unit: p.unit })}
                          aria-invalid={usdMicros(shownCost(p)) === undefined || undefined}
                          value={shownCost(p)}
                          placeholder={t(pricesKeys.cost.unknown, "unknown")}
                          onChange={edit(p.item, "cost")}
                        />
                        <span className="w-16">{t(pricesKeys.per, "a {unit}", { unit: p.unit })}</span>
                      </label>
                    </li>
                  ))}
                </ul>
              </div>
            ))}
          </section>
        ))}
        <footer className="flex items-center gap-2">
          <Button type="button" size={button("sm")} disabled={busy || invalid || changes === 0} onClick={save}>
            {changes === 0 ? t(pricesKeys.save, "Save") : t(pricesKeys.save.count, "Save {n}", { n: changes })}
          </Button>
          {changes > 0 && (
            <Button type="button" variant="ghost" size={button("sm")} disabled={busy} onClick={() => setDrafts({})}>
              {t(pricesKeys.reset, "Undo changes")}
            </Button>
          )}
        </footer>
      </CardContent>
    </Card>
  );
}
