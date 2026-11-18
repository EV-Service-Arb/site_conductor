"use client";

import { Badge, Button, Card, CardContent, Input, toast } from "@evinvest/uikit";
import { type ChangeEvent, useState } from "react";

import { api, ok } from "@/shared/api";
import type { Price, PricedItem, Scope } from "@/shared/api/schema";
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
/** "a request", or "per 1,000,000 token" for what is priced by the million. */
const perUnit = (t: T, p: Price) => (p.per === 1 ? t(pricesKeys.per, "a {unit}", { unit: p.unit }) : t(pricesKeys.perMany, "per {n} {unit}", { n: p.per.toLocaleString(), unit: p.unit }));

function grouped<K>(prices: Price[], by: (p: Price) => K): [K, Price[]][] {
  const groups = new Map<K, Price[]>();
  for (const p of prices) groups.set(by(p), [...(groups.get(by(p)) ?? []), p]);
  return [...groups];
}

/** What is typed, to three decimals as stored: empty is none, `undefined` not a number this field takes. */
const thousandths = (raw: string): number | null | undefined => {
  if (raw.trim() === "") return null;
  const n = Number(raw);
  return Number.isFinite(n) && n >= 0 ? Math.round(n * 1e3) / 1e3 : undefined;
};

/** The server's `Rate::price`: the cost in tokens × the multiplier, rounded up. */
const priceOf = (cost: number | null | undefined, multiplier: number | null | undefined) =>
  cost == null || multiplier == null ? null : Math.ceil((Math.round(cost * 1e3) * Math.round(multiplier * 1e3)) / 1e6);

type Field = "multiplier" | "cost";
const draftKey = (item: PricedItem, field: Field) => `${item}:${field}`;

/**
 * Every costly action, by where it happens and with what: what its units cost us in tokens and the
 * multiplier a person is charged over it, set in place; only what changed is sent.
 */
export function PricesCard({ prices, tokenUsd, onSaved }: { prices: Price[]; tokenUsd: number; onSaved: (prices: Price[]) => void }) {
  const t = useT();
  const button = useButtonSize();
  const size = useControlSize();
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  const shownMultiplier = (p: Price) => drafts[draftKey(p.item, "multiplier")] ?? p.multiplier?.multiplier.toString() ?? "";
  const shownCost = (p: Price) => drafts[draftKey(p.item, "cost")] ?? p.cost?.tokens.toString() ?? "";
  const multiplierChanged = prices.filter((p) => thousandths(shownMultiplier(p)) !== (p.multiplier?.multiplier ?? null));
  const costChanged = prices.filter((p) => thousandths(shownCost(p)) !== (p.cost?.tokens ?? null));
  const invalid = multiplierChanged.some((p) => thousandths(shownMultiplier(p)) === undefined) || costChanged.some((p) => thousandths(shownCost(p)) === undefined);
  const changes = multiplierChanged.length + costChanged.length;
  const unsold = prices.filter((p) => p.price === null).length;
  const usd = (tokens: number) => (tokens * tokenUsd).toLocaleString(undefined, { style: "currency", currency: "USD", maximumFractionDigits: 6 });
  const uncosted = prices.filter((p) => p.cost === null).length;

  const save = () => {
    setBusy(true);
    ok(
      api.PUT("/admin/tokens/prices", {
        body: {
          multipliers: Object.fromEntries(multiplierChanged.map((p) => [p.item, thousandths(shownMultiplier(p))])),
          costs: Object.fromEntries(costChanged.map((p) => [p.item, thousandths(shownCost(p))])),
        },
      }),
    )
      .then(({ prices: saved }) => {
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
              "Cost: what it costs us, in tokens ({rate}). ×: what a person is charged over it, 1 at cost, 0 for free. Either empty is not sold, so it is refused: a Places request, a map, a browser's start, a walk step. Cached answers and a person's own Maps keys cost nothing.",
              { rate: t(pricesKeys.rate, "1 token = {usd}", { usd: usd(1) }) },
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
                        <Input
                          type="number"
                          inputMode="decimal"
                          min={0}
                          step="any"
                          size={size}
                          className="w-24 tabular-nums"
                          aria-label={t(pricesKeys.cost.label, "{item}: cost in tokens {per}", { item: p.title, per: perUnit(t, p) })}
                          aria-invalid={thousandths(shownCost(p)) === undefined || undefined}
                          value={shownCost(p)}
                          placeholder={t(pricesKeys.cost.unknown, "unknown")}
                          onChange={edit(p.item, "cost")}
                        />
                        <span className="w-24">
                          {t(pricesKeys.tokens, "tokens")} {perUnit(t, p)}
                          {thousandths(shownCost(p)) != null && <span className="block tabular-nums">{usd(thousandths(shownCost(p))!)}</span>}
                        </span>
                      </label>
                      <label className="flex items-center gap-1.5 text-xs text-ink-soft">
                        <span>×</span>
                        <Input
                          type="number"
                          inputMode="decimal"
                          min={0}
                          step="any"
                          size={size}
                          className="w-20 tabular-nums"
                          aria-label={t(pricesKeys.multiplier.label, "{item}: multiplier", { item: p.title })}
                          aria-invalid={thousandths(shownMultiplier(p)) === undefined || undefined}
                          value={shownMultiplier(p)}
                          placeholder={t(pricesKeys.unsold, "not sold")}
                          onChange={edit(p.item, "multiplier")}
                        />
                      </label>
                      <span className="w-24 text-sm tabular-nums text-ink">
                        {priceOf(thousandths(shownCost(p)), thousandths(shownMultiplier(p))) ?? t(pricesKeys.unsold, "not sold")}
                        <span className="text-xs text-ink-soft"> {t(pricesKeys.tokens, "tokens")}</span>
                      </span>
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
