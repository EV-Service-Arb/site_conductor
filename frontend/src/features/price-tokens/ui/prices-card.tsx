"use client";

import {
  Badge,
  Button,
  Card,
  CardContent,
  Input,
  toast,
} from "@evinvest/uikit";
import { useState } from "react";

import {
  type Price,
  type PricedItem,
  type Scope,
  setPrices,
} from "@/entities/tokens";
import { type T, nav, useLocale, useT } from "@/shared/i18n";
import { formatDateTime } from "@/shared/lib/format";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize, useControlSize } from "@/shared/ui/touch";
import { prices as pricesKeys } from "../model/i18n";

const SCOPE_NAME = {
  conductor: (t) => t(pricesKeys.scope.conductor, "GBPs and the map"),
  review_archive: (t) => t(nav.reviewArchive, "Review Archive"),
} satisfies Record<Scope, (t: T) => string>;

/** In first-seen order. */
function grouped<K>(prices: Price[], by: (p: Price) => K): [K, Price[]][] {
  const groups = new Map<K, Price[]>();
  for (const p of prices) groups.set(by(p), [...(groups.get(by(p)) ?? []), p]);
  return [...groups];
}

/** An input as a price: empty is off sale, `undefined` not a price. */
const parsed = (raw: string): number | null | undefined => {
  if (raw.trim() === "") return null;
  const n = Number(raw);
  return Number.isSafeInteger(n) && n >= 0 ? n : undefined;
};

/** Every priced item, by who charges and with what, priced in place: only what changed is sent. */
export function PricesCard({
  prices,
  onSaved,
}: {
  prices: Price[];
  onSaved: (prices: Price[]) => void;
}) {
  const t = useT();
  const locale = useLocale();
  const button = useButtonSize();
  const size = useControlSize();
  const [drafts, setDrafts] = useState<Partial<Record<PricedItem, string>>>({});
  const [busy, setBusy] = useState(false);
  const shown = (p: Price) => drafts[p.item] ?? p.tokens?.toString() ?? "";
  const changed = prices.filter((p) => parsed(shown(p)) !== p.tokens);
  const invalid = changed.some((p) => parsed(shown(p)) === undefined);
  const unsold = prices.filter((p) => p.tokens === null).length;
  const scopes = grouped(prices, (p) => p.scope);

  const save = () => {
    setBusy(true);
    setPrices(
      Object.fromEntries(changed.map((p) => [p.item, parsed(shown(p))])),
    )
      .then((saved) => {
        toast.positive(t(pricesKeys.saved, "Prices saved"));
        setDrafts({});
        onSaved(saved);
      })
      .catch((err: unknown) => notifyFailure(err, t))
      .finally(() => setBusy(false));
  };

  return (
    <Card className="gap-3 py-4">
      <CardContent className="flex flex-col gap-4 px-4">
        <header className="flex flex-wrap items-baseline gap-2">
          <h2 className="text-sm font-semibold text-ink">
            {t(pricesKeys.title, "Prices")}
          </h2>
          {unsold > 0 && (
            <Badge tone="warn">
              {t(pricesKeys.unsold.count, "{n} not sold", { n: unsold })}
            </Badge>
          )}
          <p className="basis-full text-xs text-ink-soft">
            {t(
              pricesKeys.why,
              "Tokens each thing costs a person. Empty is not sold: a Places request is refused, a walk step not walked. Cached answers and a person's own Maps keys cost nothing.",
            )}
          </p>
        </header>
        {scopes.map(([scope, ofScope]) => (
          <section key={scope} className="flex flex-col gap-3">
            <h3 className="text-xs font-semibold tracking-wide text-ink-soft uppercase">
              {SCOPE_NAME[scope](t)}
            </h3>
            {grouped(ofScope, (p) => p.tool_title).map(
              ([tool, items]) => (
                <div key={tool} className="flex flex-col gap-1">
                  <h4 className="text-sm text-ink">{tool}</h4>
                  <ul className="flex flex-col divide-y divide-border">
                    {items.map((p) => {
                      const value = shown(p);
                      const bad = parsed(value) === undefined;
                      return (
                        <li
                          key={p.item}
                          className="flex flex-wrap items-center gap-x-4 gap-y-0.5 py-2"
                        >
                          <label
                            htmlFor={p.item}
                            className="flex min-w-0 flex-1 basis-64 flex-col"
                          >
                            <span className="text-sm text-ink">{p.title}</span>
                            <span className="text-xs text-ink-soft">
                              {p.description}
                            </span>
                          </label>
                          <Input
                            id={p.item}
                            type="number"
                            inputMode="numeric"
                            min={0}
                            size={size}
                            className="w-28 tabular-nums"
                            aria-invalid={bad || undefined}
                            value={value}
                            placeholder={t(pricesKeys.unsold, "not sold")}
                            onChange={(e) =>
                              setDrafts((d) => ({
                                ...d,
                                [p.item]: e.target.value,
                              }))
                            }
                          />
                          <span className="w-48 text-xs text-ink-soft">
                            {p.set_by !== null &&
                              p.set_at !== null &&
                              t(pricesKeys.setBy, "{by}, {at}", {
                                by: p.set_by,
                                at: formatDateTime(p.set_at, locale),
                              })}
                          </span>
                        </li>
                      );
                    })}
                  </ul>
                </div>
              ),
            )}
          </section>
        ))}
        <footer className="flex items-center gap-2">
          <Button
            type="button"
            size={button("sm")}
            disabled={busy || invalid || changed.length === 0}
            onClick={save}
          >
            {changed.length === 0
              ? t(pricesKeys.save, "Save")
              : t(pricesKeys.save.count, "Save {n}", { n: changed.length })}
          </Button>
          {changed.length > 0 && (
            <Button
              type="button"
              variant="ghost"
              size={button("sm")}
              disabled={busy}
              onClick={() => setDrafts({})}
            >
              {t(pricesKeys.reset, "Undo changes")}
            </Button>
          )}
        </footer>
      </CardContent>
    </Card>
  );
}
