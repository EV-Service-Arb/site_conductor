"use client";

import { Badge, Button, Card, CardContent, Field, FieldLabel, Input } from "@evinvest/uikit";
import { KeyRound } from "lucide-react";
import { useId, useState } from "react";

import { type MapsKey, type Prices, disconnect, setMapsKey, testConnection } from "@/entities/gmb";
import { useLocale, useT } from "@/shared/i18n";
import { formatDateTime } from "@/shared/lib/format";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize, useControlSize } from "@/shared/ui/touch";

/** A gmail's own Maps key: tried first for its GBPs, then for its owner's others, before the shared keys that cost tokens. */
export function MapsKeyCard({ gmail, mapsKey, prices, onChanged }: { gmail: number; mapsKey: MapsKey | null; prices: Prices; onChanged: () => void }) {
  const t = useT();
  const locale = useLocale();
  const id = useId();
  const button = useButtonSize();
  const size = useControlSize();
  const [editing, setEditing] = useState(false);
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const act = async (run: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await run();
      onChanged();
      setEditing(false);
      setKey("");
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
    }
  };
  const failing = mapsKey !== null && (mapsKey.checked.error !== null || mapsKey.out !== null);
  const cost =
    prices.details === null || prices.search === null || prices.autocomplete === null
      ? t("The shared keys are not priced yet: without a key of your own, Google lookups are refused.")
      : t("On the shared keys: {details} tokens a lookup, {search} a search, {autocomplete} a suggestion.", { details: prices.details, search: prices.search, autocomplete: prices.autocomplete });

  return (
    <Card className="gap-3 py-4">
      <CardContent className="flex flex-col gap-3 px-4">
        <header className="flex items-center gap-2">
          <KeyRound aria-hidden className="size-4 text-ink-soft" />
          <h3 className="flex-1 text-sm font-semibold text-ink">{t("Google Maps key")}</h3>
          {mapsKey && <Badge tone={failing ? "error" : "positive"}>{(failing ? t("Failing") : t("Works"))}</Badge>}
        </header>
        {mapsKey && !editing ? (
          <>
            <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
              <dt className="text-ink-soft">{t("Key")}</dt>
              <dd className="text-right font-mono text-ink">••••{mapsKey.last4}</dd>
              <dt className="text-ink-soft">{t("Set")}</dt>
              <dd className="text-right text-ink-soft">{formatDateTime(mapsKey.set_at, locale)}</dd>
            </dl>
            <p className="text-xs text-ink-soft">
              {t("Checked {at}", { at: formatDateTime(mapsKey.checked.at, locale) })}
              {mapsKey.checked.error !== null && <span className="block text-accent-error">{mapsKey.checked.error}</span>}
              {mapsKey.out !== null && <span className="block text-accent-warn">{t("Passed over now: {why}", { why: mapsKey.out })}</span>}
            </p>
            <div className="flex flex-wrap gap-2">
              <Button variant="outline" size={button("sm")} disabled={busy} onClick={() => void act(() => testConnection(gmail, "maps_key"))}>
                {t("Test")}
              </Button>
              <Button variant="ghost" size={button("sm")} disabled={busy} onClick={() => setEditing(true)}>
                {t("Replace")}
              </Button>
              <Button
                variant="ghost"
                size={button("sm")}
                disabled={busy}
                onClick={() =>
                  void act(() => disconnect(gmail, "maps_key"))
                }
              >
                {t("Remove")}
              </Button>
            </div>
          </>
        ) : (
          <form
            className="flex flex-col gap-3"
            onSubmit={(e) => {
              e.preventDefault();
              if (key.trim() !== "") void act(() => setMapsKey(gmail, key.trim()));
            }}
          >
            <Field>
              <FieldLabel htmlFor={`${id}-key`}>{t("Key")}</FieldLabel>
              <Input id={`${id}-key`} size={size} type="password" value={key} onChange={(e) => setKey(e.target.value)} autoComplete="off" placeholder="AIza…" />
            </Field>
            <div className="flex gap-2">
              <Button type="submit" size={button("sm")} disabled={key.trim() === "" || busy}>
                {t("Check and keep")}
              </Button>
              {mapsKey && (
                <Button type="button" variant="ghost" size={button("sm")} disabled={busy} onClick={() => setEditing(false)}>
                  {t("Cancel")}
                </Button>
              )}
            </div>
          </form>
        )}
        <p className="text-xs text-ink-soft">{t("Tried first for this gmail's GBPs, then for your other gmails', before the shared keys. Your own key costs no tokens.")}</p>
        <p className="text-xs text-ink-soft">{cost}</p>
      </CardContent>
    </Card>
  );
}
